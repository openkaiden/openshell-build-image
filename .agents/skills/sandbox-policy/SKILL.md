---
name: sandbox-policy
description: Understand and edit the OpenShell sandbox policy — base YAML, network rule schema, how inference and agent fragments are merged, and how to test changes
---

# Sandbox Policy

How the sandbox policy is assembled, what each section means, and how to add or modify rules.

## Description

Every built image contains `/etc/openshell/policy.yaml`. The openshell runtime reads it at startup to configure filesystem access, optional process settings, and which network endpoints each binary is allowed to reach.

The final policy is assembled by `build_policy()` in `src/main.rs` by merging four sources:

1. **Base policy** — `assets/policy.yaml`, committed to the repo. Contains filesystem config and baseline network rules shared by all images (git, gh CLI).
2. **Inference fragment** — returned by `Inference::policy_yaml(agent_binary, base_url)`. Adds network rules for the LLM backend. Only included when both `--agent` and `--inference` are given (the method needs the agent binary path to scope the rule).
3. **Agent fragment** — returned by `Agent::policy_yaml()`. Adds agent-specific network rules (e.g., Claude needs `platform.claude.com` for telemetry). Only included if the string is non-empty.
4. **Workspace fragment** — constructed from `workspace.network.hosts` in `.kaiden/workspace.json`. Adds a single `workspace` rule whose endpoints are the user-declared hosts and whose binaries are the four PATH globs (`/bin/**`, `/usr/bin/**`, `/usr/local/bin/**`, `/sandbox/.local/bin/**`) plus the agent binary when present. Only included when `network.hosts` is non-empty. An invalid host entry causes the build to fail immediately.

## YAML schema

The full schema is modelled in `src/policy/mod.rs` (all structs use `#[serde(deny_unknown_fields)]` — unknown keys cause a parse error).

### Top-level sections

```yaml
version: 1

filesystem_policy:          # what paths the agent can read/write
  include_workdir: true
  read_only:  [/usr, /lib, /proc, /dev/urandom, /app, /etc, /var/log]
  read_write: [/sandbox, /tmp, /dev/null]

landlock:
  compatibility: best_effort

network_policies:           # keyed map — key is the merge slug
  <slug>:
    name: <display-name>
    endpoints: [...]
    binaries:  [...]
```

Only `version` is required. All other sections, including `network_policies`, are optional; an omitted `network_policies` defaults to an empty map.

### Network endpoint fields

```yaml
endpoints:
  - host: api.example.com     # hostname; wildcards accepted ("*-aiplatform.googleapis.com")
    port: 443                 # single port; use `ports` for a list
    ports: [80, 443]          # alternative to `port` — covers all listed ports
    protocol: rest            # rest | grpc | ssh | (omit for any)
    tls: terminate            # terminate | passthrough | (omit)
    enforcement: enforce      # enforce | audit | (omit)
    access: full              # full | read-only | (omit)
    rules:                    # optional L7 allow-list (REST only)
      - allow:
          method: GET
          path: "/**/info/refs*"
      - allow:
          method: POST
          path: "/**/git-upload-pack"
```

Omitting a field means "any value is allowed for that dimension". A rule with only `host` and `port` allows all methods and paths.

### Binary scoping

```yaml
binaries:
  - { path: /sandbox/.local/bin/claude }
```

The rule only applies when the connecting process's binary matches one of these paths. The runtime resolves the path via `/proc/{pid}/exe` and walks the ancestor tree.

## How fragments are merged

`build_policy()` in `src/main.rs`:

```rust
let mut sandbox_policy = policy::parse_sandbox_policy(base_yaml)?;

if let (Some(inference), Some(agent)) = (inference, agent) {
    let fragment = policy::parse_sandbox_policy(&inference.policy_yaml(agent.binary_path(), base_url))?;
    sandbox_policy.network_policies.extend(fragment.network_policies);
}

if let Some(agent) = agent {
    let fragment_yaml = agent.policy_yaml();
    if !fragment_yaml.is_empty() {
        let fragment = policy::parse_sandbox_policy(fragment_yaml)?;
        sandbox_policy.network_policies.extend(fragment.network_policies);
    }
}

if let Some(hosts) = workspace
    .and_then(|ws| ws.network.as_ref())
    .map(|net| net.hosts.as_slice())
    .filter(|h| !h.is_empty())
{
    let agent_binary = agent.map(|a| a.binary_path());
    sandbox_policy.network_policies.insert(
        "workspace".to_string(),
        workspace_hosts_policy(hosts, agent_binary)?,
    );
}
```

`network_policies` is a `BTreeMap<String, NetworkPolicyRule>`. The BTreeMap key is the merge slug (e.g. `"anthropic"`, `"claude_code"`). `extend()` means: if a fragment uses a slug that already exists in the base, the base entry is silently replaced. The `name` field inside the rule is separate — it is what the runtime displays, and should match the slug by convention.

**Merge order**: base → inference → agent → workspace. A workspace rule uses `insert()`, so it can override any earlier rule with the slug `"workspace"` (none exists in base or the other fragments).

## Existing network rules

| Slug | Source | Allowed endpoints | Scoped to |
|---|---|---|---|
| `github_ssh_over_https` | base | `github.com:443` (git smart HTTP read-only) | `/usr/bin/git` |
| `github_rest_api` | base | `api.github.com:443` (read-only) | `/usr/bin/gh` |
| `anthropic` | AnthropicInference | `api.anthropic.com:443`, `statsig.anthropic.com:443` (or custom endpoint) | agent binary |
| `vertexai` | VertexAiInference | `oauth2.googleapis.com:443`, `aiplatform.googleapis.com:443`, `*-aiplatform.googleapis.com:443` | agent binary |
| `ollama` | OllamaInference | `host.openshell.internal:11434` (or custom host:port) | agent binary |
| `claude_code` | ClaudeAgent | `raw.githubusercontent.com:443`, `platform.claude.com:443`, `api.github.com:443` (read-only) | `/sandbox/.local/bin/claude` |
| `workspace` | `workspace.network.hosts` | all user-declared hosts (port defaults to 443) | `/bin/**`, `/usr/bin/**`, `/usr/local/bin/**`, `/sandbox/.local/bin/**`, agent binary |

## Where to make changes

### Modify the base policy (`assets/policy.yaml`)

Add, remove, or edit entries under `network_policies`. The slug (YAML key) must be unique across the whole merged document. Do not add unknown YAML fields — the parser rejects them.

Example: allow `curl` to reach an internal registry:

```yaml
network_policies:
  internal_registry:
    name: internal-registry
    endpoints:
      - { host: registry.internal.example.com, port: 443 }
    binaries:
      - { path: /usr/bin/curl }
```

Unit test in `src/policy/mod.rs` — add a `round_trip_*` test to confirm the new entry survives parse → serialize → re-parse.

### Modify an inference policy rule (`src/inference/<provider>.rs`)

Edit the `policy_yaml()` method. The fragment must be a valid policy document (starts with `version: 1`, contains `network_policies:`). The slug must not collide with base policy slugs.

If the provider supports `--endpoint`, the method receives `base_url: Some(url)` and should parse it with `super::parse_host_port` instead of using the hardcoded default:

```rust
fn policy_yaml(&self, agent_binary: &str, base_url: Option<&str>) -> String {
    let (host, port) = base_url
        .and_then(super::parse_host_port)
        .unwrap_or_else(|| ("api.example.com".to_string(), 443));
    format!(r#"version: 1
network_policies:
  myprovider:
    name: myprovider
    endpoints:
      - {{ host: {host}, port: {port}, protocol: rest, enforcement: enforce, access: full, tls: terminate }}
    binaries:
      - {{ path: {agent_binary} }}
"#)
}
```

Remember: this fragment is only included when both agent and inference are selected.

### Add or modify an agent policy rule (`src/agent/<agent>.rs`)

Override `policy_yaml()` on the `Agent` impl. Return an empty string if no agent-level rules are needed (the default). The method returns `&str` (a string literal), not `String`:

```rust
fn policy_yaml(&self) -> &str {
    r#"version: 1
network_policies:
  myagent_telemetry:
    name: myagent-telemetry
    endpoints:
      - { host: telemetry.myagent.example.com, port: 443 }
    binaries:
      - { path: /sandbox/.myagent/bin/myagent }
"#
}
```

Unlike the inference rule, the binary path is hardcoded here (it is the agent's own binary, which does not change per-inference).

## Testing policy changes

### Unit tests

**`src/policy/mod.rs`** — test that a new entry round-trips correctly:

```rust
#[test]
fn new_rule_round_trips() {
    let yaml = "version: 1\nnetwork_policies:\n  myrule:\n    name: my-rule\n    endpoints:\n      - { host: example.com, port: 443 }\n    binaries:\n      - { path: /usr/bin/curl }\n";
    let p = parse_sandbox_policy(yaml).expect("parse");
    let out = serialize_sandbox_policy(&p).expect("serialize");
    let p2 = parse_sandbox_policy(&out).expect("re-parse");
    assert_eq!(p2.network_policies["myrule"].name, "my-rule");
}
```

**`src/main.rs`** — test that `build_policy()` produces the expected output:

```rust
#[test]
fn build_policy_with_myprovider_inference_includes_expected_host() {
    let yaml = build_policy(
        BASE_POLICY_YAML,
        Some(&agent::ClaudeAgent),
        Some(&inference::MyProviderInference),
        None,
        None,
    ).unwrap();
    assert!(yaml.contains("api.myprovider.com"));
}
```

**`src/agent/<agent>.rs` or `src/inference/<provider>.rs`** — test the raw YAML string returned by `policy_yaml()`:

```rust
#[test]
fn policy_yaml_has_myrule_name() {
    assert!(MyAgent.policy_yaml().contains("name: myagent-telemetry"));
}

#[test]
fn policy_yaml_has_expected_host() {
    assert!(MyAgent.policy_yaml().contains("telemetry.myagent.example.com"));
}
```

### Integration test

Inspect the policy in the built image:

```sh
podman run --rm openshell-test-ubuntu-claude:integration -c "cat /etc/openshell/policy.yaml"
```

Or use the integration test check helpers — they read `/etc/openshell/policy.yaml` and assert on specific `name:` values:

```sh
# run just the policy tests for a specific image combination
cargo test --test integration_test ubuntu_claude::policy -- --include-ignored --test-threads=1
```

## Common mistakes

- **Unknown YAML field** — adding a field not in `src/policy/mod.rs` causes `parse_sandbox_policy` to return an error. The build will fail with a deserialisation error. Check the struct definitions in `src/policy/mod.rs` before adding new fields.
- **Duplicate slug** — if your fragment uses the same BTreeMap key as an existing rule (including base policy slugs), it silently replaces it. Use a unique slug, or intentionally replace a base rule.
- **Inference fragment omitted** — if a user runs with `--inference` but no `--agent`, the inference network rule is not added (the merge requires both). This is by design: the rule must name a binary, and there is no agent binary without an agent.
- **Empty agent policy** — returning an empty string from `Agent::policy_yaml()` is valid and means no agent-level rule. Returning a malformed YAML string causes the build to fail with a parse error.
