---
name: maintain-addons
description: Modify OCI addon selection and builds in host and VM runtimes, including platform and libc matching and backend tests
---

# Maintain OCI addons

Use this guide when changing `--addon`, OCI manifest selection, libc probing, or addon builds.

## Description

The CLI accepts zero or more `--addon <IMAGE>` flags. Each addon is applied in argument order after the project base image. A later addon may replace an earlier file or `ENV` value. Builds without addons follow the existing path. The VM selects addons inside the guest's Buildah store and writes an OCI config sidecar beside its flattened rootfs tarball, preserving environment metadata that a tarball cannot carry.

## Instructions

### Step 1 — follow the data flow

- `src/main.rs`: clap stores addon references in `Cli.addon: Vec<String>` and passes the complete request to the selected backend. `src/backend.rs` implements the four complete-build backends and the shared host pipeline. Preserve the full list, including duplicates and order.
- `crates/addon-rules/src/addon.rs`: `AddonCatalog` parses OCI indexes and single manifests. `select()` matches the base platform and, when needed, libc. The descriptor annotation key is `ai.openkaiden.openshell-build-image.libc`.
- `crates/addon-rules/src/containerfile.rs`: `ResolvedAddon` pins each selected manifest digest, reads `config.Env`, and emits its `COPY --from=<repository>@<digest> / /` and `ENV` instructions in order. Validate references and escape environment values before writing instructions.
- `crates/addon-rules/src/libc.rs`: shared marker rules for host tar inspection and guest mounted-filesystem inspection.
- `crates/vm-guest-helper/src/main.rs`: guest resolution using Skopeo for OCI metadata and Buildah for local images. The versioned request comes from `vm-image-builder`; `vm-build` runs the helper and exports the config sidecar.

### Step 2 — preserve selection rules

- A single image requires a matching OS and architecture; it does not require a libc probe.
- An index with multiple platforms and one libc value, or no libc annotations, selects by platform without a libc probe.
- If an index has multiple libc values, determine the base libc once and select the descriptor matching OS, architecture, and libc. Reject no match, duplicate match, and partial libc annotation rather than guessing.
- Ignore non-image and attestation descriptors. Use the selected descriptor's digest rather than the index tag, since glibc and musl descriptors may share a platform.

### Step 3 — keep host operations in one image store

- `crates/container-image-builder/src/base.rs::prepare_base()` reuses or pulls the base in the selected runtime, inspects its actual platform, and probes libc only when requested. The probe exports a temporary stopped container's filesystem; it must not require a shell inside the base. Remove the container and temporary files on success and failure.
- Apple `container export` needs a materialized rootfs. Its probe creates a container with `/dev/null` as the entrypoint, expects that process start to fail, then exports the stopped container. Do not run a binary from the base image to determine libc.
- `src/host_addon.rs::resolve()` loads catalogs, requests at most one base libc probe, selects variants, ensures selected payloads are local, and reads their image configuration. Repeated references may reuse cached metadata, but still produce repeated `COPY` and `ENV` instructions. `src/backend.rs` keeps Docker's temporary base tag and cleanup in its Docker implementation.
- Podman and Docker inspect indexes with their own `manifest inspect` commands. Docker may omit OCI descriptor annotations from this output; when an unannotated index has duplicate platforms, `src/host_addon.rs` reads the raw index with `docker buildx imagetools inspect --raw` before libc selection. A single local image can require a fallback to `image inspect`; Docker's `RepoDigests` must match the requested repository. Pull only selected manifest digests.
- Docker addon builds temporarily tag the inspected base image ID because BuildKit cannot use a bare local ID in `FROM`. Remove the tag after the build, including on build failure. For Docker, HTTP-only IPv6 loopback addon registries are rejected during manifest inspection; use HTTPS for `[::1]`.
- Apple's `container image inspect` does not supply the raw index descriptor annotations needed for libc selection. `src/registry.rs` retrieves OCI metadata from the registry and uses Apple's configured registry credentials when challenged. Keep registry responses bounded and verify their digests. Apple's prepared base keeps its original local reference because adding `@digest` to a tag may trigger another pull.
- `crates/container-image-builder/src/lib.rs::build_with_local_images_on_platform()` builds from prepared images. Podman and Docker disable pulls and receive the inspected base platform, including for a non-native base. Apple's build uses its local store without a `--pull` flag.
- The VM guest loads catalogs before pulling its base, probes a temporary mounted Buildah container only when multiple libc values require it, pulls selected addon digests, and builds with `--pull=never`. Keep Skopeo's `containers-storage:` lookups and Buildah on the guest's VFS store. Buildah prefixes locally tagged results with `localhost/`; the config sidecar lookup must account for this.

### Step 4 — update the right tests

- Add selection cases to `crates/addon-rules/src/addon.rs` tests, including single image, one libc across platforms, multiple libc values, unannotated indexes, and ambiguous or missing matches.
- Add Containerfile ordering, digest, and `ENV` escaping cases to `crates/addon-rules/src/containerfile.rs` tests.
- Add host command capture cases to `src/host_addon.rs` and base probe cases to `crates/container-image-builder/src/base.rs`. Keep Docker and Apple tests distinct from Podman tests. Test registry HTTP behavior in `src/registry.rs`.
- Add guest selection, order, probe, and error cases to `crates/vm-guest-helper/src/main.rs`. Build the guest rootfs with `make build-vm-rootfs` and use the VM runtime workflow to test live selection and sidecar metadata.
- Run `cargo fmt --check && cargo clippy -- -D warnings && cargo test` and `cargo test -p container-image-builder`. These unit tests use fake runners; they do not prove that Docker or Apple's `container` can complete a live build.
- For live addon E2E, build local fixtures in three OCI shapes: `single`, `platforms` (amd64 and arm64, one libc), and `platforms-libc` (both platforms, glibc and musl). Give every variant a distinct file marker and environment value. Cover no addon, each shape on musl and glibc bases, multiple addon orders, repeated references, a non-native base, unsupported platform, and undetectable libc. Assert the selected marker and digest, absence of other variants, environment order, and probe count. Add Docker and Apple jobs only where those runtimes are available, and label their results separately from mocked tests.

## Checklist

- [ ] The `--addon` list retains duplicates and CLI order; no-addon builds still work.
- [ ] Platform and libc selection uses the base image selected by the build runtime.
- [ ] The base is not downloaded a second time, and only selected addon payloads are pulled.
- [ ] Generated `COPY` and `ENV` instructions use selected digests and preserve order.
- [ ] Unit tests and available live backend tests cover the changed behavior.
- [ ] README and E2E documentation reflect supported runtimes and known gaps.
