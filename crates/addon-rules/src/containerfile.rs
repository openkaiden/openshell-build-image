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

use crate::addon::AddonVariant;
use serde_json::Value;

#[derive(Debug, PartialEq)]
pub enum ContainerfileError {
    ImageReference { image: String },
    ManifestDigest { digest: String },
    ImageConfig { reason: String },
    EnvironmentName { name: String },
    EnvironmentValue { name: String },
}

impl std::fmt::Display for ContainerfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContainerfileError::ImageReference { image } => write!(
                f,
                "invalid image reference {image:?}: expected a nonempty OCI image reference"
            ),
            ContainerfileError::ManifestDigest { digest } => {
                write!(f, "invalid selected addon manifest digest {digest:?}")
            }
            ContainerfileError::ImageConfig { reason } => {
                write!(f, "invalid addon image config: {reason}")
            }
            ContainerfileError::EnvironmentName { name } => {
                write!(f, "invalid addon environment variable name {name:?}")
            }
            ContainerfileError::EnvironmentValue { name } => {
                write!(
                    f,
                    "addon environment variable {name:?} contains a control character"
                )
            }
        }
    }
}

impl std::error::Error for ContainerfileError {}

/// Validate the reference before inserting it into a FROM or COPY instruction.
/// The build engine validates the registry, repository, tag, and digest syntax.
pub fn parse_image_reference(image: &str) -> Result<String, ContainerfileError> {
    if image.is_empty()
        || image.starts_with('-')
        || !image
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-/:@[]+".contains(&byte))
    {
        return Err(ContainerfileError::ImageReference {
            image: image.to_string(),
        });
    }
    Ok(image.to_string())
}

/// A selected manifest and its environment, ready for Containerfile output.
pub struct ResolvedAddon {
    image: String,
    environment: Vec<(String, String)>,
}

impl ResolvedAddon {
    /// Pin the selected manifest and read `config.Env` from its raw OCI config JSON.
    pub fn from_variant(
        requested_image: &str,
        variant: &AddonVariant,
        config_json: &str,
    ) -> Result<Self, ContainerfileError> {
        let image = pin_image_reference(requested_image, &variant.digest)?;
        let environment = parse_environment(config_json)?;
        Ok(Self { image, environment })
    }
}

pub fn pin_image_reference(image: &str, digest: &str) -> Result<String, ContainerfileError> {
    parse_image_reference(image)?;
    let valid_digest = match digest.split_once(':') {
        Some(("sha256", hex)) => {
            hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
        }
        Some(("sha512", hex)) => {
            hex.len() == 128 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
        }
        _ => false,
    };
    if !valid_digest {
        return Err(ContainerfileError::ManifestDigest {
            digest: digest.to_owned(),
        });
    }
    let unpinned = image.split_once('@').map_or(image, |(name, _)| name);
    let last_component = unpinned.rsplit('/').next().unwrap_or(unpinned);
    let repository = last_component.find(':').map_or(unpinned, |offset| {
        &unpinned[..unpinned.len() - last_component.len() + offset]
    });
    if repository.is_empty() {
        return Err(ContainerfileError::ImageReference {
            image: image.to_owned(),
        });
    }
    Ok(format!("{repository}@{digest}"))
}

fn parse_environment(config_json: &str) -> Result<Vec<(String, String)>, ContainerfileError> {
    let config: Value =
        serde_json::from_str(config_json).map_err(|error| ContainerfileError::ImageConfig {
            reason: format!("invalid JSON: {error}"),
        })?;
    if !config.is_object() {
        return Err(invalid_image_config("image config must be an object"));
    }
    let environment = match config.get("config") {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(Value::Object(config)) => match config.get("Env") {
            None | Some(Value::Null) => return Ok(Vec::new()),
            Some(Value::Array(environment)) => environment,
            _ => return Err(invalid_image_config("config.Env must be an array")),
        },
        _ => return Err(invalid_image_config("config must be an object")),
    };
    environment
        .iter()
        .map(|entry| {
            let entry = entry
                .as_str()
                .ok_or_else(|| invalid_image_config("config.Env entries must be strings"))?;
            let (name, value) = entry
                .split_once('=')
                .ok_or_else(|| invalid_image_config("config.Env entries must contain '='"))?;
            validate_environment(name, value)?;
            Ok((name.to_owned(), value.to_owned()))
        })
        .collect()
}

fn invalid_image_config(reason: &str) -> ContainerfileError {
    ContainerfileError::ImageConfig {
        reason: reason.to_owned(),
    }
}

fn validate_environment(name: &str, value: &str) -> Result<(), ContainerfileError> {
    let mut bytes = name.bytes();
    let valid_first = bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_');
    if !valid_first || !bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_') {
        return Err(ContainerfileError::EnvironmentName {
            name: name.to_owned(),
        });
    }
    if value.chars().any(char::is_control) {
        return Err(ContainerfileError::EnvironmentValue {
            name: name.to_owned(),
        });
    }
    Ok(())
}

fn escape_environment_value(value: &str, append_to_path: bool) -> String {
    let mut escaped = String::with_capacity(value.len() + 2);
    escaped.push('"');
    if append_to_path {
        // Expand the previous stage's PATH, but keep dollars in the addon value literal.
        escaped.push_str("$PATH:");
    }
    for character in value.chars() {
        if matches!(character, '\\' | '"' | '$') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped.push('"');
    escaped
}

pub fn generate(
    from: &str,
    copy_containerfile: bool,
    addons: &[ResolvedAddon],
) -> Result<String, ContainerfileError> {
    let image = parse_image_reference(from)?;
    Ok(format!(
        "# System base\nFROM {image} AS system\n\nUSER 0\n\n{}",
        final_stage(copy_containerfile, addons)
    ))
}

fn final_stage(copy_containerfile: bool, addons: &[ResolvedAddon]) -> String {
    let mut output = String::from("# Final base image\nFROM system AS final\n\n");
    for addon in addons {
        output.push_str("COPY --from=");
        output.push_str(&addon.image);
        output.push_str(" / /\n");
        for (name, value) in &addon.environment {
            output.push_str("ENV ");
            output.push_str(name);
            output.push('=');
            output.push_str(&escape_environment_value(value, name == "PATH"));
            output.push('\n');
        }
        output.push('\n');
    }
    if copy_containerfile {
        output.push_str(
            "COPY build-containerfile /tmp/build-containerfile\n\
             RUN cp /tmp/build-containerfile \"$HOME/Containerfile\" && rm /tmp/build-containerfile\n\n",
        );
    }
    output.push('\n');
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::addon::Platform;
    use serde_json::json;

    fn build_cf(from: &str) -> Result<String, ContainerfileError> {
        generate(from, false, &[])
    }

    fn resolved_addon(image: &str, digest_character: char, environment: &[&str]) -> ResolvedAddon {
        let variant = AddonVariant {
            platform: Platform::new("linux", "amd64", None),
            libc: None,
            digest: format!("sha256:{}", digest_character.to_string().repeat(64)),
        };
        let config = json!({"config": {"Env": environment}}).to_string();
        ResolvedAddon::from_variant(image, &variant, &config).unwrap()
    }

    #[test]
    fn arbitrary_image_references_are_preserved() {
        for image in [
            "alpine",
            "docker.io/library/alpine:3.24",
            "registry.fedoraproject.org/fedora:latest",
            "localhost:5000/project/tools:dev",
            "ghcr.io/example/project@sha256:0123456789abcdef",
            "example/project:tag@sha256:0123456789abcdef",
        ] {
            let content = build_cf(image).unwrap();
            assert!(content.contains(&format!("FROM {image} AS system\n")));
            assert!(
                !content.contains("RUN "),
                "unexpected build command: {content}"
            );
        }
    }

    #[test]
    fn invalid_references_cannot_change_containerfile_instructions() {
        for image in [
            "",
            " ",
            "alpine AS injected",
            "alpine\nRUN touch /injected",
            "alpine\r",
            "alpine\t",
            "alpine\0",
            "alpine#comment",
            "alpine\\",
            "alpine$OTHER",
            "-addon",
        ] {
            let err = build_cf(image).unwrap_err();
            assert!(matches!(err, ContainerfileError::ImageReference { .. }));
            assert!(err.to_string().contains("invalid image reference"));
        }
    }

    #[test]
    fn resolved_addons_are_copied_and_configured_in_argument_order() {
        let first = resolved_addon(
            "registry.example:5000/first:latest",
            'a',
            &[
                "SHARED=first",
                "FIRST=yes",
                "PATH=/usr/local/bin",
                r#"QUOTE=he said "hello"\backslash"#,
                "EMPTY=",
                "TOKEN=a=b=c",
            ],
        );
        let second = resolved_addon(
            &format!("registry.example/second@sha256:{}", "c".repeat(64)),
            'b',
            &["SHARED=second", "SECOND=yes", "PATH=/opt/second bin:$PATH"],
        );
        let content = generate("alpine:3.24", true, &[first, second]).unwrap();
        let first_copy = format!(
            "COPY --from=registry.example:5000/first@sha256:{} / /",
            "a".repeat(64)
        );
        let second_copy = format!(
            "COPY --from=registry.example/second@sha256:{} / /",
            "b".repeat(64)
        );
        let first_position = content.find(&first_copy).unwrap();
        let second_position = content.find(&second_copy).unwrap();
        assert!(content.find("FROM system AS final").unwrap() < first_position);
        assert!(first_position < content.find("ENV SHARED=\"first\"").unwrap());
        assert!(content.find("ENV SHARED=\"first\"").unwrap() < second_position);
        assert!(second_position < content.find("ENV SHARED=\"second\"").unwrap());
        assert!(second_position < content.find("COPY build-containerfile").unwrap());
        let first_path = r#"ENV PATH="$PATH:/usr/local/bin""#;
        let second_path = r#"ENV PATH="$PATH:/opt/second bin:\$PATH""#;
        assert!(content.contains(first_path));
        assert!(content.contains(second_path));
        assert!(content.find(first_path).unwrap() < content.find(second_path).unwrap());
        assert!(content.contains(r#"ENV QUOTE="he said \"hello\"\\backslash""#));
        assert!(content.contains("ENV EMPTY=\"\""));
        assert!(content.contains("ENV TOKEN=\"a=b=c\""));
        assert!(content.contains("ENV FIRST=\"yes\""));
        assert!(content.contains("ENV SECOND=\"yes\""));
        assert!(!content.contains("first:latest"));
    }

    #[test]
    fn selected_manifest_digest_replaces_tag_or_index_digest() {
        for (image, repository) in [
            ("addon", "addon"),
            (
                "localhost:5000/project/addon:latest",
                "localhost:5000/project/addon",
            ),
            (
                "localhost:5000/project/addon",
                "localhost:5000/project/addon",
            ),
            (
                "ghcr.io/example/addon:tag@sha256:0123456789abcdef",
                "ghcr.io/example/addon",
            ),
        ] {
            let addon = resolved_addon(image, 'a', &[]);
            let content = generate("alpine", false, &[addon]).unwrap();
            assert!(content.contains(&format!(
                "COPY --from={repository}@sha256:{} / /",
                "a".repeat(64)
            )));
        }
    }

    #[test]
    fn malformed_addon_config_and_unsafe_environment_are_rejected() {
        let variant = AddonVariant {
            platform: Platform::new("linux", "amd64", None),
            libc: None,
            digest: format!("sha256:{}", "a".repeat(64)),
        };
        for config in [
            "not JSON",
            r#"{"config":{"Env":"KEY=value"}}"#,
            r#"{"config":{"Env":[3]}}"#,
            r#"{"config":{"Env":["NO_EQUALS"]}}"#,
        ] {
            assert!(matches!(
                ResolvedAddon::from_variant("addon", &variant, config),
                Err(ContainerfileError::ImageConfig { .. })
            ));
        }
        for name in ["", "1INVALID", "BAD-NAME", "BAD NAME", "BAD\nNAME"] {
            let config = json!({"config": {"Env": [format!("{name}=value")]}}).to_string();
            assert!(matches!(
                ResolvedAddon::from_variant("addon", &variant, &config),
                Err(ContainerfileError::EnvironmentName { .. })
            ));
        }
        let config = json!({"config": {"Env": ["VALUE=hello\nRUN injected"]}}).to_string();
        assert!(matches!(
            ResolvedAddon::from_variant("addon", &variant, &config),
            Err(ContainerfileError::EnvironmentValue { .. })
        ));
        let invalid_digest = AddonVariant {
            digest: "sha256:not-a-digest".to_owned(),
            ..variant.clone()
        };
        assert!(matches!(
            ResolvedAddon::from_variant("addon", &invalid_digest, "{}"),
            Err(ContainerfileError::ManifestDigest { .. })
        ));
        assert!(matches!(
            ResolvedAddon::from_variant("addon\nRUN injected", &variant, "{}"),
            Err(ContainerfileError::ImageReference { .. })
        ));
    }

    #[test]
    fn addon_without_configured_environment_only_copies_files() {
        let variant = AddonVariant {
            platform: Platform::new("linux", "amd64", None),
            libc: None,
            digest: format!("sha256:{}", "a".repeat(64)),
        };
        let addon = ResolvedAddon::from_variant("addon", &variant, r#"{"config":{}}"#).unwrap();
        let content = generate("alpine", false, &[addon]).unwrap();
        assert!(content.contains("COPY --from=addon@sha256:"));
        assert!(!content.contains("ENV "));
    }

    #[test]
    fn copy_containerfile_is_supported_on_all_base_images() {
        for config in [
            "docker.io/library/ubuntu:24.04",
            "registry.fedoraproject.org/fedora:latest",
            "docker.io/library/alpine:3.24",
            "ghcr.io/example/project:dev",
        ] {
            let content = generate(config, true, &[]).unwrap();
            assert!(content.contains("COPY build-containerfile /tmp/build-containerfile"));
            assert!(content.contains("RUN cp /tmp/build-containerfile \"$HOME/Containerfile\""));
            assert!(!content.contains("--chown=sandbox"));
            let without_copy = build_cf(config).unwrap();
            assert!(!without_copy.contains("build-containerfile"));
        }
    }

    // CA cert tests

    #[test]
    fn host_ca_certificates_are_not_copied() {
        for content in [
            build_cf("docker.io/library/ubuntu:24.04").unwrap(),
            build_cf("registry.fedoraproject.org/fedora:latest").unwrap(),
            build_cf("registry.access.redhat.com/ubi10/ubi:latest").unwrap(),
            build_cf("docker.io/library/alpine:3.24").unwrap(),
        ] {
            assert!(
                !content.contains("COPY certs/"),
                "unexpected cert COPY: {content}"
            );
            assert!(!content.contains("system-ca.crt"));
            assert!(!content.contains("RUN update-ca-certificates"));
            assert!(!content.contains("RUN update-ca-trust"));
        }
    }
}
