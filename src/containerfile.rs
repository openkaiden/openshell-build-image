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

use std::collections::HashMap;

use crate::agent::Agent;
use crate::config::Config;
use crate::feature::StagedFeature;

#[derive(Debug, PartialEq)]
pub enum ContainerfileError {
    NotSupported { image: String },
}

impl std::fmt::Display for ContainerfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContainerfileError::NotSupported { image } => {
                write!(f, "base image '{image}' is not supported")
            }
        }
    }
}

impl std::error::Error for ContainerfileError {}

#[allow(clippy::too_many_arguments)]
pub fn generate(
    config: &Config,
    agent: Option<&dyn Agent>,
    features: &[StagedFeature],
    with_agent_settings: bool,
    skill_names: &[String],
    env_vars: &HashMap<String, String>,
    with_policy: bool,
    with_ca_certs: bool,
    copy_containerfile: bool,
) -> Result<String, ContainerfileError> {
    let tag = &config.base_image.tag;
    let system_stage = match config.base_image.image.as_str() {
        "fedora" => dnf_system_stage(
            "registry.fedoraproject.org/fedora",
            tag,
            &[
                "bind-utils",
                "ca-certificates",
                "curl",
                "iproute",
                "iptables",
                "iputils",
                "net-tools",
                "nftables",
                "nmap-ncat",
                "openssh-server",
                "procps-ng",
                "traceroute",
                "which",
            ],
            with_ca_certs,
        ),
        "ubi" => dnf_system_stage(
            "registry.access.redhat.com/ubi10/ubi",
            tag,
            &[
                "bind-utils",
                "ca-certificates",
                "iputils",
                "net-tools",
                "nftables",
                "nmap-ncat",
                "openssh-server",
                "procps-ng",
                "which",
            ],
            with_ca_certs,
        ),
        "hummingbird" => dnf_system_stage(
            "registry.access.redhat.com/hi/core-runtime",
            tag,
            &[
                "bind-utils",
                "iproute",
                "openssh-server",
                "procps-ng",
                "which",
                "tar",
            ],
            with_ca_certs,
        ),
        "ubuntu" => ubuntu_system_stage(tag, with_ca_certs),
        image => {
            return Err(ContainerfileError::NotSupported {
                image: image.to_string(),
            });
        }
    };
    Ok(format!(
        "{system_stage}\n{}",
        final_stage(
            agent,
            features,
            with_agent_settings,
            skill_names,
            env_vars,
            with_policy,
            copy_containerfile
        )
    ))
}

fn skills_section(agent: Option<&dyn Agent>, skill_names: &[String]) -> String {
    if skill_names.is_empty() {
        return String::new();
    }
    let skills_dir = match agent.map(|a| a.skills_dir()).filter(|d| !d.is_empty()) {
        Some(d) => d,
        None => return String::new(),
    };
    let mut out = String::new();
    for name in skill_names {
        out.push_str(&format!("COPY skills/{name}/ {skills_dir}/{name}/\n"));
    }
    out.push('\n');
    out
}

/// Renders the feature installation section for the `final` stage.
///
/// Each feature's files are COPYed from the build context into the image, then
/// install.sh is run with options passed as env var assignments. `_REMOTE_USER`
/// and `_REMOTE_USER_HOME` are set before the block so scripts can resolve the
/// target user. Each feature's `containerEnv` is set immediately after its
/// install so subsequent features can reference it.
fn features_section(features: &[StagedFeature]) -> String {
    if features.is_empty() {
        return String::new();
    }

    let mut out = String::new();
    out.push_str("ENV _REMOTE_USER=\"root\"\n");
    out.push_str("ENV _REMOTE_USER_HOME=\"/sandbox\"\n");
    out.push_str("RUN mkdir -p \"$_REMOTE_USER_HOME\"\n");

    for feature in features {
        out.push('\n');
        out.push_str(&format!("# Feature: {}\n", feature.id));

        let install_dir = format!("/tmp/feature-install/{}", feature.dir_name);
        out.push_str(&format!(
            "COPY features/{}/ {install_dir}/\n",
            feature.dir_name
        ));

        // Build sorted option assignments: VAR="value" (embedded " escaped).
        let mut opt_pairs: Vec<(&String, &String)> = feature.merged_options.iter().collect();
        opt_pairs.sort_by_key(|(k, _)| k.as_str());
        let opts_prefix = if opt_pairs.is_empty() {
            String::new()
        } else {
            let opts = opt_pairs
                .iter()
                .map(|(k, v)| format!("{}=\"{}\"", k, v.replace('"', "\\\"")))
                .collect::<Vec<_>>()
                .join(" ");
            format!("{opts} ")
        };

        out.push_str(&format!(
            "RUN chmod +x {install_dir}/install.sh && \\\n    {opts_prefix}{install_dir}/install.sh\n"
        ));

        // containerEnv: one ENV per variable, sorted, double-quoted.
        if !feature.container_env.is_empty() {
            let mut env_pairs: Vec<(&String, &String)> = feature.container_env.iter().collect();
            env_pairs.sort_by_key(|(k, _)| k.as_str());
            for (k, v) in env_pairs {
                let escaped = v.replace('"', "\\\"");
                out.push_str(&format!("ENV {k}=\"{escaped}\"\n"));
            }
        }
    }
    out.push_str("RUN rm -rf /tmp/feature-install\n");
    out.push('\n');
    out
}

fn ubuntu_system_stage(tag: &str, with_ca_certs: bool) -> String {
    let ca_cert_section = if with_ca_certs {
        "COPY certs/system-ca.crt /usr/local/share/ca-certificates/system-ca.crt\nRUN update-ca-certificates\n\n"
    } else {
        ""
    };
    format!(
        r#"# System base
FROM docker.io/library/ubuntu:{tag} AS system

# Core system dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        dnsutils \
        iproute2 \
        iptables \
        nftables \
        iputils-ping \
        net-tools \
        netcat-openbsd \
        openssh-sftp-server \
        procps \
        traceroute \
    && rm -rf /var/lib/apt/lists/*
{ca_cert_section}

"#
    )
}

fn dnf_system_stage(base_image: &str, tag: &str, packages: &[&str], with_ca_certs: bool) -> String {
    let pkg_lines = packages
        .iter()
        .map(|p| format!("        {p} \\"))
        .collect::<Vec<_>>()
        .join("\n");
    let ca_cert_section = if with_ca_certs {
        "COPY certs/system-ca.crt /etc/pki/ca-trust/source/anchors/system-ca.crt\nRUN update-ca-trust\n\n"
    } else {
        ""
    };
    format!(
        r#"# System base
FROM {base_image}:{tag} AS system

# Core system dependencies
USER 0
{ca_cert_section}RUN dnf install -y --setopt=install_weak_deps=False \
{pkg_lines}
    && dnf clean all

"#
    )
}

fn final_stage(
    agent: Option<&dyn Agent>,
    features: &[StagedFeature],
    with_agent_settings: bool,
    skill_names: &[String],
    env_vars: &HashMap<String, String>,
    with_policy: bool,
    copy_containerfile: bool,
) -> String {
    let containerfile_section = if copy_containerfile {
        "COPY build-containerfile /tmp/build-containerfile\n\
         RUN cp /tmp/build-containerfile \"$HOME/Containerfile\" && rm /tmp/build-containerfile\n\n"
    } else {
        ""
    };
    let agent_section = agent
        .map(|a| format!("{}\n\n", a.install()))
        .unwrap_or_default();
    let agent_settings_section = if with_agent_settings {
        "COPY agent-settings/ /sandbox/\n\n"
    } else {
        ""
    };
    let skills_section = skills_section(agent, skill_names);
    let features_section = features_section(features);
    let env_vars_section = if env_vars.is_empty() {
        String::new()
    } else {
        let mut pairs: Vec<(&String, &String)> = env_vars.iter().collect();
        pairs.sort_by_key(|(k, _)| k.as_str());
        let mut out = pairs
            .iter()
            .map(|(k, v)| format!("ENV {}=\"{}\"\n", k, v.replace('"', "\\\"")))
            .collect::<String>();
        out.push('\n');
        out
    };
    let policy_section = if with_policy {
        "COPY policy.yaml /etc/openshell/policy.yaml\n\n"
    } else {
        ""
    };
    format!(
        r#"# Final base image
FROM system AS final

{features_section}{policy_section}{env_vars_section}{agent_settings_section}{skills_section}{agent_section}{containerfile_section}
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::Agent;
    use crate::config::{BaseImageConfig, Config};

    fn build_cf(
        config: &Config,
        agent: Option<&dyn Agent>,
        features: &[StagedFeature],
        with_agent_settings: bool,
        skill_names: &[String],
        with_policy: bool,
    ) -> Result<String, ContainerfileError> {
        generate(
            config,
            agent,
            features,
            with_agent_settings,
            skill_names,
            &HashMap::new(),
            with_policy,
            false,
            false,
        )
    }

    fn build_cf_with_ca_certs(config: &Config) -> String {
        generate(
            config,
            None,
            &[],
            false,
            &[],
            &HashMap::new(),
            false,
            true,
            false,
        )
        .unwrap()
    }

    fn ubuntu_config(tag: &str) -> Config {
        Config {
            version: 1,
            base_image: BaseImageConfig {
                image: "ubuntu".to_string(),
                tag: tag.to_string(),
            },
        }
    }

    fn fedora_config() -> Config {
        Config {
            version: 1,
            base_image: BaseImageConfig {
                image: "fedora".to_string(),
                tag: "latest".to_string(),
            },
        }
    }

    fn ubi_config() -> Config {
        Config {
            version: 1,
            base_image: BaseImageConfig {
                image: "ubi".to_string(),
                tag: "latest".to_string(),
            },
        }
    }

    fn hummingbird_config() -> Config {
        Config {
            version: 1,
            base_image: BaseImageConfig {
                image: "hummingbird".to_string(),
                tag: "latest-builder".to_string(),
            },
        }
    }

    struct MockAgent;

    impl Agent for MockAgent {
        fn id(&self) -> &str {
            "mock"
        }

        fn install(&self) -> String {
            "RUN echo mock-agent".to_string()
        }

        fn binary_path(&self) -> &str {
            "/sandbox/.local/bin/mock-agent"
        }

        fn skills_dir(&self) -> &str {
            "/sandbox/.mock/skills"
        }
    }

    fn mock_feature(id: &str, dir_name: &str) -> StagedFeature {
        StagedFeature {
            id: id.to_string(),
            dir_name: dir_name.to_string(),
            merged_options: std::collections::HashMap::new(),
            container_env: std::collections::HashMap::new(),
        }
    }

    #[test]
    fn ubuntu_generates_successfully() {
        let config = ubuntu_config("noble-20251013");
        assert!(build_cf(&config, None, &[], false, &[], false).is_ok());
    }

    #[test]
    fn ubuntu_containerfile_contains_tag() {
        let config = ubuntu_config("noble-20251013");
        let content = build_cf(&config, None, &[], false, &[], false).unwrap();

        assert!(content.contains("FROM docker.io/library/ubuntu:noble-20251013 AS system"));
    }

    #[test]
    fn ubuntu_containerfile_tag_is_substituted() {
        let content = build_cf(&ubuntu_config("24.04"), None, &[], false, &[], false).unwrap();

        assert!(content.contains("FROM docker.io/library/ubuntu:24.04 AS system"));
        assert!(!content.contains("{tag}"));
    }

    #[test]
    fn ubuntu_with_agent_includes_install() {
        let content = build_cf(
            &ubuntu_config("noble-20251013"),
            Some(&MockAgent),
            &[],
            false,
            &[],
            false,
        )
        .unwrap();
        assert!(content.contains("RUN echo mock-agent"));
    }

    #[test]
    fn ubuntu_without_agent_omits_install() {
        let content = build_cf(
            &ubuntu_config("noble-20251013"),
            None,
            &[],
            false,
            &[],
            false,
        )
        .unwrap();

        assert!(!content.contains("RUN echo mock-agent"));
    }

    #[test]
    fn fedora_generates_successfully() {
        assert!(build_cf(&fedora_config(), None, &[], false, &[], false).is_ok());
    }

    #[test]
    fn fedora_containerfile_contains_tag() {
        let content = build_cf(&fedora_config(), None, &[], false, &[], false).unwrap();

        assert!(content.contains("FROM registry.fedoraproject.org/fedora:latest AS system"));
    }

    #[test]
    fn fedora_containerfile_tag_is_substituted() {
        let content = build_cf(&fedora_config(), None, &[], false, &[], false).unwrap();

        assert!(!content.contains("{tag}"));
    }

    #[test]
    fn fedora_with_agent_includes_install() {
        let content = build_cf(&fedora_config(), Some(&MockAgent), &[], false, &[], false).unwrap();

        assert!(content.contains("RUN echo mock-agent"));
    }

    #[test]
    fn fedora_without_agent_omits_install() {
        let content = build_cf(&fedora_config(), None, &[], false, &[], false).unwrap();

        assert!(!content.contains("RUN echo mock-agent"));
    }

    #[test]
    fn ubi_generates_successfully() {
        assert!(build_cf(&ubi_config(), None, &[], false, &[], false).is_ok());
    }

    #[test]
    fn ubi_containerfile_contains_tag() {
        let content = build_cf(&ubi_config(), None, &[], false, &[], false).unwrap();

        assert!(content.contains("FROM registry.access.redhat.com/ubi10/ubi:latest AS system"));
    }

    #[test]
    fn ubi_containerfile_tag_is_substituted() {
        let content = build_cf(&ubi_config(), None, &[], false, &[], false).unwrap();

        assert!(!content.contains("{tag}"));
    }

    #[test]
    fn ubi_with_agent_includes_install() {
        let content = build_cf(&ubi_config(), Some(&MockAgent), &[], false, &[], false).unwrap();

        assert!(content.contains("RUN echo mock-agent"));
    }

    #[test]
    fn ubi_without_agent_omits_install() {
        let content = build_cf(&ubi_config(), None, &[], false, &[], false).unwrap();

        assert!(!content.contains("RUN echo mock-agent"));
    }

    #[test]
    fn ubi_copies_policy_yaml() {
        let content = build_cf(&ubi_config(), None, &[], false, &[], true).unwrap();
        assert!(content.contains("COPY policy.yaml /etc/openshell/policy.yaml"));
    }

    #[test]
    fn hummingbird_generates_successfully() {
        assert!(build_cf(&hummingbird_config(), None, &[], false, &[], false).is_ok());
    }

    #[test]
    fn hummingbird_containerfile_contains_tag() {
        let content = build_cf(&hummingbird_config(), None, &[], false, &[], false).unwrap();

        assert!(
            content.contains(
                "FROM registry.access.redhat.com/hi/core-runtime:latest-builder AS system"
            )
        );
    }

    #[test]
    fn hummingbird_containerfile_tag_is_substituted() {
        let content = build_cf(&hummingbird_config(), None, &[], false, &[], false).unwrap();

        assert!(!content.contains("{tag}"));
    }

    #[test]
    fn hummingbird_with_agent_includes_install() {
        let content = build_cf(
            &hummingbird_config(),
            Some(&MockAgent),
            &[],
            false,
            &[],
            false,
        )
        .unwrap();

        assert!(content.contains("RUN echo mock-agent"));
    }

    #[test]
    fn hummingbird_without_agent_omits_install() {
        let content = build_cf(&hummingbird_config(), None, &[], false, &[], false).unwrap();

        assert!(!content.contains("RUN echo mock-agent"));
    }

    #[test]
    fn hummingbird_copies_policy_yaml() {
        let content = build_cf(&hummingbird_config(), None, &[], false, &[], true).unwrap();
        assert!(content.contains("COPY policy.yaml /etc/openshell/policy.yaml"));
    }

    #[test]
    fn hummingbird_containerfile_includes_iproute() {
        let content = build_cf(&hummingbird_config(), None, &[], false, &[], false).unwrap();

        assert!(
            content.contains("iproute"),
            "hummingbird image must install iproute for network namespace support"
        );
    }

    #[test]
    fn not_supported_error_message() {
        let err = ContainerfileError::NotSupported {
            image: "centos".to_string(),
        };
        assert_eq!(err.to_string(), "base image 'centos' is not supported");
    }

    #[test]
    fn unknown_image_returns_not_supported() {
        let config = Config {
            version: 1,
            base_image: BaseImageConfig {
                image: "centos".to_string(),
                tag: "latest".to_string(),
            },
        };
        let err = build_cf(&config, None, &[], false, &[], false).unwrap_err();

        assert_eq!(
            err,
            ContainerfileError::NotSupported {
                image: "centos".to_string()
            }
        );
    }
    #[test]
    fn feature_copy_instruction_present() {
        let feature = mock_feature("./tools/my-feature", "feature-0");
        let content =
            build_cf(&ubuntu_config("24.04"), None, &[feature], false, &[], false).unwrap();

        assert!(content.contains("COPY features/feature-0/"));
        assert!(content.contains("/tmp/feature-install/feature-0/install.sh"));
    }

    #[test]
    fn feature_options_in_run_command() {
        let mut feature = mock_feature("./tools/my-feature", "feature-0");
        feature
            .merged_options
            .insert("VERSION".to_string(), "1.0".to_string());
        let content =
            build_cf(&ubuntu_config("24.04"), None, &[feature], false, &[], false).unwrap();

        assert!(content.contains("VERSION=\"1.0\""));
    }

    #[test]
    fn feature_container_env_emitted_as_env_instruction() {
        let mut feature = mock_feature("./tools/my-feature", "feature-0");
        feature
            .container_env
            .insert("CARGO_HOME".to_string(), "/home/sandbox/.cargo".to_string());
        let content =
            build_cf(&ubuntu_config("24.04"), None, &[feature], false, &[], false).unwrap();

        assert!(content.contains("ENV CARGO_HOME=\"/home/sandbox/.cargo\""));
    }

    #[test]
    fn feature_install_dir_cleaned_up() {
        let feature = mock_feature("./tools/my-feature", "feature-0");
        let content =
            build_cf(&ubuntu_config("24.04"), None, &[feature], false, &[], false).unwrap();

        assert!(content.contains("RUN rm -rf /tmp/feature-install\n"));
    }

    #[test]
    fn no_features_produces_same_output_as_before() {
        let with_empty = build_cf(&ubuntu_config("24.04"), None, &[], false, &[], false).unwrap();

        assert!(!with_empty.contains("# Feature:"));
        assert!(!with_empty.contains("_REMOTE_USER"));
        assert!(!with_empty.contains("rm -rf /tmp/feature-install"));
    }

    #[test]
    fn ubuntu_copies_policy_yaml() {
        let content = build_cf(&ubuntu_config("24.04"), None, &[], false, &[], true).unwrap();
        assert!(content.contains("COPY policy.yaml /etc/openshell/policy.yaml"));
    }

    #[test]
    fn ubuntu_omits_policy_yaml_without_flag() {
        let content = build_cf(&ubuntu_config("24.04"), None, &[], false, &[], false).unwrap();
        assert!(!content.contains("COPY policy.yaml /etc/openshell/policy.yaml"));
    }

    #[test]
    fn fedora_copies_policy_yaml() {
        let content = build_cf(&fedora_config(), None, &[], false, &[], true).unwrap();
        assert!(content.contains("COPY policy.yaml /etc/openshell/policy.yaml"));
    }

    #[test]
    fn ubuntu_with_agent_settings_includes_copy() {
        let content = build_cf(&ubuntu_config("24.04"), None, &[], true, &[], false).unwrap();

        assert!(content.contains("COPY agent-settings/ /sandbox/"));
    }

    #[test]
    fn ubuntu_without_agent_settings_omits_copy() {
        let content = build_cf(&ubuntu_config("24.04"), None, &[], false, &[], false).unwrap();

        assert!(!content.contains("agent-settings/"));
    }

    #[test]
    fn fedora_with_agent_settings_includes_copy() {
        let content = build_cf(&fedora_config(), None, &[], true, &[], false).unwrap();

        assert!(content.contains("COPY agent-settings/ /sandbox/"));
    }

    #[test]
    fn agent_settings_copy_appears_before_agent_install() {
        let content = build_cf(
            &ubuntu_config("24.04"),
            Some(&MockAgent),
            &[],
            true,
            &[],
            false,
        )
        .unwrap();

        let copy_pos = content.find("COPY agent-settings/").unwrap();
        let install_pos = content.find("RUN echo mock-agent").unwrap();
        assert!(
            copy_pos < install_pos,
            "agent-settings COPY must appear before agent install"
        );
    }

    #[test]
    fn skills_copy_present_for_agent_with_skills_dir() {
        let skills = vec!["my-skill".to_string()];
        let content = build_cf(
            &ubuntu_config("24.04"),
            Some(&MockAgent),
            &[],
            false,
            &skills,
            false,
        )
        .unwrap();
        assert!(content.contains("COPY skills/my-skill/"));
    }

    #[test]
    fn skills_copy_uses_agent_skills_dir() {
        let skills = vec!["my-skill".to_string()];
        let content = build_cf(
            &ubuntu_config("24.04"),
            Some(&MockAgent),
            &[],
            false,
            &skills,
            false,
        )
        .unwrap();
        assert!(content.contains("/sandbox/.mock/skills/my-skill/"));
    }

    #[test]
    fn skills_copy_omitted_when_no_skills() {
        let content = build_cf(
            &ubuntu_config("24.04"),
            Some(&MockAgent),
            &[],
            false,
            &[],
            false,
        )
        .unwrap();

        assert!(!content.contains("skills/"));
    }

    #[test]
    fn skills_copy_omitted_when_no_agent() {
        let skills = vec!["my-skill".to_string()];
        let content = build_cf(&ubuntu_config("24.04"), None, &[], false, &skills, false).unwrap();

        assert!(!content.contains("skills/"));
    }

    #[test]
    fn skills_copy_appears_before_agent_install() {
        let skills = vec!["my-skill".to_string()];
        let content = build_cf(
            &ubuntu_config("24.04"),
            Some(&MockAgent),
            &[],
            false,
            &skills,
            false,
        )
        .unwrap();
        let skills_pos = content.find("COPY skills/my-skill/").unwrap();
        let install_pos = content.find("RUN echo mock-agent").unwrap();
        assert!(
            skills_pos < install_pos,
            "skills COPY must appear before agent install"
        );
    }

    #[test]
    fn multiple_skills_each_get_copy_instruction() {
        let skills = vec!["skill-a".to_string(), "skill-b".to_string()];
        let content = build_cf(
            &ubuntu_config("24.04"),
            Some(&MockAgent),
            &[],
            false,
            &skills,
            false,
        )
        .unwrap();
        assert!(content.contains("COPY skills/skill-a/"));
        assert!(content.contains("COPY skills/skill-b/"));
    }

    // env_vars

    #[test]
    fn env_vars_emitted_as_env_instructions() {
        let mut vars = HashMap::new();
        vars.insert(
            "ANTHROPIC_BASE_URL".to_string(),
            "https://proxy.example.com".to_string(),
        );
        let content = generate(
            &ubuntu_config("24.04"),
            None,
            &[],
            false,
            &[],
            &vars,
            false,
            false,
            false,
        )
        .unwrap();
        assert!(content.contains("ENV ANTHROPIC_BASE_URL=\"https://proxy.example.com\""));
    }

    #[test]

    fn empty_env_vars_produces_no_extra_env_instruction() {
        let content = build_cf(&ubuntu_config("24.04"), None, &[], false, &[], false).unwrap();

        assert!(!content.contains("ENV ANTHROPIC_BASE_URL="));
    }

    #[test]
    fn multiple_env_vars_are_sorted_alphabetically() {
        let mut vars = HashMap::new();
        vars.insert("Z_VAR".to_string(), "z".to_string());
        vars.insert("A_VAR".to_string(), "a".to_string());
        let content = generate(
            &ubuntu_config("24.04"),
            None,
            &[],
            false,
            &[],
            &vars,
            false,
            false,
            false,
        )
        .unwrap();
        let a_pos = content.find("ENV A_VAR=").unwrap();
        let z_pos = content.find("ENV Z_VAR=").unwrap();
        assert!(a_pos < z_pos, "env vars must be sorted alphabetically");
    }

    #[test]
    fn copy_containerfile_is_supported_on_all_base_images() {
        for config in [
            ubuntu_config("24.04"),
            fedora_config(),
            ubi_config(),
            hummingbird_config(),
        ] {
            let content = generate(
                &config,
                None,
                &[],
                false,
                &[],
                &HashMap::new(),
                false,
                false,
                true,
            )
            .unwrap();
            assert!(content.contains("COPY build-containerfile /tmp/build-containerfile"));
            assert!(content.contains("RUN cp /tmp/build-containerfile \"$HOME/Containerfile\""));
            assert!(!content.contains("--chown=sandbox"));
            let without_copy = build_cf(&config, None, &[], false, &[], false).unwrap();
            assert!(!without_copy.contains("build-containerfile"));
        }
    }

    // CA cert tests

    #[test]
    fn ca_certs_omitted_when_false() {
        for content in [
            build_cf(&ubuntu_config("24.04"), None, &[], false, &[], false).unwrap(),
            build_cf(&fedora_config(), None, &[], false, &[], false).unwrap(),
            build_cf(&ubi_config(), None, &[], false, &[], false).unwrap(),
            build_cf(&hummingbird_config(), None, &[], false, &[], false).unwrap(),
        ] {
            assert!(
                !content.contains("COPY certs/"),
                "unexpected cert COPY: {content}"
            );
        }
    }

    #[test]
    fn dnf_ca_certs_included_when_true() {
        for content in [
            build_cf_with_ca_certs(&fedora_config()),
            build_cf_with_ca_certs(&ubi_config()),
            build_cf_with_ca_certs(&hummingbird_config()),
        ] {
            assert!(
                content.contains(
                    "COPY certs/system-ca.crt /etc/pki/ca-trust/source/anchors/system-ca.crt"
                ),
                "missing COPY instruction: {content}"
            );
            assert!(
                content.contains("RUN update-ca-trust"),
                "missing update-ca-trust: {content}"
            );
        }
    }

    #[test]
    fn ubuntu_ca_certs_included_when_true() {
        let content = build_cf_with_ca_certs(&ubuntu_config("24.04"));
        assert!(
            content.contains(
                "COPY certs/system-ca.crt /usr/local/share/ca-certificates/system-ca.crt"
            ),
            "missing COPY instruction: {content}"
        );
        assert!(
            content.contains("RUN update-ca-certificates"),
            "missing update-ca-certificates: {content}"
        );
    }

    #[test]
    fn dnf_ca_cert_appears_before_dnf_install() {
        for content in [
            build_cf_with_ca_certs(&fedora_config()),
            build_cf_with_ca_certs(&ubi_config()),
            build_cf_with_ca_certs(&hummingbird_config()),
        ] {
            let cert_pos = content.find("COPY certs/system-ca.crt").unwrap();
            let dnf_pos = content.find("RUN dnf install").unwrap();
            assert!(
                cert_pos < dnf_pos,
                "CA cert COPY must appear before dnf install"
            );
        }
    }

    #[test]
    fn ubuntu_ca_cert_appears_after_apt_install() {
        let content = build_cf_with_ca_certs(&ubuntu_config("24.04"));
        let apt_pos = content.find("RUN apt-get update").unwrap();
        let cert_pos = content.find("COPY certs/system-ca.crt").unwrap();
        assert!(
            cert_pos > apt_pos,
            "CA cert COPY must appear after apt-get install"
        );
    }
}
