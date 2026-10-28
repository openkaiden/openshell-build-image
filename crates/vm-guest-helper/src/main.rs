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

//! Runs inside the build VM, where Buildah and Skopeo share one image store.

use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::path::Path;
use std::process::Command;

use addon_rules::addon::{AddonCatalog, Platform};
use addon_rules::containerfile::{self, ResolvedAddon};
use addon_rules::libc::{Libc, libc_marker};
use serde_json::Value;
use sha2::{Digest, Sha256};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

trait Engine {
    fn pull(&self, image: &str) -> Result<()>;
    fn raw_manifest(&self, image: &str) -> Result<Vec<u8>>;
    fn remote_config(&self, image: &str) -> Result<String>;
    fn local_config(&self, image: &str) -> Result<String>;
    fn base_libc(&self, image: &str) -> Result<Libc>;
}

struct GuestEngine;

fn output(program: &str, arguments: &[&str]) -> Result<Vec<u8>> {
    let result = Command::new(program).args(arguments).output()?;
    if !result.status.success() {
        return Err(format!(
            "{} {} failed: {}",
            program,
            arguments.join(" "),
            String::from_utf8_lossy(&result.stderr).trim()
        )
        .into());
    }
    Ok(result.stdout)
}

fn output_text(program: &str, arguments: &[&str]) -> Result<String> {
    Ok(String::from_utf8(output(program, arguments)?)?)
}

impl Engine for GuestEngine {
    fn pull(&self, image: &str) -> Result<()> {
        output("buildah", &["pull", "--quiet", image])?;
        Ok(())
    }

    fn raw_manifest(&self, image: &str) -> Result<Vec<u8>> {
        output(
            "skopeo",
            &["inspect", "--raw", &format!("docker://{image}")],
        )
    }

    fn remote_config(&self, image: &str) -> Result<String> {
        output_text(
            "skopeo",
            &["inspect", "--config", &format!("docker://{image}")],
        )
    }

    fn local_config(&self, image: &str) -> Result<String> {
        output_text(
            "skopeo",
            &[
                "inspect",
                "--config",
                &format!("containers-storage:{image}"),
            ],
        )
    }

    fn base_libc(&self, image: &str) -> Result<Libc> {
        let container = output_text("buildah", &["from", "--pull=never", image])?;
        let container = container.trim();
        let result = (|| {
            let mount = output_text("buildah", &["mount", container])?;
            detect_libc(Path::new(mount.trim()))
        })();
        let umount = output("buildah", &["umount", container]);
        let remove = output("buildah", &["rm", container]);
        result.and_then(|libc| {
            umount?;
            remove?;
            Ok(libc)
        })
    }
}

fn detect_libc(root: &Path) -> Result<Libc> {
    let mut glibc = false;
    let mut musl = false;
    for directory in ["lib", "lib64", "usr/lib", "usr/lib64"] {
        scan_library_dir(root, Path::new(directory), &mut glibc, &mut musl)?;
    }
    // Prefer musl when both loaders are present. This is a selection policy
    // for mixed-libc images, not a claim about which libc came first.
    match (glibc, musl) {
        (true, false) => Ok(Libc::Glibc),
        (_, true) => Ok(Libc::Musl),
        (false, false) => Err(format!(
            "base libc is unclear (glibc markers: {glibc}, musl markers: {musl})"
        )
        .into()),
    }
}

fn scan_library_dir(root: &Path, relative: &Path, glibc: &mut bool, musl: &mut bool) -> Result<()> {
    let directory = root.join(relative);
    if !directory.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = relative.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_dir() {
            scan_library_dir(root, &path, glibc, musl)?;
        } else if kind.is_file() || kind.is_symlink() {
            match libc_marker(&path) {
                Some(Libc::Glibc) => *glibc = true,
                Some(Libc::Musl) => *musl = true,
                None => {}
            }
        }
    }
    Ok(())
}

fn image_platform(config: &str) -> Result<Platform> {
    let value: Value = serde_json::from_str(config)?;
    let os = value
        .get("os")
        .and_then(Value::as_str)
        .ok_or("image config has no os")?;
    let architecture = value
        .get("architecture")
        .and_then(Value::as_str)
        .ok_or("image config has no architecture")?;
    Ok(Platform::new(
        os,
        architecture,
        value.get("variant").and_then(Value::as_str),
    ))
}

/// Buildah prefixes locally tagged results with `localhost/`. Skopeo would
/// otherwise interpret a short name as a Docker Hub repository.
fn local_result_name(tag: &str) -> String {
    let first = tag.split('/').next().unwrap_or(tag);
    if tag.contains('/') && (first == "localhost" || first.contains('.') || first.contains(':')) {
        tag.to_owned()
    } else {
        format!("localhost/{tag}")
    }
}

fn catalog(engine: &dyn Engine, image: &str) -> Result<AddonCatalog> {
    let raw = engine.raw_manifest(image)?;
    let document: Value = serde_json::from_slice(&raw)?;
    let text = std::str::from_utf8(&raw)?;
    if document.get("manifests").is_some() {
        Ok(AddonCatalog::parse_index(text)?)
    } else {
        let digest = format!("sha256:{:x}", Sha256::digest(&raw));
        let config = engine.remote_config(image)?;
        Ok(AddonCatalog::parse_single(text, &config, &digest)?)
    }
}

fn prepare(engine: &dyn Engine, request: &Value) -> Result<Option<String>> {
    if request.get("version").and_then(Value::as_u64) != Some(1) {
        return Err("unsupported VM build request version".into());
    }
    let from = request
        .get("from")
        .and_then(Value::as_str)
        .ok_or("missing from")?;
    let addons = request
        .get("addons")
        .and_then(Value::as_array)
        .ok_or("missing addons array")?;
    if addons.is_empty() {
        return Ok(None);
    }
    containerfile::parse_image_reference(from)?;
    let images: Vec<&str> = addons
        .iter()
        .map(|value| value.as_str().ok_or("addon must be a string"))
        .collect::<std::result::Result<_, _>>()?;
    let mut catalogs = HashMap::new();
    for image in &images {
        containerfile::parse_image_reference(image)?;
        if !catalogs.contains_key(*image) {
            catalogs.insert(
                *image,
                catalog(engine, image).map_err(|reason| format!("addon {image:?}: {reason}"))?,
            );
        }
    }
    let needs_libc = catalogs.values().any(AddonCatalog::requires_base_libc);
    engine.pull(from)?;
    let platform = image_platform(&engine.local_config(from)?)?;
    let libc = if needs_libc {
        Some(engine.base_libc(from)?)
    } else {
        None
    };
    let mut configs = HashMap::new();
    let mut resolved = Vec::with_capacity(images.len());
    for image in images {
        let variant = catalogs[image]
            .select(&platform, libc.map(Libc::as_str))
            .map_err(|reason| format!("addon {image:?}: {reason}"))?;
        let pinned = containerfile::pin_image_reference(image, &variant.digest)?;
        let config = if let Some(config) = configs.get(&pinned) {
            config
        } else {
            engine.pull(&pinned)?;
            let config = engine.local_config(&pinned)?;
            let selected_platform = image_platform(&config)?;
            if selected_platform.os != variant.platform.os
                || selected_platform.architecture != variant.platform.architecture
            {
                return Err(format!(
                    "selected image platform {selected_platform} differs from descriptor {}",
                    variant.platform
                )
                .into());
            }
            configs.insert(pinned.clone(), config);
            &configs[&pinned]
        };
        resolved.push(
            ResolvedAddon::from_variant(image, variant, config)
                .map_err(|reason| format!("addon {image:?}: {reason}"))?,
        );
    }
    let copy = request
        .get("copy_containerfile")
        .and_then(Value::as_bool)
        .ok_or("missing copy_containerfile")?;
    Ok(Some(containerfile::generate(from, copy, &resolved)?))
}

fn main_inner() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    match args.as_slice() {
        [_, command, request_file, containerfile] if command == "prepare" => {
            let request: Value = serde_json::from_slice(&fs::read(request_file)?)?;
            if let Some(content) = prepare(&GuestEngine, &request)? {
                fs::write(containerfile, &content)?;
                let context = Path::new(containerfile)
                    .parent()
                    .ok_or("no context directory")?;
                if request.get("copy_containerfile").and_then(Value::as_bool) == Some(true) {
                    fs::write(context.join("build-containerfile"), &content)?;
                }
                fs::write(context.join(".vm-addons-enabled"), [])?;
            }
            Ok(())
        }
        [_, command, tag, output_path] if command == "config" => {
            let content = GuestEngine.local_config(&local_result_name(tag))?;
            let _: Value = serde_json::from_str(&content)?;
            fs::write(output_path, content)?;
            Ok(())
        }
        _ => Err("usage: vm-guest-helper prepare REQUEST CONTAINERFILE | config TAG OUTPUT".into()),
    }
}

fn main() {
    if let Err(error) = main_inner() {
        eprintln!("vm-guest-helper: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    const SINGLE: &str = "example.test/single:latest";
    const INDEX: &str = "example.test/index:latest";

    struct FakeEngine {
        pulls: RefCell<Vec<String>>,
        probes: Cell<usize>,
        libc: Libc,
        index: String,
    }

    impl FakeEngine {
        fn new(multiple_libc: bool) -> Self {
            let descriptors: Vec<Value> = if multiple_libc {
                ["glibc", "musl"]
                    .iter()
                    .enumerate()
                    .map(|(index, libc)| {
                        serde_json::json!({
                            "mediaType": "application/vnd.oci.image.manifest.v1+json",
                            "digest": format!("sha256:{}", if index == 0 { "a" } else { "b" }.repeat(64)),
                            "platform": {"os": "linux", "architecture": "arm64"},
                            "annotations": {addon_rules::addon::LIBC_ANNOTATION: libc}
                        })
                    })
                    .collect()
            } else {
                vec![serde_json::json!({
                    "mediaType": "application/vnd.oci.image.manifest.v1+json",
                    "digest": format!("sha256:{}", "c".repeat(64)),
                    "platform": {"os": "linux", "architecture": "arm64"}
                })]
            };
            Self {
                pulls: RefCell::new(Vec::new()),
                probes: Cell::new(0),
                libc: Libc::Musl,
                index: serde_json::json!({
                    "schemaVersion": 2,
                    "mediaType": "application/vnd.oci.image.index.v1+json",
                    "manifests": descriptors
                })
                .to_string(),
            }
        }
    }

    impl Engine for FakeEngine {
        fn pull(&self, image: &str) -> Result<()> {
            self.pulls.borrow_mut().push(image.to_owned());
            Ok(())
        }

        fn raw_manifest(&self, image: &str) -> Result<Vec<u8>> {
            if image == SINGLE {
                Ok(serde_json::json!({
                    "schemaVersion": 2,
                    "mediaType": "application/vnd.oci.image.manifest.v1+json",
                    "config": {"digest": format!("sha256:{}", "d".repeat(64))},
                    "layers": []
                })
                .to_string()
                .into_bytes())
            } else {
                Ok(self.index.as_bytes().to_vec())
            }
        }

        fn remote_config(&self, _image: &str) -> Result<String> {
            Ok(serde_json::json!({"os": "linux", "architecture": "arm64"}).to_string())
        }

        fn local_config(&self, image: &str) -> Result<String> {
            if image == "example.test/base:latest" {
                Ok(serde_json::json!({"os": "linux", "architecture": "arm64"}).to_string())
            } else {
                let variant = if image.contains(&"b".repeat(64)) {
                    "musl"
                } else if image.contains(&"a".repeat(64)) {
                    "glibc"
                } else {
                    "single"
                };
                Ok(serde_json::json!({
                    "os": "linux", "architecture": "arm64",
                    "config": {"Env": [format!("OSBI_VARIANT={variant}")]}
                })
                .to_string())
            }
        }

        fn base_libc(&self, _image: &str) -> Result<Libc> {
            self.probes.set(self.probes.get() + 1);
            Ok(self.libc)
        }
    }

    fn request(addons: &[&str]) -> Value {
        serde_json::json!({
            "version": 1,
            "from": "example.test/base:latest",
            "addons": addons,
            "tag": "result:latest",
            "copy_containerfile": true
        })
    }

    #[test]
    fn repeated_addons_keep_order_and_pull_selected_digest_once() {
        let engine = FakeEngine::new(true);
        let file = prepare(&engine, &request(&[INDEX, SINGLE, INDEX]))
            .unwrap()
            .unwrap();
        let selected = format!("example.test/index@sha256:{}", "b".repeat(64));
        assert_eq!(
            file.matches(&format!("COPY --from={selected} / /")).count(),
            2
        );
        assert_eq!(file.matches("ENV OSBI_VARIANT=\"musl\"").count(), 2);
        assert!(
            file.find(&selected).unwrap() < file.find("COPY --from=example.test/single@").unwrap()
        );
        assert_eq!(engine.probes.get(), 1);
        assert_eq!(engine.pulls.borrow().len(), 3); // base and two distinct payloads
        assert_eq!(engine.pulls.borrow()[1], selected);
        assert!(file.contains("COPY build-containerfile"));
    }

    #[test]
    fn one_platform_variant_needs_no_libc_probe() {
        let engine = FakeEngine::new(false);
        let file = prepare(&engine, &request(&[INDEX])).unwrap().unwrap();
        assert!(file.contains(&format!("example.test/index@sha256:{}", "c".repeat(64))));
        assert_eq!(engine.probes.get(), 0);
    }

    #[test]
    fn wrong_platform_is_rejected_before_pulling_an_addon() {
        let engine = FakeEngine::new(false);
        let mut index: Value = serde_json::from_str(&engine.index).unwrap();
        index["manifests"][0]["platform"]["architecture"] = Value::from("amd64");
        let engine = FakeEngine {
            index: index.to_string(),
            ..engine
        };
        let error = prepare(&engine, &request(&[INDEX])).unwrap_err();
        assert!(error.to_string().contains("no addon variant matches"));
        assert_eq!(
            engine.pulls.borrow().as_slice(),
            ["example.test/base:latest"]
        );
    }

    #[test]
    fn no_addons_leave_the_existing_containerfile_path_alone() {
        let engine = FakeEngine::new(true);
        assert!(prepare(&engine, &request(&[])).unwrap().is_none());
        assert!(engine.pulls.borrow().is_empty());
    }

    #[test]
    fn mounted_base_prefers_musl_when_both_loaders_exist() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("usr/lib/aarch64-linux-gnu")).unwrap();
        fs::write(
            root.path()
                .join("usr/lib/aarch64-linux-gnu/ld-linux-aarch64.so.1"),
            [],
        )
        .unwrap();
        assert_eq!(detect_libc(root.path()).unwrap(), Libc::Glibc);
        fs::create_dir_all(root.path().join("lib")).unwrap();
        fs::write(root.path().join("lib/ld-musl-aarch64.so.1"), []).unwrap();
        assert_eq!(detect_libc(root.path()).unwrap(), Libc::Musl);
    }

    #[test]
    fn mounted_base_without_loaders_has_unclear_libc() {
        let root = tempfile::tempdir().unwrap();
        assert!(
            detect_libc(root.path())
                .unwrap_err()
                .to_string()
                .contains("unclear")
        );
    }

    #[test]
    fn built_tags_are_looked_up_under_buildahs_local_namespace() {
        assert_eq!(
            local_result_name("result:latest"),
            "localhost/result:latest"
        );
        assert_eq!(
            local_result_name("team/result:latest"),
            "localhost/team/result:latest"
        );
        assert_eq!(
            local_result_name("ghcr.io/team/result:latest"),
            "ghcr.io/team/result:latest"
        );
        assert_eq!(
            local_result_name("localhost/result:latest"),
            "localhost/result:latest"
        );
        assert_eq!(local_result_name("result.test"), "localhost/result.test");
    }
}
