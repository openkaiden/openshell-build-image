---
name: add-inference
description: Step-by-step checklist for adding a new inference provider to openshell-build-image, covering the trait implementation, agent wiring, unit tests, integration tests, and README
argument-hint: "<provider-name>"
---

# Add Inference Provider

End-to-end checklist for making a new LLM backend available via `--inference`.

## Description

Adding a new inference provider touches six layers: the inference module, `main.rs`, each agent that will support it, unit tests throughout, integration tests, and the README. The existing providers are the canonical reference:

- **`anthropic`** — cloud, fixed default endpoint, supports `--endpoint` override, bakes env var into image for Claude.
- **`vertexai`** — cloud, proprietary fixed endpoint, rejects `--endpoint`, no default URL.
- **`ollama`** — local, default URL (`localhost:11434`), rewrites `localhost` → `host.openshell.internal`, writes opencode config file.
- **`openai`** — cloud, default endpoint `api.openai.com`, supports `--endpoint` override, opencode only, writes opencode config with native `openai/<model>` or `@ai-sdk/openai-compatible` custom provider.

## Step 1 — implement the `Inference` trait (`src/inference/<provider>.rs`)

Create `src/inference/<provider>.rs`. The trait has one required method:

```rust
use super::Inference;

pub struct MyProviderInference;

impl Inference for MyProviderInference {
    fn policy_yaml(&self, agent_binary: &str, base_url: Option<&str>) -> String {
        // ...
    }
}
```

**`policy_yaml`** returns a YAML fragment that is merged into the sandbox policy. It receives:
- `agent_binary` — the absolute path to the agent binary (scope the policy to it).
- `base_url` — the resolved endpoint URL when `--endpoint` was passed (and the provider supports it); `None` otherwise.

Follow the existing pattern closest to the new provider:

- **Cloud provider with `--endpoint` support** (anthropic pattern): if `base_url` is `Some`, parse its host and port with `super::parse_host_port` and use them; otherwise use the provider's default hostnames.
- **Cloud provider with fixed endpoint** (vertexai pattern): ignore `base_url` entirely (`_base_url`).
- **Local provider** (ollama pattern): call `base_url.and_then(super::parse_host_port).unwrap_or_else(|| (DEFAULT_HOST, DEFAULT_PORT))`. Export `DEFAULT_BASE_URL` as a `pub(crate) const` so `main.rs` can reference it for the localhost-rewrite logic.

The YAML shape must follow the existing structure — one network policy with `name`, `endpoints`, and `binaries`:

```yaml
version: 1
network_policies:
  myprovider:
    name: myprovider
    endpoints:
      - { host: api.myprovider.com, port: 443, protocol: rest, enforcement: enforce, access: full, tls: terminate }
    binaries:
      - { path: <agent_binary> }
```

Unit tests to write in the same file (inside `#[cfg(test)] mod tests`):
- `policy_yaml_contains_<provider>_endpoint` — default host appears in output.
- `policy_yaml_embeds_agent_binary` — agent binary path appears in output.
- `policy_yaml_has_<provider>_name` — `name: myprovider` appears in output.
- If `--endpoint` is supported: `policy_yaml_with_custom_endpoint_uses_proxy_host`, `policy_yaml_with_custom_endpoint_omits_default_host`, `policy_yaml_with_custom_endpoint_custom_port`.
- If endpoint is fixed: one test confirming `base_url` is ignored (pass a URL and assert the fixed host still appears).

## Step 2 — register in `src/inference/mod.rs`

Four additions:

```rust
mod myprovider;                          // 1. declare the module

pub(crate) use myprovider::DEFAULT_BASE_URL as MYPROVIDER_DEFAULT_BASE_URL;  // 2. re-export default URL (if local provider)

#[cfg(test)]
pub use myprovider::MyProviderInference; // 3. export for test use

// 4. add variant to the enum
#[derive(Clone, PartialEq, ValueEnum)]
pub enum InferenceKind {
    Anthropic,
    Ollama,
    #[value(name = "vertexai")]
    VertexAi,
    MyProvider,          // add here
}

// 5. add arm to from_kind()
pub fn from_kind(kind: InferenceKind) -> Box<dyn Inference> {
    match kind {
        ...
        InferenceKind::MyProvider => Box::new(myprovider::MyProviderInference),
    }
}
```

If the CLI value should differ from the Rust variant name (e.g. `vertexai` instead of `VertexAi`), add `#[value(name = "myprovider")]` above the variant.

## Step 3 — wire into `main.rs`

### `resolve_base_url()` — local providers only

If the new provider has a default local URL that needs `localhost` → `host.openshell.internal` rewriting, add a match arm following the Ollama pattern:

```rust
fn resolve_base_url(inference: Option<&inference::InferenceKind>, endpoint: Option<&str>) -> Option<String> {
    match inference {
        Some(inference::InferenceKind::MyProvider) => {
            let raw = endpoint.unwrap_or(inference::MYPROVIDER_DEFAULT_BASE_URL);
            Some(host::rewrite_localhost(raw))
        }
        ...
    }
}
```

For cloud providers this is not needed — `resolve_base_url` should return `None` unless `--endpoint` is given (anthropic pattern) or always `None` (vertexai pattern).

### Validation in `run()` — if `--endpoint` is unsupported

If the new provider has a proprietary fixed endpoint and must reject `--endpoint`, add an early check at the top of `run()`:

```rust
if endpoint.is_some() && inference_kind == Some(inference::InferenceKind::MyProvider) {
    return Err("--endpoint is not supported for the myprovider inference provider".into());
}
```

## Step 4 — update each agent that supports the new inference

For each agent that should work with the new provider:

### `supported_inference()` (`src/agent/claude.rs` and/or `src/agent/opencode/mod.rs`)

Add the new variant to the returned vec:

```rust
fn supported_inference(&self) -> Vec<inference::InferenceKind> {
    vec![
        inference::InferenceKind::Anthropic,
        inference::InferenceKind::VertexAi,
        inference::InferenceKind::MyProvider,  // add here
    ]
}
```

### `set_inference()` — if the provider requires agent config files

If using the new provider with a particular agent should write config files into the image (like Ollama writes `.config/opencode/config.json` for opencode), add an arm to `set_inference()`:

```rust
fn set_inference(&self, files: HashMap<String, String>, inference: Option<&inference::InferenceKind>, base_url: Option<&str>, model: Option<&str>) -> HashMap<String, String> {
    match inference {
        Some(inference::InferenceKind::MyProvider) if base_url.is_some() || model.is_some() => {
            myprovider::configure(files, base_url, model)
        }
        ...
    }
}
```

The `configure` function lives in `src/agent/opencode/myprovider.rs` (a new submodule), following the structure of `src/agent/opencode/anthropic.rs` and `src/agent/opencode/ollama.rs`.

### `env_vars()` — if the provider requires baked-in environment variables

If the new provider requires an env var baked into the image (like `ANTHROPIC_BASE_URL` for Claude + Anthropic), add a branch to `env_vars()`:

```rust
fn env_vars(&self, inference: Option<&inference::InferenceKind>, endpoint: Option<&str>, _model: Option<&str>) -> HashMap<String, String> {
    let mut vars = HashMap::new();
    if let (Some(inference::InferenceKind::MyProvider), Some(url)) = (inference, endpoint) {
        vars.insert("MYPROVIDER_BASE_URL".to_string(), url.to_string());
    }
    vars
}
```

### Unit tests to add in each agent file

- `supported_inference_includes_myprovider` — asserts the new variant is in the vec.
- `supported_inference_excludes_myprovider` in agents that do NOT support it.
- If `set_inference` was updated: tests for the new config file creation, following the existing `set_inference_with_ollama_*` pattern.
- If `env_vars` was updated: tests for the new env var, following `env_vars_with_anthropic_and_endpoint_sets_anthropic_base_url`.

### Unit tests to add in `src/main.rs`

- `build_policy_with_myprovider_inference_includes_<host>` — call `build_policy()` with the new inference and assert the expected host appears.
- If `resolve_base_url` was updated: `resolve_base_url_myprovider_*` tests following the `resolve_base_url_ollama_*` pattern.
- If validation was added: `run_with_endpoint_and_myprovider_returns_error`.

## Step 5 — integration tests (`tests/integration_test.rs`)

### Policy check helper

If the new inference produces a distinguishable policy entry, add a `check_myprovider_policy(image, expected)` helper following `check_anthropic_policy` / `check_ollama_policy`.

Then add `has_myprovider` to the `image_tests!` macro — add the parameter, add a generated `policy_has_myprovider_rules` test inside the macro body, and update every existing `image_tests!` call to pass `has_myprovider: false` (or `true` for the new combinations).

### Image singletons and accessors

Add one `OnceLock` + accessor per agent that supports the new inference, following the naming convention `<base_image>_<agent>_<provider>_image`. At minimum add ubuntu variants:

```rust
static UBUNTU_CLAUDE_MYPROVIDER_IMAGE: OnceLock<String> = OnceLock::new();

fn ubuntu_claude_myprovider_image() -> &'static str {
    UBUNTU_CLAUDE_MYPROVIDER_IMAGE.get_or_init(|| {
        build_image(
            "openshell-test-ubuntu-claude-myprovider:integration",
            &["--agent", "claude", "--inference", "myprovider"],
        )
    })
}
```

Add variants for every base image the new provider should be tested with.

### `image_tests!` calls and cleanup

Add `image_tests!` calls for the new combinations, then add every new tag to the `cleanup_images` array.

### Behavioural `mod` block

If the provider writes config files or env vars, add a `mod myprovider { ... }` block with:
- Positive assertion (expected file/env var is present).
- Content assertion (expected host/key appears in the file).
- Negative assertion (artifact absent when provider not selected).
- Rejection test if `--endpoint` is unsupported (no `#[ignore]` needed — never calls podman).

## Step 6 — update `.agents/skills/add-inference/SKILL.md`

Add the new provider to the canonical reference list at the top of the **Description** section of this skill, following the same one-line format as the existing entries:

```text
- **`myprovider`** — <cloud|local>, <endpoint behaviour>, <any special wiring>.
```

This keeps the reference list accurate and gives the next contributor a concrete example to follow for a similar provider.

## Step 7 — update `README.md`

- **Intro layer list** — if the new provider adds a new entry to the "Inference network rules" bullet, update the description.
- **"Configuring inference" table** — add a row with the `--inference` value, supported agents, and what the provider connects to.
- **"--endpoint" table** — add a row for each agent + new provider combination, stating whether `--endpoint` is supported and what effect it has.
- **"--model" table** — add a row for each agent + new provider combination, stating what file the model is written to.
- **"Agent × Inference Supported Features" table** — add a row for each agent that supports the new provider, filling in the "Inference settings", "Endpoint override", and "Model selection" columns.
- **"Sandbox policy" section** — add a sentence describing what endpoints the new inference rule allows.
- **"Full option reference" table** — update the `--inference` row description to include the new value.

## Checklist

- [ ] `src/inference/<provider>.rs` created with `Inference` trait implementation
- [ ] `src/inference/<provider>.rs` unit tests cover all endpoint and binary-embedding cases
- [ ] `src/inference/mod.rs`: `mod`, `pub use` (cfg test), `InferenceKind` variant, `from_kind` arm added
- [ ] `DEFAULT_BASE_URL` exported from mod.rs if the provider is local
- [ ] `resolve_base_url()` updated in `main.rs` if the provider has a default local URL
- [ ] Rejection added in `run()` if `--endpoint` is unsupported
- [ ] `supported_inference()` updated in each agent that supports the new provider
- [ ] `supported_inference()` exclusion tests added for agents that don't support it
- [ ] `set_inference()` updated (and new opencode submodule written) if config files are needed
- [ ] `env_vars()` updated if a baked-in env var is needed
- [ ] `build_policy_with_myprovider_*` unit tests added in `src/main.rs`
- [ ] Integration test policy check helper added; `image_tests!` macro and all existing calls updated
- [ ] Image singletons, accessors, `image_tests!` calls, and cleanup entries added
- [ ] Behavioural `mod` block written with positive, content, negative, and rejection tests
- [ ] `.agents/skills/add-inference/SKILL.md` Description updated with the new provider entry
- [ ] README updated: inference table, endpoint table, model table, agent×inference table, policy section, option reference
- [ ] `/check` passes (fmt + clippy + unit tests)
- [ ] `/copyright-headers` run on any new `.rs` files
