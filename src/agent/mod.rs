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

mod claude;
mod codex;
mod opencode;

#[cfg(test)]
pub use claude::ClaudeAgent;
#[cfg(test)]
pub use codex::CodexAgent;
#[cfg(test)]
pub use opencode::OpencodeAgent;

use clap::ValueEnum;
use std::collections::HashMap;

use crate::inference;

pub trait Agent {
    fn id(&self) -> &str;
    fn install(&self) -> String;
    fn binary_path(&self) -> &str;
    fn policy_yaml(&self) -> &str {
        ""
    }
    fn skip_onboarding(&self, files: HashMap<String, String>) -> HashMap<String, String> {
        files
    }
    /// Returns the inference kinds this agent supports.
    fn supported_inference(&self) -> Vec<inference::InferenceKind> {
        vec![]
    }
    /// Merges inference provider configuration into `files` and returns the result.
    /// `base_url` overrides the provider's default endpoint when `Some`.
    /// `model` sets the default model when `Some`.
    fn set_inference(
        &self,
        files: HashMap<String, String>,
        _inference: Option<&inference::InferenceKind>,
        _base_url: Option<&str>,
        _model: Option<&str>,
    ) -> HashMap<String, String> {
        files
    }
    /// Returns environment variables to bake into the image for this agent.
    /// `endpoint` overrides the inference provider's default URL when `Some`.
    /// `model` sets the default model when `Some`.
    fn env_vars(
        &self,
        _inference: Option<&inference::InferenceKind>,
        _endpoint: Option<&str>,
        _model: Option<&str>,
    ) -> HashMap<String, String> {
        HashMap::new()
    }
    fn skills_dir(&self) -> &str {
        ""
    }
}

#[derive(Clone, ValueEnum)]
pub enum AgentKind {
    Claude,
    Codex,
    Opencode,
}

pub fn from_kind(kind: AgentKind) -> Box<dyn Agent> {
    match kind {
        AgentKind::Claude => Box::new(claude::ClaudeAgent),
        AgentKind::Codex => Box::new(codex::CodexAgent),
        AgentKind::Opencode => Box::new(opencode::OpencodeAgent),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_kind_claude_installs_claude() {
        let agent = from_kind(AgentKind::Claude);
        assert!(agent.install().contains("https://claude.ai/install.sh"));
    }

    #[test]
    fn from_kind_opencode_installs_opencode() {
        let agent = from_kind(AgentKind::Opencode);
        assert!(agent.install().contains("https://opencode.ai/install"));
    }

    #[test]
    fn from_kind_codex_installs_codex() {
        let agent = from_kind(AgentKind::Codex);
        assert!(
            agent
                .install()
                .contains("https://chatgpt.com/codex/install.sh")
        );
    }

    #[test]
    fn opencode_skip_onboarding_is_noop() {
        let agent = from_kind(AgentKind::Opencode);
        let mut files = HashMap::new();
        files.insert("some.json".to_string(), "content".to_string());
        let result = agent.skip_onboarding(files.clone());
        assert_eq!(result, files);
    }
}
