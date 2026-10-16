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

//! Resolve OCI addons into the image store used by a host container runtime.

use std::collections::HashMap;
use std::process::{Command, Output};

use container_image_builder::base::{BasePlatform, prepare_base};
use container_image_builder::{ContainerCli, Runner};
use openshell_build_image::addon::{AddonCatalog, AddonVariant, Platform};
use openshell_build_image::containerfile::{self, ResolvedAddon};
use serde_json::{Value, json};

use crate::registry::RegistryClient;

#[derive(Debug)]
pub struct HostAddonError(String);

impl std::fmt::Display for HostAddonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for HostAddonError {}

fn error(message: impl Into<String>) -> HostAddonError {
    HostAddonError(message.into())
}

fn addon_error(image: &str, reason: impl std::fmt::Display) -> HostAddonError {
    error(format!("addon {image:?}: {reason}"))
}

pub struct HostAddons {
    pub base: String,
    pub platform: BasePlatform,
    pub addons: Vec<ResolvedAddon>,
}

/// BuildKit does not resolve a bare Docker image ID in `FROM`. Give a tagged
/// base a temporary name tied to its inspected ID, then remove that name after
/// the build. A requested repository digest is already immutable and can be
/// used directly; an untagged image ID has no other name to preserve when the
/// temporary tag is removed.
pub struct DockerBuildBase<'a> {
    reference: String,
    temporary: bool,
    runner: &'a dyn Runner,
    _reservation: Option<tempfile::TempDir>,
}

impl<'a> DockerBuildBase<'a> {
    pub fn new(
        requested: &str,
        inspected_id: &str,
        runner: &'a dyn Runner,
    ) -> Result<Self, HostAddonError> {
        if requested.contains('@') {
            return Ok(Self {
                reference: requested.to_owned(),
                temporary: false,
                runner,
                _reservation: None,
            });
        }
        let hex = requested.strip_prefix("sha256:").unwrap_or(requested);
        if hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(error(
                "Docker addon builds require a base image tag or repository digest, not a bare image ID",
            ));
        }
        let reservation = tempfile::Builder::new()
            .prefix("osbi-docker-base-")
            .tempdir()
            .map_err(|reason| error(format!("cannot reserve Docker base tag: {reason}")))?;
        let suffix = reservation
            .path()
            .file_name()
            .expect("temporary directory has a name")
            .to_string_lossy()
            .to_ascii_lowercase();
        let reference = format!("osbi-build-base:{suffix}");
        let mut tag = Command::new("docker");
        tag.args(["image", "tag", inspected_id, &reference]);
        let status = runner
            .run(&mut tag)
            .map_err(|reason| error(format!("Docker base tagging failed: {reason}")))?;
        if !status.success() {
            return Err(error(format!(
                "Docker base tagging failed with exit code {:?}",
                status.code()
            )));
        }
        Ok(Self {
            reference,
            temporary: true,
            runner,
            _reservation: Some(reservation),
        })
    }

    pub fn reference(&self) -> &str {
        &self.reference
    }

    pub fn cleanup(&mut self) -> Result<(), HostAddonError> {
        if !self.temporary {
            return Ok(());
        }
        let mut remove = Command::new("docker");
        remove.args(["image", "rm", &self.reference]);
        let status = self
            .runner
            .run(&mut remove)
            .map_err(|reason| error(format!("temporary Docker base removal failed: {reason}")))?;
        if !status.success() {
            return Err(error(format!(
                "temporary Docker base removal failed with exit code {:?}",
                status.code()
            )));
        }
        self.temporary = false;
        Ok(())
    }
}

impl Drop for DockerBuildBase<'_> {
    fn drop(&mut self) {
        if let Err(reason) = self.cleanup() {
            log::warn!("{reason}");
        }
    }
}

/// Read catalogs before preparing the base, so the base filesystem is probed
/// once only if at least one catalog has multiple libc choices. All selected
/// payloads are made available in the same runtime store before the build.
pub fn resolve(
    from: &str,
    images: &[String],
    cli: &ContainerCli,
    runner: &dyn Runner,
) -> Result<HostAddons, HostAddonError> {
    let mut catalog_cache: HashMap<String, AddonCatalog> = HashMap::new();
    let mut catalogs = Vec::with_capacity(images.len());
    for image in images {
        let catalog = if let Some(cached) = catalog_cache.get(image) {
            cached.clone()
        } else {
            let loaded = load_catalog(image, cli, runner)?;
            catalog_cache.insert(image.clone(), loaded.clone());
            loaded
        };
        catalogs.push(catalog);
    }
    let needs_libc = catalogs.iter().any(AddonCatalog::requires_base_libc);
    let base = prepare_base(from, cli, runner, needs_libc)
        .map_err(|reason| error(format!("base image {from:?}: {reason}")))?;
    let platform = Platform::new(
        &base.platform.os,
        &base.platform.architecture,
        base.platform.variant.as_deref(),
    );
    let libc = base.libc.map(|libc| libc.as_str());
    let mut config_cache: HashMap<String, String> = HashMap::new();
    let mut resolved = Vec::with_capacity(images.len());
    for (image, catalog) in images.iter().zip(catalogs.iter()) {
        let variant = catalog
            .select(&platform, libc)
            .map_err(|reason| addon_error(image, reason))?;
        let selected_ref = containerfile::pin_image_reference(image, &variant.digest)
            .map_err(|reason| addon_error(image, reason))?;
        let config_json = if let Some(cached) = config_cache.get(&selected_ref) {
            cached.clone()
        } else {
            let config = selected_config(&selected_ref, variant, cli, runner)
                .map_err(|reason| addon_error(image, reason))?;
            config_cache.insert(selected_ref, config.clone());
            config
        };
        resolved.push(
            ResolvedAddon::from_variant(image, variant, &config_json)
                .map_err(|reason| addon_error(image, reason))?,
        );
    }
    Ok(HostAddons {
        base: base.local_reference,
        platform: base.platform,
        addons: resolved,
    })
}

fn load_catalog(
    image: &str,
    cli: &ContainerCli,
    runner: &dyn Runner,
) -> Result<AddonCatalog, HostAddonError> {
    if *cli == ContainerCli::MacOsContainer {
        let registry =
            RegistryClient::new(image, runner).map_err(|reason| addon_error(image, reason))?;
        let document = registry
            .manifest()
            .map_err(|reason| addon_error(image, reason))?;
        let value: Value =
            serde_json::from_str(&document.json).map_err(|reason| addon_error(image, reason))?;
        if value.get("manifests").is_some() {
            AddonCatalog::parse_index(&document.json).map_err(|reason| addon_error(image, reason))
        } else {
            let config_digest = value
                .get("config")
                .and_then(|config| config.get("digest"))
                .and_then(Value::as_str)
                .ok_or_else(|| addon_error(image, "single manifest has no config digest"))?;
            let config = registry
                .config(config_digest)
                .map_err(|reason| addon_error(image, reason))?;
            AddonCatalog::parse_single(&document.json, &config, &document.digest)
                .map_err(|reason| addon_error(image, reason))
        }
    } else {
        let mut inspect = Command::new(cli.binary());
        inspect.arg("manifest").arg("inspect");
        if *cli == ContainerCli::Podman && loopback_registry(image) {
            inspect.arg("--tls-verify=false");
        } else if *cli == ContainerCli::Docker
            && loopback_registry(image)
            && !ipv6_loopback_registry(image)
        {
            // Unlike `docker manifest inspect`, `docker pull` cannot enable
            // insecure access per command. Do not accept HTTP-only IPv6 here
            // and then fail after choosing its manifest.
            inspect.arg("--insecure");
        }
        inspect.arg(image);
        let output = runner
            .output(&mut inspect)
            .map_err(|reason| addon_error(image, reason))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let known_single = *cli == ContainerCli::Podman
                && (stderr.contains("Treating single images as manifest lists is not implemented")
                    || stderr.contains("not a manifest list"));
            // `manifest inspect` can consult a registry even when a single
            // image is already in the local store. A locally built addon may
            // have no registry at all, so inspect the local image before
            // treating that manifest error as fatal.
            if known_single || inspect_local_image(image, cli, runner)?.status.success() {
                return single_catalog_from_local(image, cli, runner);
            }
            if *cli == ContainerCli::Docker && ipv6_loopback_registry(image) {
                return Err(addon_error(
                    image,
                    format!(
                        "manifest inspection failed: {}; Docker addons from IPv6 loopback require a registry reachable over HTTPS",
                        stderr.trim()
                    ),
                ));
            }
            return Err(addon_error(
                image,
                format!("manifest inspection failed: {}", stderr.trim()),
            ));
        }
        let raw =
            std::str::from_utf8(&output.stdout).map_err(|reason| addon_error(image, reason))?;
        let value: Value =
            serde_json::from_str(raw).map_err(|reason| addon_error(image, reason))?;
        if value.get("manifests").is_some() {
            let catalog =
                AddonCatalog::parse_index(raw).map_err(|reason| addon_error(image, reason))?;
            if *cli == ContainerCli::Docker && has_unannotated_duplicate_platform(&catalog) {
                // Docker's manifest inspect can discard OCI descriptor annotations.
                // Buildx's --raw output preserves them for libc selection.
                let mut inspect_raw = Command::new("docker");
                inspect_raw.args(["buildx", "imagetools", "inspect", "--raw", image]);
                let output = runner.output(&mut inspect_raw).map_err(|reason| {
                    addon_error(image, format!("raw manifest inspection failed: {reason}"))
                })?;
                if !output.status.success() {
                    return Err(addon_error(
                        image,
                        format!(
                            "raw manifest inspection failed: {}",
                            String::from_utf8_lossy(&output.stderr).trim()
                        ),
                    ));
                }
                let raw = std::str::from_utf8(&output.stdout)
                    .map_err(|reason| addon_error(image, reason))?;
                AddonCatalog::parse_index(raw).map_err(|reason| addon_error(image, reason))
            } else {
                Ok(catalog)
            }
        } else {
            single_catalog_from_local(image, cli, runner)
        }
    }
}

fn has_unannotated_duplicate_platform(catalog: &AddonCatalog) -> bool {
    let variants = catalog.variants();
    variants.iter().enumerate().any(|(index, variant)| {
        variant.libc.is_none()
            && variants[index + 1..]
                .iter()
                .any(|other| other.libc.is_none() && other.platform == variant.platform)
    })
}

fn single_catalog_from_local(
    image: &str,
    cli: &ContainerCli,
    runner: &dyn Runner,
) -> Result<AddonCatalog, HostAddonError> {
    let inspected = ensure_local_image(image, cli, runner)?;
    let image_data = inspected_image(&inspected, image)?;
    let platform = platform_from_local_image(&image_data, image)?;
    let digest = if *cli == ContainerCli::Podman {
        required_str(&image_data, "Digest", image)?
    } else {
        let wanted = normalized_repository(image);
        image_data
            .get("RepoDigests")
            .and_then(Value::as_array)
            .and_then(|digests| {
                digests
                    .iter()
                    .filter_map(Value::as_str)
                    .find_map(|reference| {
                        let (repository, digest) = reference.split_once('@')?;
                        (normalized_repository(repository) == wanted).then_some(digest)
                    })
            })
            .ok_or_else(|| {
                addon_error(image, "local single image has no digest for its repository")
            })?
    };
    AddonCatalog::single_image(platform, digest).map_err(|reason| addon_error(image, reason))
}

fn selected_config(
    selected_ref: &str,
    variant: &AddonVariant,
    cli: &ContainerCli,
    runner: &dyn Runner,
) -> Result<String, HostAddonError> {
    let inspected = ensure_local_image(selected_ref, cli, runner)?;
    let image_data = inspected_image(&inspected, selected_ref)?;
    if *cli == ContainerCli::MacOsContainer {
        let variants = image_data
            .get("variants")
            .and_then(Value::as_array)
            .ok_or_else(|| error("Apple image inspection has no variants"))?;
        let selected = variants
            .iter()
            .find(|candidate| {
                candidate.get("digest").and_then(Value::as_str) == Some(variant.digest.as_str())
            })
            .ok_or_else(|| {
                error("Apple image inspection does not contain the selected manifest")
            })?;
        let config = selected
            .get("config")
            .ok_or_else(|| error("Apple image inspection has no selected config"))?;
        serde_json::to_string(config).map_err(|reason| error(reason.to_string()))
    } else {
        let platform = platform_from_local_image(&image_data, selected_ref)?;
        if platform.os != variant.platform.os
            || platform.architecture != variant.platform.architecture
        {
            return Err(error(format!(
                "selected image platform {platform} differs from descriptor {}",
                variant.platform
            )));
        }
        let config = image_data
            .get("Config")
            .ok_or_else(|| error("selected image inspection has no Config"))?;
        serde_json::to_string(&json!({"config": config}))
            .map_err(|reason| error(reason.to_string()))
    }
}

fn ensure_local_image(
    image: &str,
    cli: &ContainerCli,
    runner: &dyn Runner,
) -> Result<Vec<u8>, HostAddonError> {
    let first = inspect_local_image(image, cli, runner)?;
    if first.status.success() {
        return Ok(first.stdout);
    }
    let mut pull = Command::new(cli.binary());
    if *cli == ContainerCli::MacOsContainer {
        pull.args(["image", "pull"]);
        if loopback_registry(image) {
            pull.args(["--scheme", "http"]);
        }
    } else {
        pull.arg("pull");
        if *cli == ContainerCli::Podman && loopback_registry(image) {
            pull.arg("--tls-verify=false");
        }
    }
    pull.arg(image);
    let status = runner
        .run(&mut pull)
        .map_err(|reason| error(format!("image pull failed: {reason}")))?;
    if !status.success() {
        return Err(error(format!(
            "image pull failed with exit code {:?}",
            status.code()
        )));
    }
    let second = inspect_local_image(image, cli, runner)?;
    if !second.status.success() {
        return Err(error(format!(
            "image inspection after pull failed: {}",
            String::from_utf8_lossy(&second.stderr).trim()
        )));
    }
    Ok(second.stdout)
}

fn inspect_local_image(
    image: &str,
    cli: &ContainerCli,
    runner: &dyn Runner,
) -> Result<Output, HostAddonError> {
    let mut inspect = Command::new(cli.binary());
    inspect.args(["image", "inspect", image]);
    runner
        .output(&mut inspect)
        .map_err(|reason| error(format!("image inspection failed: {reason}")))
}

fn inspected_image(json: &[u8], image: &str) -> Result<Value, HostAddonError> {
    let value: Value = serde_json::from_slice(json).map_err(|reason| addon_error(image, reason))?;
    value
        .as_array()
        .and_then(|images| (images.len() == 1).then(|| images[0].clone()))
        .ok_or_else(|| addon_error(image, "expected one inspected image"))
}

fn platform_from_local_image(image_data: &Value, image: &str) -> Result<Platform, HostAddonError> {
    let os = required_str(image_data, "Os", image)?;
    let architecture = required_str(image_data, "Architecture", image)?;
    let variant = image_data.get("Variant").and_then(Value::as_str);
    Ok(Platform::new(os, architecture, variant))
}

fn required_str<'a>(value: &'a Value, key: &str, image: &str) -> Result<&'a str, HostAddonError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| addon_error(image, format!("image inspection has no {key}")))
}

fn loopback_registry(image: &str) -> bool {
    let host = image.split('/').next().unwrap_or(image);
    host == "localhost"
        || host.starts_with("localhost:")
        || host == "127.0.0.1"
        || host.starts_with("127.0.0.1:")
        || ipv6_loopback_registry(image)
}

fn ipv6_loopback_registry(image: &str) -> bool {
    let host = image.split('/').next().unwrap_or(image);
    host == "[::1]" || host.starts_with("[::1]:")
}

fn normalized_repository(image: &str) -> String {
    let name = image.split_once('@').map_or(image, |(name, _)| name);
    let last = name.rsplit('/').next().unwrap_or(name);
    let name = last
        .find(':')
        .map_or(name, |offset| &name[..name.len() - last.len() + offset]);
    let first = name.split('/').next().unwrap_or(name);
    let explicit_registry = first == "localhost" || first.contains('.') || first.contains(':');
    let (registry, repository) = if explicit_registry {
        name.split_once('/').unwrap_or((first, ""))
    } else {
        ("docker.io", name)
    };
    let registry = match registry {
        "index.docker.io" | "registry-1.docker.io" => "docker.io",
        other => other,
    };
    if registry == "docker.io" && !repository.contains('/') {
        format!("docker.io/library/{repository}")
    } else {
        format!("{registry}/{repository}")
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::fs::File;
    use std::io;
    use std::path::Path;
    use std::process::ExitStatus;

    use super::*;

    const BASE: &str = "example.test/base:latest";
    const ADDON: &str = "example.test/addon:latest";

    struct FakeRunner {
        manifest: Option<Value>,
        raw_manifest: Option<Value>,
        selected_ref: String,
        selected_digest: String,
        selected_available: Cell<bool>,
        libc_marker: Option<&'static str>,
        commands: RefCell<Vec<Vec<String>>>,
        built_containerfile: RefCell<Option<String>>,
    }

    impl FakeRunner {
        fn new(manifest: Option<Value>, digest: &str, selected_available: bool) -> Self {
            Self {
                manifest,
                raw_manifest: None,
                selected_ref: containerfile::pin_image_reference(ADDON, digest).unwrap(),
                selected_digest: digest.to_owned(),
                selected_available: Cell::new(selected_available),
                libc_marker: None,
                commands: RefCell::new(Vec::new()),
                built_containerfile: RefCell::new(None),
            }
        }

        fn commands(&self) -> Vec<Vec<String>> {
            self.commands.borrow().clone()
        }

        fn output_for(&self, args: &[String]) -> Output {
            if args[1..3] == ["manifest", "inspect"] {
                return match &self.manifest {
                    Some(manifest) => output(manifest.clone(), 0),
                    None => output(json!(null), 1),
                };
            }
            if args[1..3] == ["buildx", "imagetools"] {
                return match &self.raw_manifest {
                    Some(manifest) => output(manifest.clone(), 0),
                    None => output(json!(null), 1),
                };
            }
            if args[1..3] == ["image", "inspect"] && args[3] == BASE {
                return output(
                    json!([{"Id": "a".repeat(64), "Os": "linux", "Architecture": "arm64"}]),
                    0,
                );
            }
            if args[1..3] == ["image", "inspect"]
                && (args[3] == self.selected_ref || args[3] == ADDON)
                && self.selected_available.get()
            {
                return output(
                    json!([{
                        "Digest": self.selected_digest,
                        "Os": "linux",
                        "Architecture": "arm64",
                        "Config": {"Env": ["ADDON_TEST=selected"]},
                        "RepoDigests": [
                            format!("example.test/other@sha256:{}", "e".repeat(64)),
                            self.selected_ref
                        ]
                    }]),
                    0,
                );
            }
            output(json!(null), 1)
        }
    }

    impl Runner for FakeRunner {
        fn run(&self, cmd: &mut Command) -> io::Result<ExitStatus> {
            let args = command_args(cmd);
            if args[1] == "pull" {
                self.selected_available.set(true);
            } else if args[1] == "build" {
                let path = &args[args.iter().position(|part| part == "-f").unwrap() + 1];
                *self.built_containerfile.borrow_mut() = Some(std::fs::read_to_string(path)?);
            } else if args[1] == "export" {
                let mut archive = tar::Builder::new(File::create(Path::new(&args[3]))?);
                if let Some(path) = self.libc_marker {
                    let mut header = tar::Header::new_gnu();
                    header.set_size(0);
                    header.set_mode(0o644);
                    header.set_cksum();
                    archive.append_data(&mut header, path, io::empty())?;
                }
                archive.finish()?;
            }
            self.commands.borrow_mut().push(args);
            Ok(exit_with(0))
        }

        fn output(&self, cmd: &mut Command) -> io::Result<Output> {
            let args = command_args(cmd);
            let result = self.output_for(&args);
            self.commands.borrow_mut().push(args);
            Ok(result)
        }
    }

    fn command_args(cmd: &Command) -> Vec<String> {
        std::iter::once(cmd.get_program())
            .chain(cmd.get_args())
            .map(|part| part.to_string_lossy().into_owned())
            .collect()
    }

    fn exit_with(code: i32) -> ExitStatus {
        Command::new("sh")
            .args(["-c", &format!("exit {code}")])
            .status()
            .unwrap()
    }

    fn output(value: Value, code: i32) -> Output {
        Output {
            status: exit_with(code),
            stdout: serde_json::to_vec(&value).unwrap(),
            stderr: Vec::new(),
        }
    }

    fn index(entries: &[(&str, &str, &str)]) -> Value {
        let manifests: Vec<_> = entries
            .iter()
            .map(|(architecture, libc, digest)| {
                json!({
                    "mediaType": "application/vnd.oci.image.manifest.v1+json",
                    "digest": digest,
                    "platform": {"os": "linux", "architecture": architecture},
                    "annotations": {"ai.openkaiden.openshell-build-image.libc": libc}
                })
            })
            .collect();
        json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.image.index.v1+json",
            "manifests": manifests
        })
    }

    #[test]
    fn podman_selects_multi_libc_once_for_repeated_addon() {
        let glibc = format!("sha256:{}", "b".repeat(64));
        let musl = format!("sha256:{}", "c".repeat(64));
        let mut runner = FakeRunner::new(
            Some(index(&[
                ("arm64", "glibc", &glibc),
                ("arm64", "musl", &musl),
            ])),
            &musl,
            true,
        );
        runner.libc_marker = Some("lib/ld-musl-aarch64.so.1");
        let images = vec![ADDON.to_owned(), ADDON.to_owned()];

        let prepared = resolve(BASE, &images, &ContainerCli::Podman, &runner).unwrap();
        let file = containerfile::generate(&prepared.base, false, &prepared.addons).unwrap();
        assert_eq!(
            file.matches(&format!("COPY --from={} / /", runner.selected_ref))
                .count(),
            2
        );
        assert_eq!(file.matches("ENV ADDON_TEST=\"selected\"").count(), 2);
        let commands = runner.commands();
        assert_eq!(
            commands.iter().filter(|args| args[1] == "manifest").count(),
            1
        );
        assert_eq!(
            commands.iter().filter(|args| args[1] == "create").count(),
            1
        );
        assert_eq!(
            commands.iter().filter(|args| args[1] == "export").count(),
            1
        );
        assert_eq!(commands.iter().filter(|args| args[1] == "pull").count(), 0);
    }

    #[test]
    fn podman_backend_keeps_repeated_addons_across_trait_boundary() {
        use crate::backend::{BuildRequest, BuildResult, ImageBackend, PodmanBackend};

        let glibc = format!("sha256:{}", "b".repeat(64));
        let musl = format!("sha256:{}", "c".repeat(64));
        let mut runner = FakeRunner::new(
            Some(index(&[
                ("arm64", "glibc", &glibc),
                ("arm64", "musl", &musl),
            ])),
            &musl,
            true,
        );
        runner.libc_marker = Some("lib/ld-musl-aarch64.so.1");
        let addons = vec![ADDON.to_owned(), ADDON.to_owned()];
        let backend = PodmanBackend { runner: &runner };
        let result = backend
            .build(&BuildRequest {
                from: BASE,
                addons: &addons,
                tag: "test:latest",
                copy_containerfile: false,
            })
            .unwrap();
        assert_eq!(
            result,
            BuildResult::Image {
                tag: "test:latest".to_owned()
            }
        );
        let file = runner.built_containerfile.borrow();
        let file = file.as_deref().expect("build should read Containerfile");
        let copy = format!("COPY --from={} / /", runner.selected_ref);
        assert_eq!(file.matches(&copy).count(), 2);
        assert_eq!(file.matches("ENV ADDON_TEST=\"selected\"").count(), 2);
        assert!(file.find(&copy).unwrap() < file.rfind(&copy).unwrap());
        let commands = runner.commands();
        assert_eq!(
            commands.iter().filter(|args| args[1] == "create").count(),
            1
        );
        assert_eq!(commands.iter().filter(|args| args[1] == "build").count(), 1);
    }

    #[test]
    fn docker_pulls_only_selected_platform_from_single_libc_index() {
        let amd64 = format!("sha256:{}", "b".repeat(64));
        let arm64 = format!("sha256:{}", "c".repeat(64));
        let runner = FakeRunner::new(
            Some(index(&[
                ("amd64", "musl", &amd64),
                ("arm64", "musl", &arm64),
            ])),
            &arm64,
            false,
        );

        let prepared = resolve(BASE, &[ADDON.to_owned()], &ContainerCli::Docker, &runner).unwrap();
        let file = containerfile::generate(&prepared.base, false, &prepared.addons).unwrap();
        assert!(file.contains(&format!("COPY --from={} / /", runner.selected_ref)));
        let commands = runner.commands();
        assert!(commands.contains(&vec!["docker".into(), "pull".into(), runner.selected_ref]));
        assert!(!commands.iter().any(|args| args[1] == "create"));
        assert!(
            !commands
                .iter()
                .any(|args| args.iter().any(|arg| arg.contains(&amd64)))
        );
    }

    #[test]
    fn docker_reads_raw_index_when_manifest_inspect_loses_libc_annotations() {
        let glibc = format!("sha256:{}", "b".repeat(64));
        let musl = format!("sha256:{}", "c".repeat(64));
        let raw = index(&[("arm64", "glibc", &glibc), ("arm64", "musl", &musl)]);
        let mut stripped = raw.clone();
        for descriptor in stripped["manifests"].as_array_mut().unwrap() {
            descriptor.as_object_mut().unwrap().remove("annotations");
        }
        let mut runner = FakeRunner::new(Some(stripped), &musl, true);
        runner.raw_manifest = Some(raw);
        runner.libc_marker = Some("lib/ld-musl-aarch64.so.1");

        let prepared = resolve(BASE, &[ADDON.to_owned()], &ContainerCli::Docker, &runner).unwrap();
        let file = containerfile::generate(&prepared.base, false, &prepared.addons).unwrap();
        assert!(file.contains(&format!("COPY --from={} / /", runner.selected_ref)));
        assert!(runner.commands().contains(&vec![
            "docker".into(),
            "buildx".into(),
            "imagetools".into(),
            "inspect".into(),
            "--raw".into(),
            ADDON.into(),
        ]));
        assert_eq!(
            runner
                .commands()
                .iter()
                .filter(|args| args[1] == "export")
                .count(),
            1
        );
    }

    #[test]
    fn podman_uses_local_single_image_when_manifest_registry_is_unavailable() {
        let digest = format!("sha256:{}", "d".repeat(64));
        let runner = FakeRunner::new(None, &digest, true);

        let prepared = resolve(BASE, &[ADDON.to_owned()], &ContainerCli::Podman, &runner).unwrap();
        let file = containerfile::generate(&prepared.base, false, &prepared.addons).unwrap();
        assert!(file.contains(&format!("COPY --from={} / /", runner.selected_ref)));
        assert!(!runner.commands().iter().any(|args| args[1] == "pull"));
    }

    #[test]
    fn docker_single_image_uses_digest_for_requested_repository() {
        let digest = format!("sha256:{}", "d".repeat(64));
        let runner = FakeRunner::new(None, &digest, true);

        let prepared = resolve(BASE, &[ADDON.to_owned()], &ContainerCli::Docker, &runner).unwrap();
        let file = containerfile::generate(&prepared.base, false, &prepared.addons).unwrap();
        assert!(file.contains(&format!("COPY --from={} / /", runner.selected_ref)));
        assert!(!file.contains("example.test/other"));
        assert_eq!(
            normalized_repository("alpine:3.24"),
            "docker.io/library/alpine"
        );
        assert_eq!(
            normalized_repository("index.docker.io/library/alpine@sha256:abc"),
            "docker.io/library/alpine"
        );
    }

    #[test]
    fn docker_rejects_http_only_ipv6_loopback_before_pull() {
        let digest = format!("sha256:{}", "d".repeat(64));
        let runner = FakeRunner::new(None, &digest, false);
        let image = "[::1]:5000/addon:latest";

        let reason = resolve(BASE, &[image.to_owned()], &ContainerCli::Docker, &runner)
            .err()
            .expect("manifest inspection must reject an unavailable IPv6 registry");
        assert!(
            reason
                .to_string()
                .contains("require a registry reachable over HTTPS")
        );
        let commands = runner.commands();
        assert!(commands.contains(&vec![
            "docker".into(),
            "manifest".into(),
            "inspect".into(),
            image.into(),
        ]));
        assert!(!commands.iter().any(|args| args[1] == "pull"));
        assert!(
            !commands
                .iter()
                .any(|args| args.contains(&"--insecure".into()))
        );
    }

    #[test]
    fn docker_build_base_tags_the_inspected_image_and_removes_the_tag() {
        let digest = format!("sha256:{}", "d".repeat(64));
        let runner = FakeRunner::new(None, &digest, true);
        let id = format!("sha256:{}", "a".repeat(64));

        let mut base = DockerBuildBase::new(BASE, &id, &runner).unwrap();
        let reference = base.reference().to_owned();
        assert!(reference.starts_with("osbi-build-base:"));
        assert_eq!(
            runner.commands(),
            vec![vec!["docker", "image", "tag", &id, &reference]]
        );
        base.cleanup().unwrap();
        assert_eq!(
            runner.commands().last().unwrap(),
            &["docker", "image", "rm", &reference]
        );
    }

    #[test]
    fn docker_digest_base_needs_no_temporary_tag() {
        let digest = format!("sha256:{}", "d".repeat(64));
        let runner = FakeRunner::new(None, &digest, true);
        let requested = format!("example.test/base@sha256:{}", "a".repeat(64));
        let mut base = DockerBuildBase::new(&requested, &digest, &runner).unwrap();
        assert_eq!(base.reference(), requested);
        base.cleanup().unwrap();
        assert!(runner.commands().is_empty());

        let error = DockerBuildBase::new(&digest, &digest, &runner)
            .err()
            .expect("a bare ID cannot be used as a Docker base");
        assert!(error.to_string().contains("bare image ID"));
    }
}
