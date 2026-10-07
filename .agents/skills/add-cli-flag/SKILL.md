---
name: add-cli-flag
description: Step-by-step checklist for adding a new CLI flag to openshell-build-image, covering all locations from the clap struct to integration tests
argument-hint: "<flag-name> <description>"
---

# Add CLI Flag

End-to-end checklist for introducing a new CLI argument to openshell-build-image.

## Description

Every CLI flag touches at least four files. Missing any one of them compiles fine but leaves the flag silently unused or untested. Follow the steps below in order.

The existing flags are the canonical reference:
- `--agent` / `--inference` — enum flags backed by `ValueEnum`, gated by a compatibility check in `run()`
- `--endpoint` — `Option<String>` that overrides a provider URL, validated early in `run()`, flows into `resolve_base_url()` and `stage_agent_settings()`
- `--model` — `Option<String>` threaded through `stage_agent_settings()` and `agent.env_vars()`
- `--ssl-certs` — `Option<String>` with `num_args = 0..=1` and `default_missing_value = ""` (optional value flag); converted to `Option<Option<PathBuf>>` before being passed to `run()`

## Step 1 — declare the argument in the `Cli` struct (`src/main.rs`)

Add a field to the `Cli` struct with a `#[arg(...)]` attribute:

```rust
#[arg(long, help = "Short description shown in --help")]
my_flag: Option<String>,  // or bool, Option<MyEnum>, etc.
```

Common patterns:
- `Option<String>` — optional string value (`--my-flag value`)
- `Option<MyEnum>` where `MyEnum: ValueEnum` — enum with validated variants
- `bool` with `action = clap::ArgAction::SetTrue` — a plain switch

## Step 2 — pass it to `run()` (`src/main.rs`)

In `main()`, add the field to the `run()` call:

```rust
cli.my_flag.as_deref(),  // for Option<String>
cli.my_flag,             // for bool or Option<Enum>
```

Then extend the `run()` signature:

```rust
fn run(
    ...
    my_flag: Option<&str>,   // add here
    ssl_certs: Option<Option<PathBuf>>,
    runner: &dyn build::Runner,
) -> Result<(), Box<dyn std::error::Error>> {
```

`run()` already has `#[allow(clippy::too_many_arguments)]` — no need to add it again.

## Step 3 — validate early in `run()` (`src/main.rs`)

If the new flag has incompatible combinations with existing flags, reject them at the top of `run()` before any work begins, following the `--endpoint` + `vertexai` pattern:

```rust
if my_flag.is_some() && some_condition {
    return Err("--my-flag is not supported when ...".into());
}
```

Keep error messages lowercase, starting with the flag name.

## Step 4 — implement the behaviour

Where the flag's effect lives depends on what it does:

### Flag affects what is staged into the image context

Thread it through `stage_agent_settings()` (for agent config files / env vars) or add a new staging function following the `stage_skills()` pattern.

### Flag affects the policy

Pass it to `build_policy()` and into the relevant `inference.policy_yaml()` or `agent.policy_yaml()` implementation.

### Flag affects the Containerfile

Pass it to `containerfile::generate()` in `src/containerfile.rs`.

### Flag adds a new enum variant (agent or inference kind)

1. Add the variant to `AgentKind` or `InferenceKind` (both derive `ValueEnum`, so clap picks it up automatically).
2. Add the concrete implementation struct in a new submodule under `src/agent/` or `src/inference/`.
3. Add the `mod` declaration and the `from_kind` arm in `src/agent/mod.rs` or `src/inference/mod.rs`.
4. Export the struct under `#[cfg(test)]` for unit test use — follow the existing pattern:
   ```rust
   #[cfg(test)]
   pub use my_module::MyImpl;
   ```

## Step 5 — unit tests (`src/main.rs`, `#[cfg(test)]`)

Add tests that exercise the new parameter through `run()` using `FakeRunner`, which fakes the podman call:

```rust
#[test]
fn run_with_my_flag_succeeds() {
    let tmp = tempfile::tempdir().unwrap();
    let result = run(
        "test:latest",
        Some(tmp.path().to_path_buf()),
        tmp.path(),
        None,        // agent_kind
        None,        // inference_kind
        None,        // endpoint
        None,        // model
        Some("my-value"),  // my_flag
        None,        // ssl_certs
        &FakeRunner(0),
    );
    assert!(result.is_ok(), "expected Ok, got {result:?}");
}
```

If the flag has rejection logic, add a dedicated test that asserts `result.is_err()` and checks the error message text.

For flags that write files into the image context, test through `stage_agent_settings()` or the relevant staging function directly — those functions take a `context_dir` and you can assert on the files created there.

## Step 6 — integration tests (`tests/integration_test.rs`)

### If the flag produces a new image variant

1. Add a `static MY_IMAGE: OnceLock<String> = OnceLock::new();` alongside the related statics.
2. Add an accessor `fn my_image() -> &'static str { MY_IMAGE.get_or_init(|| build_image("openshell-test-my-variant:integration", &["--my-flag", "value"])) }`.
3. Add it to the `cleanup_images` array in the `#[ctor::dtor]` at the bottom of the file.

### Tests for the flag's observable effect

Add a `mod my_flag { use super::*; ... }` block with at least:
- A positive assertion (the expected file, env var, or policy rule is present).
- A negative assertion (the artifact is absent when the flag is not passed).
- A rejection test if the flag has an invalid-combination case — rejection tests do not need `#[ignore]` because they never call podman.

## Step 7 — update `README.md`

The README has five places that may need updating. The first is mandatory; the rest depend on the flag's scope.

### Always: "Full option reference" table

At the bottom of the README, every flag appears in this table. Add a row:

```markdown
| `--my-flag <VALUE>` | Short description matching the clap `help` string |
```

### If the flag adds a new conceptual layer: intro list

The numbered list near the top of the README describes what the tool assembles layer by layer. If the new flag introduces a genuinely new layer (or a new bullet under an existing layer), update that list. For a sub-capability of an existing layer, add an indented bullet following the `--endpoint` and `--model` style.

### If the flag's effect varies by agent: "Agent Supported Features" table

This table has a row per agent and a column per capability. Add a column if the flag exposes a capability that only some agents support:

```markdown
| Agent      | ... | My Feature |
| ---------- | --- | ---------- |
| `claude`   | ... | Yes / No   |
| `opencode` | ... | Yes / No   |
```

### If the flag's effect varies by agent × inference: "Agent × Inference Supported Features" table

Add a column to the existing table, following the "Endpoint override" or "Model selection" pattern — include the mechanism (what file or env var is written) in the cell, not just Yes/No.

### If the flag has significant user-facing behavior: dedicated section or subsection

Flags with their own behavior description get either:
- A **top-level `##` section** (if the flag is the primary subject, like `--agent` or `--inference`).
- A **`###` subsection** inside an existing section (like `--endpoint` and `--model` live under "Configuring inference").

The section should cover: what the flag does, any per-agent or per-agent×inference variations (link to the table if one exists), and a `sh` code block with at least one concrete example command.

## Checklist

- [ ] Field added to `Cli` struct with `#[arg(...)]`
- [ ] Field passed through `main()` → `run()` signature → `run()` body
- [ ] Incompatible combinations rejected at the top of `run()` with a clear error
- [ ] Behaviour implemented in the appropriate module
- [ ] New enum variant exported under `#[cfg(test)]` if applicable
- [ ] Unit tests cover the happy path, rejection, and any file-content assertions
- [ ] Integration test singleton + accessor + cleanup entry added
- [ ] Integration test `mod` block covers positive, negative, and rejection cases
- [ ] README "Full option reference" table updated
- [ ] README intro layer list updated if the flag adds a new layer or sub-capability
- [ ] README "Agent Supported Features" table updated if the flag is agent-gated
- [ ] README "Agent × Inference Supported Features" table updated if the flag varies by inference
- [ ] README dedicated section or subsection added for significant user-facing behaviour
- [ ] `/check` passes (fmt + clippy + unit tests)
- [ ] `/copyright-headers` run on any new `.rs` files
