---
name: vm-runtime
description: Build, sign, and run the `--runtime vm` backend — the libkrun microVM, its embedded root filesystem, and the errors each step can produce
---

# VM Runtime

Work on and run `--runtime vm`, the backend that builds images inside a libkrun microVM instead of calling a container engine on the host.

## Description

Use this skill when the task involves `--runtime vm`, libkrun, the build VM's root filesystem, or any file under `crates/vm-image-builder/`.

How a VM build works, in one paragraph: the binary unpacks a Linux root filesystem (Alpine with `buildah` in it) that is compressed inside the executable, boots a microVM from it with libkrun, shares three directories into the VM over virtio-fs (the rootfs, the build context, the output directory), and runs `/usr/local/bin/vm-build` inside, which drives `buildah` and writes a flattened rootfs tarball back to the output directory.

Four things must be true before a VM build can run:

1. It is an **Apple Silicon Mac** — libkrun uses Apple's Hypervisor.framework.
2. **libkrun is installed** (`brew install libkrun/krun/libkrun`) — the binary links against it rather than bundling it.
3. The binary was **built with `--features vm`** (off by default; it links against libkrun).
4. The binary was **signed with the hypervisor entitlement** — re-signed after every rebuild, since building clears the signature.

Only the third is reported plainly; the others surface as libkrun errors, which the table at the end of this skill maps back to a cause.

The root filesystem is not a fifth requirement: a release binary carries its own.

## Instructions

### Run a VM build

```bash
export LIBCLANG_PATH="$(xcode-select -p)/usr/lib"
export DYLD_FALLBACK_LIBRARY_PATH="$LIBCLANG_PATH:$(brew --prefix)/lib"
export PKG_CONFIG_PATH="$(brew --prefix)/lib/pkgconfig"

cargo build --release --features vm

codesign --sign - --entitlements entitlements.plist --force \
  target/release/openshell-image-builder

DYLD_LIBRARY_PATH="$(brew --prefix)/lib" \
  ./target/release/openshell-image-builder --runtime vm -v myimage:latest
```

`DYLD_LIBRARY_PATH` is needed at **run** time because libkrun loads its kernel, `libkrunfw`, by name and no rpath covers Homebrew's directory.

The result is a tarball, not an image in a local store — `./myimage-latest.tar` by default, or the path given to `--vm-output`. Check it the way CI does:

```bash
mkdir -p /tmp/check
tar -xf ./myimage-latest.tar -C /tmp/check ./etc/passwd
grep '^sandbox:' /tmp/check/etc/passwd
```

### Know where the root filesystem comes from

In order:

1. `--vm-rootfs <DIR>` if passed — always wins.
2. Otherwise the copy embedded in the binary, unpacked on first use to
   `~/Library/Application Support/openshell-image-builder/vm-rootfs/<version>/`.

Later runs reuse the unpacked copy. It is re-unpacked when the version changes, when the embedded archive changes, or when the unpacked tree is damaged. Deleting that directory is always safe.

A binary built **without** `OPENSHELL_IMAGE_BUILDER_VM_ROOTFS_ARCHIVE` embeds nothing, so a plain `cargo build --features vm` has no rootfs to fall back on and needs `--vm-rootfs`.

### Know how the guest resolves names

libkrun's TSI network mode gives the guest no NIC, so there is no DHCP to hand it a `resolv.conf`, and a query reaches whatever nameserver the guest was told to use — the host's resolver settings never apply, only its routing and firewall rules. A fixed public resolver would therefore fail behind a firewall that allows DNS only to the site's resolver, and would be blind to internal names.

In order:

1. `--vm-dns <ADDR>` if passed, repeatable — always wins, and a loopback or link-local address is rejected.
2. Otherwise the host's own nameservers, read on every run by `dns::default_nameservers` (`scutil --dns`, then `/etc/resolv.conf`), minus the ones the guest cannot reach.
3. Otherwise `1.1.1.1`, the fallback the rootfs also ships.

They travel in `OPENSHELL_VM_DNS` (space-separated) through `krun_set_exec`'s `envp`; `vm-build` turns them into a `resolv.conf` on tmpfs and bind-mounts it over `/etc/resolv.conf`. It is never written into the rootfs — that is a cache directory shared by concurrent builds, and the embedded one is fixed at compile time. `-vv` logs what was discovered.

### Change what is inside the build VM

Edit `crates/vm-image-builder/vm-image/Containerfile` (what the VM contains) or `vm-build` (what runs inside it), then rebuild the rootfs. **The script needs Podman on a `linux/arm64` machine** — it does not run on an Intel host, and on macOS it needs a running `podman machine`.

```bash
# writes the directory and the tarball
./crates/vm-image-builder/vm-image/make-rootfs.sh ./vm-rootfs ./vm-rootfs.tar
```

Test against it directly:

```bash
./target/release/openshell-image-builder --runtime vm --vm-rootfs ./vm-rootfs myimage:latest
```

Embed it, which is what a release build does:

```bash
OPENSHELL_IMAGE_BUILDER_VM_ROOTFS_ARCHIVE=$PWD/vm-rootfs.tar \
  cargo build --release --features vm
```

The build prints `Embedded VM rootfs: <bytes>` when it picked the archive up, and a warning naming the variable when it did not. Re-sign after rebuilding.

### Run the checks

`cargo test` and `cargo clippy` do **not** compile the VM FFI code by default. Run both forms:

```bash
cargo fmt --check && cargo clippy -- -D warnings && cargo test
cargo clippy --workspace --features vm -- -D warnings && cargo test --workspace --features vm
```

## Errors and what they mean

| Message | Cause | Fix |
|---|---|---|
| `VM builds are not available: this binary was built without the 'vm' feature` | Built without `--features vm` | Rebuild with `--features vm` |
| `VM builds are not available: libkrun requires macOS on Apple Silicon` | Wrong platform | Nothing to fix — the backend cannot run there |
| `no VM rootfs is embedded in this build` | Built without `OPENSHELL_IMAGE_BUILDER_VM_ROOTFS_ARCHIVE` | Pass `--vm-rootfs`, or rebuild with the variable set |
| `VM rootfs '<path>': not a directory` / `missing /usr/local/bin/vm-build` | `--vm-rootfs` points somewhere that is not a build rootfs | Point it at a directory made by `make-rootfs.sh` |
| `Couldn't find or load libkrunfw.5.dylib` | libkrun cannot find its kernel at run time | Set `DYLD_LIBRARY_PATH="$(brew --prefix)/lib"` |
| `libkrun call krun_start_enter failed` with no other output | Signature lacks the hypervisor entitlement, or the host is not bare metal | Re-run `codesign` with `entitlements.plist`; check `sysctl -n kern.hv_support` is `1` |
| `failed to run custom build command for krun-sys` / `Library not loaded: @rpath/libclang.dylib` | bindgen cannot load libclang | Set `LIBCLANG_PATH` **and** `DYLD_FALLBACK_LIBRARY_PATH` as shown above |
| The build dies out of memory inside the VM | `buildah` uses the `vfs` storage driver, which copies every layer | Raise `--vm-memory` (default 4096) |
| `cannot start a VM from a process with N threads` | `KrunRunner::run` forks; that is only safe single-threaded | Do not spawn threads before the build |
| `--vm-dns <addr> cannot be reached from inside the VM` | A loopback or link-local address means the VM, not the host | Pass the address the host's local resolver forwards to |
| A `FROM` or `RUN` in the VM cannot resolve a name the host resolves fine | The host's nameservers were not usable, so the VM fell back to `1.1.1.1` | Run with `-vv` to see which were discovered; pass `--vm-dns` |

## Where the code lives

| Path | What it holds |
|---|---|
| `src/main.rs` | `--vm-*` flags, `select_runtime` / `select_vm` / `vm_config` |
| `src/vm_rootfs.rs` | Unpacking the embedded rootfs, and its cache |
| `build.rs` | Compressing that rootfs into the binary at build time |
| `crates/vm-image-builder/src/lib.rs` | `VmConfig`, `VmBuild`, `build()`, the `VmRunner` trait, `VmBuildError` |
| `crates/vm-image-builder/src/dns.rs` | Discovering the host's nameservers to give the guest |
| `crates/vm-image-builder/src/krun.rs` | The libkrun FFI calls, behind the `krun` feature |
| `crates/vm-image-builder/vm-image/Containerfile` | What the build VM contains |
| `crates/vm-image-builder/vm-image/vm-build` | What runs inside the VM |
| `crates/vm-image-builder/vm-image/make-rootfs.sh` | Builds the rootfs directory and tarball |
| `.github/workflows/vm-runtime.yml` | The only CI that compiles and boots the VM backend |

## Checklist for changing the build VM

- [ ] Edited `Containerfile` or `vm-build`.
- [ ] Rebuilt with `make-rootfs.sh ./vm-rootfs ./vm-rootfs.tar` on `linux/arm64`.
- [ ] Ran a build with `--vm-rootfs ./vm-rootfs` and checked the output tarball.
- [ ] Rebuilt with `OPENSHELL_IMAGE_BUILDER_VM_ROOTFS_ARCHIVE` set, re-signed, and ran once without `--vm-rootfs`.
- [ ] Ran both check suites (with and without `--features vm`).
- [ ] Updated the README if a requirement or flag changed.
