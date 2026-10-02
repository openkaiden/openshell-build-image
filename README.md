# openshell-image-builder

[OpenShell](https://github.com/NVIDIA/OpenShell-Community) is NVIDIA's runtime environment for autonomous AI agents. It provides isolated sandboxes where agents can safely run and iterate — without risk to the host system or your credentials.

OpenShell ships a set of [pre-built sandbox images](https://github.com/NVIDIA/OpenShell-Community), but they are general-purpose. `openshell-image-builder` lets you build your own: lightweight, workspace-specific images that contain only what you need — without writing a Containerfile by hand.

The tool assembles the image in layers — base image, agent installation, agent settings, OpenShell network policy, project-specific toolchains, and image-mount init scripts. Use `--runtime` to select which container CLI drives the build (`podman`, `docker`, or the macOS `container` CLI):

1. **Base image** — Ubuntu, Fedora, Red Hat UBI, or Red Hat Hardened Images (HummingBird), any tag. Ubuntu 24.04 is the default.
2. **Agent installation** (`--agent`) — the agent binary is pre-installed in `PATH`.
3. **Agent settings** (`--with-agent-settings`) — only included when this flag is set.
   - **User settings** — settings files are pre-populated with settings files provided by the user.
   - **Auto-onboarding** — settings files are updated to skip onboarding steps (choose theme, trust directory, etc).
   - **Skills** — skills are copied in the agent's skills directory.
   - **Inference settings** (`--inference`) — inference provider definition is added to settings files.
   - **Endpoint override** (`--endpoint`) — optional custom URL for inference provider is set in inference provider definition.
   - **Model** (`--model`) — default model is baked into the agent's settings files.
4. **OpenShell policy** (`--with-policy`) — `/etc/openshell/policy.yaml` copied into the image only when `--with-policy` is passed.
   - **Base policy** — Git operations over HTTPS and the GitHub REST API.
   - **Agent network rules** — agent-specific endpoints are added by `--agent`.
   - **Inference network rules** — LLM backend endpoints are added by `--inference`.
   - **Workspace network rules** — user-defined hosts declared in `.kaiden/workspace.json` are added to the policy when `--with-workspace-config` is used.
5. **Installation of project-specific toolchains** — toolchains and utilities declared as Dev Container Features in `.kaiden/workspace.json` are installed in the image when `--with-workspace-config` is used.
6. **Image-mount init scripts** (`--image-mount`) — shell init snippets from images-to-mount YAML files are appended to `/sandbox/.bashrc` and `/sandbox/.zshrc`.

### workspace.json fields

`.kaiden/workspace.json` is the per-workspace configuration file, read when `--with-workspace-config` is passed. The following fields are supported:

| Field | Description | Details |
| ----- | ----------- | ------- |
| `features` | Dev Container Features to install in the image | [Dev Container Features](#dev-container-features) |
| `skills` | Skill directories to copy into the agent's skills directory | [Skills](#skills) |
| `network.hosts` | Hostnames (and optional ports) to allow through the sandbox network policy | [Workspace network rules](#workspace-network-rules) |
| ~~`network.mode`~~ | ~~`allow` or `deny` — OpenShell always enforces deny mode; allow-all is not supported~~ | ~~not used by the image builder~~ |
| ~~`environment`~~ | ~~Environment variables to inject into the workspace~~ | ~~not used by the image builder~~ |
| ~~`mcp`~~ | ~~MCP server configuration (command-based and URL-based servers)~~ | ~~not used by the image builder~~ |
| ~~`mounts`~~ | ~~Host directories to mount in the workspace~~ | ~~not used by the image builder~~ |
| ~~`ports`~~ | ~~TCP ports to expose from the workspace~~ | ~~not used by the image builder~~ |
| ~~`secrets`~~ | ~~Secret names to inject into the workspace~~ | ~~not used by the image builder~~ |

### Agent Supported Features

| Agent      | User settings | Auto-onboarding | Skills |
| ---------- | ------------- | --------------- | ------ |
| `claude`   | Yes           | Yes             | Yes<br>`~/.claude/skills/`   |
| `opencode` | Yes           | N/A             | Yes<br>`~/.opencode/skills/` |

### Agent × Inference Supported Features

| Agent      | Inference   | Inference settings             | Endpoint override                          | Model selection                                  |
| ---------- | ----------- | ------------------------------ | ------------------------------------------ | ------------------------------------------------ |
| `claude`   | `anthropic` | N/A                            | Yes<br>`ENV ANTHROPIC_BASE_URL`            | Yes<br>`model` in `.claude/settings.json`        |
| `claude`   | `vertexai`  | N/A                            | No<br>fixed endpoint                       | Yes<br>`model` in `.claude/settings.json`        |
| `opencode` | `anthropic` | N/A, Yes if endpoint override  | Yes<br>opencode config `baseURL`           | Yes<br>`model` in `.config/opencode/config.json` |
| `opencode` | `vertexai`  | N/A                            | No<br>fixed endpoint                       | Yes<br>`model` in `.config/opencode/config.json` |
| `opencode` | `ollama`    | Yes<br>Ollama provider config  | Yes<br>`baseURL` in Ollama provider config | Yes<br>`model` in `.config/opencode/config.json` |
| `opencode` | `openai`    | Yes if model or endpoint       | Yes<br>`baseURL` in custom provider config | Yes<br>`model` in `.config/opencode/config.json` |

## Quick start

Build an image with a single command:

```sh
openshell-image-builder --runtime podman myimage:latest
```

`<TAG>` and `--runtime` are the only required arguments — `--runtime` selects the container CLI (`podman`, `docker`, or `container`), and `<TAG>` sets the tag for the built image. By default, the tool uses Ubuntu 24.04 as the base image.

## Configuring the base image

To use a different base image or tag, create a configuration file.

### File location

The tool looks for a `config.toml` file in this order, using the first directory found:

1. Directory given by the `--config` flag
2. Directory set in the `OPENSHELL_IMAGE_BUILDER_CONFIG` environment variable
3. The platform config directory:
   - Linux: `$XDG_CONFIG_HOME/openshell-image-builder/` (defaults to `~/.config/openshell-image-builder/`)
   - macOS: `~/Library/Application Support/openshell-image-builder/`
   - Windows: `%APPDATA%\openshell-image-builder\`

If no `config.toml` is found in the resolved directory, or the file is empty, built-in defaults are used.

If a directory is given explicitly (via `--config` or the environment variable) but it does not exist, the command fails immediately.

### Base images

**Ubuntu** (default)

```toml
[openshell_image_builder.base_image]
image = "ubuntu"
tag   = "24.04"
```

**Fedora**

```toml
[openshell_image_builder.base_image]
image = "fedora"
tag   = "latest"
```

**Red Hat UBI**

```toml
[openshell_image_builder.base_image]
image = "ubi"
tag   = "latest"
```

**Red Hat Hardened Images (Hummingbird)**

```toml
[openshell_image_builder.base_image]
image = "hummingbird"
tag   = "latest-builder"
```

### Full schema reference

```toml
[openshell_image_builder]
version = 1

[openshell_image_builder.base_image]
image = "ubuntu"   # "ubuntu", "fedora", "ubi", or "hummingbird"
tag   = "24.04"
```

| Field                                      | Default  | Description                  |
| ------------------------------------------ | -------- | ---------------------------- |
| `openshell_image_builder.version`          | `1`      | Configuration schema version |
| `openshell_image_builder.base_image.image` | `ubuntu` | Base image name (`ubuntu`, `fedora`, `ubi`, or `hummingbird`) |
| `openshell_image_builder.base_image.tag`   | `24.04`  | Base image tag — Ubuntu: `24.04`, `22.04`, …; Fedora: `latest`, `43`, `42`, …; UBI: `latest`, `10.2-1780377767`, …; Hummingbird: `latest-builder`, … |

### Loading from a specific config directory

Pass `--config` to point to a directory explicitly (the tool reads `config.toml` inside it):

```sh
openshell-image-builder --runtime podman --config /path/to/config/dir myimage:latest
```

Or set the environment variable instead:

```sh
OPENSHELL_IMAGE_BUILDER_CONFIG=/path/to/config/dir openshell-image-builder myimage:latest
```

## Logging

Use `-v` (info) or `-vv` (debug) to increase log verbosity — useful for tracing which config file is loaded:

```sh
openshell-image-builder --runtime podman -v myimage:latest
```

## Enterprise environments

### Corporate proxy support (`--ssl-certs`)

In environments where outbound HTTPS traffic is intercepted by a corporate proxy (e.g. Netskope, Zscaler, or a custom MITM proxy), `dnf install` and `apt-get install` fail during the build because the proxy presents a self-signed or corporate-issued certificate that the container doesn't trust.

By default, the tool auto-discovers the host's CA bundle and copies it into the build context, installing it in the image. The certificate is trusted both **during the build** (so package installation succeeds) and **at runtime** (so the agent can reach its LLM backend through the same proxy).

**Auto-discover (default)** — the tool searches for a CA bundle in common system locations and uses the first one it finds:

| Distribution | Path |
| ------------ | ---- |
| Debian / Ubuntu / Gentoo | `/etc/ssl/certs/ca-certificates.crt` |
| Fedora / RHEL 6 | `/etc/pki/tls/certs/ca-bundle.crt` |
| Fedora / RHEL 7+ / CentOS / Rocky / Alma | `/etc/pki/ca-trust/extracted/pem/tls-ca-bundle.pem` |
| OpenSUSE | `/etc/ssl/ca-bundle.pem` |
| SUSE / older | `/etc/ssl/certs/ca-bundle.crt` |
| Alpine | `/etc/ssl/cert.pem` |
| Arch | `/etc/ca-certificates/extracted/tls-ca-bundle.pem` |

If none of the above paths exist, the build proceeds without adding any certificates.

**Explicit file** — point directly to a specific CA bundle with `--ssl-certs`. The build fails immediately if the file does not exist:

```sh
openshell-image-builder --ssl-certs /etc/pki/tls/certs/ca-bundle.crt myimage:latest
```

**Disable** — pass `--disable-ssl-certs` to skip certificate bundling entirely:

```sh
openshell-image-builder --disable-ssl-certs myimage:latest
```

#### How it works

The CA bundle is copied into the build context and installed in the image's system trust store during the `system` stage, before any packages are installed:

- **Fedora / UBI / Hummingbird** — copied to `/etc/pki/ca-trust/source/anchors/system-ca.crt`, then `update-ca-trust` is run before `dnf install`.
- **Ubuntu** — copied to `/usr/local/share/ca-certificates/system-ca.crt`, then `update-ca-certificates` is run after `apt-get install` (Ubuntu mirrors use HTTP, so the cert is not needed for package installation itself, but is available to the agent at runtime).

Because the `final` image stage inherits the full filesystem from `system`, the CA bundle and updated trust database are present in the running sandbox.

#### Example — agent build behind a corporate proxy

```sh
# Auto-discover the host CA bundle and build with Claude Code (default behaviour)
openshell-image-builder \
  --agent claude \
  --inference anthropic \
  myimage:latest

# Point to a specific bundle instead
openshell-image-builder \
  --ssl-certs /usr/local/share/ca-certificates/my-corp-ca.crt \
  --agent claude \
  --inference anthropic \
  myimage:latest
```

## Installing an agent

Pass `--agent` to install an agent into the image.

| Agent       | Value      | Description                    |
| ----------- | ---------- | ------------------------------ |
| Claude Code | `claude`   | Anthropic's Claude Code CLI    |
| OpenCode    | `opencode` | OpenCode AI coding agent       |

```sh
openshell-image-builder --runtime podman --agent claude myimage:latest
openshell-image-builder --runtime podman --agent opencode myimage:latest
```

## Agent settings

Pass `--with-agent-settings` to generate and include agent settings in the image. Without this flag, no settings files are written and no auto-configuration is applied — the image contains only the agent binary.

You can pre-populate the sandbox home directory with settings files specific to an agent. Place the files under:

```
<settings dir>/agents/<agent>/
```

where `<settings dir>` is the directory described in [Configuring the base image](#configuring-the-base-image), and `<agent>` matches the value passed to `--agent` (`claude` or `opencode`).

All files and subdirectories are copied into `/sandbox/` (the sandbox user's home directory), owned by the `sandbox` user. The copy happens before the agent is installed, so the agent installer can create additional files on top without overwriting your settings.

### Example — Claude Code settings file

```sh
mkdir -p ~/.config/openshell-image-builder/agents/claude/.claude
cp ~/.claude/settings.json \
   ~/.config/openshell-image-builder/agents/claude/.claude/settings.json

openshell-image-builder --runtime podman --agent claude --with-agent-settings myimage:latest
```

The file will be present at `/sandbox/.claude/settings.json` in the image.

### Example — OpenCode settings file

```sh
mkdir -p ~/.config/openshell-image-builder/agents/opencode/.config/opencode
cp ~/.config/opencode/config.json \
   ~/.config/openshell-image-builder/agents/opencode/.config/opencode/config.json

openshell-image-builder --runtime podman --agent opencode --with-agent-settings myimage:latest
```

The file will be present at `/sandbox/.config/opencode/config.json` in the image.

### Automatic configuration — Claude Code

When `--agent claude --with-agent-settings` is used, the builder automatically creates or updates `/sandbox/.claude.json` with the following settings to skip the interactive onboarding dialogs that would otherwise appear on first launch:

- `hasCompletedOnboarding: true` — marks the setup wizard as complete.
- `projects["/sandbox"].hasTrustDialogAccepted: true` — pre-accepts the workspace trust prompt for the sandbox home directory.

If you provide your own `.claude.json` in the agent settings directory, the builder merges these fields into it, preserving any other fields you have set.

## Configuring inference

Pass `--inference` to allow the agent to reach its LLM backend. This is separate from `--agent` because the same inference provider can serve multiple agents.

| Inference | Value       | Agents                  | Description                         |
| --------- | ----------- | ----------------------- | ----------------------------------- |
| Anthropic | `anthropic` | `claude`, `opencode`    | Anthropic API (`api.anthropic.com`) |
| Vertex AI | `vertexai`  | `claude`, `opencode`    | Google Vertex AI (`oauth2.googleapis.com`, `aiplatform.googleapis.com`, `*-aiplatform.googleapis.com`) |
| Ollama    | `ollama`    | `opencode`              | Local models on the host machine, reached via `host.openshell.internal:11434` |
| OpenAI    | `openai`    | `opencode`              | OpenAI API (`api.openai.com`), or any OpenAI-compatible endpoint via `--endpoint` |

```sh
openshell-image-builder --runtime podman --agent claude --inference anthropic myimage:latest
openshell-image-builder --runtime podman --agent opencode --inference anthropic myimage:latest
openshell-image-builder --runtime podman --agent claude --inference vertexai myimage:latest
openshell-image-builder --runtime podman --agent opencode --inference vertexai myimage:latest
openshell-image-builder --runtime podman --agent opencode --inference ollama myimage:latest
openshell-image-builder --runtime podman --agent opencode --inference openai myimage:latest
```

### Custom endpoint (`--endpoint`)

Use `--endpoint` to override the inference provider's default URL — useful for routing through a proxy, a local instance, or a non-default port.

| Agent      | Inference   | Supported | Effect |
| ---------- | ----------- | --------- | ------ |
| `claude`   | `anthropic` | ✅        | Baked into the image as `ENV ANTHROPIC_BASE_URL=<url>` |
| `claude`   | `vertexai`  | ❌        | Rejected — Vertex AI has a proprietary fixed endpoint |
| `opencode` | `anthropic` | ✅        | Written to opencode config as `provider.anthropic.options.baseURL` |
| `opencode` | `vertexai`  | ❌        | Rejected — Vertex AI has a proprietary fixed endpoint |
| `opencode` | `ollama`    | ✅        | Written to opencode config as `provider.ollama.options.baseURL`; `localhost` in the URL is rewritten to `host.openshell.internal`; defaults to `http://host.openshell.internal:11434/v1` if omitted |
| `opencode` | `openai`    | ✅        | When provided, opencode is configured to use a custom `@ai-sdk/openai-compatible` provider with `options.baseURL` set to the given URL |

```sh
# Route Claude Code through a custom Anthropic API proxy
openshell-image-builder \
  --runtime podman \
  --agent claude --inference anthropic \
  --endpoint https://my-anthropic-proxy.example.com \
  myimage:latest

# Route OpenCode through a custom Anthropic API proxy
openshell-image-builder \
  --runtime podman \
  --agent opencode --inference anthropic \
  --endpoint https://my-anthropic-proxy.example.com \
  myimage:latest

# Connect OpenCode to Ollama running on a non-default port
openshell-image-builder \
  --runtime podman \
  --agent opencode --inference ollama \
  --endpoint http://localhost:9999/v1 \
  myimage:latest
```

### Default model (`--model`)

Use `--model` to bake a default model into the image. The agent uses this model without requiring a runtime flag.

| Agent      | Inference   | Effect |
| ---------- | ----------- | ------ |
| `claude`   | any         | Written to `.claude/settings.json` as `"model": "<model>"` |
| `opencode` | `anthropic` | Written to opencode config as top-level `"model"` field (can be combined with `--endpoint`) |
| `opencode` | `vertexai`  | Written to opencode config as top-level `"model"` field |
| `opencode` | `ollama`    | Written to opencode config as top-level `"model": "ollama/<model>"` field; only the specified model is registered in the models map |
| `opencode` | `openai`    | Written to opencode config as `"model": "openai/<model>"` (native OpenAI) or `"model": "custom/<model>"` (with `--endpoint`) |

```sh
# Pin Claude Code to a specific model
openshell-image-builder \
  --runtime podman \
  --agent claude --inference anthropic \
  --model claude-opus-4-8 \
  myimage:latest

# Pin OpenCode to a specific Anthropic model
openshell-image-builder \
  --runtime podman \
  --agent opencode --inference anthropic \
  --model claude-opus-4-8 \
  myimage:latest

# Pin OpenCode to a specific Ollama model
openshell-image-builder \
  --runtime podman \
  --agent opencode --inference ollama \
  --model qwen3-coder:30b \
  myimage:latest
```

The model string is passed through as-is — use whatever identifier your agent and provider expect.

### Automatic configuration — OpenCode + Ollama

When `--agent opencode --inference ollama --with-agent-settings` is used, the builder automatically writes `/sandbox/.config/opencode/config.json` to configure OpenCode's Ollama provider:

```json
{
  "$schema": "https://opencode.ai/config.json",
  "provider": {
    "ollama": {
      "npm": "@ai-sdk/openai-compatible",
      "options": {
        "baseURL": "http://host.openshell.internal:11434/v1"
      },
      "models": {
        "lfm2.5":          { "tools": true },
        "qwen3-coder:30b": { "tools": true }
      }
    }
  }
}
```

When `--model` is also given, the top-level `"model"` field is added (as `"ollama/<model>"`) and the `models` map is replaced with a single entry for the specified model.

`host.openshell.internal` is the hostname used inside the sandbox to reach the container host, where Ollama is expected to be running on its default port (`11434`). Ollama must be running on the host before the sandbox is started.

## Sandbox policy

Pass `--with-policy` to include `/etc/openshell/policy.yaml` in the image. Without this flag, no policy file is written and the image contains no OpenShell policy. The policy file is read by the OpenShell runtime and defines the sandbox security policy for the container:

- **Filesystem policy** — which paths are read-only, read-write, or inaccessible to the `sandbox` user.
- **Network policies** — which binaries are allowed to connect to which hosts and ports.

```sh
openshell-image-builder --runtime podman --agent claude --inference anthropic --with-policy myimage:latest
```

The policy is built in four layers, merged in order:

1. **Base** ([`assets/policy.yaml`](assets/policy.yaml)) — general-purpose tooling: Git operations over HTTPS and the GitHub REST API via `gh`.
2. **Inference** (added by `--inference`) — LLM backend endpoints scoped to the agent binary. For example, `--inference anthropic` adds `api.anthropic.com` and `statsig.anthropic.com`; `--inference vertexai` adds `oauth2.googleapis.com` and `aiplatform.googleapis.com` (including the `*-aiplatform.googleapis.com` wildcard); `--inference ollama` adds `host.openshell.internal:11434` for local model access; `--inference openai` adds `api.openai.com` (or the custom endpoint host when `--endpoint` is used).
3. **Agent** (added by `--agent`) — agent-specific endpoints. For example, `--agent claude` adds `platform.claude.com`, `raw.githubusercontent.com`, and the GitHub REST API for Claude's coding tools; `--agent opencode` adds `opencode.ai`, `registry.npmjs.org`, and `models.dev`.
4. **Workspace** (added from `network.hosts` in `.kaiden/workspace.json` when `--with-workspace-config` is used) — user-defined hosts that any binary in standard PATH directories (`/bin`, `/usr/bin`, `/usr/local/bin`, `/sandbox/.local/bin`) and the agent binary (when present) may reach. See [Workspace network rules](#workspace-network-rules).

## Dev Container Features

The tool supports [Dev Container Features](https://containers.dev/implementors/features/) declared in `.kaiden/workspace.json`. Pass `--with-workspace-config` to enable this; without it the file is not read and no features are installed.

### workspace.json schema

```json
{
  "features": {
    "<feature-ref>": {
      "<option>": "<value>"
    }
  }
}
```

Each key in `features` is a feature reference; each value is a map of options passed to the feature's `install.sh`.

### Feature references

| Format                 | Example                                  | Resolves to              |
| ---------------------- | ---------------------------------------- | ------------------------ |
| OCI registry reference | `ghcr.io/devcontainers/features/rust:1`  | downloaded from registry |
| Local path             | `./my-feature`                           | `.kaiden/my-feature/`    |

Local paths are resolved relative to `.kaiden/`: `./my-feature` points to `.kaiden/my-feature/`.

OCI references without an explicit registry default to `ghcr.io`. Tags and digests (`@sha256:…`) are both supported. Direct `http://` / `https://` tarball URLs are not supported.

### Example

```json
{
  "features": {
    "ghcr.io/devcontainers/features/rust:1": {
      "version": "stable",
      "profile": "minimal"
    },
    "./my-feature": {}
  }
}
```

With the above, `./my-feature` refers to a local feature at `.kaiden/my-feature/`.

### Installation order

Features are installed in the order defined by each feature's `installsAfter` field in its `devcontainer-feature.json`. Within the same dependency level, features are processed in alphabetical order by reference.

## Skills

Agents can be extended with *skills* — named toolkits that an agent discovers at startup. Declare skill directories in `.kaiden/workspace.json` and pass `--with-workspace-config` to include them in the build:

```json
{
  "skills": [
    "./my-skill",
    "./.kaiden/skills/another-skill"
  ]
}
```

Each entry is a path to a directory (relative to the workspace root). The directory name becomes the skill name in the image.

During the build, each skill directory is copied into the agent's skills directory:

| Agent      | Skills directory               |
| ---------- | ------------------------------ |
| `claude`   | `/sandbox/.claude/skills/`     |
| `opencode` | `/sandbox/.opencode/skills/`   |

With `--agent claude` and `"skills": ["./my-skill"]`, the skill lands at `/sandbox/.claude/skills/my-skill/` in the image, owned by the `sandbox` user.

Skills without a corresponding `--agent` flag are silently ignored — the agent determines where skills go, so a build without an agent produces no skill COPY instructions.

### How it works

When `--with-workspace-config` is passed, the tool reads `.kaiden/workspace.json` and:

1. Downloads and extracts each OCI feature into a temporary build context directory (`/tmp/openshell-image-builder…`).
2. Copies local feature directories into the same build context.
3. Passes the build context to `podman build`, where each feature is installed via:
   ```dockerfile
   COPY features/<dir>/ /tmp/feature-install/<dir>/
   RUN chmod +x /tmp/feature-install/<dir>/install.sh && \
       OPTION="value" /tmp/feature-install/<dir>/install.sh
   ```
4. Cleans up all feature files from the image with `RUN rm -rf /tmp/feature-install` after all features are installed.

Features run as root so install scripts can write to system paths.

## Workspace network rules

The OpenShell sandbox enforces a **deny-by-default** network policy: all outbound connections are blocked unless explicitly listed in the policy. There is no supported way to allow all hosts — the sandbox does not implement an allow-all mode. The `network.mode` field in `workspace.json` (which some orchestrators read to switch between `deny` and `allow`) is ignored by the image builder; the policy is always assembled in deny mode with explicit allow-rules.

Use the `network.hosts` field in `.kaiden/workspace.json` to allow additional hosts — for example, package registries or internal APIs that your project's toolchain needs to reach. Pass `--with-workspace-config` to enable this; without it the file is not read and no workspace network rules are added.

```json
{
  "network": {
    "hosts": [
      "index.crates.io",
      "static.crates.io",
      "static.rust-lang.org"
    ]
  }
}
```

Each entry is a hostname, optionally followed by a port (`host:port`). When no port is given, port 443 is used.

The builder merges a single `workspace` network policy rule into `policy.yaml` that covers all listed hosts. The rule authorises the following binaries to connect to those hosts:

| Binary glob | Covers |
|---|---|
| `/bin/**` | Core system utilities |
| `/usr/bin/**` | Standard system binaries (e.g. `curl`) |
| `/usr/local/bin/**` | Locally installed tools |
| `/sandbox/.local/bin/**` | User-local binaries |
| agent binary | The agent binary (e.g. `/sandbox/.local/bin/claude`) when `--agent` is used |

An invalid or unparseable host entry (e.g. a bare space or malformed URL) causes the build to fail immediately with a descriptive error message.

### Example — Rust project with crates.io access

```json
{
  "features": {
    "ghcr.io/devcontainers/features/rust:1": {}
  },
  "network": {
    "hosts": [
      "index.crates.io",
      "static.crates.io",
      "static.rust-lang.org"
    ]
  }
}
```

With this configuration, `cargo build` and `cargo fetch` inside the sandbox can download crate metadata and source tarballs.

## Image-mount init scripts

Use `--image-mount` to bake the initialisation snippet from an [images-to-mount](https://github.com/feloy/images-to-mount) YAML file into the image's shell startup files. The flag can be repeated to process multiple YAML files.

### YAML format

```yaml
image: docker.io/curlimages/curl:latest   # the image to mount — not used by this tool
init: export PATH=$MOUNT/usr/bin:$PATH    # shell snippet added to .bashrc and .zshrc
```

The `image` field is present in the file but is ignored by `openshell-image-builder`. Only the `init` field is read.

`$MOUNT` in the `init` value is replaced at build time with `/sandbox/mnt/<name>`, where `<name>` is the filename stem of the YAML file — for example, `curl.yaml` → `/sandbox/mnt/curl`.

### What gets written

The resolved `init` snippet (with `$MOUNT` replaced) is appended to:

- `/sandbox/.bashrc` — sourced by interactive bash sessions
- `/sandbox/.zshrc` — sourced by interactive zsh sessions (created if it does not already exist)

The append happens in the same `RUN` layer that creates the profile files, so both files are owned by the `sandbox` user.

### Example

```sh
# From a local YAML file — mount name derived from filename: "curl"
openshell-image-builder \
  --runtime podman \
  --image-mount /path/to/curl.yaml \
  myimage:latest

# From a URL
openshell-image-builder \
  --runtime podman \
  --image-mount https://raw.githubusercontent.com/feloy/images-to-mount/main/curl.yaml \
  myimage:latest

# Multiple mounts — the flag can be repeated
openshell-image-builder \
  --runtime podman \
  --image-mount /path/to/curl.yaml \
  --image-mount /path/to/jq.yaml \
  myimage:latest
```

## Full option reference

```
openshell-image-builder [OPTIONS] <TAG>
```

| Argument / Option              | Description                                                        |
| ------------------------------ | ------------------------------------------------------------------ |
| `<TAG>`                        | Tag for the built image (e.g. `myimage:latest`)                    |
| `--runtime <RUNTIME>`          | Container CLI to use for building images (`podman`, `docker`, `container`) |
| `--config <CONFIG>`            | Path to config directory containing `config.toml` (env: `OPENSHELL_IMAGE_BUILDER_CONFIG`) |
| `--agent <AGENT>`              | Agent to install in the image (`claude`, `opencode`)               |
| `--inference <INFERENCE>`      | Inference server the agent will connect to (`anthropic`, `vertexai`, `ollama`, `openai`) |
| `--endpoint <URL>`             | Override the inference provider's default endpoint URL (see [Custom endpoint](#custom-endpoint---endpoint)) |
| `--model <MODEL>`              | Default model for the agent to use (see [Default model](#default-model---model)) |
| `--with-workspace-config`      | Read `.kaiden/workspace.json` and apply its features, skills, and network rules |
| `--with-policy`                | Include OpenShell sandbox policy (`/etc/openshell/policy.yaml`) in the image   |
| `--with-agent-settings`        | Generate and include agent settings in the image (see [Agent settings](#agent-settings)) |
| `--ssl-certs <FILE>`           | Use a specific CA bundle instead of the auto-discovered one (see [Corporate proxy support](#corporate-proxy-support---ssl-certs)). The build fails immediately if the file does not exist. |
| `--disable-ssl-certs`          | Disable bundling CA certificates into the image. By default, the tool auto-discovers and includes system CA certificates. |
| `--image-mount <PATH\|URL>`    | Append the `init` snippet from an images-to-mount YAML file to `.bashrc` and `.zshrc`; may be repeated (see [Image-mount init scripts](#image-mount-init-scripts)) |
| `-v` / `-vv`                   | Increase log verbosity (info / debug)                              |

## Examples

### Claude Code agent + Anthropic models provider

```sh
$ openshell-image-builder \
  --runtime podman \
  --agent claude \
  --inference anthropic \
  --model claude-sonnet-4-6 \
  --with-agent-settings \
  sandbox_image:claude_anthropic

$ openshell provider create \
  --type generic \
  --credential ANTHROPIC_API_KEY=sk-ant-... \
  --name claude_anthropic_provider

$ openshell sandbox create \
  --from sandbox_image:claude_anthropic \
  --provider claude_anthropic_provider \
  --upload . \
  --name claude_anthropic_sandbox \
  --no-auto-providers \
  -- claude

# Or, with podman driver, you can mount the files
# (https://docs.nvidia.com/openshell/reference/sandbox-compute-drivers#podman-driver-config-mounts)
$ openshell sandbox create \
  --from sandbox_image:claude_anthropic \
  --provider claude_anthropic_provider \
  --driver-config-json '{"podman":{"mounts":[{"type":"bind","source":"/path/to/your/sources","target":"/sandbox/work","read_only":false}]}}' \
  --name claude_anthropic_sandbox \
  --no-auto-providers \
  -- claude
```

### OpenCode agent + Anthropic models provider

```sh
$ openshell-image-builder \
  --runtime podman \
  --agent opencode \
  --inference anthropic \
  --model claude-sonnet-4-6 \
  --with-agent-settings \
  sandbox_image:opencode_anthropic

$ openshell provider create \
  --type generic \
  --credential ANTHROPIC_API_KEY=sk-ant-... \
  --name opencode_anthropic_provider

$ openshell sandbox create \
  --from sandbox_image:opencode_anthropic \
  --provider opencode_anthropic_provider \
  --upload . \
  --name opencode_anthropic_sandbox \
  --no-auto-providers \
  -- opencode

# Or, with podman driver, you can mount the files
# (https://docs.nvidia.com/openshell/reference/sandbox-compute-drivers#podman-driver-config-mounts)
$ openshell sandbox create \
  --from sandbox_image:opencode_anthropic \
  --provider opencode_anthropic_provider \
  --driver-config-json '{"podman":{"mounts":[{"type":"bind","source":"/path/to/your/sources","target":"/sandbox/work","read_only":false}]}}' \
  --name opencode_anthropic_sandbox \
  --no-auto-providers \
  -- opencode
```

### Claude Code agent + Vertex AI models provider

```sh
$ openshell-image-builder \
  --runtime podman \
  --agent claude \
  --inference vertexai \
  --model claude-sonnet-4-6 \
  --with-agent-settings \
  sandbox_image:claude_vertexai

$ openshell settings set \
  --global \
  --key providers_v2_enabled \
  --value true \
  --yes

# change value of VERTEX_AI_PROJECT_ID and VERTEX_AI_REGION
$ openshell provider create \
  --name vertex-local \
  --type google-vertex-ai \
  --from-gcloud-adc \
  --config VERTEX_AI_PROJECT_ID=my-gcp-project \
  --config VERTEX_AI_REGION=global

# change with your preferred model
$ openshell inference set \
  --provider vertex-local \
  --model claude-sonnet-4-6

# Change source paths for mounts
$ openshell sandbox create \
  --from sandbox_image:claude_vertexai \
  --provider vertex-local \
  --env ANTHROPIC_BASE_URL="https://inference.local" \
  --env ANTHROPIC_API_KEY=unused \
  --env CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS=1 \
  --driver-config-json '{"podman":{"mounts":[{"type":"bind","source":"/path/to/your/sources","target":"/sandbox/work","read_only":false}]}}' \
  --name claude_vertexai_sandbox \
  --no-auto-providers \
  -- bash -c 'cd /sandbox/work && claude --bare'
```

### OpenCode agent + Ollama (local models)

Ollama must be running on the host before starting the sandbox.

```sh
$ openshell-image-builder \
  --runtime podman \
  --agent opencode \
  --inference ollama \
  --model qwen3-coder:30b \
  --with-agent-settings \
  sandbox_image:opencode_ollama

$ openshell sandbox create \
  --from sandbox_image:opencode_ollama \
  --upload . \
  --name opencode_ollama_sandbox \
  --no-auto-providers \
  -- opencode

# Or, with podman driver, you can mount the files
# (https://docs.nvidia.com/openshell/reference/sandbox-compute-drivers#podman-driver-config-mounts)
$ openshell sandbox create \
  --from sandbox_image:opencode_ollama \
  --driver-config-json '{"podman":{"mounts":[{"type":"bind","source":"/path/to/your/sources","target":"/sandbox/work","read_only":false}]}}' \
  --name opencode_ollama_sandbox \
  --no-auto-providers \
  -- opencode
```

### OpenCode agent + OpenAI models provider

```sh
$ openshell-image-builder \
  --runtime podman \
  --agent opencode \
  --inference openai \
  --model gpt-4o \
  --with-agent-settings \
  sandbox_image:opencode_openai

$ openshell provider create \
  --type generic \
  --credential OPENAI_API_KEY=sk-... \
  --name opencode_openai_provider

$ openshell sandbox create \
  --from sandbox_image:opencode_openai \
  --provider opencode_openai_provider \
  --upload . \
  --name opencode_openai_sandbox \
  --no-auto-providers \
  -- opencode

# Or, with podman driver, you can mount the files
# (https://docs.nvidia.com/openshell/reference/sandbox-compute-drivers#podman-driver-config-mounts)
$ openshell sandbox create \
  --from sandbox_image:opencode_openai \
  --provider opencode_openai_provider \
  --driver-config-json '{"podman":{"mounts":[{"type":"bind","source":"/path/to/your/sources","target":"/sandbox/work","read_only":false}]}}' \
  --name opencode_openai_sandbox \
  --no-auto-providers \
  -- opencode
```

To use an OpenAI-compatible endpoint (e.g. Azure OpenAI, a local proxy, or another provider's API):

```sh
$ openshell-image-builder \
  --runtime podman \
  --agent opencode \
  --inference openai \
  --endpoint https://my-openai-proxy.example.com/v1 \
  --model gpt-4o \
  --with-agent-settings \
  sandbox_image:opencode_openai_custom
```

### OpenCode agent + Vertex AI models provider

```sh
$ openshell-image-builder \
  --runtime podman \
  --agent opencode \
  --inference vertexai \
  --model claude-sonnet-4-6 \
  --with-agent-settings \
  sandbox_image:opencode_vertexai

$ openshell settings set \
  --global \
  --key providers_v2_enabled \
  --value true \
  --yes

# change value of VERTEX_AI_PROJECT_ID and VERTEX_AI_REGION
$ openshell provider create \
  --name vertex-local \
  --type google-vertex-ai \
  --from-gcloud-adc \
  --config VERTEX_AI_PROJECT_ID=my-gcp-project \
  --config VERTEX_AI_REGION=global

# change with your preferred model
$ openshell inference set \
  --provider vertex-local \
  --model claude-sonnet-4-6

# Change source paths for mounts
$ openshell sandbox create \
  --from sandbox_image:opencode_vertexai \
  --provider vertex-local \
  --env ANTHROPIC_BASE_URL="https://inference.local/v1" \
  --env ANTHROPIC_API_KEY=unused \
  --env CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS=1 \
  --driver-config-json '{"podman":{"mounts":[{"type":"bind","source":"/path/to/your/sources","target":"/sandbox/work","read_only":false}]}}' \
  --name opencode_vertexai_sandbox \
  --no-auto-providers \
  -- bash -c 'cd /sandbox/work && opencode'
```
