# OCI addon E2E fixtures

`build.sh` builds three addon formats in the local Podman image store. Supply
a unique local image prefix:

```bash
bash tests/e2e/addons/build.sh localhost/osbi-addon-fixture-local
bash tests/e2e/addons/verify.sh localhost/osbi-addon-fixture-local
```

The workflow builds and verifies these fixtures on pull requests with read-only
permissions. On `main`, it runs `build.sh`, `verify.sh`, and `publish.sh` in the
same job, passing the same local image prefix to each script. `publish.sh` pushes the
resulting Podman images and indexes directly to
`ghcr.io/openkaiden/tests/addon-single`, `addon-platforms`, and
`addon-platforms-libc`. It runs only when the fixture Containerfile, scripts,
or workflow changes on `main`; pull requests do not publish images. The
workflow summary contains digest-pinned references for E2E tests.

`refs.sh` pins the published references and selected child manifest digests
from the successful main run at `f324fd76894a5d86942e7ef619b4dd89a0ee6091`.
The main E2E workflow runs `e2e.sh` against these references with Podman,
Docker, and Apple container. It runs `assert-image.sh` inside each result to
check files, environment values, and selected `COPY` digests. Ubuntu and Alpine
builds cover each fixture shape, including glibc and musl selection. The Ubuntu
jobs also check addon order, repeated references, unsupported platforms, and
bases without a detectable libc. Podman and Docker also build an image for the
opposite CPU architecture without running it. Each build records its runtime commands so
the tests can assert that libc detection exports the base exactly when needed.
The resulting `platforms-libc` image is used for the OpenShell sandbox and
network-policy test. `cleanup.sh` removes the built images and downloaded
fixtures after the job. The VM runtime workflow tests its guest-side addon
resolver with the same pinned `platforms-libc` index on Fedora and Alpine.
The OpenShell VM E2E job builds with that index on Ubuntu and Alpine, then
checks that OpenShell starts a sandbox from the exported rootfs and enforces
its network policy. Addon selection and metadata are checked by the VM
runtime workflow rather than repeated in the OpenShell E2E job.
Apple container builds use the OpenShell VM driver to run the exported image
as a sandbox.

The canonical OCI index descriptor annotation key for libc selection is
`ai.openkaiden.openshell-build-image.libc`. The `platforms-libc` index uses
`glibc` and `musl` values; the `platforms` index has no libc annotations. Addon
selection code must use this exact key and these values.

| Fixture | Registry object | Variants |
| --- | --- | --- |
| `single-amd64`, `single-arm64` | Separate single manifests | One Linux architecture each |
| `platforms` | OCI index | `linux/amd64`, `linux/arm64`; no libc annotations |
| `platforms-libc` | OCI index | Both architectures with `glibc` and `musl` descriptors |

Each variant contains its own
`/usr/local/share/openshell-addon-test/<fixture>/selected` and
`variants/<variant>` markers, plus a shared `last-applied` marker. It sets
`OSBI_TEST_<FIXTURE>` and `OSBI_TEST_VARIANT` in the image configuration, so
tests can check both fixture selection and addon order.

The `linux/amd64` single-image fixture contains:

```text
/usr/local/share/openshell-addon-test/
├── last-applied             → single:linux-amd64
└── single/
    ├── selected             → fixture=single
    │                          os=linux
    │                          architecture=amd64
    │                          libc=none
    │                          variant=linux-amd64
    └── variants/linux-amd64 → same content as selected
```

The `linux/arm64` variant in the unannotated `platforms` index contains:

```text
/usr/local/share/openshell-addon-test/
├── last-applied             → platforms:linux-arm64
└── platforms/
    ├── selected             → fixture=platforms
    │                          os=linux
    │                          architecture=arm64
    │                          libc=none
    │                          variant=linux-arm64
    └── variants/linux-arm64 → same content as selected
```

Here, `libc=none` is marker text; the `platforms` index has no libc annotation.

The `linux/amd64` musl variant of `platforms-libc` contains:

```text
/usr/local/share/openshell-addon-test/
├── last-applied                  → platforms-libc:linux-amd64-musl
└── platforms-libc/
    ├── selected                  → fixture=platforms-libc
    │                               os=linux
    │                               architecture=amd64
    │                               libc=musl
    │                               variant=linux-amd64-musl
    └── variants/linux-amd64-musl → same content as selected
```
