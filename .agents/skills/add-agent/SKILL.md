---
name: add-agent
description: Step-by-step checklist for adding a new agent to openshell-build-image, covering the Agent trait, mod.rs registration, unit tests, integration tests, and README
argument-hint: "<agent-name>"
---

# Add Agent

End-to-end checklist for making a new AI coding agent available via `--agent`.

## Description

Adding a new agent touches five layers: the agent module, `src/agent/mod.rs`, unit tests, integration tests (including extending the `image_tests!` macro), and the README. The existing agents are the canonical reference:

- **`claude`** — Claude Code CLI, curl installer, onboarding skip via `.claude.json`, agent-level network policy, anthropic+vertexai inference, skills at `/sandbox/.claude/skills`.
- **`opencode`** — Opencode CLI, curl installer, per-inference config submodule pattern (one `configure()` per provider), all three inference providers, skills at `/sandbox/.opencode/skills`.

## Step 1 — choose the file structure

Two patterns exist:

**Simple agent** (one implementation file, no per-inference config submodules) — use when the agent does not need to write provider-specific config files:

```
src/agent/myagent.rs
```

**Module directory** (separate submodule per supported inference provider) — use when `set_inference()` writes provider-specific config files into the image:

```
src/agent/myagent/
├── mod.rs             ← Agent trait impl, dispatches set_inference()
├── anthropic.rs       ← configure(files, base_url, model) → HashMap
├── ollama.rs
└── vertexai.rs
```

Choose the module directory when `set_inference()` is non-trivial. The `mod.rs` then delegates:

```rust
fn set_inference(&self, files: HashMap<String, String>, inference: Option<&inference::InferenceKind>, base_url: Option<&str>, model: Option<&str>) -> HashMap<String, String> {
    match inference {
        Some(inference::InferenceKind::Anthropic) => anthropic::configure(files, base_url, model),
        Some(inference::InferenceKind::Ollama)    => ollama::configure(files, base_url, model),
        Some(inference::InferenceKind::VertexAi)  => vertexai::configure(files, model),
        _ => files,
    }
}
```

## Step 2 — implement the `Agent` trait

Create the file(s) chosen in Step 1. The trait lives in `src/agent/mod.rs`. Three methods are required (no default); the rest are optional:

```rust
use std::collections::HashMap;
use crate::inference;

pub struct MyAgent;

impl super::Agent for MyAgent {
    // --- required ---

    fn id(&self) -> &str {
        "myagent"  // must match the AgentKind variant's CLI value
    }

    fn install(&self) -> String {
        // Returns a Containerfile RUN instruction (and ENV PATH extension) that
        // installs the agent binary at its chosen path under /sandbox. Follow
        // the curl pattern used by both existing agents:
        //   RUN curl -fsSL https://... | sh
        //   ENV PATH=/sandbox/.local/bin:$PATH
        "RUN curl -fsSL https://myagent.example.com/install.sh | sh\n\
         ENV PATH=/sandbox/.myagent/bin:$PATH".to_string()
    }

    fn binary_path(&self) -> &str {
        // Absolute path to the agent binary inside the image.
        // Used to scope network policy rules to this binary only.
        "/sandbox/.myagent/bin/myagent"
    }

    // --- optional (override when needed) ---

    fn policy_yaml(&self) -> String {
        // Agent-level network policy fragment (merged with inference policy).
        // Return empty string if no agent-specific network rules are needed.
        // See claude.rs for an example with download.example.com allowlist.
        String::new()
    }

    fn skip_onboarding(&self, mut files: HashMap<String, String>) -> HashMap<String, String> {
        // Insert config files (keyed by path relative to /sandbox) that
        // suppress interactive first-run prompts. Return files unchanged if
        // no onboarding skip is needed.
        files.insert(
            ".myagent/settings.json".to_string(),
            r#"{"onboardingCompleted":true}"#.to_string(),
        );
        files
    }

    fn supported_inference(&self) -> Vec<inference::InferenceKind> {
        // List the InferenceKind variants this agent can be configured for.
        // Return an empty vec if the agent has no inference integration.
        vec![
            inference::InferenceKind::Anthropic,
            inference::InferenceKind::VertexAi,
        ]
    }

    fn set_inference(&self, files: HashMap<String, String>, inference: Option<&inference::InferenceKind>, base_url: Option<&str>, model: Option<&str>) -> HashMap<String, String> {
        // Write provider-specific config files into the image context.
        // Return files unchanged if no config is needed.
        files
    }

    fn env_vars(&self, inference: Option<&inference::InferenceKind>, endpoint: Option<&str>, _model: Option<&str>) -> HashMap<String, String> {
        // Bake environment variables into the Containerfile ENV instruction.
        // Use when the agent reads configuration from env vars at runtime.
        HashMap::new()
    }

    fn skills_dir(&self) -> &str {
        // Path inside the image where skills are copied. Return "" to disable skills support.
        "/sandbox/.myagent/skills"
    }
}
```

## Step 3 — register in `src/agent/mod.rs`

Four additions:

```rust
mod myagent;                            // 1. declare the module

#[cfg(test)]
pub use myagent::MyAgent;              // 2. export for test use

// 3. add variant to the enum
#[derive(Clone, PartialEq, ValueEnum)]
pub enum AgentKind {
    Claude,
    Opencode,
    MyAgent,     // add here; add #[value(name = "myagent")] if CLI name differs
}

// 4. add arm to from_kind()
pub fn from_kind(kind: AgentKind) -> Box<dyn Agent> {
    match kind {
        ...
        AgentKind::MyAgent => Box::new(myagent::MyAgent),
    }
}
```

If the CLI value should differ from the Rust variant name (e.g., a hyphenated name), add `#[value(name = "my-agent")]` above the variant.

## Step 4 — unit tests

### In the agent file (`src/agent/myagent.rs` or `src/agent/myagent/mod.rs`)

Add a `#[cfg(test)] mod tests` block. Cover all implemented methods:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_is_myagent() {
        assert_eq!(MyAgent.id(), "myagent");
    }

    #[test]
    fn install_contains_installer_url() {
        assert!(MyAgent.install().contains("https://myagent.example.com/install.sh"));
    }

    #[test]
    fn install_extends_path() {
        assert!(MyAgent.install().contains("ENV PATH="));
    }

    #[test]
    fn binary_path_is_absolute() {
        assert!(MyAgent.binary_path().starts_with('/'));
    }

    // If skip_onboarding() was implemented:
    #[test]
    fn skip_onboarding_writes_settings_file() {
        let files = MyAgent.skip_onboarding(std::collections::HashMap::new());
        assert!(files.contains_key(".myagent/settings.json"));
    }

    // If supported_inference() was implemented:
    #[test]
    fn supported_inference_includes_anthropic() {
        assert!(MyAgent.supported_inference().contains(&crate::inference::InferenceKind::Anthropic));
    }

    // If skills_dir() was implemented:
    #[test]
    fn skills_dir_is_correct() {
        assert_eq!(MyAgent.skills_dir(), "/sandbox/.myagent/skills");
    }
}
```

If per-inference config submodules exist (`src/agent/myagent/anthropic.rs` etc.), add unit tests inside each submodule following the `opencode::anthropic` pattern: assert the config file key and the host/model appearing in the value.

### In `src/main.rs` (`#[cfg(test)]`)

- `build_policy_with_myagent_includes_binary` — call `build_policy()` with the new agent and assert the binary path appears in the output.
- If `policy_yaml()` returns non-empty content: `build_policy_with_myagent_includes_agent_network_rule`.

## Step 5 — integration tests (`tests/integration_test.rs`)

This step has four parts. **Adding a new agent requires extending the `image_tests!` macro itself**, which is the largest change.

### Part A — extend `image_tests!` macro

Add a `has_myagent` boolean parameter to the macro and a corresponding generated test:

```rust
macro_rules! image_tests {
    ($name:ident, $image_fn:ident,
     has_claude: $has_claude:expr,
     has_opencode: $has_opencode:expr,
     has_myagent: $has_myagent:expr,   // add here
     has_anthropic: $has_anthropic:expr,
     has_vertexai: $has_vertexai:expr,
     has_ollama: $has_ollama:expr) => {
        mod $name {
            use super::*;
            // ... existing tests ...

            // add:
            #[test]
            #[ignore]
            fn myagent_in_path() {
                if $has_myagent {
                    check_myagent_in_path($image_fn());
                } else {
                    check_binary_absent($image_fn(), "myagent");
                }
            }
        }
    };
}
```

Also add the `check_myagent_in_path` helper alongside the existing `check_claude_in_path` and `check_opencode_in_path` helpers:

```rust
fn check_myagent_in_path(image: &str) {
    let out = run_in_image(image, &["which", "myagent"]);
    assert!(
        out.status.success(),
        "myagent not found in PATH: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}
```

If the agent has a distinct `policy_yaml()`, add `check_myagent_policy(image, expected)` alongside `check_anthropic_policy` / `check_ollama_policy`, and add a `policy_has_myagent_rules` test inside the macro body.

**Update every existing `image_tests!` call** to add `has_myagent: false` — there are around 24 calls (6 combinations × 4 base images); update all of them.

### Part B — image singletons and accessors

Add one `OnceLock` and one accessor per base image × inference combination the new agent supports. Follow the naming convention exactly (`<base_image>_<agent>_<inference>_image`). At minimum cover all four base images with all supported inference providers:

```rust
static UBUNTU_MYAGENT_IMAGE: OnceLock<String> = OnceLock::new();

fn ubuntu_myagent_image() -> &'static str {
    UBUNTU_MYAGENT_IMAGE.get_or_init(|| {
        build_image(
            "openshell-test-ubuntu-myagent:integration",
            &["--agent", "myagent", "--inference", "anthropic"],
        )
    })
}
```

### Part C — `image_tests!` calls

Add one call per new combination in the matrix block, with the correct `has_*` flags:

```rust
image_tests!(ubuntu_myagent, ubuntu_myagent_image,
    has_claude: false, has_opencode: false, has_myagent: true,
    has_anthropic: true, has_vertexai: false, has_ollama: false);
```

### Part D — cleanup

Add every new image tag to the `cleanup_images` array in the `#[ctor::dtor]` block at the bottom of the file:

```rust
"openshell-test-ubuntu-myagent:integration",
"openshell-test-fedora-myagent:integration",
// ... one per new singleton ...
```

### Part E — behavioural `mod` block

Add a `mod myagent { use super::*; ... }` block with:

- **Binary in PATH** — `which myagent` succeeds in an image built with `--agent myagent`.
- **Onboarding skip** — if `skip_onboarding()` writes a file, assert the file exists.
- **Inference config** — if `set_inference()` writes config files, assert the file exists and contains the expected host/model.
- **Env var** — if `env_vars()` returns entries, assert they are set in the image.
- **Skills dir** — if `skills_dir()` is non-empty, assert the directory exists.
- **Negative** — assert the binary is absent in an image built without `--agent myagent`.

## Step 6 — update `.agents/skills/add-agent/SKILL.md`

Add the new agent to the canonical reference list at the top of the **Description** section of this skill, following the same one-line format as the existing entries:

```
- **`myagent`** — <agent CLI name>, <installer>, <onboarding skip mechanism>, <supported inference>, <skills path>.
```

This keeps the reference list accurate for the next contributor.

## Step 7 — update `.agents/skills/README.md`

No change needed — this skill is already registered. Skip this step.

## Step 8 — update `README.md`

Six places reference agents; all must be kept in sync:

1. **Intro layer list** (near the top) — the bullet "One of two supported AI coding agents" lists agent names. Add the new agent in the same style.

2. **"Installing an agent" section** — add a subsection or paragraph for the new agent describing the `--agent myagent` flag and any prerequisites.

3. **"Agent Supported Features" table** — add a row for the new agent with Yes/No for each capability column (Onboarding skip, Skills support, etc.).

4. **"Agent × Inference Supported Features" table** — add a row per supported inference, filling in the "Inference settings", "Endpoint override", and "Model selection" columns.

5. **"Sandbox policy" section** — if the agent adds its own network policy rules, add a sentence describing what endpoints are allowed and why.

6. **"Full option reference" table** — update the `--agent` row's description to include the new value alongside the existing ones.

## Checklist

- [ ] File structure chosen (single file vs. module directory)
- [ ] `id()`, `install()`, `binary_path()` implemented (required methods)
- [ ] Optional methods overridden as needed: `policy_yaml()`, `skip_onboarding()`, `supported_inference()`, `set_inference()`, `env_vars()`, `skills_dir()`
- [ ] Per-inference submodules written if module directory pattern used
- [ ] `src/agent/mod.rs`: `mod`, `pub use` (cfg test), `AgentKind` variant, `from_kind` arm added
- [ ] Unit tests in agent file cover all implemented methods
- [ ] `build_policy_with_myagent_*` unit tests added in `src/main.rs`
- [ ] `image_tests!` macro extended with `has_myagent` parameter and generated test
- [ ] `check_myagent_in_path()` helper added; `check_myagent_policy()` if needed
- [ ] All existing `image_tests!` calls updated with `has_myagent: false`
- [ ] Image singletons, accessors, `image_tests!` calls, and cleanup entries added
- [ ] Behavioural `mod myagent` block written covering binary, onboarding, inference, skills, and negative cases
- [ ] `.agents/skills/add-agent/SKILL.md` Description updated with the new agent entry
- [ ] README intro list updated
- [ ] README "Installing an agent" section updated
- [ ] README "Agent Supported Features" table updated
- [ ] README "Agent × Inference Supported Features" table updated
- [ ] README "Sandbox policy" section updated if needed
- [ ] README "Full option reference" `--agent` row updated
- [ ] `/check` passes (fmt + clippy + unit tests)
- [ ] `/copyright-headers` run on any new `.rs` files
