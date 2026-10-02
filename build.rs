// Copyright (C) 2026 Red Hat, Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//
// SPDX-License-Identifier: Apache-2.0

//! Stages the VM build rootfs so that `src/vm_rootfs.rs` can embed it.
//!
//! `--runtime vm` boots from a Linux filesystem with `buildah` in it, built by
//! `crates/vm-image-builder/vm-image/make-rootfs.sh`. Embedding that filesystem
//! in the binary is what lets a downloaded build run without the user first
//! producing one with Podman.
//!
//! Point `OPENSHELL_IMAGE_BUILDER_VM_ROOTFS_ARCHIVE` at the tarball the script
//! writes and it is compressed into `OUT_DIR` alongside a digest of the
//! compressed bytes, which the binary uses as its cache key. Without the
//! variable the build still succeeds and writes empty placeholders, leaving
//! `--vm-rootfs` as the only way to give such a build a rootfs.

use std::io::Read;
use std::path::PathBuf;
use std::{env, fs};

use sha2::{Digest, Sha256};

/// Env var naming the tarball to embed.
const ARCHIVE_ENV: &str = "OPENSHELL_IMAGE_BUILDER_VM_ROOTFS_ARCHIVE";

/// Compressed rootfs written into `OUT_DIR`.
const ARCHIVE_OUT: &str = "vm-rootfs.tar.zst";

/// Hex digest of [`ARCHIVE_OUT`], written into `OUT_DIR` next to it.
const DIGEST_OUT: &str = "vm-rootfs.sha256";

/// First four bytes of a zstd frame, used to tell an already-compressed
/// archive from a plain tarball without trusting the file name.
const ZSTD_MAGIC: [u8; 4] = [0x28, 0xb5, 0x2f, 0xfd];

/// Compression level for a tarball this script compresses itself.
///
/// Deliberately short of zstd's maximum: the rootfs is around 80 MiB, and the
/// last few levels cost minutes of build time for a couple of percent of size.
/// A release pipeline that wants those percent can compress the archive itself
/// — an archive that arrives already compressed is embedded untouched.
const COMPRESSION_LEVEL: i32 = 12;

fn main() {
    println!("cargo:rerun-if-env-changed={ARCHIVE_ENV}");

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR not set"));

    let Some(archive) = env::var_os(ARCHIVE_ENV).map(PathBuf::from) else {
        println!("cargo:warning={ARCHIVE_ENV} not set: building without an embedded VM rootfs");
        println!(
            "cargo:warning=Build one with crates/vm-image-builder/vm-image/make-rootfs.sh, or pass --vm-rootfs at run time"
        );
        write_placeholders(&out_dir);
        return;
    };

    println!("cargo:rerun-if-changed={}", archive.display());

    let contents = fs::read(&archive)
        .unwrap_or_else(|e| panic!("read {ARCHIVE_ENV} ({}): {e}", archive.display()));
    assert!(
        !contents.is_empty(),
        "{ARCHIVE_ENV} is empty: {}",
        archive.display()
    );

    let compressed = if contents.starts_with(&ZSTD_MAGIC) {
        verify_frame(&contents, &archive);
        contents
    } else {
        compress(&contents, &archive)
    };

    let digest = hex(&Sha256::digest(&compressed));
    let size = compressed.len();

    fs::write(out_dir.join(ARCHIVE_OUT), compressed)
        .unwrap_or_else(|e| panic!("write {ARCHIVE_OUT}: {e}"));
    fs::write(out_dir.join(DIGEST_OUT), &digest)
        .unwrap_or_else(|e| panic!("write {DIGEST_OUT}: {e}"));

    println!("cargo:warning=Embedded VM rootfs: {size} bytes ({digest})");
}

/// Compresses a plain tarball, checking that it decompresses back to the same
/// bytes.
///
/// The archive is embedded and shipped, so a silently truncated one would only
/// surface as a failed extraction on a user's machine.
fn compress(contents: &[u8], archive: &std::path::Path) -> Vec<u8> {
    let compressed = zstd::encode_all(contents, COMPRESSION_LEVEL)
        .unwrap_or_else(|e| panic!("compress {}: {e}", archive.display()));

    let mut roundtrip = Vec::with_capacity(contents.len());
    zstd::Decoder::new(compressed.as_slice())
        .and_then(|mut decoder| decoder.read_to_end(&mut roundtrip))
        .unwrap_or_else(|e| panic!("verify compressed {}: {e}", archive.display()));
    assert!(
        roundtrip == contents,
        "compressed {} does not decompress to the original bytes",
        archive.display()
    );

    compressed
}

/// Checks that an archive that arrived already compressed decodes as a whole
/// zstd frame.
///
/// The magic bytes only identify the first four bytes of a frame. Embedding on
/// that alone would let a truncated or corrupt archive through the build and
/// surface as a failed extraction on a user's machine, which is exactly what
/// [`compress`] verifies against for an archive this script compresses itself.
fn verify_frame(contents: &[u8], archive: &std::path::Path) {
    let mut decoded = Vec::new();
    zstd::Decoder::new(contents)
        .and_then(|mut decoder| decoder.read_to_end(&mut decoded))
        .unwrap_or_else(|e| panic!("verify compressed {}: {e}", archive.display()));
    assert!(
        !decoded.is_empty(),
        "compressed {} decompresses to nothing",
        archive.display()
    );
}

/// Writes the empty files `src/vm_rootfs.rs` includes when no archive was
/// staged. `include_bytes!` needs its target to exist at compile time even
/// when nothing was embedded.
fn write_placeholders(out_dir: &std::path::Path) {
    fs::write(out_dir.join(ARCHIVE_OUT), b"")
        .unwrap_or_else(|e| panic!("write {ARCHIVE_OUT}: {e}"));
    fs::write(out_dir.join(DIGEST_OUT), b"").unwrap_or_else(|e| panic!("write {DIGEST_OUT}: {e}"));
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}
