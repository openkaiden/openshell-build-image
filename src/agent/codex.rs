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

use super::Agent;

pub struct CodexAgent;

impl Agent for CodexAgent {
    fn id(&self) -> &str {
        "codex"
    }

    fn install(&self) -> String {
        "RUN curl -fsSL https://chatgpt.com/codex/install.sh | sh\nENV PATH=/sandbox/.local/bin:$PATH"
            .to_string()
    }

    fn binary_path(&self) -> &str {
        "/sandbox/.local/bin/codex"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_id_is_codex() {
        assert_eq!(CodexAgent.id(), "codex");
    }

    #[test]
    fn install_is_nonempty() {
        assert!(!CodexAgent.install().is_empty());
    }

    #[test]
    fn install_contains_codex_installer() {
        assert!(
            CodexAgent
                .install()
                .contains("https://chatgpt.com/codex/install.sh")
        );
    }

    #[test]
    fn install_adds_local_bin_to_path() {
        assert!(
            CodexAgent
                .install()
                .contains("ENV PATH=/sandbox/.local/bin:$PATH")
        );
    }

    #[test]
    fn binary_path_is_local_bin_codex() {
        assert_eq!(CodexAgent.binary_path(), "/sandbox/.local/bin/codex");
    }

    #[test]
    fn skills_dir_is_empty() {
        assert_eq!(CodexAgent.skills_dir(), "");
    }

    #[test]
    fn skip_onboarding_is_noop() {
        let mut files = std::collections::HashMap::new();
        files.insert("some.json".to_string(), "content".to_string());
        let result = CodexAgent.skip_onboarding(files.clone());
        assert_eq!(result, files);
    }
}
