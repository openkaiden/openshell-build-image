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

use std::process::{Command, Output};
use std::sync::OnceLock;

// ---------------------------------------------------------------------------
// Image build helpers
// ---------------------------------------------------------------------------

fn fedora_config_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("config.toml"),
        "[openshell_image_builder.base_image]\nimage = \"fedora\"\ntag = \"latest\"\n",
    )
    .unwrap();
    dir
}

fn ubi_config_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("config.toml"),
        "[openshell_image_builder.base_image]\nimage = \"ubi\"\ntag = \"latest\"\n",
    )
    .unwrap();
    dir
}

fn hummingbird_config_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("config.toml"),
        "[openshell_image_builder.base_image]\nimage = \"hummingbird\"\ntag = \"latest-builder\"\n",
    )
    .unwrap();
    dir
}

fn build_image(tag: &str, extra_args: &[&str]) -> String {
    let binary = env!("CARGO_BIN_EXE_openshell-image-builder");
    let status = Command::new(binary)
        .args(["--runtime", "podman"])
        .args(extra_args)
        .arg(tag)
        .status()
        .expect("binary should run");
    assert!(status.success(), "image build failed for tag {tag}");
    tag.to_string()
}

fn run_in_image(image: &str, cmd: &str) -> Output {
    Command::new("podman")
        .args(["run", "--rm", image, "-c", cmd])
        .output()
        .expect("podman run should execute")
}

// ---------------------------------------------------------------------------
// One OnceLock per image variant — each image is built at most once
// ---------------------------------------------------------------------------

static UBUNTU_CLAUDE_SETTINGS_IMAGE: OnceLock<String> = OnceLock::new();
static UBUNTU_CLAUDE_WITH_CLAUDE_JSON_IMAGE: OnceLock<String> = OnceLock::new();
static UBUNTU_OPENCODE_SETTINGS_IMAGE: OnceLock<String> = OnceLock::new();
static UBUNTU_IMAGE: OnceLock<String> = OnceLock::new();
static UBUNTU_CLAUDE_IMAGE: OnceLock<String> = OnceLock::new();
static UBUNTU_OPENCODE_IMAGE: OnceLock<String> = OnceLock::new();
static UBUNTU_CLAUDE_VERTEXAI_IMAGE: OnceLock<String> = OnceLock::new();
static UBUNTU_OPENCODE_VERTEXAI_IMAGE: OnceLock<String> = OnceLock::new();
static FEDORA_IMAGE: OnceLock<String> = OnceLock::new();
static FEDORA_CLAUDE_IMAGE: OnceLock<String> = OnceLock::new();
static FEDORA_OPENCODE_IMAGE: OnceLock<String> = OnceLock::new();
static FEDORA_CLAUDE_VERTEXAI_IMAGE: OnceLock<String> = OnceLock::new();
static FEDORA_OPENCODE_VERTEXAI_IMAGE: OnceLock<String> = OnceLock::new();
static UBI_IMAGE: OnceLock<String> = OnceLock::new();
static UBI_CLAUDE_IMAGE: OnceLock<String> = OnceLock::new();
static UBI_OPENCODE_IMAGE: OnceLock<String> = OnceLock::new();
static UBI_CLAUDE_VERTEXAI_IMAGE: OnceLock<String> = OnceLock::new();
static UBI_OPENCODE_VERTEXAI_IMAGE: OnceLock<String> = OnceLock::new();
static HUMMINGBIRD_IMAGE: OnceLock<String> = OnceLock::new();
static HUMMINGBIRD_CLAUDE_IMAGE: OnceLock<String> = OnceLock::new();
static HUMMINGBIRD_OPENCODE_IMAGE: OnceLock<String> = OnceLock::new();
static HUMMINGBIRD_CLAUDE_VERTEXAI_IMAGE: OnceLock<String> = OnceLock::new();
static HUMMINGBIRD_OPENCODE_VERTEXAI_IMAGE: OnceLock<String> = OnceLock::new();
static UBUNTU_CLAUDE_SKILLS_IMAGE: OnceLock<String> = OnceLock::new();
static UBUNTU_OPENCODE_SKILLS_IMAGE: OnceLock<String> = OnceLock::new();
static NO_WORKSPACE_CONFIG_OCI_FEATURE_UBUNTU_IMAGE: OnceLock<String> = OnceLock::new();
static NO_WORKSPACE_CONFIG_LOCAL_FEATURE_UBUNTU_IMAGE: OnceLock<String> = OnceLock::new();
static NO_WORKSPACE_CONFIG_CLAUDE_SKILLS_UBUNTU_IMAGE: OnceLock<String> = OnceLock::new();
static NO_WORKSPACE_CONFIG_OPENCODE_SKILLS_UBUNTU_IMAGE: OnceLock<String> = OnceLock::new();
static NO_WORKSPACE_CONFIG_NETWORK_HOSTS_UBUNTU_IMAGE: OnceLock<String> = OnceLock::new();
static UBUNTU_OPENCODE_OLLAMA_IMAGE: OnceLock<String> = OnceLock::new();
static FEDORA_OPENCODE_OLLAMA_IMAGE: OnceLock<String> = OnceLock::new();
static UBI_OPENCODE_OLLAMA_IMAGE: OnceLock<String> = OnceLock::new();
static HUMMINGBIRD_OPENCODE_OLLAMA_IMAGE: OnceLock<String> = OnceLock::new();
static UBUNTU_OPENCODE_OPENAI_IMAGE: OnceLock<String> = OnceLock::new();
static FEDORA_OPENCODE_OPENAI_IMAGE: OnceLock<String> = OnceLock::new();
static UBI_OPENCODE_OPENAI_IMAGE: OnceLock<String> = OnceLock::new();
static HUMMINGBIRD_OPENCODE_OPENAI_IMAGE: OnceLock<String> = OnceLock::new();
static UBUNTU_OPENCODE_OPENAI_MODEL_IMAGE: OnceLock<String> = OnceLock::new();
static UBUNTU_NO_POLICY_IMAGE: OnceLock<String> = OnceLock::new();
static UBUNTU_CLAUDE_NO_AGENT_SETTINGS_IMAGE: OnceLock<String> = OnceLock::new();
static UBUNTU_SSL_CERTS_IMAGE: OnceLock<String> = OnceLock::new();
static FEDORA_SSL_CERTS_IMAGE: OnceLock<String> = OnceLock::new();
static UBUNTU_NO_CERTS_IMAGE: OnceLock<String> = OnceLock::new();
static FEDORA_NO_CERTS_IMAGE: OnceLock<String> = OnceLock::new();
static UBUNTU_IMAGE_MOUNT_IMAGE: OnceLock<String> = OnceLock::new();

fn ssl_cert_fixture_path() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/test-ca.crt")
}

fn ubuntu_ssl_certs_image() -> &'static str {
    UBUNTU_SSL_CERTS_IMAGE.get_or_init(|| {
        let cert = ssl_cert_fixture_path();
        let cert_str = cert.to_str().unwrap();
        build_image(
            "openshell-test-ubuntu-ssl-certs:integration",
            &["--ssl-certs", cert_str],
        )
    })
}

fn fedora_ssl_certs_image() -> &'static str {
    FEDORA_SSL_CERTS_IMAGE.get_or_init(|| {
        let config = fedora_config_dir();
        let cert = ssl_cert_fixture_path();
        let cert_str = cert.to_str().unwrap();
        build_image(
            "openshell-test-fedora-ssl-certs:integration",
            &[
                "--config",
                config.path().to_str().unwrap(),
                "--ssl-certs",
                cert_str,
            ],
        )
    })
}

fn ubuntu_no_certs_image() -> &'static str {
    UBUNTU_NO_CERTS_IMAGE.get_or_init(|| {
        build_image(
            "openshell-test-ubuntu-no-certs:integration",
            &["--disable-ssl-certs"],
        )
    })
}

fn fedora_no_certs_image() -> &'static str {
    FEDORA_NO_CERTS_IMAGE.get_or_init(|| {
        let config = fedora_config_dir();
        build_image(
            "openshell-test-fedora-no-certs:integration",
            &[
                "--config",
                config.path().to_str().unwrap(),
                "--disable-ssl-certs",
            ],
        )
    })
}

fn config_dir_with_agent_settings(agent: &str, files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let agent_dir = dir.path().join("agents").join(agent);
    std::fs::create_dir_all(&agent_dir).unwrap();
    for (name, content) in files {
        std::fs::write(agent_dir.join(name), content).unwrap();
    }
    dir
}

fn ubuntu_claude_settings_image() -> &'static str {
    UBUNTU_CLAUDE_SETTINGS_IMAGE.get_or_init(|| {
        let config = config_dir_with_agent_settings("claude", &[("my-claude-settings", "")]);
        build_image(
            "openshell-test-ubuntu-claude-settings:integration",
            &[
                "--config",
                config.path().to_str().unwrap(),
                "--agent",
                "claude",
                "--with-agent-settings",
            ],
        )
    })
}

fn ubuntu_claude_with_claude_json_image() -> &'static str {
    UBUNTU_CLAUDE_WITH_CLAUDE_JSON_IMAGE.get_or_init(|| {
        let config = config_dir_with_agent_settings(
            "claude",
            &[(".claude.json", r#"{"existingField": "myvalue"}"#)],
        );
        build_image(
            "openshell-test-ubuntu-claude-with-claude-json:integration",
            &[
                "--config",
                config.path().to_str().unwrap(),
                "--agent",
                "claude",
                "--with-agent-settings",
            ],
        )
    })
}

fn ubuntu_opencode_settings_image() -> &'static str {
    UBUNTU_OPENCODE_SETTINGS_IMAGE.get_or_init(|| {
        let config = config_dir_with_agent_settings("opencode", &[("my-opencode-settings", "")]);
        build_image(
            "openshell-test-ubuntu-opencode-settings:integration",
            &[
                "--config",
                config.path().to_str().unwrap(),
                "--agent",
                "opencode",
                "--with-agent-settings",
            ],
        )
    })
}

fn ubuntu_image() -> &'static str {
    UBUNTU_IMAGE
        .get_or_init(|| build_image("openshell-test-ubuntu:integration", &["--with-policy"]))
}

fn ubuntu_claude_image() -> &'static str {
    UBUNTU_CLAUDE_IMAGE.get_or_init(|| {
        build_image(
            "openshell-test-ubuntu-claude:integration",
            &[
                "--agent",
                "claude",
                "--inference",
                "anthropic",
                "--with-policy",
                "--with-agent-settings",
            ],
        )
    })
}

fn fedora_image() -> &'static str {
    FEDORA_IMAGE.get_or_init(|| {
        let config = fedora_config_dir();
        build_image(
            "openshell-test-fedora:integration",
            &["--config", config.path().to_str().unwrap(), "--with-policy"],
        )
    })
}

fn fedora_claude_image() -> &'static str {
    FEDORA_CLAUDE_IMAGE.get_or_init(|| {
        let config = fedora_config_dir();
        build_image(
            "openshell-test-fedora-claude:integration",
            &[
                "--config",
                config.path().to_str().unwrap(),
                "--agent",
                "claude",
                "--inference",
                "anthropic",
                "--with-policy",
            ],
        )
    })
}

fn ubuntu_opencode_image() -> &'static str {
    UBUNTU_OPENCODE_IMAGE.get_or_init(|| {
        build_image(
            "openshell-test-ubuntu-opencode:integration",
            &[
                "--agent",
                "opencode",
                "--inference",
                "anthropic",
                "--with-policy",
            ],
        )
    })
}

fn fedora_opencode_image() -> &'static str {
    FEDORA_OPENCODE_IMAGE.get_or_init(|| {
        let config = fedora_config_dir();
        build_image(
            "openshell-test-fedora-opencode:integration",
            &[
                "--config",
                config.path().to_str().unwrap(),
                "--agent",
                "opencode",
                "--inference",
                "anthropic",
                "--with-policy",
            ],
        )
    })
}

fn ubuntu_claude_vertexai_image() -> &'static str {
    UBUNTU_CLAUDE_VERTEXAI_IMAGE.get_or_init(|| {
        build_image(
            "openshell-test-ubuntu-claude-vertexai:integration",
            &[
                "--agent",
                "claude",
                "--inference",
                "vertexai",
                "--with-policy",
            ],
        )
    })
}

fn ubuntu_opencode_vertexai_image() -> &'static str {
    UBUNTU_OPENCODE_VERTEXAI_IMAGE.get_or_init(|| {
        build_image(
            "openshell-test-ubuntu-opencode-vertexai:integration",
            &[
                "--agent",
                "opencode",
                "--inference",
                "vertexai",
                "--with-policy",
            ],
        )
    })
}

fn fedora_claude_vertexai_image() -> &'static str {
    FEDORA_CLAUDE_VERTEXAI_IMAGE.get_or_init(|| {
        let config = fedora_config_dir();
        build_image(
            "openshell-test-fedora-claude-vertexai:integration",
            &[
                "--config",
                config.path().to_str().unwrap(),
                "--agent",
                "claude",
                "--inference",
                "vertexai",
                "--with-policy",
            ],
        )
    })
}

fn fedora_opencode_vertexai_image() -> &'static str {
    FEDORA_OPENCODE_VERTEXAI_IMAGE.get_or_init(|| {
        let config = fedora_config_dir();
        build_image(
            "openshell-test-fedora-opencode-vertexai:integration",
            &[
                "--config",
                config.path().to_str().unwrap(),
                "--agent",
                "opencode",
                "--inference",
                "vertexai",
                "--with-policy",
            ],
        )
    })
}

fn ubi_image() -> &'static str {
    UBI_IMAGE.get_or_init(|| {
        let config = ubi_config_dir();
        build_image(
            "openshell-test-ubi:integration",
            &["--config", config.path().to_str().unwrap(), "--with-policy"],
        )
    })
}

fn ubi_claude_image() -> &'static str {
    UBI_CLAUDE_IMAGE.get_or_init(|| {
        let config = ubi_config_dir();
        build_image(
            "openshell-test-ubi-claude:integration",
            &[
                "--config",
                config.path().to_str().unwrap(),
                "--agent",
                "claude",
                "--inference",
                "anthropic",
                "--with-policy",
            ],
        )
    })
}

fn ubi_opencode_image() -> &'static str {
    UBI_OPENCODE_IMAGE.get_or_init(|| {
        let config = ubi_config_dir();
        build_image(
            "openshell-test-ubi-opencode:integration",
            &[
                "--config",
                config.path().to_str().unwrap(),
                "--agent",
                "opencode",
                "--inference",
                "anthropic",
                "--with-policy",
            ],
        )
    })
}

fn ubi_claude_vertexai_image() -> &'static str {
    UBI_CLAUDE_VERTEXAI_IMAGE.get_or_init(|| {
        let config = ubi_config_dir();
        build_image(
            "openshell-test-ubi-claude-vertexai:integration",
            &[
                "--config",
                config.path().to_str().unwrap(),
                "--agent",
                "claude",
                "--inference",
                "vertexai",
                "--with-policy",
            ],
        )
    })
}

fn ubi_opencode_vertexai_image() -> &'static str {
    UBI_OPENCODE_VERTEXAI_IMAGE.get_or_init(|| {
        let config = ubi_config_dir();
        build_image(
            "openshell-test-ubi-opencode-vertexai:integration",
            &[
                "--config",
                config.path().to_str().unwrap(),
                "--agent",
                "opencode",
                "--inference",
                "vertexai",
                "--with-policy",
            ],
        )
    })
}

fn hummingbird_image() -> &'static str {
    HUMMINGBIRD_IMAGE.get_or_init(|| {
        let config = hummingbird_config_dir();
        build_image(
            "openshell-test-hummingbird:integration",
            &["--config", config.path().to_str().unwrap(), "--with-policy"],
        )
    })
}

fn hummingbird_claude_image() -> &'static str {
    HUMMINGBIRD_CLAUDE_IMAGE.get_or_init(|| {
        let config = hummingbird_config_dir();
        build_image(
            "openshell-test-hummingbird-claude:integration",
            &[
                "--config",
                config.path().to_str().unwrap(),
                "--agent",
                "claude",
                "--inference",
                "anthropic",
                "--with-policy",
            ],
        )
    })
}

fn hummingbird_opencode_image() -> &'static str {
    HUMMINGBIRD_OPENCODE_IMAGE.get_or_init(|| {
        let config = hummingbird_config_dir();
        build_image(
            "openshell-test-hummingbird-opencode:integration",
            &[
                "--config",
                config.path().to_str().unwrap(),
                "--agent",
                "opencode",
                "--inference",
                "anthropic",
                "--with-policy",
            ],
        )
    })
}

fn hummingbird_claude_vertexai_image() -> &'static str {
    HUMMINGBIRD_CLAUDE_VERTEXAI_IMAGE.get_or_init(|| {
        let config = hummingbird_config_dir();
        build_image(
            "openshell-test-hummingbird-claude-vertexai:integration",
            &[
                "--config",
                config.path().to_str().unwrap(),
                "--agent",
                "claude",
                "--inference",
                "vertexai",
                "--with-policy",
            ],
        )
    })
}

fn hummingbird_opencode_vertexai_image() -> &'static str {
    HUMMINGBIRD_OPENCODE_VERTEXAI_IMAGE.get_or_init(|| {
        let config = hummingbird_config_dir();
        build_image(
            "openshell-test-hummingbird-opencode-vertexai:integration",
            &[
                "--config",
                config.path().to_str().unwrap(),
                "--agent",
                "opencode",
                "--inference",
                "vertexai",
                "--with-policy",
            ],
        )
    })
}

// ---------------------------------------------------------------------------
// Shared assertion helpers
// ---------------------------------------------------------------------------

fn check_users_and_groups(image: &str) {
    for user in ["sandbox", "supervisor"] {
        let out = run_in_image(image, &format!("id {user}"));
        assert!(out.status.success(), "{user} user not found in image");
    }

    for group in ["sandbox", "supervisor"] {
        let out = run_in_image(image, &format!("getent group {group}"));
        assert!(out.status.success(), "{group} group not found in image");
    }

    let out = run_in_image(image, "whoami");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "sandbox",
        "default image user is not sandbox"
    );

    let out = run_in_image(image, "echo $HOME");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "/sandbox",
        "sandbox home directory is not /sandbox"
    );
}

fn check_packages(image: &str) {
    for pkg in ["curl", "ip", "tar"] {
        let out = run_in_image(image, &format!("which {pkg}"));
        assert!(out.status.success(), "{pkg} not found in image");
    }
}

fn check_bash_entrypoint(image: &str) {
    let out = Command::new("podman")
        .args(["inspect", "--format", "{{json .Config.Entrypoint}}", image])
        .output()
        .expect("podman inspect should execute");
    assert!(out.status.success(), "podman inspect failed");

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("/bin/bash"),
        "expected /bin/bash entrypoint, got: {stdout}"
    );
}

fn check_policy_yaml(image: &str) {
    let out = run_in_image(image, "test -f /etc/openshell/policy.yaml");
    assert!(
        out.status.success(),
        "policy.yaml not found in /etc/openshell/"
    );
}

fn check_claude_in_path(image: &str, expected: bool) {
    let out = run_in_image(image, "which claude");
    if expected {
        assert!(out.status.success(), "claude not found in PATH");
    } else {
        assert!(!out.status.success(), "claude should not be in PATH");
    }
}

fn check_opencode_in_path(image: &str, expected: bool) {
    let out = run_in_image(image, "which opencode");
    if expected {
        assert!(out.status.success(), "opencode not found in PATH");
    } else {
        assert!(!out.status.success(), "opencode should not be in PATH");
    }
}

fn check_claude_policy(image: &str, expected: bool) {
    let out = run_in_image(image, "cat /etc/openshell/policy.yaml");
    assert!(out.status.success(), "failed to read policy.yaml");
    let policy = String::from_utf8_lossy(&out.stdout);
    if expected {
        assert!(
            policy.contains("name: claude-code"),
            "claude_code policy rule not found in policy.yaml"
        );
    } else {
        assert!(
            !policy.contains("name: claude-code"),
            "claude_code policy rule should not be present in policy.yaml"
        );
    }
}

fn check_opencode_policy(image: &str, expected: bool) {
    let out = run_in_image(image, "cat /etc/openshell/policy.yaml");
    assert!(out.status.success(), "failed to read policy.yaml");
    let policy = String::from_utf8_lossy(&out.stdout);
    if expected {
        assert!(
            policy.contains("name: opencode"),
            "opencode policy rule not found in policy.yaml"
        );
    } else {
        assert!(
            !policy.contains("name: opencode"),
            "opencode policy rule should not be present in policy.yaml"
        );
    }
}

fn check_anthropic_policy(image: &str, expected: bool) {
    let out = run_in_image(image, "cat /etc/openshell/policy.yaml");
    assert!(out.status.success(), "failed to read policy.yaml");
    let policy = String::from_utf8_lossy(&out.stdout);
    if expected {
        assert!(
            policy.contains("name: anthropic"),
            "anthropic inference policy rule not found in policy.yaml"
        );
    } else {
        assert!(
            !policy.contains("name: anthropic"),
            "anthropic inference policy rule should not be present in policy.yaml"
        );
    }
}

fn check_vertexai_policy(image: &str, expected: bool) {
    let out = run_in_image(image, "cat /etc/openshell/policy.yaml");
    assert!(out.status.success(), "failed to read policy.yaml");
    let policy = String::from_utf8_lossy(&out.stdout);
    if expected {
        assert!(
            policy.contains("name: vertexai"),
            "vertexai inference policy rule not found in policy.yaml"
        );
    } else {
        assert!(
            !policy.contains("name: vertexai"),
            "vertexai inference policy rule should not be present in policy.yaml"
        );
    }
}

fn check_ollama_policy(image: &str, expected: bool) {
    let out = run_in_image(image, "cat /etc/openshell/policy.yaml");
    assert!(out.status.success(), "failed to read policy.yaml");
    let policy = String::from_utf8_lossy(&out.stdout);
    if expected {
        assert!(
            policy.contains("name: ollama"),
            "ollama inference policy rule not found in policy.yaml"
        );
    } else {
        assert!(
            !policy.contains("name: ollama"),
            "ollama inference policy rule should not be present in policy.yaml"
        );
    }
}

fn check_openai_policy(image: &str, expected: bool) {
    let out = run_in_image(image, "cat /etc/openshell/policy.yaml");
    assert!(out.status.success(), "failed to read policy.yaml");
    let policy = String::from_utf8_lossy(&out.stdout);
    if expected {
        assert!(
            policy.contains("name: openai"),
            "openai inference policy rule not found in policy.yaml"
        );
    } else {
        assert!(
            !policy.contains("name: openai"),
            "openai inference policy rule should not be present in policy.yaml"
        );
    }
}

// ---------------------------------------------------------------------------
// Matrix: base_image × agent — one test module per variant
// ---------------------------------------------------------------------------

macro_rules! image_tests {
    ($mod_name:ident, $image_fn:ident, has_claude: $has_claude:literal, has_opencode: $has_opencode:literal, has_anthropic: $has_anthropic:literal, has_vertexai: $has_vertexai:literal, has_ollama: $has_ollama:literal, has_openai: $has_openai:literal) => {
        mod $mod_name {
            use super::*;

            #[test]
            #[ignore]
            fn users_and_groups_exist() {
                check_users_and_groups($image_fn());
            }

            #[test]
            #[ignore]
            fn packages_installed() {
                check_packages($image_fn());
            }

            #[test]
            #[ignore]
            fn bash_entrypoint() {
                check_bash_entrypoint($image_fn());
            }

            #[test]
            #[ignore]
            fn claude_in_path() {
                check_claude_in_path($image_fn(), $has_claude);
            }

            #[test]
            #[ignore]
            fn opencode_in_path() {
                check_opencode_in_path($image_fn(), $has_opencode);
            }

            #[test]
            #[ignore]
            fn policy_yaml_present() {
                check_policy_yaml($image_fn());
            }

            #[test]
            #[ignore]
            fn policy_has_claude_rules() {
                check_claude_policy($image_fn(), $has_claude);
            }

            #[test]
            #[ignore]
            fn policy_has_opencode_rules() {
                check_opencode_policy($image_fn(), $has_opencode);
            }

            #[test]
            #[ignore]
            fn policy_has_anthropic_rules() {
                check_anthropic_policy($image_fn(), $has_anthropic);
            }

            #[test]
            #[ignore]
            fn policy_has_vertexai_rules() {
                check_vertexai_policy($image_fn(), $has_vertexai);
            }

            #[test]
            #[ignore]
            fn policy_has_ollama_rules() {
                check_ollama_policy($image_fn(), $has_ollama);
            }

            #[test]
            #[ignore]
            fn policy_has_openai_rules() {
                check_openai_policy($image_fn(), $has_openai);
            }
        }
    };
}

image_tests!(ubuntu,                  ubuntu_image,                  has_claude: false, has_opencode: false, has_anthropic: false, has_vertexai: false, has_ollama: false, has_openai: false);
image_tests!(ubuntu_claude,           ubuntu_claude_image,           has_claude: true,  has_opencode: false, has_anthropic: true,  has_vertexai: false, has_ollama: false, has_openai: false);
image_tests!(ubuntu_opencode,         ubuntu_opencode_image,         has_claude: false, has_opencode: true,  has_anthropic: true,  has_vertexai: false, has_ollama: false, has_openai: false);
image_tests!(ubuntu_claude_vertexai,  ubuntu_claude_vertexai_image,  has_claude: true,  has_opencode: false, has_anthropic: false, has_vertexai: true,  has_ollama: false, has_openai: false);
image_tests!(ubuntu_opencode_vertexai,ubuntu_opencode_vertexai_image,has_claude: false, has_opencode: true,  has_anthropic: false, has_vertexai: true,  has_ollama: false, has_openai: false);
image_tests!(fedora,                  fedora_image,                  has_claude: false, has_opencode: false, has_anthropic: false, has_vertexai: false, has_ollama: false, has_openai: false);
image_tests!(fedora_claude,           fedora_claude_image,           has_claude: true,  has_opencode: false, has_anthropic: true,  has_vertexai: false, has_ollama: false, has_openai: false);
image_tests!(fedora_opencode,         fedora_opencode_image,         has_claude: false, has_opencode: true,  has_anthropic: true,  has_vertexai: false, has_ollama: false, has_openai: false);
image_tests!(fedora_claude_vertexai,  fedora_claude_vertexai_image,  has_claude: true,  has_opencode: false, has_anthropic: false, has_vertexai: true,  has_ollama: false, has_openai: false);
image_tests!(fedora_opencode_vertexai,fedora_opencode_vertexai_image,has_claude: false, has_opencode: true,  has_anthropic: false, has_vertexai: true,  has_ollama: false, has_openai: false);
image_tests!(ubi,                     ubi_image,                     has_claude: false, has_opencode: false, has_anthropic: false, has_vertexai: false, has_ollama: false, has_openai: false);
image_tests!(ubi_claude,              ubi_claude_image,              has_claude: true,  has_opencode: false, has_anthropic: true,  has_vertexai: false, has_ollama: false, has_openai: false);
image_tests!(ubi_opencode,            ubi_opencode_image,            has_claude: false, has_opencode: true,  has_anthropic: true,  has_vertexai: false, has_ollama: false, has_openai: false);
image_tests!(ubi_claude_vertexai,     ubi_claude_vertexai_image,     has_claude: true,  has_opencode: false, has_anthropic: false, has_vertexai: true,  has_ollama: false, has_openai: false);
image_tests!(ubi_opencode_vertexai,   ubi_opencode_vertexai_image,   has_claude: false, has_opencode: true,  has_anthropic: false, has_vertexai: true,  has_ollama: false, has_openai: false);
image_tests!(hummingbird,                     hummingbird_image,                     has_claude: false, has_opencode: false, has_anthropic: false, has_vertexai: false, has_ollama: false, has_openai: false);
image_tests!(hummingbird_claude,              hummingbird_claude_image,              has_claude: true,  has_opencode: false, has_anthropic: true,  has_vertexai: false, has_ollama: false, has_openai: false);
image_tests!(hummingbird_opencode,            hummingbird_opencode_image,            has_claude: false, has_opencode: true,  has_anthropic: true,  has_vertexai: false, has_ollama: false, has_openai: false);
image_tests!(hummingbird_claude_vertexai,     hummingbird_claude_vertexai_image,     has_claude: true,  has_opencode: false, has_anthropic: false, has_vertexai: true,  has_ollama: false, has_openai: false);
image_tests!(hummingbird_opencode_vertexai,   hummingbird_opencode_vertexai_image,   has_claude: false, has_opencode: true,  has_anthropic: false, has_vertexai: true,  has_ollama: false, has_openai: false);
image_tests!(ubuntu_opencode_ollama,          ubuntu_opencode_ollama_image,          has_claude: false, has_opencode: true,  has_anthropic: false, has_vertexai: false, has_ollama: true,  has_openai: false);
image_tests!(fedora_opencode_ollama,          fedora_opencode_ollama_image,          has_claude: false, has_opencode: true,  has_anthropic: false, has_vertexai: false, has_ollama: true,  has_openai: false);
image_tests!(ubi_opencode_ollama,             ubi_opencode_ollama_image,             has_claude: false, has_opencode: true,  has_anthropic: false, has_vertexai: false, has_ollama: true,  has_openai: false);
image_tests!(hummingbird_opencode_ollama,     hummingbird_opencode_ollama_image,     has_claude: false, has_opencode: true,  has_anthropic: false, has_vertexai: false, has_ollama: true,  has_openai: false);
image_tests!(ubuntu_opencode_openai,          ubuntu_opencode_openai_image,          has_claude: false, has_opencode: true,  has_anthropic: false, has_vertexai: false, has_ollama: false, has_openai: true);
image_tests!(fedora_opencode_openai,          fedora_opencode_openai_image,          has_claude: false, has_opencode: true,  has_anthropic: false, has_vertexai: false, has_ollama: false, has_openai: true);
image_tests!(ubi_opencode_openai,             ubi_opencode_openai_image,             has_claude: false, has_opencode: true,  has_anthropic: false, has_vertexai: false, has_ollama: false, has_openai: true);
image_tests!(hummingbird_opencode_openai,     hummingbird_opencode_openai_image,     has_claude: false, has_opencode: true,  has_anthropic: false, has_vertexai: false, has_ollama: false, has_openai: true);

// ---------------------------------------------------------------------------
// Workspace helpers for feature-based builds
// ---------------------------------------------------------------------------

fn workspace_dir(workspace_json: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let kaiden = dir.path().join(".kaiden");
    std::fs::create_dir_all(&kaiden).unwrap();
    std::fs::write(kaiden.join("workspace.json"), workspace_json).unwrap();
    dir
}

fn build_image_with_workspace(tag: &str, workspace_json: &str, extra_args: &[&str]) -> String {
    let dir = workspace_dir(workspace_json);
    let binary = env!("CARGO_BIN_EXE_openshell-image-builder");
    let status = Command::new(binary)
        .current_dir(dir.path())
        .args(["--runtime", "podman"])
        .arg("--with-workspace-config")
        .args(extra_args)
        .arg(tag)
        .status()
        .expect("binary should run");
    assert!(status.success(), "image build failed for tag {tag}");
    tag.to_string()
}

// ---------------------------------------------------------------------------
// Feature image singletons — ubuntu (default) and fedora variants
// ---------------------------------------------------------------------------

static FEATURE_COMMON_UTILS_UBUNTU_IMAGE: OnceLock<String> = OnceLock::new();
static FEATURE_NODE_UBUNTU_IMAGE: OnceLock<String> = OnceLock::new();
static FEATURE_PYTHON_UBUNTU_IMAGE: OnceLock<String> = OnceLock::new();
static FEATURE_COMMON_UTILS_FEDORA_IMAGE: OnceLock<String> = OnceLock::new();
static FEATURE_NODE_FEDORA_IMAGE: OnceLock<String> = OnceLock::new();
static FEATURE_PYTHON_FEDORA_IMAGE: OnceLock<String> = OnceLock::new();
static FEATURE_COMMON_UTILS_UBI_IMAGE: OnceLock<String> = OnceLock::new();
static FEATURE_NODE_UBI_IMAGE: OnceLock<String> = OnceLock::new();
static FEATURE_PYTHON_UBI_IMAGE: OnceLock<String> = OnceLock::new();
static FEATURE_LOCAL_UBUNTU_IMAGE: OnceLock<String> = OnceLock::new();
static FEATURE_LOCAL_FEDORA_IMAGE: OnceLock<String> = OnceLock::new();
static FEATURE_LOCAL_UBI_IMAGE: OnceLock<String> = OnceLock::new();

const COMMON_UTILS_WORKSPACE: &str = r#"{
    "features": {
        "ghcr.io/devcontainers/features/common-utils:2": {
            "installZsh": true
        }
    }
}"#;

const NODE_WORKSPACE: &str = r#"{
    "features": {
        "ghcr.io/devcontainers/features/node:1": {
            "version": "22"
        }
    }
}"#;

const PYTHON_WORKSPACE: &str = r#"{
    "features": {
        "ghcr.io/devcontainers/features/python:1": {
            "version": "os-provided",
            "installTools": true
        }
    }
}"#;

const NETWORK_HOSTS_WORKSPACE: &str = r#"{
    "network": {
        "hosts": ["example.com"]
    }
}"#;

fn feature_common_utils_ubuntu_image() -> &'static str {
    FEATURE_COMMON_UTILS_UBUNTU_IMAGE.get_or_init(|| {
        build_image_with_workspace(
            "openshell-test-feature-common-utils-ubuntu:integration",
            COMMON_UTILS_WORKSPACE,
            &[],
        )
    })
}

fn feature_node_ubuntu_image() -> &'static str {
    FEATURE_NODE_UBUNTU_IMAGE.get_or_init(|| {
        build_image_with_workspace(
            "openshell-test-feature-node-ubuntu:integration",
            NODE_WORKSPACE,
            &[],
        )
    })
}

fn feature_python_ubuntu_image() -> &'static str {
    FEATURE_PYTHON_UBUNTU_IMAGE.get_or_init(|| {
        build_image_with_workspace(
            "openshell-test-feature-python-ubuntu:integration",
            PYTHON_WORKSPACE,
            &[],
        )
    })
}

fn feature_common_utils_fedora_image() -> &'static str {
    FEATURE_COMMON_UTILS_FEDORA_IMAGE.get_or_init(|| {
        let config = fedora_config_dir();
        build_image_with_workspace(
            "openshell-test-feature-common-utils-fedora:integration",
            COMMON_UTILS_WORKSPACE,
            &["--config", config.path().to_str().unwrap()],
        )
    })
}

fn feature_node_fedora_image() -> &'static str {
    FEATURE_NODE_FEDORA_IMAGE.get_or_init(|| {
        let config = fedora_config_dir();
        build_image_with_workspace(
            "openshell-test-feature-node-fedora:integration",
            NODE_WORKSPACE,
            &["--config", config.path().to_str().unwrap()],
        )
    })
}

fn feature_python_fedora_image() -> &'static str {
    FEATURE_PYTHON_FEDORA_IMAGE.get_or_init(|| {
        let config = fedora_config_dir();
        build_image_with_workspace(
            "openshell-test-feature-python-fedora:integration",
            PYTHON_WORKSPACE,
            &["--config", config.path().to_str().unwrap()],
        )
    })
}

fn feature_common_utils_ubi_image() -> &'static str {
    FEATURE_COMMON_UTILS_UBI_IMAGE.get_or_init(|| {
        let config = ubi_config_dir();
        build_image_with_workspace(
            "openshell-test-feature-common-utils-ubi:integration",
            COMMON_UTILS_WORKSPACE,
            &["--config", config.path().to_str().unwrap()],
        )
    })
}

fn feature_node_ubi_image() -> &'static str {
    FEATURE_NODE_UBI_IMAGE.get_or_init(|| {
        let config = ubi_config_dir();
        build_image_with_workspace(
            "openshell-test-feature-node-ubi:integration",
            NODE_WORKSPACE,
            &["--config", config.path().to_str().unwrap()],
        )
    })
}

fn feature_python_ubi_image() -> &'static str {
    FEATURE_PYTHON_UBI_IMAGE.get_or_init(|| {
        let config = ubi_config_dir();
        build_image_with_workspace(
            "openshell-test-feature-python-ubi:integration",
            PYTHON_WORKSPACE,
            &["--config", config.path().to_str().unwrap()],
        )
    })
}

fn local_feature_workspace_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let kaiden = dir.path().join(".kaiden");
    let feature_dir = kaiden.join("my-feature");
    std::fs::create_dir_all(&feature_dir).unwrap();
    std::fs::write(
        kaiden.join("workspace.json"),
        r#"{"features": {"./my-feature": {"filename": "hello-from-feature"}}}"#,
    )
    .unwrap();
    std::fs::write(
        feature_dir.join("devcontainer-feature.json"),
        r#"{"id": "my-feature", "version": "1.0.0", "name": "My Test Feature", "options": {"filename": {"type": "string", "default": "default-filename"}}}"#,
    )
    .unwrap();
    std::fs::write(
        feature_dir.join("install.sh"),
        "#!/bin/sh\nsh \"$(dirname \"$0\")/main.sh\"\n",
    )
    .unwrap();
    std::fs::write(
        feature_dir.join("main.sh"),
        "#!/bin/sh\ntouch \"$_REMOTE_USER_HOME/$FILENAME\"\n",
    )
    .unwrap();
    dir
}

fn build_image_with_local_feature(tag: &str, extra_args: &[&str]) -> String {
    let dir = local_feature_workspace_dir();
    let binary = env!("CARGO_BIN_EXE_openshell-image-builder");
    let status = Command::new(binary)
        .current_dir(dir.path())
        .args(["--runtime", "podman"])
        .arg("--with-workspace-config")
        .args(extra_args)
        .arg(tag)
        .status()
        .expect("binary should run");
    assert!(status.success(), "image build failed for tag {tag}");
    tag.to_string()
}

fn feature_local_ubuntu_image() -> &'static str {
    FEATURE_LOCAL_UBUNTU_IMAGE.get_or_init(|| {
        build_image_with_local_feature("openshell-test-feature-local-ubuntu:integration", &[])
    })
}

fn feature_local_fedora_image() -> &'static str {
    FEATURE_LOCAL_FEDORA_IMAGE.get_or_init(|| {
        let config = fedora_config_dir();
        build_image_with_local_feature(
            "openshell-test-feature-local-fedora:integration",
            &["--config", config.path().to_str().unwrap()],
        )
    })
}

fn feature_local_ubi_image() -> &'static str {
    FEATURE_LOCAL_UBI_IMAGE.get_or_init(|| {
        let config = ubi_config_dir();
        build_image_with_local_feature(
            "openshell-test-feature-local-ubi:integration",
            &["--config", config.path().to_str().unwrap()],
        )
    })
}

// ---------------------------------------------------------------------------
// Skills image helpers
// ---------------------------------------------------------------------------

fn skills_workspace_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let kaiden = dir.path().join(".kaiden");
    std::fs::create_dir_all(&kaiden).unwrap();
    let skill_dir = dir.path().join("my-skill");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(skill_dir.join("SKILL.md"), "# my-skill\n").unwrap();
    std::fs::write(
        kaiden.join("workspace.json"),
        r#"{"skills": ["./my-skill"]}"#,
    )
    .unwrap();
    dir
}

fn build_image_with_skills(tag: &str, extra_args: &[&str]) -> String {
    let dir = skills_workspace_dir();
    let binary = env!("CARGO_BIN_EXE_openshell-image-builder");
    let status = Command::new(binary)
        .current_dir(dir.path())
        .args(["--runtime", "podman"])
        .arg("--with-workspace-config")
        .args(extra_args)
        .arg(tag)
        .status()
        .expect("binary should run");
    assert!(status.success(), "image build failed for tag {tag}");
    tag.to_string()
}

fn ubuntu_claude_skills_image() -> &'static str {
    UBUNTU_CLAUDE_SKILLS_IMAGE.get_or_init(|| {
        build_image_with_skills(
            "openshell-test-ubuntu-claude-skills:integration",
            &["--agent", "claude"],
        )
    })
}

fn ubuntu_opencode_skills_image() -> &'static str {
    UBUNTU_OPENCODE_SKILLS_IMAGE.get_or_init(|| {
        build_image_with_skills(
            "openshell-test-ubuntu-opencode-skills:integration",
            &["--agent", "opencode"],
        )
    })
}

// ---------------------------------------------------------------------------
// Helpers and singletons for --with-workspace-config absence tests
// ---------------------------------------------------------------------------

/// Like build_image_with_workspace but WITHOUT --with-workspace-config, so the
/// workspace file is present on disk but deliberately ignored by the tool.
fn build_image_in_workspace_dir(tag: &str, workspace_json: &str, extra_args: &[&str]) -> String {
    let dir = workspace_dir(workspace_json);
    let binary = env!("CARGO_BIN_EXE_openshell-image-builder");
    let status = Command::new(binary)
        .current_dir(dir.path())
        .args(["--runtime", "podman"])
        .args(extra_args)
        .arg(tag)
        .status()
        .expect("binary should run");
    assert!(status.success(), "image build failed for tag {tag}");
    tag.to_string()
}

fn no_workspace_config_oci_feature_ubuntu_image() -> &'static str {
    NO_WORKSPACE_CONFIG_OCI_FEATURE_UBUNTU_IMAGE.get_or_init(|| {
        build_image_in_workspace_dir(
            "openshell-test-no-workspace-config-oci-feature-ubuntu:integration",
            COMMON_UTILS_WORKSPACE,
            &[],
        )
    })
}

fn no_workspace_config_local_feature_ubuntu_image() -> &'static str {
    NO_WORKSPACE_CONFIG_LOCAL_FEATURE_UBUNTU_IMAGE.get_or_init(|| {
        let dir = local_feature_workspace_dir();
        let binary = env!("CARGO_BIN_EXE_openshell-image-builder");
        let status = Command::new(binary)
            .current_dir(dir.path())
            .args(["--runtime", "podman"])
            .arg("openshell-test-no-workspace-config-local-feature-ubuntu:integration")
            .status()
            .expect("binary should run");
        assert!(status.success(), "image build failed");
        "openshell-test-no-workspace-config-local-feature-ubuntu:integration".to_string()
    })
}

fn no_workspace_config_claude_skills_ubuntu_image() -> &'static str {
    NO_WORKSPACE_CONFIG_CLAUDE_SKILLS_UBUNTU_IMAGE.get_or_init(|| {
        let dir = skills_workspace_dir();
        let binary = env!("CARGO_BIN_EXE_openshell-image-builder");
        let status = Command::new(binary)
            .current_dir(dir.path())
            .args(["--runtime", "podman"])
            .args(["--agent", "claude"])
            .arg("openshell-test-no-workspace-config-claude-skills-ubuntu:integration")
            .status()
            .expect("binary should run");
        assert!(status.success(), "image build failed");
        "openshell-test-no-workspace-config-claude-skills-ubuntu:integration".to_string()
    })
}

fn no_workspace_config_opencode_skills_ubuntu_image() -> &'static str {
    NO_WORKSPACE_CONFIG_OPENCODE_SKILLS_UBUNTU_IMAGE.get_or_init(|| {
        let dir = skills_workspace_dir();
        let binary = env!("CARGO_BIN_EXE_openshell-image-builder");
        let status = Command::new(binary)
            .current_dir(dir.path())
            .args(["--runtime", "podman"])
            .args(["--agent", "opencode"])
            .arg("openshell-test-no-workspace-config-opencode-skills-ubuntu:integration")
            .status()
            .expect("binary should run");
        assert!(status.success(), "image build failed");
        "openshell-test-no-workspace-config-opencode-skills-ubuntu:integration".to_string()
    })
}

fn no_workspace_config_network_hosts_ubuntu_image() -> &'static str {
    NO_WORKSPACE_CONFIG_NETWORK_HOSTS_UBUNTU_IMAGE.get_or_init(|| {
        build_image_in_workspace_dir(
            "openshell-test-no-workspace-config-network-hosts-ubuntu:integration",
            NETWORK_HOSTS_WORKSPACE,
            &["--with-policy"],
        )
    })
}

fn ubuntu_opencode_ollama_image() -> &'static str {
    UBUNTU_OPENCODE_OLLAMA_IMAGE.get_or_init(|| {
        build_image(
            "openshell-test-ubuntu-opencode-ollama:integration",
            &[
                "--agent",
                "opencode",
                "--inference",
                "ollama",
                "--with-policy",
                "--with-agent-settings",
            ],
        )
    })
}

fn fedora_opencode_ollama_image() -> &'static str {
    FEDORA_OPENCODE_OLLAMA_IMAGE.get_or_init(|| {
        let config = fedora_config_dir();
        build_image(
            "openshell-test-fedora-opencode-ollama:integration",
            &[
                "--config",
                config.path().to_str().unwrap(),
                "--agent",
                "opencode",
                "--inference",
                "ollama",
                "--with-policy",
            ],
        )
    })
}

fn ubi_opencode_ollama_image() -> &'static str {
    UBI_OPENCODE_OLLAMA_IMAGE.get_or_init(|| {
        let config = ubi_config_dir();
        build_image(
            "openshell-test-ubi-opencode-ollama:integration",
            &[
                "--config",
                config.path().to_str().unwrap(),
                "--agent",
                "opencode",
                "--inference",
                "ollama",
                "--with-policy",
            ],
        )
    })
}

fn hummingbird_opencode_ollama_image() -> &'static str {
    HUMMINGBIRD_OPENCODE_OLLAMA_IMAGE.get_or_init(|| {
        let config = hummingbird_config_dir();
        build_image(
            "openshell-test-hummingbird-opencode-ollama:integration",
            &[
                "--config",
                config.path().to_str().unwrap(),
                "--agent",
                "opencode",
                "--inference",
                "ollama",
                "--with-policy",
            ],
        )
    })
}

fn ubuntu_opencode_openai_image() -> &'static str {
    UBUNTU_OPENCODE_OPENAI_IMAGE.get_or_init(|| {
        build_image(
            "openshell-test-ubuntu-opencode-openai:integration",
            &[
                "--agent",
                "opencode",
                "--inference",
                "openai",
                "--with-policy",
            ],
        )
    })
}

fn fedora_opencode_openai_image() -> &'static str {
    FEDORA_OPENCODE_OPENAI_IMAGE.get_or_init(|| {
        let config = fedora_config_dir();
        build_image(
            "openshell-test-fedora-opencode-openai:integration",
            &[
                "--config",
                config.path().to_str().unwrap(),
                "--agent",
                "opencode",
                "--inference",
                "openai",
                "--with-policy",
            ],
        )
    })
}

fn ubi_opencode_openai_image() -> &'static str {
    UBI_OPENCODE_OPENAI_IMAGE.get_or_init(|| {
        let config = ubi_config_dir();
        build_image(
            "openshell-test-ubi-opencode-openai:integration",
            &[
                "--config",
                config.path().to_str().unwrap(),
                "--agent",
                "opencode",
                "--inference",
                "openai",
                "--with-policy",
            ],
        )
    })
}

fn hummingbird_opencode_openai_image() -> &'static str {
    HUMMINGBIRD_OPENCODE_OPENAI_IMAGE.get_or_init(|| {
        let config = hummingbird_config_dir();
        build_image(
            "openshell-test-hummingbird-opencode-openai:integration",
            &[
                "--config",
                config.path().to_str().unwrap(),
                "--agent",
                "opencode",
                "--inference",
                "openai",
                "--with-policy",
            ],
        )
    })
}

fn ubuntu_opencode_openai_model_image() -> &'static str {
    UBUNTU_OPENCODE_OPENAI_MODEL_IMAGE.get_or_init(|| {
        build_image(
            "openshell-test-ubuntu-opencode-openai-model:integration",
            &[
                "--agent",
                "opencode",
                "--inference",
                "openai",
                "--model",
                MODEL_OPENAI,
                "--with-agent-settings",
            ],
        )
    })
}

fn ubuntu_no_policy_image() -> &'static str {
    UBUNTU_NO_POLICY_IMAGE
        .get_or_init(|| build_image("openshell-test-ubuntu-no-policy:integration", &[]))
}

fn ubuntu_claude_no_agent_settings_image() -> &'static str {
    UBUNTU_CLAUDE_NO_AGENT_SETTINGS_IMAGE.get_or_init(|| {
        build_image(
            "openshell-test-ubuntu-claude-no-agent-settings:integration",
            &["--agent", "claude"],
        )
    })
}

fn ubuntu_image_mount_image() -> &'static str {
    UBUNTU_IMAGE_MOUNT_IMAGE.get_or_init(|| {
        // Write a temporary YAML file with a known name so the mount name is predictable.
        let dir = tempfile::tempdir().unwrap();
        let yaml_path = dir.path().join("curl.yaml");
        std::fs::write(
            &yaml_path,
            "image: docker.io/curlimages/curl:latest\ninit: export PATH=$MOUNT/usr/bin:$PATH\n",
        )
        .unwrap();
        let tag = build_image(
            "openshell-test-ubuntu-image-mount:integration",
            &["--image-mount", yaml_path.to_str().unwrap()],
        );
        // `dir` is dropped here; the image is already built so the file is no longer needed.
        tag
    })
}

// ---------------------------------------------------------------------------
// Feature integration tests — one macro per feature, instantiated per base image
// ---------------------------------------------------------------------------

macro_rules! feature_common_utils_tests {
    ($mod_name:ident, $image_fn:ident, $base_image_fn:ident) => {
        mod $mod_name {
            use super::*;

            #[test]
            #[ignore]
            fn zsh_installed() {
                let out = run_in_image($image_fn(), "which zsh");
                assert!(out.status.success(), "zsh not found in image");
            }

            #[test]
            #[ignore]
            fn zsh_not_in_base_image() {
                let out = run_in_image($base_image_fn(), "which zsh");
                assert!(!out.status.success(), "zsh should not be in base image");
            }
        }
    };
}

macro_rules! feature_node_tests {
    ($mod_name:ident, $image_fn:ident, $base_image_fn:ident) => {
        mod $mod_name {
            use super::*;

            #[test]
            #[ignore]
            fn node_in_path() {
                let out = run_in_image($image_fn(), "node --version");
                assert!(out.status.success(), "node not found in PATH");
            }

            #[test]
            #[ignore]
            fn node_not_in_base_image() {
                let out = run_in_image($base_image_fn(), "which node");
                assert!(!out.status.success(), "node should not be in base image");
            }

            #[test]
            #[ignore]
            fn npm_in_path() {
                let out = run_in_image($image_fn(), "npm --version");
                assert!(out.status.success(), "npm not found in PATH");
            }

            #[test]
            #[ignore]
            fn npm_not_in_base_image() {
                let out = run_in_image($base_image_fn(), "which npm");
                assert!(!out.status.success(), "npm should not be in base image");
            }
        }
    };
}

macro_rules! feature_python_tests {
    ($mod_name:ident, $image_fn:ident, $base_image_fn:ident) => {
        mod $mod_name {
            use super::*;

            #[test]
            #[ignore]
            fn python3_available() {
                let out = run_in_image($image_fn(), "python3 --version");
                assert!(out.status.success(), "python3 not found in image");
            }

            #[test]
            #[ignore]
            fn flake8_installed() {
                let out = run_in_image($image_fn(), "which flake8");
                assert!(out.status.success(), "flake8 not found in PATH");
            }

            #[test]
            #[ignore]
            fn flake8_not_in_base_image() {
                let out = run_in_image($base_image_fn(), "which flake8");
                assert!(!out.status.success(), "flake8 should not be in base image");
            }

            #[test]
            #[ignore]
            fn pylint_installed() {
                let out = run_in_image($image_fn(), "which pylint");
                assert!(out.status.success(), "pylint not found in PATH");
            }

            #[test]
            #[ignore]
            fn pylint_not_in_base_image() {
                let out = run_in_image($base_image_fn(), "which pylint");
                assert!(!out.status.success(), "pylint should not be in base image");
            }
        }
    };
}

macro_rules! feature_local_tests {
    ($mod_name:ident, $image_fn:ident, $base_image_fn:ident) => {
        mod $mod_name {
            use super::*;

            #[test]
            #[ignore]
            fn file_created_by_feature() {
                let out = run_in_image($image_fn(), "test -f /sandbox/hello-from-feature");
                assert!(out.status.success(), "file not created by local feature");
            }

            #[test]
            #[ignore]
            fn file_not_in_base_image() {
                let out = run_in_image($base_image_fn(), "test -f /sandbox/hello-from-feature");
                assert!(!out.status.success(), "file should not exist in base image");
            }
        }
    };
}

feature_local_tests!(
    feature_local_ubuntu,
    feature_local_ubuntu_image,
    ubuntu_image
);
feature_local_tests!(
    feature_local_fedora,
    feature_local_fedora_image,
    fedora_image
);
feature_local_tests!(feature_local_ubi, feature_local_ubi_image, ubi_image);

feature_common_utils_tests!(
    feature_common_utils_ubuntu,
    feature_common_utils_ubuntu_image,
    ubuntu_image
);
feature_common_utils_tests!(
    feature_common_utils_fedora,
    feature_common_utils_fedora_image,
    fedora_image
);
feature_common_utils_tests!(
    feature_common_utils_ubi,
    feature_common_utils_ubi_image,
    ubi_image
);
feature_node_tests!(feature_node_ubuntu, feature_node_ubuntu_image, ubuntu_image);
feature_node_tests!(feature_node_fedora, feature_node_fedora_image, fedora_image);
feature_node_tests!(feature_node_ubi, feature_node_ubi_image, ubi_image);
feature_python_tests!(
    feature_python_ubuntu,
    feature_python_ubuntu_image,
    ubuntu_image
);
feature_python_tests!(
    feature_python_fedora,
    feature_python_fedora_image,
    fedora_image
);
feature_python_tests!(feature_python_ubi, feature_python_ubi_image, ubi_image);

// ---------------------------------------------------------------------------
// Agent settings integration tests
// ---------------------------------------------------------------------------

mod agent_settings_claude {
    use super::*;

    #[test]
    #[ignore]
    fn settings_file_present_in_sandbox() {
        let out = run_in_image(
            ubuntu_claude_settings_image(),
            "test -f /sandbox/my-claude-settings",
        );
        assert!(
            out.status.success(),
            "claude settings file not found in /sandbox"
        );
    }

    #[test]
    #[ignore]
    fn settings_file_not_in_image_without_settings() {
        let out = run_in_image(ubuntu_claude_image(), "test -f /sandbox/my-claude-settings");
        assert!(
            !out.status.success(),
            "claude settings file should not be present in image built without agent settings"
        );
    }

    #[test]
    #[ignore]
    fn settings_file_owned_by_sandbox() {
        let out = run_in_image(
            ubuntu_claude_settings_image(),
            "stat -c '%U' /sandbox/my-claude-settings",
        );
        assert!(out.status.success(), "failed to stat claude settings file");
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            "sandbox",
            "claude settings file not owned by sandbox"
        );
    }
}

mod agent_settings_opencode {
    use super::*;

    #[test]
    #[ignore]
    fn settings_file_present_in_sandbox() {
        let out = run_in_image(
            ubuntu_opencode_settings_image(),
            "test -f /sandbox/my-opencode-settings",
        );
        assert!(
            out.status.success(),
            "opencode settings file not found in /sandbox"
        );
    }

    #[test]
    #[ignore]
    fn settings_file_not_in_image_without_settings() {
        let out = run_in_image(
            ubuntu_opencode_image(),
            "test -f /sandbox/my-opencode-settings",
        );
        assert!(
            !out.status.success(),
            "opencode settings file should not be present in image built without agent settings"
        );
    }

    #[test]
    #[ignore]
    fn settings_file_owned_by_sandbox() {
        let out = run_in_image(
            ubuntu_opencode_settings_image(),
            "stat -c '%U' /sandbox/my-opencode-settings",
        );
        assert!(
            out.status.success(),
            "failed to stat opencode settings file"
        );
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            "sandbox",
            "opencode settings file not owned by sandbox"
        );
    }
}

// ---------------------------------------------------------------------------
// Claude onboarding-skip integration tests
// ---------------------------------------------------------------------------

mod claude_onboarding {
    use super::*;

    #[test]
    #[ignore]
    fn claude_json_present_without_settings_dir() {
        let out = run_in_image(ubuntu_claude_image(), "test -f /sandbox/.claude.json");
        assert!(out.status.success(), ".claude.json not found in /sandbox");
    }

    #[test]
    #[ignore]
    fn claude_json_has_completed_onboarding() {
        let out = run_in_image(
            ubuntu_claude_image(),
            r#"grep -q '"hasCompletedOnboarding": true' /sandbox/.claude.json"#,
        );
        assert!(
            out.status.success(),
            "hasCompletedOnboarding is not true in .claude.json"
        );
    }

    #[test]
    #[ignore]
    fn claude_json_has_trust_dialog_accepted() {
        let out = run_in_image(
            ubuntu_claude_image(),
            r#"grep -q '"hasTrustDialogAccepted": true' /sandbox/.claude.json"#,
        );
        assert!(
            out.status.success(),
            "hasTrustDialogAccepted is not true in .claude.json"
        );
    }

    #[test]
    #[ignore]
    fn claude_json_owned_by_sandbox() {
        let out = run_in_image(ubuntu_claude_image(), "stat -c '%U' /sandbox/.claude.json");
        assert!(out.status.success(), "failed to stat .claude.json");
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            "sandbox",
            ".claude.json not owned by sandbox"
        );
    }

    #[test]
    #[ignore]
    fn claude_json_merges_with_existing_content() {
        let out = run_in_image(
            ubuntu_claude_with_claude_json_image(),
            r#"grep -q '"existingField": "myvalue"' /sandbox/.claude.json"#,
        );
        assert!(
            out.status.success(),
            "existing content not preserved in merged .claude.json"
        );
    }

    #[test]
    #[ignore]
    fn claude_json_sets_flags_when_merging_with_existing_content() {
        let out = run_in_image(
            ubuntu_claude_with_claude_json_image(),
            r#"grep -q '"hasCompletedOnboarding": true' /sandbox/.claude.json"#,
        );
        assert!(
            out.status.success(),
            "hasCompletedOnboarding not set after merging with existing .claude.json"
        );
    }

    #[test]
    #[ignore]
    fn opencode_has_no_claude_json() {
        let out = run_in_image(ubuntu_opencode_image(), "test -f /sandbox/.claude.json");
        assert!(
            !out.status.success(),
            ".claude.json should not be present in an opencode image"
        );
    }
}

// ---------------------------------------------------------------------------
// Skills integration tests
// ---------------------------------------------------------------------------

mod skills_claude {
    use super::*;

    #[test]
    #[ignore]
    fn skill_dir_present_in_claude_skills_path() {
        let out = run_in_image(
            ubuntu_claude_skills_image(),
            "test -d /sandbox/.claude/skills/my-skill",
        );
        assert!(
            out.status.success(),
            "skill directory not found in /sandbox/.claude/skills/"
        );
    }

    #[test]
    #[ignore]
    fn skill_file_present_in_skill_dir() {
        let out = run_in_image(
            ubuntu_claude_skills_image(),
            "test -f /sandbox/.claude/skills/my-skill/SKILL.md",
        );
        assert!(
            out.status.success(),
            "SKILL.md not found in /sandbox/.claude/skills/my-skill/"
        );
    }

    #[test]
    #[ignore]
    fn skill_dir_owned_by_sandbox() {
        let out = run_in_image(
            ubuntu_claude_skills_image(),
            "stat -c '%U' /sandbox/.claude/skills/my-skill",
        );
        assert!(out.status.success(), "failed to stat skill directory");
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            "sandbox",
            "skill directory not owned by sandbox"
        );
    }

    #[test]
    #[ignore]
    fn skill_not_present_in_image_without_skills() {
        let out = run_in_image(
            ubuntu_claude_image(),
            "test -d /sandbox/.claude/skills/my-skill",
        );
        assert!(
            !out.status.success(),
            "skill directory should not be present in image built without skills"
        );
    }
}

mod skills_opencode {
    use super::*;

    #[test]
    #[ignore]
    fn skill_dir_present_in_opencode_skills_path() {
        let out = run_in_image(
            ubuntu_opencode_skills_image(),
            "test -d /sandbox/.opencode/skills/my-skill",
        );
        assert!(
            out.status.success(),
            "skill directory not found in /sandbox/.opencode/skills/"
        );
    }

    #[test]
    #[ignore]
    fn skill_file_present_in_skill_dir() {
        let out = run_in_image(
            ubuntu_opencode_skills_image(),
            "test -f /sandbox/.opencode/skills/my-skill/SKILL.md",
        );
        assert!(
            out.status.success(),
            "SKILL.md not found in /sandbox/.opencode/skills/my-skill/"
        );
    }

    #[test]
    #[ignore]
    fn skill_dir_owned_by_sandbox() {
        let out = run_in_image(
            ubuntu_opencode_skills_image(),
            "stat -c '%U' /sandbox/.opencode/skills/my-skill",
        );
        assert!(out.status.success(), "failed to stat skill directory");
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            "sandbox",
            "skill directory not owned by sandbox"
        );
    }

    #[test]
    #[ignore]
    fn skill_not_present_in_image_without_skills() {
        let out = run_in_image(
            ubuntu_opencode_image(),
            "test -d /sandbox/.opencode/skills/my-skill",
        );
        assert!(
            !out.status.success(),
            "skill directory should not be present in image built without skills"
        );
    }
}

// ---------------------------------------------------------------------------
// Tests: workspace content excluded when --with-workspace-config is absent
// ---------------------------------------------------------------------------

mod without_workspace_config {
    use super::*;

    #[test]
    #[ignore]
    fn oci_feature_not_installed() {
        // COMMON_UTILS_WORKSPACE declares common-utils which installs zsh.
        // Without --with-workspace-config the workspace file must be ignored.
        let out = run_in_image(no_workspace_config_oci_feature_ubuntu_image(), "which zsh");
        assert!(
            !out.status.success(),
            "zsh should not be installed when --with-workspace-config is absent"
        );
    }

    #[test]
    #[ignore]
    fn local_feature_not_applied() {
        // The local feature workspace creates /sandbox/hello-from-feature.
        // Without --with-workspace-config that file must not exist.
        let out = run_in_image(
            no_workspace_config_local_feature_ubuntu_image(),
            "test -f /sandbox/hello-from-feature",
        );
        assert!(
            !out.status.success(),
            "local feature file should not exist when --with-workspace-config is absent"
        );
    }

    #[test]
    #[ignore]
    fn claude_skills_not_copied() {
        let out = run_in_image(
            no_workspace_config_claude_skills_ubuntu_image(),
            "test -d /sandbox/.claude/skills/my-skill",
        );
        assert!(
            !out.status.success(),
            "claude skill directory should not exist when --with-workspace-config is absent"
        );
    }

    #[test]
    #[ignore]
    fn opencode_skills_not_copied() {
        let out = run_in_image(
            no_workspace_config_opencode_skills_ubuntu_image(),
            "test -d /sandbox/.opencode/skills/my-skill",
        );
        assert!(
            !out.status.success(),
            "opencode skill directory should not exist when --with-workspace-config is absent"
        );
    }

    #[test]
    #[ignore]
    fn network_hosts_not_in_policy() {
        let out = run_in_image(
            no_workspace_config_network_hosts_ubuntu_image(),
            "cat /etc/openshell/policy.yaml",
        );
        assert!(out.status.success(), "failed to read policy.yaml");
        let policy = String::from_utf8_lossy(&out.stdout);
        assert!(
            !policy.contains("name: workspace"),
            "workspace network rule should not be present when --with-workspace-config is absent"
        );
    }
}

// ---------------------------------------------------------------------------
// OpenCode + Ollama config integration tests
// ---------------------------------------------------------------------------

mod opencode_ollama {
    use super::*;

    #[test]
    #[ignore]
    fn config_json_present() {
        let out = run_in_image(
            ubuntu_opencode_ollama_image(),
            "test -f /sandbox/.config/opencode/config.json",
        );
        assert!(
            out.status.success(),
            "opencode config.json not found at /sandbox/.config/opencode/config.json"
        );
    }

    #[test]
    #[ignore]
    fn config_json_contains_host_openshell_internal() {
        let out = run_in_image(
            ubuntu_opencode_ollama_image(),
            "grep -q 'host.openshell.internal' /sandbox/.config/opencode/config.json",
        );
        assert!(
            out.status.success(),
            "host.openshell.internal not found in opencode config.json"
        );
    }

    #[test]
    #[ignore]
    fn config_json_contains_qwen3_coder_model() {
        let out = run_in_image(
            ubuntu_opencode_ollama_image(),
            "grep -q 'qwen3-coder:30b' /sandbox/.config/opencode/config.json",
        );
        assert!(
            out.status.success(),
            "qwen3-coder:30b not found in opencode config.json"
        );
    }

    #[test]
    #[ignore]
    fn config_json_contains_lfm2_5_model() {
        let out = run_in_image(
            ubuntu_opencode_ollama_image(),
            "grep -q 'lfm2.5' /sandbox/.config/opencode/config.json",
        );
        assert!(
            out.status.success(),
            "lfm2.5 not found in opencode config.json"
        );
    }

    #[test]
    #[ignore]
    fn config_json_owned_by_sandbox() {
        let out = run_in_image(
            ubuntu_opencode_ollama_image(),
            "stat -c '%U' /sandbox/.config/opencode/config.json",
        );
        assert!(out.status.success(), "failed to stat opencode config.json");
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            "sandbox",
            "opencode config.json not owned by sandbox"
        );
    }

    #[test]
    #[ignore]
    fn config_json_not_present_without_ollama() {
        let out = run_in_image(
            ubuntu_opencode_image(),
            "test -f /sandbox/.config/opencode/config.json",
        );
        assert!(
            !out.status.success(),
            "opencode config.json should not be present when built without --inference ollama"
        );
    }

    #[test]
    #[ignore]
    fn claude_with_ollama_inference_is_rejected() {
        let binary = env!("CARGO_BIN_EXE_openshell-image-builder");
        let status = Command::new(binary)
            .args([
                "--runtime",
                "podman",
                "--agent",
                "claude",
                "--inference",
                "ollama",
                "should-not-be-built:test",
            ])
            .status()
            .expect("binary should run");
        assert!(
            !status.success(),
            "building with --agent claude --inference ollama should fail"
        );
    }
}

// ---------------------------------------------------------------------------
// OpenCode + OpenAI config integration tests
// ---------------------------------------------------------------------------

mod opencode_openai {
    use super::*;

    #[test]
    #[ignore]
    fn config_json_not_present_without_model_or_endpoint() {
        let out = run_in_image(
            ubuntu_opencode_openai_image(),
            "test -f /sandbox/.config/opencode/config.json",
        );
        assert!(
            !out.status.success(),
            "opencode config.json should not be present when built without --model or --endpoint"
        );
    }

    #[test]
    #[ignore]
    fn claude_with_openai_inference_is_rejected() {
        let binary = env!("CARGO_BIN_EXE_openshell-image-builder");
        let status = Command::new(binary)
            .args([
                "--runtime",
                "podman",
                "--agent",
                "claude",
                "--inference",
                "openai",
                "should-not-be-built:test",
            ])
            .status()
            .expect("binary should run");
        assert!(
            !status.success(),
            "building with --agent claude --inference openai should fail"
        );
    }
}

// ---------------------------------------------------------------------------
// --model integration tests
// ---------------------------------------------------------------------------

const MODEL_CLAUDE: &str = "claude-sonnet-4-6";
const MODEL_OLLAMA: &str = "qwen3-coder:30b";
const MODEL_OPENAI: &str = "gpt-4o";

static UBUNTU_CLAUDE_ANTHROPIC_MODEL_IMAGE: OnceLock<String> = OnceLock::new();
static UBUNTU_CLAUDE_VERTEXAI_MODEL_IMAGE: OnceLock<String> = OnceLock::new();
static UBUNTU_OPENCODE_ANTHROPIC_MODEL_IMAGE: OnceLock<String> = OnceLock::new();
static UBUNTU_OPENCODE_OLLAMA_MODEL_IMAGE: OnceLock<String> = OnceLock::new();

fn ubuntu_claude_anthropic_model_image() -> &'static str {
    UBUNTU_CLAUDE_ANTHROPIC_MODEL_IMAGE.get_or_init(|| {
        build_image(
            "openshell-test-ubuntu-claude-anthropic-model:integration",
            &[
                "--agent",
                "claude",
                "--inference",
                "anthropic",
                "--model",
                MODEL_CLAUDE,
                "--with-agent-settings",
            ],
        )
    })
}

fn ubuntu_claude_vertexai_model_image() -> &'static str {
    UBUNTU_CLAUDE_VERTEXAI_MODEL_IMAGE.get_or_init(|| {
        build_image(
            "openshell-test-ubuntu-claude-vertexai-model:integration",
            &[
                "--agent",
                "claude",
                "--inference",
                "vertexai",
                "--model",
                MODEL_CLAUDE,
                "--with-agent-settings",
            ],
        )
    })
}

fn ubuntu_opencode_anthropic_model_image() -> &'static str {
    UBUNTU_OPENCODE_ANTHROPIC_MODEL_IMAGE.get_or_init(|| {
        build_image(
            "openshell-test-ubuntu-opencode-anthropic-model:integration",
            &[
                "--agent",
                "opencode",
                "--inference",
                "anthropic",
                "--model",
                MODEL_CLAUDE,
                "--with-agent-settings",
            ],
        )
    })
}

fn ubuntu_opencode_ollama_model_image() -> &'static str {
    UBUNTU_OPENCODE_OLLAMA_MODEL_IMAGE.get_or_init(|| {
        build_image(
            "openshell-test-ubuntu-opencode-ollama-model:integration",
            &[
                "--agent",
                "opencode",
                "--inference",
                "ollama",
                "--model",
                MODEL_OLLAMA,
                "--with-agent-settings",
            ],
        )
    })
}

// claude + anthropic + model: .claude/settings.json written with "model" field
mod model_claude_anthropic {
    use super::*;

    #[test]
    #[ignore]
    fn claude_settings_json_present() {
        let out = run_in_image(
            ubuntu_claude_anthropic_model_image(),
            "test -f /sandbox/.claude/settings.json",
        );
        assert!(
            out.status.success(),
            ".claude/settings.json not found in /sandbox"
        );
    }

    #[test]
    #[ignore]
    fn claude_settings_json_contains_model() {
        let cmd = format!(
            "grep -q '\"{}\"' /sandbox/.claude/settings.json",
            MODEL_CLAUDE
        );
        let out = run_in_image(ubuntu_claude_anthropic_model_image(), &cmd);
        assert!(
            out.status.success(),
            "model value not found in .claude/settings.json"
        );
    }

    #[test]
    #[ignore]
    fn claude_settings_json_owned_by_sandbox() {
        let out = run_in_image(
            ubuntu_claude_anthropic_model_image(),
            "stat -c '%U' /sandbox/.claude/settings.json",
        );
        assert!(out.status.success(), "failed to stat .claude/settings.json");
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            "sandbox",
            ".claude/settings.json not owned by sandbox"
        );
    }

    #[test]
    #[ignore]
    fn claude_settings_json_not_present_without_model() {
        let out = run_in_image(
            ubuntu_claude_image(),
            "test -f /sandbox/.claude/settings.json",
        );
        assert!(
            !out.status.success(),
            ".claude/settings.json should not be present when built without --model"
        );
    }
}

// claude + vertexai + model: .claude/settings.json written with "model" field
mod model_claude_vertexai {
    use super::*;

    #[test]
    #[ignore]
    fn claude_settings_json_present() {
        let out = run_in_image(
            ubuntu_claude_vertexai_model_image(),
            "test -f /sandbox/.claude/settings.json",
        );
        assert!(
            out.status.success(),
            ".claude/settings.json not found in /sandbox"
        );
    }

    #[test]
    #[ignore]
    fn claude_settings_json_contains_model() {
        let cmd = format!(
            "grep -q '\"{}\"' /sandbox/.claude/settings.json",
            MODEL_CLAUDE
        );
        let out = run_in_image(ubuntu_claude_vertexai_model_image(), &cmd);
        assert!(
            out.status.success(),
            "model value not found in .claude/settings.json"
        );
    }
}

// opencode + anthropic + model (no --endpoint): config.json written with "model" field
mod model_opencode_anthropic {
    use super::*;

    #[test]
    #[ignore]
    fn opencode_config_json_present() {
        let out = run_in_image(
            ubuntu_opencode_anthropic_model_image(),
            "test -f /sandbox/.config/opencode/config.json",
        );
        assert!(
            out.status.success(),
            "opencode config.json not found at /sandbox/.config/opencode/config.json"
        );
    }

    #[test]
    #[ignore]
    fn opencode_config_json_contains_model() {
        let cmd = format!(
            "grep -q '\"{}\"' /sandbox/.config/opencode/config.json",
            MODEL_CLAUDE
        );
        let out = run_in_image(ubuntu_opencode_anthropic_model_image(), &cmd);
        assert!(
            out.status.success(),
            "model value not found in opencode config.json"
        );
    }

    #[test]
    #[ignore]
    fn opencode_config_json_not_present_without_model_or_endpoint() {
        let out = run_in_image(
            ubuntu_opencode_image(),
            "test -f /sandbox/.config/opencode/config.json",
        );
        assert!(
            !out.status.success(),
            "opencode config.json should not be present when built without --model or --endpoint"
        );
    }

    #[test]
    #[ignore]
    fn opencode_config_json_owned_by_sandbox() {
        let out = run_in_image(
            ubuntu_opencode_anthropic_model_image(),
            "stat -c '%U' /sandbox/.config/opencode/config.json",
        );
        assert!(out.status.success(), "failed to stat opencode config.json");
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            "sandbox",
            "opencode config.json not owned by sandbox"
        );
    }
}

// opencode + ollama + model: config.json uses "ollama/<model>" prefix, only specified model listed
mod model_opencode_ollama {
    use super::*;

    #[test]
    #[ignore]
    fn opencode_config_json_contains_model_with_provider_prefix() {
        let cmd = format!(
            "grep -q '\"ollama/{}\"' /sandbox/.config/opencode/config.json",
            MODEL_OLLAMA
        );
        let out = run_in_image(ubuntu_opencode_ollama_model_image(), &cmd);
        assert!(
            out.status.success(),
            "prefixed model value ollama/{MODEL_OLLAMA} not found in opencode config.json"
        );
    }

    #[test]
    #[ignore]
    fn opencode_config_json_does_not_contain_preset_models() {
        let out = run_in_image(
            ubuntu_opencode_ollama_model_image(),
            "grep -q 'lfm2.5' /sandbox/.config/opencode/config.json",
        );
        assert!(
            !out.status.success(),
            "preset model lfm2.5 should not be present when --model is specified"
        );
    }

    #[test]
    #[ignore]
    fn opencode_config_json_still_contains_base_url() {
        let out = run_in_image(
            ubuntu_opencode_ollama_model_image(),
            "grep -q 'host.openshell.internal' /sandbox/.config/opencode/config.json",
        );
        assert!(
            out.status.success(),
            "host.openshell.internal not found in opencode config.json"
        );
    }
}

// opencode + openai + model: config.json uses "openai/<model>" prefix (native provider)
mod model_opencode_openai {
    use super::*;

    #[test]
    #[ignore]
    fn config_json_present() {
        let out = run_in_image(
            ubuntu_opencode_openai_model_image(),
            "test -f /sandbox/.config/opencode/config.json",
        );
        assert!(
            out.status.success(),
            "opencode config.json not found when built with --inference openai --model"
        );
    }

    #[test]
    #[ignore]
    fn opencode_config_json_contains_model_with_openai_prefix() {
        let cmd = format!(
            "grep -q '\"openai/{}\"' /sandbox/.config/opencode/config.json",
            MODEL_OPENAI
        );
        let out = run_in_image(ubuntu_opencode_openai_model_image(), &cmd);
        assert!(
            out.status.success(),
            "prefixed model value openai/{MODEL_OPENAI} not found in opencode config.json"
        );
    }

    #[test]
    #[ignore]
    fn config_json_owned_by_sandbox() {
        let out = run_in_image(
            ubuntu_opencode_openai_model_image(),
            "stat -c '%U' /sandbox/.config/opencode/config.json",
        );
        assert!(out.status.success(), "failed to stat opencode config.json");
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            "sandbox",
            "opencode config.json not owned by sandbox"
        );
    }
}

// ---------------------------------------------------------------------------
// --endpoint integration tests
// ---------------------------------------------------------------------------

const ANTHROPIC_PROXY_URL: &str = "https://my-anthropic-proxy.example.com";
const OLLAMA_CUSTOM_ENDPOINT: &str = "http://localhost:9999/v1";

static UBUNTU_CLAUDE_ANTHROPIC_ENDPOINT_IMAGE: OnceLock<String> = OnceLock::new();
static UBUNTU_OPENCODE_ANTHROPIC_ENDPOINT_IMAGE: OnceLock<String> = OnceLock::new();
static UBUNTU_OPENCODE_OLLAMA_CUSTOM_ENDPOINT_IMAGE: OnceLock<String> = OnceLock::new();

fn ubuntu_claude_anthropic_endpoint_image() -> &'static str {
    UBUNTU_CLAUDE_ANTHROPIC_ENDPOINT_IMAGE.get_or_init(|| {
        build_image(
            "openshell-test-ubuntu-claude-anthropic-endpoint:integration",
            &[
                "--agent",
                "claude",
                "--inference",
                "anthropic",
                "--endpoint",
                ANTHROPIC_PROXY_URL,
                "--with-policy",
            ],
        )
    })
}

fn ubuntu_opencode_anthropic_endpoint_image() -> &'static str {
    UBUNTU_OPENCODE_ANTHROPIC_ENDPOINT_IMAGE.get_or_init(|| {
        build_image(
            "openshell-test-ubuntu-opencode-anthropic-endpoint:integration",
            &[
                "--agent",
                "opencode",
                "--inference",
                "anthropic",
                "--endpoint",
                ANTHROPIC_PROXY_URL,
                "--with-policy",
                "--with-agent-settings",
            ],
        )
    })
}

fn ubuntu_opencode_ollama_custom_endpoint_image() -> &'static str {
    UBUNTU_OPENCODE_OLLAMA_CUSTOM_ENDPOINT_IMAGE.get_or_init(|| {
        build_image(
            "openshell-test-ubuntu-opencode-ollama-custom-endpoint:integration",
            &[
                "--agent",
                "opencode",
                "--inference",
                "ollama",
                "--endpoint",
                OLLAMA_CUSTOM_ENDPOINT,
                "--with-policy",
                "--with-agent-settings",
            ],
        )
    })
}

// claude + anthropic + custom endpoint: policy replaced, ANTHROPIC_BASE_URL baked in
mod endpoint_claude_anthropic {
    use super::*;

    #[test]
    #[ignore]
    fn policy_uses_proxy_host_instead_of_api_anthropic() {
        let out = run_in_image(
            ubuntu_claude_anthropic_endpoint_image(),
            "grep -q 'my-anthropic-proxy.example.com' /etc/openshell/policy.yaml",
        );
        assert!(out.status.success(), "proxy host not found in policy.yaml");
    }

    #[test]
    #[ignore]
    fn policy_does_not_contain_default_anthropic_host() {
        let out = run_in_image(
            ubuntu_claude_anthropic_endpoint_image(),
            "grep -q 'api.anthropic.com' /etc/openshell/policy.yaml",
        );
        assert!(
            !out.status.success(),
            "api.anthropic.com must not appear in policy.yaml when --endpoint is used"
        );
    }

    #[test]
    #[ignore]
    fn anthropic_base_url_env_set_to_proxy() {
        let cmd = format!("test \"$ANTHROPIC_BASE_URL\" = \"{}\"", ANTHROPIC_PROXY_URL);
        let out = run_in_image(ubuntu_claude_anthropic_endpoint_image(), &cmd);
        assert!(
            out.status.success(),
            "ANTHROPIC_BASE_URL is not set to the proxy URL in the image"
        );
    }
}

// opencode + anthropic + custom endpoint: policy replaced, config.json updated
mod endpoint_opencode_anthropic {
    use super::*;

    #[test]
    #[ignore]
    fn policy_uses_proxy_host_instead_of_api_anthropic() {
        let out = run_in_image(
            ubuntu_opencode_anthropic_endpoint_image(),
            "grep -q 'my-anthropic-proxy.example.com' /etc/openshell/policy.yaml",
        );
        assert!(out.status.success(), "proxy host not found in policy.yaml");
    }

    #[test]
    #[ignore]
    fn policy_does_not_contain_default_anthropic_host() {
        let out = run_in_image(
            ubuntu_opencode_anthropic_endpoint_image(),
            "grep -q 'api.anthropic.com' /etc/openshell/policy.yaml",
        );
        assert!(
            !out.status.success(),
            "api.anthropic.com must not appear in policy.yaml when --endpoint is used"
        );
    }

    #[test]
    #[ignore]
    fn opencode_config_contains_proxy_url() {
        let cmd = format!(
            "grep -q '{}' /sandbox/.config/opencode/config.json",
            ANTHROPIC_PROXY_URL
        );
        let out = run_in_image(ubuntu_opencode_anthropic_endpoint_image(), &cmd);
        assert!(
            out.status.success(),
            "proxy URL not found in opencode config.json"
        );
    }
}

// opencode + ollama + custom endpoint: policy replaced, config.json updated
mod endpoint_opencode_ollama_custom {
    use super::*;

    #[test]
    #[ignore]
    fn policy_uses_custom_port_instead_of_default() {
        let out = run_in_image(
            ubuntu_opencode_ollama_custom_endpoint_image(),
            "grep -q '9999' /etc/openshell/policy.yaml",
        );
        assert!(
            out.status.success(),
            "custom port 9999 not found in policy.yaml"
        );
    }

    #[test]
    #[ignore]
    fn policy_does_not_use_default_ollama_port() {
        let out = run_in_image(
            ubuntu_opencode_ollama_custom_endpoint_image(),
            "grep -q '11434' /etc/openshell/policy.yaml",
        );
        assert!(
            !out.status.success(),
            "default port 11434 must not appear in policy.yaml when --endpoint overrides it"
        );
    }

    #[test]
    #[ignore]
    fn opencode_config_contains_rewritten_endpoint_url() {
        // localhost in the CLI arg is rewritten to host.openshell.internal
        let out = run_in_image(
            ubuntu_opencode_ollama_custom_endpoint_image(),
            "grep -q 'host.openshell.internal:9999' /sandbox/.config/opencode/config.json",
        );
        assert!(
            out.status.success(),
            "rewritten endpoint URL not found in opencode config.json"
        );
    }
}

// vertexai + endpoint rejection: does not require podman, never #[ignore]
mod endpoint_rejection {
    use super::*;

    #[test]
    fn vertexai_with_endpoint_exits_nonzero() {
        let binary = env!("CARGO_BIN_EXE_openshell-image-builder");
        let output = Command::new(binary)
            .args([
                "--runtime",
                "podman",
                "--inference",
                "vertexai",
                "--endpoint",
                "https://my-vertex-proxy.example.com",
                "should-not-be-built:test",
            ])
            .output()
            .expect("binary should run");
        assert!(
            !output.status.success(),
            "--inference vertexai --endpoint must exit non-zero"
        );
    }

    #[test]
    #[ignore]
    fn vertexai_with_endpoint_error_mentions_vertexai() {
        let binary = env!("CARGO_BIN_EXE_openshell-image-builder");
        let output = Command::new(binary)
            .args([
                "--runtime",
                "podman",
                "--inference",
                "vertexai",
                "--endpoint",
                "https://my-vertex-proxy.example.com",
                "should-not-be-built:test",
            ])
            .output()
            .expect("binary should run");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("vertexai"),
            "error message should mention vertexai, got: {stderr}"
        );
    }
}

// ---------------------------------------------------------------------------
// --with-policy flag tests
// ---------------------------------------------------------------------------

mod with_policy {
    use super::*;

    #[test]
    #[ignore]
    fn policy_yaml_present_when_flag_set() {
        let out = run_in_image(ubuntu_image(), "test -f /etc/openshell/policy.yaml");
        assert!(
            out.status.success(),
            "policy.yaml not found in /etc/openshell/ when --with-policy was passed"
        );
    }

    #[test]
    #[ignore]
    fn policy_yaml_absent_without_flag() {
        let out = run_in_image(
            ubuntu_no_policy_image(),
            "test ! -f /etc/openshell/policy.yaml",
        );
        assert!(
            out.status.success(),
            "policy.yaml found in /etc/openshell/ even though --with-policy was not passed"
        );
    }
}

// ---------------------------------------------------------------------------
// --with-agent-settings flag tests
// ---------------------------------------------------------------------------

mod with_agent_settings {
    use super::*;

    #[test]
    #[ignore]
    fn claude_json_lacks_trust_dialog_flag_without_agent_settings() {
        // The Claude installer creates .claude.json, but our skip_onboarding() is the
        // only thing that sets hasTrustDialogAccepted. Assert it is absent.
        let out = run_in_image(
            ubuntu_claude_no_agent_settings_image(),
            r#"grep -q '"hasTrustDialogAccepted"' /sandbox/.claude.json"#,
        );
        assert!(
            !out.status.success(),
            "hasTrustDialogAccepted should not be present when --with-agent-settings is not set"
        );
    }
}

// ---------------------------------------------------------------------------
// --ssl-certs integration tests
// ---------------------------------------------------------------------------

mod ssl_certs {
    use super::*;

    #[test]
    #[ignore]
    fn ubuntu_cert_present_in_image() {
        let out = run_in_image(
            ubuntu_ssl_certs_image(),
            "test -f /usr/local/share/ca-certificates/system-ca.crt",
        );
        assert!(
            out.status.success(),
            "system-ca.crt not found in /usr/local/share/ca-certificates/"
        );
    }

    #[test]
    #[ignore]
    fn ubuntu_cert_contains_pem_data() {
        let out = run_in_image(
            ubuntu_ssl_certs_image(),
            "grep -q 'BEGIN CERTIFICATE' /usr/local/share/ca-certificates/system-ca.crt",
        );
        assert!(
            out.status.success(),
            "system-ca.crt does not contain PEM certificate data"
        );
    }

    #[test]
    #[ignore]
    fn ubuntu_cert_absent_with_disable_flag() {
        let out = run_in_image(
            ubuntu_no_certs_image(),
            "test -f /usr/local/share/ca-certificates/system-ca.crt",
        );
        assert!(
            !out.status.success(),
            "system-ca.crt should not be present in image built with --disable-ssl-certs"
        );
    }

    #[test]
    #[ignore]
    fn fedora_cert_present_in_image() {
        let out = run_in_image(
            fedora_ssl_certs_image(),
            "test -f /etc/pki/ca-trust/source/anchors/system-ca.crt",
        );
        assert!(
            out.status.success(),
            "system-ca.crt not found in /etc/pki/ca-trust/source/anchors/"
        );
    }

    #[test]
    #[ignore]
    fn fedora_cert_contains_pem_data() {
        let out = run_in_image(
            fedora_ssl_certs_image(),
            "grep -q 'BEGIN CERTIFICATE' /etc/pki/ca-trust/source/anchors/system-ca.crt",
        );
        assert!(
            out.status.success(),
            "system-ca.crt does not contain PEM certificate data"
        );
    }

    #[test]
    #[ignore]
    fn fedora_cert_absent_with_disable_flag() {
        let out = run_in_image(
            fedora_no_certs_image(),
            "test -f /etc/pki/ca-trust/source/anchors/system-ca.crt",
        );
        assert!(
            !out.status.success(),
            "system-ca.crt should not be present in image built with --disable-ssl-certs"
        );
    }
}

// ---------------------------------------------------------------------------
// --image-mount flag tests
// ---------------------------------------------------------------------------

mod image_mount {
    use super::*;

    #[test]
    #[ignore]
    fn bashrc_contains_init_with_resolved_mount_path() {
        let out = run_in_image(
            ubuntu_image_mount_image(),
            "grep -q 'export PATH=/sandbox/mnt/curl/usr/bin' /sandbox/.bashrc",
        );
        assert!(
            out.status.success(),
            "resolved init not found in /sandbox/.bashrc"
        );
    }

    #[test]
    #[ignore]
    fn zshrc_contains_init_with_resolved_mount_path() {
        let out = run_in_image(
            ubuntu_image_mount_image(),
            "grep -q 'export PATH=/sandbox/mnt/curl/usr/bin' /sandbox/.zshrc",
        );
        assert!(
            out.status.success(),
            "resolved init not found in /sandbox/.zshrc"
        );
    }

    #[test]
    #[ignore]
    fn bashrc_owned_by_sandbox() {
        let out = run_in_image(ubuntu_image_mount_image(), "stat -c '%U' /sandbox/.bashrc");
        assert!(out.status.success(), "failed to stat /sandbox/.bashrc");
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            "sandbox",
            "/sandbox/.bashrc not owned by sandbox"
        );
    }

    #[test]
    #[ignore]
    fn zshrc_owned_by_sandbox() {
        let out = run_in_image(ubuntu_image_mount_image(), "stat -c '%U' /sandbox/.zshrc");
        assert!(out.status.success(), "failed to stat /sandbox/.zshrc");
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            "sandbox",
            "/sandbox/.zshrc not owned by sandbox"
        );
    }

    #[test]
    #[ignore]
    fn base_image_without_flag_has_no_zshrc() {
        let out = run_in_image(ubuntu_image(), "test -f /sandbox/.zshrc");
        assert!(
            !out.status.success(),
            "/sandbox/.zshrc should not exist in an image built without --image-mount"
        );
    }

    #[test]
    fn invalid_path_exits_nonzero() {
        let binary = env!("CARGO_BIN_EXE_openshell-image-builder");
        let output = Command::new(binary)
            .args([
                "--runtime",
                "podman",
                "--image-mount",
                "/nonexistent/path/tool.yaml",
                "should-not-be-built:test",
            ])
            .output()
            .expect("binary should run");
        assert!(
            !output.status.success(),
            "--image-mount with nonexistent file must exit non-zero"
        );
    }
}

// ---------------------------------------------------------------------------
// Cleanup — runs when the test process exits, after all tests complete
// ---------------------------------------------------------------------------

#[ctor::dtor]
fn cleanup_images() {
    for tag in [
        "openshell-test-ubuntu:integration",
        "openshell-test-ubuntu-claude:integration",
        "openshell-test-ubuntu-opencode:integration",
        "openshell-test-ubuntu-claude-vertexai:integration",
        "openshell-test-ubuntu-opencode-vertexai:integration",
        "openshell-test-fedora:integration",
        "openshell-test-fedora-claude:integration",
        "openshell-test-fedora-opencode:integration",
        "openshell-test-fedora-claude-vertexai:integration",
        "openshell-test-fedora-opencode-vertexai:integration",
        "openshell-test-ubi:integration",
        "openshell-test-ubi-claude:integration",
        "openshell-test-ubi-opencode:integration",
        "openshell-test-ubi-claude-vertexai:integration",
        "openshell-test-ubi-opencode-vertexai:integration",
        "openshell-test-feature-common-utils-ubuntu:integration",
        "openshell-test-feature-node-ubuntu:integration",
        "openshell-test-feature-python-ubuntu:integration",
        "openshell-test-feature-common-utils-fedora:integration",
        "openshell-test-feature-node-fedora:integration",
        "openshell-test-feature-python-fedora:integration",
        "openshell-test-feature-common-utils-ubi:integration",
        "openshell-test-feature-node-ubi:integration",
        "openshell-test-feature-python-ubi:integration",
        "openshell-test-feature-local-ubuntu:integration",
        "openshell-test-feature-local-fedora:integration",
        "openshell-test-feature-local-ubi:integration",
        "openshell-test-ubuntu-claude-settings:integration",
        "openshell-test-ubuntu-claude-with-claude-json:integration",
        "openshell-test-ubuntu-opencode-settings:integration",
        "openshell-test-ubuntu-claude-skills:integration",
        "openshell-test-ubuntu-opencode-skills:integration",
        "openshell-test-ubuntu-opencode-ollama:integration",
        "openshell-test-fedora-opencode-ollama:integration",
        "openshell-test-ubi-opencode-ollama:integration",
        "openshell-test-hummingbird-opencode-ollama:integration",
        "openshell-test-ubuntu-opencode-openai:integration",
        "openshell-test-fedora-opencode-openai:integration",
        "openshell-test-ubi-opencode-openai:integration",
        "openshell-test-hummingbird-opencode-openai:integration",
        "openshell-test-ubuntu-opencode-openai-model:integration",
        "openshell-test-ubuntu-claude-anthropic-endpoint:integration",
        "openshell-test-ubuntu-opencode-anthropic-endpoint:integration",
        "openshell-test-ubuntu-opencode-ollama-custom-endpoint:integration",
        "openshell-test-ubuntu-claude-anthropic-model:integration",
        "openshell-test-ubuntu-claude-vertexai-model:integration",
        "openshell-test-ubuntu-opencode-anthropic-model:integration",
        "openshell-test-ubuntu-opencode-ollama-model:integration",
        "openshell-test-no-workspace-config-oci-feature-ubuntu:integration",
        "openshell-test-no-workspace-config-local-feature-ubuntu:integration",
        "openshell-test-no-workspace-config-claude-skills-ubuntu:integration",
        "openshell-test-no-workspace-config-opencode-skills-ubuntu:integration",
        "openshell-test-no-workspace-config-network-hosts-ubuntu:integration",
        "openshell-test-ubuntu-no-policy:integration",
        "openshell-test-ubuntu-claude-no-agent-settings:integration",
        "openshell-test-ubuntu-ssl-certs:integration",
        "openshell-test-fedora-ssl-certs:integration",
        "openshell-test-ubuntu-no-certs:integration",
        "openshell-test-fedora-no-certs:integration",
        "openshell-test-ubuntu-image-mount:integration",
    ] {
        Command::new("podman")
            .args(["rmi", "--force", tag])
            .status()
            .ok();
    }
}
