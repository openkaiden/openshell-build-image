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

mod anthropic;
mod ollama;
mod openai;
mod vertexai;

use std::collections::HashMap;

use super::Agent;
use crate::inference;

pub struct OpencodeAgent;

impl Agent for OpencodeAgent {
    fn id(&self) -> &str {
        "opencode"
    }

    fn install(&self) -> String {
        // The installer places the binary in ~/.opencode/bin/ which is not in PATH,
        // so we symlink it into ~/.local/bin/.
        "RUN mkdir -p /sandbox && cd /tmp && curl -fsSL https://opencode.ai/install | HOME=/sandbox bash && \\\n    \
             mkdir -p /sandbox/.local/bin && \\\n    \
             ln -sf /sandbox/.opencode/bin/opencode /sandbox/.local/bin/opencode && \\\n    \
             mkdir -p /sandbox/.config/opencode\n\
         ENV PATH=/sandbox/.local/bin:$PATH"
            .to_string()
    }

    fn binary_path(&self) -> &str {
        "/sandbox/.local/bin/opencode"
    }

    fn supported_inference(&self) -> Vec<inference::InferenceKind> {
        vec![
            inference::InferenceKind::Anthropic,
            inference::InferenceKind::Ollama,
            inference::InferenceKind::OpenAi,
            inference::InferenceKind::VertexAi,
        ]
    }

    fn set_inference(
        &self,
        files: HashMap<String, String>,
        inference: Option<&inference::InferenceKind>,
        base_url: Option<&str>,
        model: Option<&str>,
    ) -> HashMap<String, String> {
        match inference {
            Some(inference::InferenceKind::Ollama) if base_url.is_some() => {
                ollama::configure(files, base_url.unwrap(), model)
            }
            Some(inference::InferenceKind::Anthropic) if base_url.is_some() || model.is_some() => {
                anthropic::configure(files, base_url, model)
            }
            Some(inference::InferenceKind::OpenAi) if base_url.is_some() || model.is_some() => {
                openai::configure(files, base_url, model)
            }
            Some(inference::InferenceKind::VertexAi) if model.is_some() => {
                vertexai::configure(files, model.unwrap())
            }
            _ => files,
        }
    }

    fn skills_dir(&self) -> &str {
        "/sandbox/.opencode/skills"
    }

    fn policy_yaml(&self) -> &str {
        r#"version: 1
network_policies:
  opencode:
    name: opencode
    endpoints:
      - { host: models.dev, port: 443, protocol: rest, enforcement: enforce, access: full, tls: terminate }
      - { host: opencode.ai, port: 443, protocol: rest, enforcement: enforce, access: full, tls: terminate }
      - { host: registry.npmjs.org, port: 443, protocol: rest, enforcement: enforce, access: read-only, tls: terminate }
      - { host: api.github.com, port: 443, protocol: rest, tls: terminate, enforcement: enforce, access: read-only }
    binaries:
      - { path: /sandbox/.local/bin/opencode }
"#
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_id_is_opencode() {
        assert_eq!(OpencodeAgent.id(), "opencode");
    }

    #[test]
    fn install_is_nonempty() {
        assert!(!OpencodeAgent.install().is_empty());
    }

    #[test]
    fn install_contains_opencode_installer() {
        assert!(
            OpencodeAgent
                .install()
                .contains("https://opencode.ai/install")
        );
    }

    #[test]
    fn install_symlinks_binary_to_local_bin() {
        assert!(
            OpencodeAgent
                .install()
                .contains("/sandbox/.local/bin/opencode")
        );
    }

    #[test]
    fn install_adds_local_bin_to_path() {
        assert!(
            OpencodeAgent
                .install()
                .contains("ENV PATH=/sandbox/.local/bin:$PATH")
        );
    }

    #[test]
    fn binary_path_is_local_bin_opencode() {
        assert_eq!(OpencodeAgent.binary_path(), "/sandbox/.local/bin/opencode");
    }

    #[test]
    fn skills_dir_is_opencode_skills() {
        assert_eq!(OpencodeAgent.skills_dir(), "/sandbox/.opencode/skills");
    }

    #[test]
    fn policy_yaml_is_nonempty() {
        assert!(!OpencodeAgent.policy_yaml().is_empty());
    }

    #[test]
    fn policy_yaml_contains_opencode_endpoint() {
        assert!(OpencodeAgent.policy_yaml().contains("opencode.ai"));
    }

    #[test]
    fn policy_yaml_contains_npm_registry() {
        assert!(OpencodeAgent.policy_yaml().contains("registry.npmjs.org"));
    }

    #[test]
    fn policy_yaml_references_opencode_binary() {
        assert!(
            OpencodeAgent
                .policy_yaml()
                .contains("/sandbox/.local/bin/opencode")
        );
    }

    #[test]
    fn supported_inference_includes_anthropic() {
        assert!(
            OpencodeAgent
                .supported_inference()
                .contains(&inference::InferenceKind::Anthropic)
        );
    }

    #[test]
    fn supported_inference_includes_ollama() {
        assert!(
            OpencodeAgent
                .supported_inference()
                .contains(&inference::InferenceKind::Ollama)
        );
    }

    #[test]
    fn supported_inference_includes_vertexai() {
        assert!(
            OpencodeAgent
                .supported_inference()
                .contains(&inference::InferenceKind::VertexAi)
        );
    }

    #[test]
    fn set_inference_with_none_returns_files_unchanged() {
        let mut files = HashMap::new();
        files.insert("existing.json".to_string(), "content".to_string());
        let result = OpencodeAgent.set_inference(files.clone(), None, None, None);
        assert_eq!(result, files);
    }

    #[test]
    fn set_inference_with_ollama_creates_opencode_config() {
        let result = OpencodeAgent.set_inference(
            HashMap::new(),
            Some(&inference::InferenceKind::Ollama),
            Some("http://host.openshell.internal:11434/v1"),
            None,
        );
        assert!(result.contains_key(".config/opencode/config.json"));
    }

    #[test]
    fn set_inference_with_ollama_without_url_returns_files_unchanged() {
        let mut files = HashMap::new();
        files.insert("existing.json".to_string(), "content".to_string());
        let result = OpencodeAgent.set_inference(
            files.clone(),
            Some(&inference::InferenceKind::Ollama),
            None,
            None,
        );
        assert_eq!(result, files);
    }

    #[test]
    fn set_inference_with_anthropic_without_url_or_model_returns_files_unchanged() {
        let mut files = HashMap::new();
        files.insert("existing.json".to_string(), "content".to_string());
        let result = OpencodeAgent.set_inference(
            files.clone(),
            Some(&inference::InferenceKind::Anthropic),
            None,
            None,
        );
        assert_eq!(result, files);
    }

    #[test]
    fn set_inference_with_anthropic_and_url_creates_opencode_config() {
        let result = OpencodeAgent.set_inference(
            HashMap::new(),
            Some(&inference::InferenceKind::Anthropic),
            Some("https://my-anthropic-proxy.example.com"),
            None,
        );
        assert!(result.contains_key(".config/opencode/config.json"));
    }

    #[test]
    fn set_inference_with_anthropic_and_url_embeds_url() {
        let result = OpencodeAgent.set_inference(
            HashMap::new(),
            Some(&inference::InferenceKind::Anthropic),
            Some("https://my-anthropic-proxy.example.com"),
            None,
        );
        let config = result.get(".config/opencode/config.json").unwrap();
        assert!(config.contains("https://my-anthropic-proxy.example.com"));
    }

    #[test]
    fn set_inference_with_anthropic_and_model_creates_opencode_config() {
        let result = OpencodeAgent.set_inference(
            HashMap::new(),
            Some(&inference::InferenceKind::Anthropic),
            None,
            Some("claude-opus-4-5"),
        );
        assert!(result.contains_key(".config/opencode/config.json"));
    }

    #[test]
    fn set_inference_with_vertexai_and_model_creates_opencode_config() {
        let result = OpencodeAgent.set_inference(
            HashMap::new(),
            Some(&inference::InferenceKind::VertexAi),
            None,
            Some("vertex/claude-opus-4-5"),
        );
        assert!(result.contains_key(".config/opencode/config.json"));
    }

    #[test]
    fn set_inference_with_vertexai_without_model_returns_files_unchanged() {
        let mut files = HashMap::new();
        files.insert("existing.json".to_string(), "content".to_string());
        let result = OpencodeAgent.set_inference(
            files.clone(),
            Some(&inference::InferenceKind::VertexAi),
            None,
            None,
        );
        assert_eq!(result, files);
    }

    #[test]
    fn supported_inference_includes_openai() {
        assert!(
            OpencodeAgent
                .supported_inference()
                .contains(&inference::InferenceKind::OpenAi)
        );
    }

    #[test]
    fn set_inference_with_openai_and_model_creates_opencode_config() {
        let result = OpencodeAgent.set_inference(
            HashMap::new(),
            Some(&inference::InferenceKind::OpenAi),
            None,
            Some("gpt-4o"),
        );
        assert!(result.contains_key(".config/opencode/config.json"));
    }

    #[test]
    fn set_inference_with_openai_and_endpoint_creates_opencode_config() {
        let result = OpencodeAgent.set_inference(
            HashMap::new(),
            Some(&inference::InferenceKind::OpenAi),
            Some("https://my-openai-proxy.example.com"),
            None,
        );
        assert!(result.contains_key(".config/opencode/config.json"));
    }

    #[test]
    fn set_inference_with_openai_without_url_or_model_returns_files_unchanged() {
        let mut files = HashMap::new();
        files.insert("existing.json".to_string(), "content".to_string());
        let result = OpencodeAgent.set_inference(
            files.clone(),
            Some(&inference::InferenceKind::OpenAi),
            None,
            None,
        );
        assert_eq!(result, files);
    }
}
