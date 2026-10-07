---
name: debug-image
description: Inspect a built openshell test image interactively — verify binaries, config files, policy, env vars after a failing integration test
argument-hint: "<image-tag>"
---

# Debug Image

Manually inspect a built test image to diagnose a failing integration test.

## Description

Integration tests build container images with podman and run commands inside them. When a test fails, the image is still on disk (unless you have already cleaned up) and can be inspected directly. This skill shows how to identify the correct image, replicate what the test does, and look inside the container to find what is wrong.

## Step 1 — identify the image tag

Test names map to image tags through the `image_tests!` macro. The test module name is the first argument to the macro, and the second argument is the accessor that contains the tag:

| Test path (from `cargo test` output) | Accessor | Image tag |
|--------------------------------------|----------|-----------|
| `ubuntu_claude::claude_in_path` | `ubuntu_claude_image()` | `openshell-test-ubuntu-claude:integration` |
| `ubuntu_opencode::policy_has_anthropic_rules` | `ubuntu_opencode_image()` | `openshell-test-ubuntu-opencode:integration` |
| `ubuntu_claude_vertexai::policy_has_vertexai_rules` | `ubuntu_claude_vertexai_image()` | `openshell-test-ubuntu-claude-vertexai:integration` |
| `fedora_opencode_ollama::opencode_in_path` | `fedora_opencode_ollama_image()` | `openshell-test-fedora-opencode-ollama:integration` |

General pattern: `<base>_<agent>[_<non-anthropic-inference>]` → `openshell-test-<base>-<agent>[-<inference>]:integration`

For tests in a named `mod` block (behavioral tests outside `image_tests!`), the image is whatever that block's function calls — read the test function to find the accessor.

## Step 2 — verify the image exists

```sh
podman images | grep openshell-test
```

If the image is missing, either the build failed (look for a panicked assertion in the test output: `"image build failed for tag ..."`) or it was already removed by the cleanup destructor. To rebuild it, go to Step 5.

## Step 3 — run a single command (replicating `run_in_image`)

The integration test helper is:

```rust
fn run_in_image(image: &str, cmd: &str) -> Output {
    Command::new("podman")
        .args(["run", "--rm", image, "-c", cmd])
        .output()
        ...
}
```

Use an explicit Bash entrypoint when running a shell command manually:

```sh
podman run --rm --entrypoint /bin/bash openshell-test-ubuntu-claude:integration -c "which claude"
podman run --rm --entrypoint /bin/bash openshell-test-ubuntu-claude:integration -c "cat /etc/openshell/policy.yaml"
```

## Step 4 — open an interactive shell

Drop into a shell for free-form exploration:

```sh
podman run -it --rm --entrypoint /bin/bash openshell-test-ubuntu-claude:integration

```

## Step 5 — rebuild an image manually

`build_image(tag, extra_args)` in the integration tests is equivalent to running:

```sh
cargo run -- <extra_args...> <tag>
```

Examples matching the common accessors:

```sh
# openshell-test-ubuntu-claude:integration
cargo run -- --agent claude --inference anthropic openshell-test-ubuntu-claude:integration

# openshell-test-ubuntu-opencode-ollama:integration
cargo run -- --agent opencode --inference ollama openshell-test-ubuntu-opencode-ollama:integration

# openshell-test-fedora-claude:integration (non-default base image needs --config)
cargo run -- --config /tmp/fedora-config --agent claude --inference anthropic openshell-test-fedora-claude:integration
```

For images that use a non-default base image (fedora, ubi, hummingbird), create a config.toml first:

```sh
mkdir /tmp/fedora-config
cat > /tmp/fedora-config/config.toml <<'EOF'
[openshell_build_image.base_image]
image = "fedora"
tag   = "42"
EOF
cargo run -- --config /tmp/fedora-config --agent claude --inference anthropic openshell-test-fedora-claude:integration
```

Enable build debug logging to see the exact `podman build` invocation:

```sh
RUST_LOG=debug cargo run -- --agent claude --inference anthropic openshell-test-ubuntu-claude:integration
```

## Step 6 — what to check per failing test

### `claude_in_path` / `opencode_in_path`

```sh
podman run --rm --entrypoint /bin/bash <image> -c "which claude"
podman run --rm --entrypoint /bin/bash <image> -c "echo $PATH"
```

The binary is installed under `/sandbox/` by the `install()` method and added to `PATH` via a Containerfile `ENV` instruction. If `which` fails, either the download in `install()` failed during the build or the `ENV PATH=` line is missing/wrong.

### `policy_yaml_exists`

```sh
podman run --rm --entrypoint /bin/bash <image> -c "test -f /etc/openshell/policy.yaml && echo found || echo missing"
podman run --rm --entrypoint /bin/bash <image> -c "cat /etc/openshell/policy.yaml"
```

### `policy_has_claude_rules` / `policy_has_anthropic_rules` / etc.

```sh
podman run --rm --entrypoint /bin/bash <image> -c "cat /etc/openshell/policy.yaml"
```

Check for the expected `name:` key. The policy check helpers look for:
- Claude agent: `name: claude-code`
- Opencode agent: `name: opencode`
- Anthropic inference: `name: anthropic`
- VertexAI inference: `name: vertexai`
- Ollama inference: `name: ollama`

### Onboarding skip / config files

```sh
# Claude — .claude.json skips onboarding
podman run --rm --entrypoint /bin/bash <image> -c "cat /sandbox/.claude.json"

# Opencode — config.json selects inference provider
podman run --rm --entrypoint /bin/bash <image> -c "cat /sandbox/.config/opencode/config.json"
```

### Environment variables baked into the image

```sh
podman run --rm --entrypoint /bin/bash <image> -c "env | grep ANTHROPIC"
podman run --rm --entrypoint /bin/bash <image> -c "printenv ANTHROPIC_BASE_URL"
```

### Skills directory

```sh
podman run --rm --entrypoint /bin/bash <image> -c "ls -la /sandbox/.claude/skills/"
podman run --rm --entrypoint /bin/bash <image> -c "ls -la /sandbox/.opencode/skills/"
```

## Step 7 — inspect the Containerfile

The Containerfile is generated in memory and piped to podman; it is not saved to disk by default. Three ways to view it:

**Option A — unit test with output capture** (cleanest): Run the matching `containerfile` unit test with stdout captured:

```sh
cargo test ubuntu_with_claude_agent_includes_install -- --nocapture
```

This prints the full Containerfile content for the matching combination.

**Option B — add a temporary debug print**: In `src/containerfile.rs`, add a `println!("{containerfile}")` at the end of `generate()`, rebuild, and run the tool:

```sh
cargo run -- --agent claude --inference anthropic debug-print:latest 2>&1 | head -100
```

Remove the print before committing.

**Option C — RUST_LOG=debug**: Shows the path to the temp file that holds the Containerfile, but the file is deleted when the process exits. Useful only when combined with a debugger or sleep.

## Inspect image metadata

```sh
# Entrypoint, Cmd, Env, User
podman inspect --format "{{json .Config}}" <image> | python3 -m json.tool

# Just the entrypoint
podman inspect --format "{{json .Config.Entrypoint}}" <image>

# Image layers and sizes
podman history <image>
```
