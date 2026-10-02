#!/bin/sh
# Copyright (C) 2026 Red Hat, Inc.
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
# http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.
#
# SPDX-License-Identifier: Apache-2.0
#
# Build and export the root filesystem for the build microVM.
# Requires Podman. The resulting rootfs has buildah installed and is
# pre-configured to use the vfs storage driver (required on virtiofs).
#
# Usage: ./make-rootfs.sh [OUTPUT_DIR] [ARCHIVE]
#   OUTPUT_DIR  where to write the rootfs  (default: ./vm-rootfs)
#   ARCHIVE     also pack the rootfs into this tarball, which the build script
#               embeds when OPENSHELL_IMAGE_BUILDER_VM_ROOTFS_ARCHIVE points at
#               it. Left uncompressed: build.rs compresses what it embeds, and
#               zstd is not part of a stock macOS.
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOTFS="${1:-./vm-rootfs}"
ARCHIVE_OUT="${2:-}"
IMAGE="openshell-vm-rootfs"

if ! command -v podman >/dev/null 2>&1; then
    echo "error: podman is required to build the rootfs" >&2
    exit 1
fi

echo "Building rootfs image for linux/arm64..."
podman build \
    --platform linux/arm64 \
    --tag "$IMAGE" \
    --file "$SCRIPT_DIR/Containerfile" \
    "$SCRIPT_DIR"

echo "Exporting rootfs to $ROOTFS..."
mkdir -p "$ROOTFS"
CONTAINER="$(podman create --platform linux/arm64 "$IMAGE")"

# /bin/sh reports only the last command's status for a pipeline, so
# `podman export | tar -x` would hide an export that died mid-stream and leave
# a truncated rootfs behind. Export to a file first and check that step alone.
# The trap removes the archive and the container even when extraction fails.
ARCHIVE="$(mktemp "${TMPDIR:-/tmp}/openshell-rootfs.XXXXXX")"
cleanup() {
    rm -f "$ARCHIVE"
    podman rm "$CONTAINER" >/dev/null 2>&1 || true
}
trap cleanup EXIT

podman export "$CONTAINER" > "$ARCHIVE"
tar -C "$ROOTFS" -xf "$ARCHIVE"

# podman create may mount the host's /etc/resolv.conf into the container,
# overwriting the one baked into the image. Write it explicitly after export.
# use-vc forces TCP for DNS queries — TSI routes TCP but may not route UDP.
printf 'nameserver 1.1.1.1\noptions use-vc\n' > "$ROOTFS/etc/resolv.conf"

# Packed from the extracted tree rather than straight from `podman export`, so
# that the archive holds exactly what an unprivileged `tar -x` could restore —
# the extraction the binary performs on the user's machine cannot then fail on
# something (a device node) it is not allowed to create. The tree sits at the
# archive root, with no leading `vm-rootfs/` component.
# COPYFILE_DISABLE stops macOS tar from storing extended attributes as extra
# `._*` members.
if [ -n "$ARCHIVE_OUT" ]; then
    echo "Packing rootfs into $ARCHIVE_OUT..."
    COPYFILE_DISABLE=1 tar -C "$ROOTFS" -cf "$ARCHIVE_OUT" .
fi

echo ""
echo "Rootfs ready at: $ROOTFS"
if [ -n "$ARCHIVE_OUT" ]; then
    echo "Archive ready at: $ARCHIVE_OUT"
    echo ""
    echo "Embed it in the binary:"
    echo "  OPENSHELL_IMAGE_BUILDER_VM_ROOTFS_ARCHIVE=$ARCHIVE_OUT \\"
    echo "    cargo build --release --features vm"
fi
echo ""
echo "Build an image with it:"
echo "  openshell-image-builder --runtime vm --vm-rootfs $ROOTFS myimage:latest"
