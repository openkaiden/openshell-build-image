# Plan: backend trait and VM addon selection

## Goal

Give Podman, Docker, Apple container, and the build VM one clear Rust build
interface while preserving their different execution environments. A Rust
`trait` defines the operations each backend provides; each backend struct
implements it. In particular, the VM must inspect and probe its base image
inside the guest, where Buildah downloads and builds from that image. The host
must not pull the VM base merely to select an addon.

Keep the current `--addon` contract: zero or more image references in argument
order; select one manifest per addon by the base platform and, only when the
index offers multiple libc choices, by the base libc. Transfer the selected
files and OCI `ENV` values in the same order. A base image is pulled at most
once into the image store used for the final build.

## Trait boundary

Put the common interface around the **complete build**, not around host-side
`prepare_base()` or `resolve_addons()`. Those steps occur in different places
for host runtimes and the VM.

```rust
struct BuildRequest<'a> {
    from: &'a str,
    addons: &'a [String],
    tag: &'a str,
    copy_containerfile: bool,
}

enum BuildResult {
    Image { tag: String },
    VmArtifact { rootfs_tar: PathBuf, config: Option<PathBuf> },
}

trait ImageBackend {
    fn check_ready(&self) -> Result<(), BuildError>;
    fn build(&self, request: &BuildRequest<'_>) -> Result<BuildResult, BuildError>;
}
```

The types above show the intended boundary, not final signatures. The backend
owns runtime-specific configuration such as the VM rootfs, DNS, resources, and
output path. The request carries ordered build inputs. The result identifies
what the backend actually produced, so `main.rs` no longer needs to infer that
from the selected runtime.

Implement the trait for `PodmanBackend`, `DockerBackend`,
`AppleContainerBackend`, and `VmBackend`. Construct the selected implementation
once after CLI validation, then call `check_ready()` and `build()`. Keep the
existing `Runner` and `VmRunner` injection points for tests. The three host
implementations should delegate their common sequence to a shared host
pipeline; their adapters own differences in manifest inspection, image
inspection, probing, pull policy, and final build invocation. Keep Docker's
temporary base tag and its cleanup within the Docker implementation.

## Implementation sequence

1. **Introduce the interface without changing behavior.** Move the branching
   in `src/main.rs::run()` behind the four backend implementations. Host builds
   continue to use `src/host_addon.rs`,
   `crates/container-image-builder/src/base.rs`, and
   `crates/container-image-builder/src/lib.rs`. VM builds continue to use
   `crates/vm-image-builder` and reject nonempty addon lists. Preserve the
   existing no-addon results: a tagged image for host runtimes and a flattened
   rootfs tarball for VM.

2. **Separate reusable addon rules from runtime operations.** Move the pure
   catalog parser, variant selection, image-reference validation, environment
   validation, and Containerfile generation into a small workspace crate
   usable by both macOS host code and a Linux guest helper. Re-export current
   public APIs temporarily if needed. Keep registry requests, container CLI
   commands, and Buildah commands out of this crate. Share libc marker rules
   between host tar inspection and guest mounted-filesystem inspection.
   Giving the guest helper a dependency on the current top-level package
   would create a build cycle: that package embeds the VM rootfs containing
   the helper.

3. **Define a versioned VM build request.** Write a JSON request into the
   shared build context with the base reference, ordered addon references,
   tag, and `copy_containerfile` choice. Extend `VmBuild` and the `vm-build`
   invocation to pass the request path. Avoid passing user-controlled image
   references through a shell command string. Keep the existing VM no-addon
   route working while the guest path is introduced.

4. **Resolve addons inside the VM.** Build a small Linux/arm64 guest helper
   against the shared crate and install it in the VM rootfs. After `vm-build`
   mounts the context and initializes Buildah's temporary image store, the
   helper performs this sequence in that one guest process:

   - Read each addon catalog as raw OCI metadata without pulling its layers.
     Use a guest OCI reader that preserves descriptor annotations, including
     the canonical `ai.openkaiden.openshell-build-image.libc` key. Handle both
     single manifests and indexes, and cache repeated references.
   - Determine whether any catalog needs the base libc. Pull or reuse the base
     once in Buildah's store and inspect the platform of the image Buildah
     actually selected.
   - If libc is needed, create a temporary Buildah working container from the
     already local base, mount its rootfs, and inspect libc loader/library
     markers there. Do this at most once per build; never execute a program
     from the base image. Always unmount and remove the temporary container.
   - Select exactly one descriptor for each addon using the shared rules.
     Pull only selected manifest digests into the same Buildah store and read
     their image configurations for `ENV` values.
   - Generate the resolved Containerfile and, when requested, its staged
     `build-containerfile` inside the shared context. Run
     `buildah bud --pull=never` with the prepared base and selected addon
     images. This makes an accidental second download a visible failure.

   Update `make-rootfs.sh` and the VM rootfs `Containerfile` to build and
   install the guest helper reproducibly. The guest helper must depend on the
   small shared crate, not on the host binary or its embedded rootfs.

5. **Complete the VM result contract before enabling VM addons.** The current
   VM export is a flattened rootfs tarball; it contains copied files but loses
   OCI `ENV` metadata. Read the completed Buildah image's config and produce a
   versioned sidecar containing its final environment, including base-image
   values, alongside the tarball. Update the consuming OpenShell
   VM driver to apply it. Keep the current rootfs tar behavior for builds with
   no addons. If the consumer cannot yet read that metadata, retain the
   explicit VM `--addon` rejection rather than reporting partial support.
   Stage outputs at temporary paths and publish the tar only after its
   sidecar is complete, so a failed addon does not leave a usable-looking
   output.

6. **Update documentation.** Describe the four implementations, where each
   backend obtains image metadata, where libc is detected, the VM artifact
   format, and any VM driver requirement in `README.md`, `AGENTS.md`, and the
   addon/VM maintenance skills.

## Verification

- Unit-test dispatch and result reporting for all four implementations, with
  and without addons. Confirm argument order and repeated references survive
  the trait boundary. Preserve existing host command-capture tests, including
  Docker's raw-index fallback and temporary tag cleanup.
- Test the shared catalog rules for single images, unannotated platform
  indexes, single-libc indexes, multi-libc indexes, partial annotations,
  duplicate matches, unsupported platforms, and invalid image configs.
- Test the guest helper with fake Buildah/OCI commands and a temporary
  filesystem. Assert one base pull or reuse, zero probes for single and
  platform-only fixtures, one probe for multiple libc choices even with
  repeated addons, selected digest pulls only, order of `COPY`/`ENV`, and
  cleanup on failure. Assert no host container command is used by VM builds.
- Extend VM E2E on the existing self-hosted Apple Silicon runner with the
  published `single`, `platforms`, and `platforms-libc` fixtures on Ubuntu
  (glibc) and Alpine (musl). Check tar markers, selected digests in the
  generated Containerfile, the sidecar and actual sandbox environment, addon
  ordering, repeated values, unsupported platforms, undetectable libc, and
  absence of a partial artifact after failure. Keep Podman, Docker, and Apple
  container E2E coverage as regression checks.
- Run `cargo fmt --check && cargo clippy -- -D warnings && cargo test`, then
  the workspace checks with `--features vm`. Rebuild the VM rootfs, sign the
  VM-enabled binary, and run a real VM build before enabling VM addons.

## Review boundaries

1. Trait refactor with unchanged runtime behavior.
2. Shared pure rules and versioned VM request/guest helper packaging.
3. VM addon resolution and Buildah build, still gated until metadata reaches
   the consumer.
4. VM artifact metadata support, driver integration, documentation, and E2E;
   then enable VM `--addon`.

## Implementation progress (2026-10-16)

- Review boundary 1 is implemented: `src/backend.rs` defines the complete-build
  trait and four backend structs. The CLI constructs one backend and reports its
  `BuildResult`; Docker owns its temporary tag and cleanup. VM addon rejection
  and the existing no-addon outputs remain in place.
- The pure rules in step 2 now live in `crates/addon-rules`, with the old public
  module paths re-exported from `src/lib.rs`. Host tar inspection uses its shared
  libc marker rules.
- Step 3 now stages `build-request.json` with version `1`, the base reference,
  ordered addons, tag, and `copy_containerfile` choice. The path is passed as
  a separate `vm-build` argument. The guest still uses the no-addon Containerfile
  route, and VM addons remain gated.
- Steps 4 and 5 remain: package a guest helper, resolve and build addons in
  Buildah's store, produce the environment sidecar, and integrate its consumer.
  The live VM and addon E2E checks in the verification section remain to run
  after those changes.
