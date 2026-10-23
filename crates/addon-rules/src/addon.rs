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

use serde_json::Value;

pub const LIBC_ANNOTATION: &str = "ai.openkaiden.openshell-build-image.libc";
const OCI_INDEX: &str = "application/vnd.oci.image.index.v1+json";
const DOCKER_INDEX: &str = "application/vnd.docker.distribution.manifest.list.v2+json";
const OCI_MANIFEST: &str = "application/vnd.oci.image.manifest.v1+json";
const DOCKER_MANIFEST: &str = "application/vnd.docker.distribution.manifest.v2+json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Platform {
    pub os: String,
    pub architecture: String,
    pub variant: Option<String>,
}

impl Platform {
    pub fn new(os: &str, architecture: &str, variant: Option<&str>) -> Self {
        Self {
            os: os.to_owned(),
            architecture: architecture.to_owned(),
            variant: variant.map(str::to_owned),
        }
    }

    fn matches(&self, candidate: &Self) -> bool {
        self.os == candidate.os
            && self.architecture == candidate.architecture
            && (self.variant.is_none()
                || candidate.variant.is_none()
                || self.variant == candidate.variant)
    }
}

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.os, self.architecture)?;
        if let Some(variant) = &self.variant {
            write!(f, "/{variant}")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddonVariant {
    pub platform: Platform,
    pub libc: Option<String>,
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddonCatalog {
    variants: Vec<AddonVariant>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum AddonError {
    InvalidMetadata(String),
    BaseLibcRequired,
    NoMatchingVariant {
        platform: Platform,
        libc: Option<String>,
    },
    AmbiguousVariant {
        platform: Platform,
        libc: Option<String>,
    },
}

impl std::fmt::Display for AddonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidMetadata(reason) => write!(f, "invalid addon image metadata: {reason}"),
            Self::BaseLibcRequired => {
                write!(
                    f,
                    "addon has multiple libc variants; inspect the base image to select one"
                )
            }
            Self::NoMatchingVariant { platform, libc } => {
                write!(f, "no addon variant matches {platform}")?;
                if let Some(libc) = libc {
                    write!(f, " with libc {libc}")?;
                }
                Ok(())
            }
            Self::AmbiguousVariant { platform, libc } => {
                write!(f, "multiple addon variants match {platform}")?;
                if let Some(libc) = libc {
                    write!(f, " with libc {libc}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for AddonError {}

impl AddonCatalog {
    pub fn variants(&self) -> &[AddonVariant] {
        &self.variants
    }

    pub fn parse_index(json: &str) -> Result<Self, AddonError> {
        let index = parse_json(json)?;
        require_schema_two(&index)?;
        let media_type = required_str(&index, "mediaType")?;
        if !matches!(media_type, OCI_INDEX | DOCKER_INDEX) {
            return Err(invalid(
                "expected an OCI image index or Docker manifest list",
            ));
        }
        let manifests = index
            .get("manifests")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid("index has no manifests array"))?;
        let mut variants = Vec::new();
        for descriptor in manifests {
            let media_type = required_str(descriptor, "mediaType")?;
            if !matches!(media_type, OCI_MANIFEST | DOCKER_MANIFEST) {
                continue;
            }
            let annotations = descriptor.get("annotations");
            if annotations.is_some_and(|value| !value.is_object()) {
                return Err(invalid("descriptor annotations must be an object"));
            }
            if annotations
                .and_then(|value| value.get("vnd.docker.reference.type"))
                .and_then(Value::as_str)
                == Some("attestation-manifest")
            {
                continue;
            }
            let platform = descriptor
                .get("platform")
                .ok_or_else(|| invalid("image descriptor has no platform"))?;
            let platform = parse_platform(platform)?;
            let digest = required_str(descriptor, "digest")?;
            validate_digest(digest)?;
            let libc = annotations
                .and_then(|value| value.get(LIBC_ANNOTATION))
                .map(|value| {
                    value
                        .as_str()
                        .filter(|value| !value.is_empty())
                        .ok_or_else(|| invalid("libc annotation must be a nonempty string"))
                })
                .transpose()?
                .map(str::to_owned);
            variants.push(AddonVariant {
                platform,
                libc,
                digest: digest.to_owned(),
            });
        }
        Self::from_variants(variants)
    }

    // A single image manifest has neither its own digest nor its platform.
    // The caller supplies the manifest digest and its referenced image config.
    pub fn parse_single(
        manifest_json: &str,
        config_json: &str,
        manifest_digest: &str,
    ) -> Result<Self, AddonError> {
        let manifest = parse_json(manifest_json)?;
        require_schema_two(&manifest)?;
        if !matches!(
            required_str(&manifest, "mediaType")?,
            OCI_MANIFEST | DOCKER_MANIFEST
        ) {
            return Err(invalid("expected an OCI or Docker image manifest"));
        }
        if !manifest.get("config").is_some_and(Value::is_object) {
            return Err(invalid("image manifest has no config descriptor"));
        }
        let config = parse_json(config_json)?;
        let platform = parse_platform(&config)?;
        Self::single_image(platform, manifest_digest)
    }

    /// Build a catalog from a runtime's inspection of a single local image.
    /// Podman's manifest command only accepts indexes, so its image inspection
    /// supplies the platform and manifest digest for this case.
    pub fn single_image(platform: Platform, manifest_digest: &str) -> Result<Self, AddonError> {
        validate_digest(manifest_digest)?;
        Self::from_variants(vec![AddonVariant {
            platform,
            libc: None,
            digest: manifest_digest.to_owned(),
        }])
    }

    fn from_variants(variants: Vec<AddonVariant>) -> Result<Self, AddonError> {
        if variants.is_empty() {
            return Err(invalid("no image variants found"));
        }
        let annotated = variants
            .iter()
            .filter(|variant| variant.libc.is_some())
            .count();
        if annotated != 0 && annotated != variants.len() {
            return Err(invalid(
                "libc annotations are present on only some image variants",
            ));
        }
        Ok(Self { variants })
    }

    pub fn requires_base_libc(&self) -> bool {
        let first = self.variants[0].libc.as_deref();
        self.variants
            .iter()
            .any(|variant| variant.libc.as_deref() != first)
    }

    pub fn select(
        &self,
        platform: &Platform,
        base_libc: Option<&str>,
    ) -> Result<&AddonVariant, AddonError> {
        let needs_base_libc = self.requires_base_libc();
        let platform_matches: Vec<_> = self
            .variants
            .iter()
            .filter(|variant| platform.matches(&variant.platform))
            .collect();
        if platform_matches.is_empty() {
            return Err(AddonError::NoMatchingVariant {
                platform: platform.clone(),
                libc: None,
            });
        }
        if needs_base_libc && base_libc.is_none() {
            return Err(AddonError::BaseLibcRequired);
        }
        let matches: Vec<_> = platform_matches
            .into_iter()
            .filter(|variant| !needs_base_libc || variant.libc.as_deref() == base_libc)
            .collect();
        let libc = needs_base_libc.then(|| base_libc.unwrap().to_owned());
        match matches.as_slice() {
            [] => Err(AddonError::NoMatchingVariant {
                platform: platform.clone(),
                libc,
            }),
            [variant] => Ok(variant),
            _ => Err(AddonError::AmbiguousVariant {
                platform: platform.clone(),
                libc,
            }),
        }
    }
}

fn parse_json(json: &str) -> Result<Value, AddonError> {
    serde_json::from_str(json).map_err(|error| invalid(format!("invalid JSON: {error}")))
}

fn require_schema_two(value: &Value) -> Result<(), AddonError> {
    if value.get("schemaVersion").and_then(Value::as_u64) == Some(2) {
        Ok(())
    } else {
        Err(invalid("expected schemaVersion 2"))
    }
}

fn required_str<'a>(value: &'a Value, key: &str) -> Result<&'a str, AddonError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| invalid(format!("missing or empty {key}")))
}

fn parse_platform(value: &Value) -> Result<Platform, AddonError> {
    let os = required_str(value, "os")?;
    let architecture = required_str(value, "architecture")?;
    let variant = value
        .get("variant")
        .map(|value| {
            value
                .as_str()
                .filter(|value| !value.is_empty())
                .ok_or_else(|| invalid("platform variant must be a nonempty string"))
        })
        .transpose()?;
    Ok(Platform::new(os, architecture, variant))
}

fn validate_digest(digest: &str) -> Result<(), AddonError> {
    let valid = match digest.split_once(':') {
        Some(("sha256", hex)) => {
            hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
        }
        Some(("sha512", hex)) => {
            hex.len() == 128 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(invalid("unsupported or malformed manifest digest"))
    }
}

fn invalid(reason: impl Into<String>) -> AddonError {
    AddonError::InvalidMetadata(reason.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn digest(number: u8) -> String {
        format!("sha256:{number:064x}")
    }

    fn descriptor(os: &str, architecture: &str, libc: Option<&str>, number: u8) -> Value {
        let mut descriptor = json!({
            "mediaType": OCI_MANIFEST,
            "digest": digest(number),
            "platform": {"os": os, "architecture": architecture}
        });
        if let Some(libc) = libc {
            descriptor["annotations"] = json!({LIBC_ANNOTATION: libc});
        }
        descriptor
    }

    fn index(manifests: Vec<Value>) -> String {
        json!({
            "schemaVersion": 2,
            "mediaType": OCI_INDEX,
            "manifests": manifests
        })
        .to_string()
    }

    #[test]
    fn single_image_gets_platform_from_config() {
        let manifest = json!({
            "schemaVersion": 2,
            "mediaType": OCI_MANIFEST,
            "config": {"mediaType": "application/vnd.oci.image.config.v1+json", "digest": digest(9)}
        });
        let config = json!({"os": "linux", "architecture": "amd64"});
        let catalog =
            AddonCatalog::parse_single(&manifest.to_string(), &config.to_string(), &digest(1))
                .unwrap();
        assert!(!catalog.requires_base_libc());
        assert_eq!(
            catalog
                .select(&Platform::new("linux", "amd64", None), None)
                .unwrap()
                .digest,
            digest(1)
        );
        assert!(matches!(
            catalog.select(&Platform::new("linux", "arm64", None), None),
            Err(AddonError::NoMatchingVariant { .. })
        ));
    }

    #[test]
    fn one_libc_across_platforms_needs_no_base_probe() {
        let catalog = AddonCatalog::parse_index(&index(vec![
            descriptor("linux", "amd64", Some("glibc"), 1),
            descriptor("linux", "arm64", Some("glibc"), 2),
            descriptor("windows", "amd64", Some("glibc"), 3),
        ]))
        .unwrap();
        assert!(!catalog.requires_base_libc());
        assert_eq!(
            catalog
                .select(&Platform::new("linux", "arm64", None), None)
                .unwrap()
                .digest,
            digest(2)
        );
    }

    #[test]
    fn multiple_libcs_require_and_use_base_libc() {
        let catalog = AddonCatalog::parse_index(&index(vec![
            descriptor("linux", "amd64", Some("glibc"), 1),
            descriptor("linux", "arm64", Some("glibc"), 2),
            descriptor("linux", "amd64", Some("musl"), 3),
            descriptor("linux", "arm64", Some("musl"), 4),
        ]))
        .unwrap();
        let platform = Platform::new("linux", "amd64", None);
        assert!(catalog.requires_base_libc());
        assert_eq!(
            catalog.select(&platform, None),
            Err(AddonError::BaseLibcRequired)
        );
        assert!(matches!(
            catalog.select(&Platform::new("windows", "amd64", None), None),
            Err(AddonError::NoMatchingVariant { .. })
        ));
        assert_eq!(
            catalog.select(&platform, Some("glibc")).unwrap().digest,
            digest(1)
        );
        assert_eq!(
            catalog.select(&platform, Some("musl")).unwrap().digest,
            digest(3)
        );
        assert!(matches!(
            catalog.select(&platform, Some("other")),
            Err(AddonError::NoMatchingVariant { .. })
        ));
    }

    #[test]
    fn unannotated_index_selects_by_platform() {
        let catalog = AddonCatalog::parse_index(&index(vec![
            descriptor("linux", "amd64", None, 1),
            descriptor("linux", "arm64", None, 2),
        ]))
        .unwrap();
        assert!(!catalog.requires_base_libc());
        assert_eq!(
            catalog
                .select(&Platform::new("linux", "arm64", None), None)
                .unwrap()
                .digest,
            digest(2)
        );
    }

    #[test]
    fn partial_annotations_and_duplicate_matches_fail() {
        let partial = AddonCatalog::parse_index(&index(vec![
            descriptor("linux", "amd64", Some("glibc"), 1),
            descriptor("linux", "arm64", None, 2),
        ]));
        assert!(matches!(partial, Err(AddonError::InvalidMetadata(_))));

        let duplicate = AddonCatalog::parse_index(&index(vec![
            descriptor("linux", "amd64", None, 1),
            descriptor("linux", "amd64", None, 2),
        ]))
        .unwrap();
        assert!(matches!(
            duplicate.select(&Platform::new("linux", "amd64", None), None),
            Err(AddonError::AmbiguousVariant { .. })
        ));
    }

    #[test]
    fn attestation_and_unknown_descriptors_are_ignored() {
        let mut attestation = descriptor("unknown", "unknown", None, 2);
        attestation["annotations"] = json!({"vnd.docker.reference.type": "attestation-manifest"});
        let other = json!({"mediaType": "application/example", "digest": digest(3)});
        let catalog = AddonCatalog::parse_index(&index(vec![
            descriptor("linux", "amd64", None, 1),
            attestation,
            other,
        ]))
        .unwrap();
        assert_eq!(catalog.variants().len(), 1);
    }

    #[test]
    fn malformed_index_is_rejected() {
        for raw in [
            "{",
            "{}",
            r#"{"schemaVersion":2,"mediaType":"application/vnd.oci.image.index.v1+json","manifests":[]}"#,
        ] {
            assert!(matches!(
                AddonCatalog::parse_index(raw),
                Err(AddonError::InvalidMetadata(_))
            ));
        }
        let mut invalid_digest = descriptor("linux", "amd64", None, 1);
        invalid_digest["digest"] = json!("sha256:bad");
        assert!(matches!(
            AddonCatalog::parse_index(&index(vec![invalid_digest])),
            Err(AddonError::InvalidMetadata(_))
        ));
    }
}
