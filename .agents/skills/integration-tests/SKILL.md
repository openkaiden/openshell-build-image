---
name: integration-tests
description: Run the integration tests in tests/integration_test.rs against real container images built with podman
argument-hint: "[test filter]"
---

# Integration Tests

Run the integration test suite for openshell-build-image.

## Description

All integration tests live in `tests/integration_test.rs`. They are marked `#[ignore]` so they are excluded from the default `cargo test` run — they build real container images with `podman` and run commands inside them, which requires a working podman installation.

Image builds are expensive. The file uses `OnceLock<String>` so each image variant is built at most once per test process regardless of how many tests share it.

## Instructions

### Run the full suite

```bash
cargo test --test integration_test -- --include-ignored --test-threads=1
```

`--test-threads=1` is required: concurrent `podman build` calls would race on shared layers.

### Run a subset

The argument after `--` is a substring match on the full test path (`module::test_name`):

```bash
# All tests in one module
cargo test --test integration_test ubuntu_claude -- --include-ignored --test-threads=1

# A single test
cargo test --test integration_test "ubuntu_claude::claude_in_path" -- --include-ignored --test-threads=1

# All skills-related tests
cargo test --test integration_test skills -- --include-ignored --test-threads=1
```

If the user passes an argument to this skill, use it as the filter.

### Prerequisites

- `podman` must be installed and functional.
- The `openshell-build-image` binary must be up to date — run `cargo build` first if `src/` changed.

## How tests are written

### Core helpers

**`build_image(tag, extra_args)`** — calls the compiled binary, appends the tag, asserts success, returns the tag string. `extra_args` maps directly to CLI flags (`--agent`, `--inference`, `--config`, …).

**`run_in_image(image, cmd)`** — runs `podman run --rm --entrypoint /bin/bash <image> -c <cmd>` and returns the raw `Output`. The command is a shell expression. Bash is invoked explicitly because the generated image inherits its entrypoint from the base image. Always returns even on non-zero exit — callers check `out.status.success()` themselves.

**`check_*` helpers** — thin wrappers around `run_in_image` and `assert!` / `assert_eq!` for assertions that appear in multiple test modules (packages, policy rules, binary presence, …). Extract a new helper when the same `run_in_image` + assertion pattern appears in more than one place.

### Image singletons (`OnceLock`)

Every distinct image is held by a `static OnceLock<String>`. A `fn foo_image() -> &'static str` accessor calls `OnceLock::get_or_init` with a closure that calls `build_image`. Multiple tests that need the same image all call the same accessor and pay for the build only once.

Pattern:

```rust
static MY_IMAGE: OnceLock<String> = OnceLock::new();

fn my_image() -> &'static str {
    MY_IMAGE.get_or_init(|| {
        build_image("openshell-test-my-variant:integration", &["--agent", "claude"])
    })
}
```

The tag must end with `:integration` (the cleanup destructor filters by that suffix in the image name).

### The `image_tests!` macro

Generates the standard matrix of ten checks for one image:

```rust
image_tests!(
    mod_name,          // Rust identifier for the generated mod
    image_fn,          // accessor function (no parentheses)
    has_claude: bool,
    has_opencode: bool,
    has_anthropic: bool,
    has_vertexai: bool,
    has_ollama: bool,
    has_openai: bool
);
```

Every generated test is `#[ignore]`. The booleans control which policy rules and which binaries in `$PATH` are expected to be present. All ten checks (packages, Claude and OpenCode binary presence, policy file, and Claude, OpenCode, Anthropic, Vertex AI, Ollama, and OpenAI policy rules) run against the same image accessor.

### Feature test macros

`feature_common_utils_tests!`, `feature_node_tests!`, `feature_python_tests!`, `feature_local_tests!` each take `(mod_name, image_fn, base_image_fn)`. They generate `#[ignore]` tests that assert the feature artifact is present in the feature image and absent in the unmodified base image. Use this two-image pattern whenever you want to prove that a feature *adds* something.

### Named `mod` blocks (behavioural tests)

For behaviour that doesn't fit the matrix (a specific file written into the image, a CLI flag, an error case), write a named `mod`:

```rust
mod my_feature {
    use super::*;

    #[test]
    #[ignore]           // omit #[ignore] only if the test needs no podman
    fn some_assertion() {
        let out = run_in_image(my_image(), "test -f /sandbox/my-file");
        assert!(out.status.success(), "my-file not found");
    }
}
```

Presence checks (`test -f`, `test -d`, `which`) and content checks (`grep -q`) are common patterns. Always include a negative test (same assertion on an image built without the feature) to guard against false positives.

Tests that only run the binary and check its exit code or stderr (rejection tests) do not need podman and should **not** be marked `#[ignore]` — they run in the normal `cargo test` pass.

### Config / workspace helpers

- `fedora_config_dir()`, `ubi_config_dir()`, `hummingbird_config_dir()` — create a `tempfile::TempDir` containing `config.toml` that sets the base image. Pass the path via `--config`.
- `config_dir_with_agent_settings(agent, files)` — creates a tempdir with `agents/<agent>/<file>` entries for testing agent settings embedding.
- `workspace_dir(workspace_json)` / `build_image_with_workspace(tag, json, extra_args)` — for devcontainer feature tests, creates a `.kaiden/workspace.json` and runs the binary with `current_dir` set to the workspace root.
- `build_image_with_local_feature(tag, extra_args)` — creates a full local-feature workspace (devcontainer-feature.json + install.sh + main.sh) for local feature tests.
- `skills_workspace_dir()` / `build_image_with_skills(tag, extra_args)` — creates a workspace with a `my-skill/SKILL.md` file referenced from `.kaiden/workspace.json` for skills embedding tests.

## Adding a new integration test

### New image variant in the standard matrix

1. Add a `static FOO_IMAGE: OnceLock<String> = OnceLock::new();` next to the related statics.
2. Add a `fn foo_image() -> &'static str { ... }` accessor that calls `build_image` with the right flags.
3. Add an `image_tests!(foo, foo_image, has_claude: …, …)` call in the matrix block.
4. Add the tag string to the `cleanup_images` list at the bottom of the file.

### New check added to every matrix variant

Add the `check_*` helper function, then add a new `#[test] #[ignore] fn` inside the `image_tests!` macro body. It will appear for every existing and future instantiation automatically.

### New behavioural test

1. If the test needs a new image, add the singleton and accessor (steps 1–2 above) and add the tag to `cleanup_images`.
2. Add a `mod my_feature { use super::*; … }` block at the logical location in the file (group with related mods).
3. Write at least: one positive assertion, one content assertion where applicable, and one negative assertion (the artifact must not appear in a baseline image built without the feature).

### Cleanup

Every image tag introduced must appear in the `cleanup_images` array in the `#[ctor::dtor]` at the end of the file. Tags not listed there will linger in the local podman store after the test run.
