# openshell-build-image

[OpenShell](https://github.com/NVIDIA/OpenShell-Community) is NVIDIA's runtime environment for autonomous AI agents. It provides isolated sandboxes where agents can safely run and iterate — without risk to the host system or your credentials.

OpenShell ships a set of [pre-built sandbox images](https://github.com/NVIDIA/OpenShell-Community), but they are general-purpose. `openshell-build-image` builds from an image prepared for your project without requiring you to write a Containerfile by hand.

The tool builds on a project image that already has the tools your project needs. Use `--runtime` to select what drives the build: a container CLI on the host (`podman`, `docker`, or the macOS `container` CLI), or a microVM (`vm`) that needs no container runtime installed at all — see [Building in a VM](#building-in-a-vm---runtime-vm).

Built-in agent installation and configuration have been removed. OCI addons can be layered onto a project image with repeatable `--addon` flags.

## Quick start

Build an image with a single command:

```sh
openshell-build-image --runtime podman --from myproject:dev myimage:latest
```

`--from`, `--runtime`, and `<TAG>` are required. `--from` selects the project image to extend, `--runtime` selects the build backend (`podman`, `docker`, `container`, or `vm`), and `<TAG>` names the result. There is no default base image.

## Building in a VM (`--runtime vm`)

The three CLI runtimes hand the build to a container engine installed on your machine. `--runtime vm` instead boots a lightweight Linux microVM with [libkrun](https://github.com/containers/libkrun) and runs `buildah` inside it, so no container engine is needed on the host.

The VM runs its own kernel in its own process namespace and sees only three directories you share with it: the VM's root filesystem, the build context, and the output directory. Nothing else on the host is reachable from the build.

### What it produces

This is the one way `--runtime vm` differs from the others in its result. A container CLI leaves a tagged image in its local image store. The VM has no access to that store, so it writes a **flattened rootfs tarball** instead:

```sh
openshell-build-image --runtime vm --from ghcr.io/example/myproject:dev myimage:latest
# -> ./myimage-latest.tar
```

The default filename comes from `<TAG>`, with `:` and `/` replaced by `-`. Override it with `--vm-output`. Load the result into an image store yourself, or unpack it:

```sh
tar -xf myimage-latest.tar -C /path/to/rootfs
```

With `--addon`, the VM selects each addon inside the guest using the base image's platform and, when an addon index offers multiple libc variants, its libc. Addons are applied in flag order. The VM also writes `<tarball>.config.json` beside the tarball, containing the built image's OCI configuration, including addon `config.Env` values. Keep that file when importing the rootfs into an image store: a flattened tarball contains files only and cannot retain image environment settings by itself.

```sh
openshell-build-image --runtime vm --from myproject:dev \
  --addon ghcr.io/example/tools:latest --addon ghcr.io/example/config:latest \
  --vm-output myimage.tar myimage:latest
# -> myimage.tar and myimage.tar.config.json
```

A custom `--vm-rootfs` built before addon support needs to be rebuilt to include the guest resolver and Skopeo.

### Requirements

`--runtime vm` works only on **macOS with Apple Silicon**, because libkrun builds on Apple's Hypervisor.framework. It also needs:

1. **libkrun**, which the binary links against rather than bundling:

   ```sh
   brew tap libkrun/krun && brew trust libkrun/krun
   brew install libkrun/krun/libkrun
   ```

   If a build fails with `Couldn't find or load libkrunfw`, add `export DYLD_LIBRARY_PATH="$(brew --prefix)/lib"`.

2. **A binary built with the `vm` feature and signed for the hypervisor.** The `aarch64-apple-darwin` download from a release has both. To build one yourself:

   ```sh
   make build-vm
   ```

   The feature is off by default because it links against libkrun, and macOS refuses the hypervisor to an unsigned binary. `make build-vm` does both, and re-signs on every build, since compiling clears the signature.

The VM's own root filesystem — a Linux tree with `buildah` in it — comes inside the binary and unpacks itself on first use, into `~/Library/Application Support/openshell-build-image/vm-rootfs`.

### Building the VM's root filesystem

Only needed to change what the build VM contains. It takes Podman on a `linux/arm64` machine:

```sh
make build-vm-rootfs                  # writes vm-rootfs/ and vm-rootfs.tar
make build-vm                         # embeds the tarball in the binary
make run-vm FROM=ghcr.io/example/myproject:dev TAG=myimage:latest
```

Or point at the directory without embedding anything: `--vm-rootfs ./vm-rootfs`. `make help` lists the rest.

### Sizing the VM

`buildah` uses the `vfs` storage driver inside the VM — virtio-fs does not support overlayfs — which stores a full copy of every layer rather than a diff. Builds therefore need more memory than the same build under overlayfs. If one fails with an out-of-memory message, raise it:

```sh
openshell-build-image \
  --runtime vm \
  --from ghcr.io/example/myproject:dev \
  --vm-cpus 4 \
  --vm-memory 8192 \
  myimage:latest
```

### DNS inside the VM

The VM has no virtual network card: libkrun forwards its connections to the host, which makes them for real. Nothing supplies the guest a `resolv.conf`, and a lookup still travels to whatever nameserver the guest is told to use — the host's resolver settings do not apply to it, only the host's routing and firewall rules.

So the build reads the host's own nameservers on each run and hands them to the VM. That is what makes a build work behind a firewall that only allows DNS to the company resolver, and what lets a `FROM` line reach a registry mirror that only internal DNS knows about.

Override it when the host's resolvers are not the ones the build should use:

```sh
openshell-build-image --runtime vm --from ghcr.io/example/myproject:dev --vm-dns 10.0.0.53 --vm-dns 10.0.0.54 myimage:latest
```

A loopback address is rejected: inside the VM, loopback is the VM. If the host resolves through one — systemd-resolved, a VPN client's local stub — pass the address it forwards to instead. When the host has no usable nameserver at all, the VM falls back to `1.1.1.1`.

## Choosing the project image

Pass `--from <IMAGE>` to extend an image prepared for your project. Image names, tags, registry ports, and digests are passed directly to the build engine; there is no list of supported distributions.

```sh
openshell-build-image --runtime podman --from localhost:5000/myproject:dev myimage:latest
openshell-build-image --runtime podman --from registry.fedoraproject.org/fedora:latest fedora-project:latest
openshell-build-image --runtime podman --from docker.io/library/alpine:3.24 alpine-project:latest
```

For a pinned image, use `--from registry.example.com/myproject@sha256:<digest>` with its full digest. The VM backend pulls the image into its own Buildah store, so it needs a registry reference accessible from the VM rather than an image present only in the host's store.

Base-image configuration files, `--config`, `OPENSHELL_BUILD_IMAGE_CONFIG`, and `.kaiden/workspace.json` are not used by the image builder. Supply the required tools in the image passed to `--from`.

## Applying OCI addons

Each `--addon` copies the selected addon's files and applies its OCI `config.Env`
values in command-line order. For environment variables other than `PATH`, a
later value replaces an earlier one. `PATH` is special: an addon value such as
`PATH=/usr/local/bin` is appended to the current image value, producing
`ENV PATH="$PATH:/usr/local/bin"` in the build Containerfile. Multiple addon
`PATH` values append in addon order, preserving the base image's existing paths.

## End-to-end tests with OpenShell

The `OpenShell E2E` workflow builds an image with this tool, starts an isolated
OpenShell **v0.1.4** gateway, and creates a sandbox from the result. It covers
Podman and Docker on Linux x86_64 and the VM driver on a self-hosted Apple
Silicon Mac. Each Linux job builds and launches the image through the same
container engine.

Each driver job uses an image matrix covering the digest-pinned Ubuntu-based
`buildpack-deps:noble-curl` and Alpine-based `alpine/curl:8.22.0` images.
Both include curl, a shell, and CA certificates.
Each matrix job installs OpenShell and builds the image builder. Each image
gets a fresh gateway, sandbox and policy state, with separate diagnostics under
`target/e2e/<driver>/{ubuntu,alpine}`. The test checks
the Containerfile copied into the produced image without starting a container.
It creates a sandbox with an explicit non-root UID/GID of 10001 and checks the
IDs and group memberships, then requires a failed curl to `https://example.com/` and a matching
OpenShell policy-denial log. It approves
that destination for curl, waits for the policy to load, and requires the same
request to succeed. DNS, TLS, and timeout failures alone cannot pass the test.

Setup and lifecycle commands live in named shell steps in
[the E2E workflow](.github/workflows/e2e.yml) and its shared composite actions: install and verify OpenShell,
build and inspect the image, generate certificates and keys, configure and start
the gateway, register its client certificates, and create the sandbox. The macOS
and Linux jobs select their own dependencies, release assets, builds, driver
settings, and cleanup. They share granular actions for installation, gateway
startup, sandbox creation, and common diagnostics and shutdown.
The VM job keeps the `integration` approval environment on its self-hosted runner.
E2E and VM Runtime also share [the rootfs build workflow](.github/workflows/build-vm-rootfs.yml).
Diagnostics and cleanup run after each image even when an assertion fails.
The matrix uses `fail-fast: false`, so a failure on one image does not cancel the
other. VM variants run one at a time to limit resource use on the self-hosted Mac;
matrix scheduling does not guarantee an image order. The result checks
`Podman on Linux`, `Docker on Linux`, and `VM on macOS` each require both image
variants to pass.
See [calling these workflows from another repository](.github/workflows/README.md)
for inputs and a complete caller example.

The Rust test connects to the already running sandbox. It checks the OpenShell
version and driver, workload identity, blocked-request evidence, and the approved
request. Ordinary `cargo test` runs its assertion guards and timeout test without
starting OpenShell. After following the workflow's setup steps locally, export
`E2E_DIR`, `E2E_NAME`, `E2E_DRIVER`, `E2E_ARTIFACTS`, and `E2E_VERSION` (the
OpenShell release version installed during setup) and run:

```sh
cargo test --locked --test e2e_test network_policy -- --ignored --nocapture --test-threads=1
```

`E2E_DIR` is the private temporary directory created by the setup step. It contains
the OpenShell binaries, client registration, and `osenv` wrapper that isolates
OpenShell's configuration from the host. Run the workflow's diagnostic and
cleanup shell steps when finished.

For macOS, install the [VM build prerequisites](#requirements), plus `e2fsprogs`,
`openssl@3`, and `jq`. The self-hosted runner uses labels `self-hosted`, `macos`,
`arm64` and the existing `integration` environment with required reviewers. It
must support Hypervisor.framework (`sysctl -n kern.hv_support` returns `1`).
The workflow checks release archive SHA-256 digests and signs the VM driver with
the hypervisor entitlement. The image builder is signed by `make build-vm`.

`make build-vm` needs `vm-rootfs.tar` to embed the build root filesystem. Obtain
it from the CI artifact or [build it on Linux arm64](#building-the-vms-root-filesystem).
The workflow unpacks it into the run's temporary directory. The OpenShell VM
driver has its own runtime embedded in the downloaded release; it is separate
from the rootfs used by this image builder.

Diagnostics are written to `target/e2e/<driver>` and uploaded by CI even on
failure. Certificates and private keys remain outside that directory. Each run
cleans up its sandbox, gateway, images, temporary files, and network.

## Logging

Use `-v` (info) or `-vv` (debug) to increase log verbosity during builds:

```sh
openshell-build-image --runtime podman --from myproject:dev -v myimage:latest
```

OpenShell manages CA certificates for the sandbox. The image builder does not discover or copy the host's CA bundle into images.

## Saving the Containerfile

Pass `--copy-containerfile` to include the exact Containerfile used for the build at `$HOME/Containerfile` **inside the image**, using the home directory and user inherited from the base image.

```sh
openshell-build-image --runtime podman --from myproject:dev --copy-containerfile myimage:latest
podman run --rm --entrypoint /bin/sh myimage:latest -c 'cat "$HOME/Containerfile"'
```

## Full option reference

```
openshell-build-image --runtime <RUNTIME> --from <IMAGE> [OPTIONS] <TAG>
```

| Argument / Option              | Description                                                        |
| ------------------------------ | ------------------------------------------------------------------ |
| `<TAG>`                        | Tag for the built image (e.g. `myimage:latest`)                    |
| `--runtime <RUNTIME>`          | Backend to build the image with (`podman`, `docker`, `container`, `vm` — see [Building in a VM](#building-in-a-vm---runtime-vm)) |
| `--from <IMAGE>`               | Required project image to build from (name, tag, or digest) |
| `--addon <IMAGE>`              | OCI addon to apply after the base image. Repeat in the desired order. |
| `--copy-containerfile`         | Copy the build Containerfile to `$HOME/Containerfile` inside the image |
| `--vm-rootfs <DIR>`            | Root filesystem the build VM boots from (`--runtime vm` only). Defaults to the one embedded in the binary. |
| `--vm-output <FILE>`           | Path for the rootfs tarball produced by `--runtime vm`. Defaults to a name derived from `<TAG>` in the current directory. |
| `--vm-cpus <N>`                | vCPUs given to the build VM (`--runtime vm` only). Default `2`.     |
| `--vm-memory <MIB>`            | RAM in MiB given to the build VM (`--runtime vm` only). Default `4096`. |
| `--vm-dns <ADDR>`              | Nameserver the build VM resolves through (`--runtime vm` only). Repeatable. Defaults to the host's own nameservers. |
| `-v` / `-vv`                   | Increase log verbosity (info / debug)                              |

The five `--vm-*` options are rejected with any other `--runtime`, rather than silently ignored.
