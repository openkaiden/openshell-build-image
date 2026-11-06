# openshell-build-image

Rust CLI using clap (derive API) for argument parsing.

## Versioning

The version in `Cargo.toml` follows `x.y.z-next` on the `main` branch (e.g. `0.1.0-next`). Never remove the `-next` suffix manually — CI handles that on release.

## Before committing

Run `/copyright-headers` to ensure all source files have Apache 2.0 license headers.

Always run the full check suite locally before staging:

```bash
cargo fmt --check && cargo clippy -- -D warnings && cargo test
```

## Rust gotchas

- Imports only used in tests must live inside the `#[cfg(test)]` module, not at the top level — otherwise clippy's `-D warnings` flags them as unused.
- `dtolnay/rust-toolchain` requires an explicit `toolchain: stable` input, it has no default.

## OCI addons

`--addon <IMAGE>` is repeatable and ordered. All runtimes select an addon manifest for the prepared base image's OS and architecture, and use its libc only when the addon index has multiple libc choices. Keep base inspection, optional libc detection, addon pulls, and the final build in the same runtime's image store. The VM resolver runs inside the guest where Buildah uses the base image.

Use [`maintain-addons`](.agents/skills/maintain-addons/SKILL.md) for the selection rules, file map, backend differences, and test coverage. Host addon fixtures run in the E2E workflow; the VM runtime workflow checks guest selection on musl and glibc bases. The OpenShell VM E2E job uses an addon-built rootfs to check sandbox startup and policy enforcement.

`src/backend.rs` defines the complete-build trait and the four runtime implementations. Pure catalog, Containerfile, and libc marker rules live in `crates/addon-rules`; `crates/vm-guest-helper` uses them inside the VM. VM builds stage a versioned request in the shared context. With addons, the VM writes a flattened rootfs tarball and an adjacent `<tarball>.config.json` sidecar that carries OCI environment metadata.

## GitHub Actions

Pin actions by commit SHA with a version comment, using the actual latest release — not just the latest major tag:

```yaml
uses: actions/checkout@<sha> # v6.0.2
```

Before writing a `uses:` line, fetch the latest release and its SHA:

```bash
gh api repos/<owner>/<repo>/releases/latest --jq '.tag_name'
gh api repos/<owner>/<repo>/git/ref/tags/<tag> --jq '.object.sha'
```

## User context

User is new to Rust — explain Rust-specific concepts (macros, import scoping, cargo commands) from first principles.
