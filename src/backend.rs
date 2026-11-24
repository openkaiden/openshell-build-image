// Copyright (C) 2026 Red Hat, Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//
// SPDX-License-Identifier: Apache-2.0

//! The complete-build boundary shared by host CLIs and the build VM.

use std::path::{Path, PathBuf};

use container_image_builder::{ContainerCli, Runner, build, build_with_local_images_on_platform};
use openshell_build_image::containerfile::{self, ResolvedAddon};
use vm_image_builder::{VmConfig, VmRequest, VmRunner, build_with_request as vm_build};

use crate::host_addon::{self, HostAddons};

pub type BuildError = Box<dyn std::error::Error>;

/// Inputs whose order and values are the same for every runtime.
pub struct BuildRequest<'a> {
    pub from: &'a str,
    pub addons: &'a [String],
    pub tag: &'a str,
    pub copy_containerfile: bool,
}

/// The output of a successful build. VM output is a flattened rootfs tarball.
#[derive(Debug, PartialEq, Eq)]
pub enum BuildResult {
    Image {
        tag: String,
    },
    VmArtifact {
        rootfs_tar: PathBuf,
        // The selected addons' OCI environment is saved beside the tarball.
        config: Option<PathBuf>,
    },
}

pub trait ImageBackend {
    fn check_ready(&self) -> Result<(), BuildError>;
    fn build(&self, request: &BuildRequest<'_>) -> Result<BuildResult, BuildError>;
}

pub struct PodmanBackend<'a> {
    pub runner: &'a dyn Runner,
}

pub struct DockerBackend<'a> {
    pub runner: &'a dyn Runner,
}

pub struct AppleContainerBackend<'a> {
    pub runner: &'a dyn Runner,
}

pub struct VmBackend<'a> {
    pub config: &'a VmConfig,
    pub runner: &'a dyn VmRunner,
    pub output: &'a Path,
}

/// Resolve host addons in the same runtime store that will run the final build.
fn prepare_host(
    request: &BuildRequest<'_>,
    cli: &ContainerCli,
    runner: &dyn Runner,
) -> Result<Option<HostAddons>, BuildError> {
    if request.addons.is_empty() {
        Ok(None)
    } else {
        Ok(Some(host_addon::resolve(
            request.from,
            request.addons,
            cli,
            runner,
        )?))
    }
}

/// Generate the context and build with either the original base or prepared
/// local images. Docker may pass a temporary local tag for its prepared base.
fn build_host(
    request: &BuildRequest<'_>,
    cli: &ContainerCli,
    runner: &dyn Runner,
    prepared: Option<&HostAddons>,
    base_override: Option<&str>,
) -> Result<BuildResult, BuildError> {
    let context = tempfile::Builder::new()
        .prefix("openshell-build-image")
        .tempdir()?;
    let base = base_override
        .or_else(|| prepared.map(|value| value.base.as_str()))
        .unwrap_or(request.from);
    let addons = prepared.map_or(&[][..], |value| value.addons.as_slice());
    let output = stage_containerfile(request, context.path(), base, addons)?;
    if let Some(prepared) = prepared {
        build_with_local_images_on_platform(
            &output,
            request.tag,
            cli,
            runner,
            context.path(),
            &prepared.platform,
        )?;
    } else {
        build(&output, request.tag, cli, runner, context.path())?;
    }
    Ok(BuildResult::Image {
        tag: request.tag.to_owned(),
    })
}

fn stage_containerfile(
    request: &BuildRequest<'_>,
    context: &Path,
    base: &str,
    addons: &[ResolvedAddon],
) -> Result<String, BuildError> {
    let output = containerfile::generate(base, request.copy_containerfile, addons)?;
    if request.copy_containerfile {
        // The build may use an inspected image ID or a temporary Docker tag.
        // The copy in the result records the base reference the user supplied.
        let copied = containerfile::generate(request.from, true, addons)?;
        std::fs::write(context.join("build-containerfile"), copied)?;
    }
    Ok(output)
}

impl ImageBackend for PodmanBackend<'_> {
    fn check_ready(&self) -> Result<(), BuildError> {
        ContainerCli::Podman.check_in_path()?;
        Ok(())
    }

    fn build(&self, request: &BuildRequest<'_>) -> Result<BuildResult, BuildError> {
        let cli = ContainerCli::Podman;
        let prepared = prepare_host(request, &cli, self.runner)?;
        build_host(request, &cli, self.runner, prepared.as_ref(), None)
    }
}

impl ImageBackend for AppleContainerBackend<'_> {
    fn check_ready(&self) -> Result<(), BuildError> {
        ContainerCli::MacOsContainer.check_in_path()?;
        Ok(())
    }

    fn build(&self, request: &BuildRequest<'_>) -> Result<BuildResult, BuildError> {
        let cli = ContainerCli::MacOsContainer;
        let prepared = prepare_host(request, &cli, self.runner)?;
        build_host(request, &cli, self.runner, prepared.as_ref(), None)
    }
}

impl ImageBackend for DockerBackend<'_> {
    fn check_ready(&self) -> Result<(), BuildError> {
        ContainerCli::Docker.check_in_path()?;
        Ok(())
    }

    fn build(&self, request: &BuildRequest<'_>) -> Result<BuildResult, BuildError> {
        let cli = ContainerCli::Docker;
        let prepared = prepare_host(request, &cli, self.runner)?;
        // BuildKit cannot use the inspected local image ID in FROM. Keep the
        // temporary tag and its cleanup inside the Docker implementation.
        let mut docker_base = prepared
            .as_ref()
            .map(|value| host_addon::DockerBuildBase::new(request.from, &value.base, self.runner))
            .transpose()?;
        let result = build_host(
            request,
            &cli,
            self.runner,
            prepared.as_ref(),
            docker_base.as_ref().map(|base| base.reference()),
        );
        let cleanup = docker_base.as_mut().map_or(Ok(()), |base| base.cleanup());
        match (result, cleanup) {
            (Ok(result), Ok(())) => Ok(result),
            (Err(reason), Ok(())) => Err(reason),
            (Ok(_), Err(reason)) => Err(reason.into()),
            (Err(build), Err(cleanup)) => Err(std::io::Error::other(format!(
                "{build}; cleanup also failed: {cleanup}"
            ))
            .into()),
        }
    }
}

impl ImageBackend for VmBackend<'_> {
    fn check_ready(&self) -> Result<(), BuildError> {
        self.runner.check_supported()?;
        self.config.check_rootfs()?;
        Ok(())
    }

    fn build(&self, request: &BuildRequest<'_>) -> Result<BuildResult, BuildError> {
        if !request.addons.is_empty() {
            let helper = self.config.rootfs.join("usr/local/bin/vm-guest-helper");
            if !helper.is_file() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!(
                        "VM rootfs {} does not contain vm-guest-helper; rebuild it to use --addon",
                        self.config.rootfs.display()
                    ),
                )
                .into());
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if std::fs::metadata(&helper)?.permissions().mode() & 0o111 == 0 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        format!("VM guest helper {} is not executable", helper.display()),
                    )
                    .into());
                }
            }
        }
        let context = tempfile::Builder::new()
            .prefix("openshell-build-image")
            .tempdir()?;
        let output = stage_containerfile(request, context.path(), request.from, &[])?;
        vm_build(
            &output,
            &VmRequest {
                from: request.from,
                addons: request.addons,
                tag: request.tag,
                copy_containerfile: request.copy_containerfile,
            },
            self.config,
            self.runner,
            context.path(),
            self.output,
        )?;
        Ok(BuildResult::VmArtifact {
            rootfs_tar: self.output.to_path_buf(),
            config: (!request.addons.is_empty()).then(|| {
                let mut name = self.output.as_os_str().to_os_string();
                name.push(".config.json");
                PathBuf::from(name)
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openshell_build_image::addon::{AddonVariant, Platform};

    #[test]
    fn copied_addon_containerfile_uses_requested_base() {
        let context = tempfile::tempdir().unwrap();
        let requested_addons = ["ghcr.io/example/tools:latest".to_owned()];
        let request = BuildRequest {
            from: "example.test/base:latest",
            addons: &requested_addons,
            tag: "example.test/result:latest",
            copy_containerfile: true,
        };
        let selected = ResolvedAddon::from_variant(
            &requested_addons[0],
            &AddonVariant {
                platform: Platform::new("linux", "amd64", None),
                libc: None,
                digest: format!("sha256:{}", "a".repeat(64)),
            },
            r#"{"config":{}}"#,
        )
        .unwrap();

        for prepared_base in ["b".repeat(64), "osbi-build-base:temporary".to_owned()] {
            let build = stage_containerfile(
                &request,
                context.path(),
                &prepared_base,
                std::slice::from_ref(&selected),
            )
            .unwrap();
            let copied =
                std::fs::read_to_string(context.path().join("build-containerfile")).unwrap();

            assert!(build.contains(&format!("FROM {prepared_base} AS system\n")));
            assert!(copied.contains("FROM example.test/base:latest AS system\n"));
            assert!(!copied.contains(&format!("FROM {prepared_base} AS system\n")));
            assert!(copied.contains("COPY --from=ghcr.io/example/tools@sha256:"));
        }
    }
}
