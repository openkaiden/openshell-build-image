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

use super::Agent;
use crate::inference;

const CLAUDE_CONFIG_FILE: &str = ".claude.json";
const CLAUDE_SETTINGS_FILE: &str = ".claude/settings.json";
const SANDBOX_HOME: &str = "/sandbox";

pub struct ClaudeAgent;

impl Agent for ClaudeAgent {
    fn id(&self) -> &str {
        "claude"
    }

    fn install(&self) -> String {
        "RUN mkdir -p /sandbox && curl -fsSL https://claude.ai/install.sh | HOME=/sandbox bash\nENV PATH=/sandbox/.local/bin:$PATH"
            .to_string()
    }

    fn binary_path(&self) -> &str {
        "/sandbox/.local/bin/claude"
    }

    fn skip_onboarding(&self, mut files: HashMap<String, String>) -> HashMap<String, String> {
        let content = files
            .get(CLAUDE_CONFIG_FILE)
            .cloned()
            .unwrap_or_else(|| "{}".to_string());
        let mut config: serde_json::Value =
            serde_json::from_str(&content).unwrap_or(serde_json::json!({}));

        config["hasCompletedOnboarding"] = serde_json::json!(true);
        if !config["projects"].is_object() {
            config["projects"] = serde_json::json!({});
        }
        config["projects"][SANDBOX_HOME]["hasTrustDialogAccepted"] = serde_json::json!(true);

        files.insert(
            CLAUDE_CONFIG_FILE.to_string(),
            serde_json::to_string_pretty(&config).expect("valid json value"),
        );
        files
    }

    fn supported_inference(&self) -> Vec<inference::InferenceKind> {
        vec![
            inference::InferenceKind::Anthropic,
            inference::InferenceKind::VertexAi,
        ]
    }

    fn set_inference(
        &self,
        mut files: HashMap<String, String>,
        _inference: Option<&inference::InferenceKind>,
        _base_url: Option<&str>,
        model: Option<&str>,
    ) -> HashMap<String, String> {
        if let Some(m) = model {
            let content = files
                .get(CLAUDE_SETTINGS_FILE)
                .cloned()
                .unwrap_or_else(|| "{}".to_string());
            let mut config: serde_json::Value =
                serde_json::from_str(&content).unwrap_or(serde_json::json!({}));
            config["model"] = serde_json::json!(m);
            files.insert(
                CLAUDE_SETTINGS_FILE.to_string(),
                serde_json::to_string_pretty(&config).expect("valid json value"),
            );
        }
        files
    }

    fn env_vars(
        &self,
        inference: Option<&inference::InferenceKind>,
        endpoint: Option<&str>,
        _model: Option<&str>,
    ) -> HashMap<String, String> {
        let mut vars = HashMap::new();
        if let (Some(inference::InferenceKind::Anthropic), Some(url)) = (inference, endpoint) {
            vars.insert("ANTHROPIC_BASE_URL".to_string(), url.to_string());
        }
        vars
    }

    fn skills_dir(&self) -> &str {
        "/sandbox/.claude/skills"
    }

    fn policy_yaml(&self) -> &str {
        r#"version: 1
network_policies:
  claude_code:
    name: claude-code
    endpoints:
      - { host: raw.githubusercontent.com, port: 443 }
      - { host: platform.claude.com, port: 443 }
      - { host: api.github.com, port: 443, protocol: rest, tls: terminate, enforcement: enforce, access: read-only }
    binaries:
      - { path: /sandbox/.local/bin/claude }
"#
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn agent_id_is_claude() {
        assert_eq!(ClaudeAgent.id(), "claude");
    }

    #[test]
    fn install_is_nonempty() {
        assert!(!ClaudeAgent.install().is_empty());
    }

    #[test]
    fn install_contains_claude_installer() {
        assert!(
            ClaudeAgent
                .install()
                .contains("https://claude.ai/install.sh")
        );
    }

    #[test]
    fn install_adds_local_bin_to_path() {
        assert!(
            ClaudeAgent
                .install()
                .contains("ENV PATH=/sandbox/.local/bin:$PATH")
        );
    }

    #[test]
    fn binary_path_is_local_bin_claude() {
        assert_eq!(ClaudeAgent.binary_path(), "/sandbox/.local/bin/claude");
    }

    #[test]
    fn policy_yaml_has_claude_code_name() {
        assert!(ClaudeAgent.policy_yaml().contains("name: claude-code"));
    }

    #[test]
    fn policy_yaml_has_platform_claude_endpoint() {
        assert!(ClaudeAgent.policy_yaml().contains("platform.claude.com"));
    }

    #[test]
    fn skip_onboarding_creates_claude_json_when_absent() {
        let result = ClaudeAgent.skip_onboarding(HashMap::new());
        assert!(result.contains_key(CLAUDE_CONFIG_FILE));
    }

    #[test]
    fn skip_onboarding_sets_completed_onboarding_flag() {
        let result = ClaudeAgent.skip_onboarding(HashMap::new());
        let json: serde_json::Value =
            serde_json::from_str(result[CLAUDE_CONFIG_FILE].as_str()).unwrap();
        assert_eq!(json["hasCompletedOnboarding"], true);
    }

    #[test]
    fn skip_onboarding_sets_trust_dialog_accepted_for_sandbox_home() {
        let result = ClaudeAgent.skip_onboarding(HashMap::new());
        let json: serde_json::Value =
            serde_json::from_str(result[CLAUDE_CONFIG_FILE].as_str()).unwrap();
        assert_eq!(
            json["projects"][SANDBOX_HOME]["hasTrustDialogAccepted"],
            true
        );
    }

    #[test]
    fn skip_onboarding_preserves_existing_fields() {
        let mut files = HashMap::new();
        files.insert(
            CLAUDE_CONFIG_FILE.to_string(),
            r#"{"existingField": "value"}"#.to_string(),
        );
        let result = ClaudeAgent.skip_onboarding(files);
        let json: serde_json::Value =
            serde_json::from_str(result[CLAUDE_CONFIG_FILE].as_str()).unwrap();
        assert_eq!(json["existingField"], "value");
    }

    #[test]
    fn skills_dir_is_claude_skills() {
        assert_eq!(ClaudeAgent.skills_dir(), "/sandbox/.claude/skills");
    }

    #[test]
    fn supported_inference_includes_anthropic() {
        assert!(
            ClaudeAgent
                .supported_inference()
                .contains(&inference::InferenceKind::Anthropic)
        );
    }

    #[test]
    fn supported_inference_includes_vertexai() {
        assert!(
            ClaudeAgent
                .supported_inference()
                .contains(&inference::InferenceKind::VertexAi)
        );
    }

    #[test]
    fn supported_inference_excludes_ollama() {
        assert!(
            !ClaudeAgent
                .supported_inference()
                .contains(&inference::InferenceKind::Ollama)
        );
    }

    #[test]
    fn env_vars_with_anthropic_and_endpoint_sets_anthropic_base_url() {
        let vars = ClaudeAgent.env_vars(
            Some(&inference::InferenceKind::Anthropic),
            Some("https://my-proxy.example.com"),
            None,
        );
        assert_eq!(
            vars.get("ANTHROPIC_BASE_URL").map(String::as_str),
            Some("https://my-proxy.example.com")
        );
    }

    #[test]
    fn env_vars_with_anthropic_and_no_endpoint_returns_empty() {
        let vars = ClaudeAgent.env_vars(Some(&inference::InferenceKind::Anthropic), None, None);
        assert!(vars.is_empty());
    }

    #[test]
    fn env_vars_with_no_inference_returns_empty() {
        let vars = ClaudeAgent.env_vars(None, Some("https://my-proxy.example.com"), None);
        assert!(vars.is_empty());
    }

    #[test]
    fn set_inference_with_model_writes_model_to_claude_settings() {
        let result = ClaudeAgent.set_inference(HashMap::new(), None, None, Some("claude-opus-4-5"));
        let json: serde_json::Value =
            serde_json::from_str(result[CLAUDE_SETTINGS_FILE].as_str()).unwrap();
        assert_eq!(json["model"], "claude-opus-4-5");
    }

    #[test]
    fn set_inference_with_model_preserves_existing_claude_settings_fields() {
        let mut files = HashMap::new();
        files.insert(
            CLAUDE_SETTINGS_FILE.to_string(),
            r#"{"theme": "dark"}"#.to_string(),
        );
        let result = ClaudeAgent.set_inference(files, None, None, Some("claude-opus-4-5"));
        let json: serde_json::Value =
            serde_json::from_str(result[CLAUDE_SETTINGS_FILE].as_str()).unwrap();
        assert_eq!(json["model"], "claude-opus-4-5");
        assert_eq!(json["theme"], "dark");
    }

    #[test]
    fn set_inference_without_model_returns_files_unchanged() {
        let mut files = HashMap::new();
        files.insert("other.json".to_string(), "content".to_string());
        let result = ClaudeAgent.set_inference(files.clone(), None, None, None);
        assert_eq!(result, files);
    }

    #[test]
    fn skip_onboarding_leaves_other_files_unchanged() {
        let mut files = HashMap::new();
        files.insert("other.json".to_string(), "content".to_string());
        let result = ClaudeAgent.skip_onboarding(files);
        assert_eq!(result["other.json"], "content");
    }
}
