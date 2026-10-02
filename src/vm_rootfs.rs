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

//! The VM build rootfs embedded in this binary, and its extraction on first
//! use.
//!
//! `--runtime vm` exists to build images on a host with no container engine on
//! it, but it boots from a Linux filesystem with `buildah` in it — and
//! producing that filesystem takes Podman on a `linux/arm64` machine. Asking
//! the user for it would put back exactly the requirement this runtime removes,
//! so the binary carries one instead: `build.rs` compresses the tarball named
//! by `OPENSHELL_IMAGE_BUILDER_VM_ROOTFS_ARCHIVE` into the executable, and the
//! first VM build unpacks it into the user's data directory.
//!
//! A build made without that variable embeds nothing and says so when asked
//! for a rootfs, leaving `--vm-rootfs` — which overrides the embedded rootfs
//! in any build — as the only way to supply one.
//! `vm-image/make-rootfs.sh` produces both the directory that flag wants and
//! the tarball to embed.

use std::fs;
use std::path::{Path, PathBuf};

use vm_image_builder::VmConfig;

/// The compressed rootfs, or empty when this binary embeds none.
const ARCHIVE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/vm-rootfs.tar.zst"));

/// Hex SHA-256 of [`ARCHIVE`], written beside it by `build.rs`.
const DIGEST: &str = include_str!(concat!(env!("OUT_DIR"), "/vm-rootfs.sha256"));

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Records which archive the extracted tree came from. Written last, inside
/// the staging directory, so it can only describe a complete extraction.
const MARKER: &str = ".rootfs-archive";

/// Prefix of a directory still being unpacked into. The version and the pid
/// complete it, so two processes unpacking at once cannot collide, and
/// [`sweep_old_versions`] can tell a staging directory from an installed one.
const STAGING_PREFIX: &str = ".staging-";

/// Reported when the binary was built without an embedded rootfs.
///
/// Whoever reads this is running the binary, not building it — a release build
/// carries a rootfs, so this is either a local build or a build from a
/// pipeline that did not stage one. Either way the only thing they can act on
/// is `--vm-rootfs`; the instructions for producing a rootfs belong to the
/// build, and `build.rs` prints them there.
const NOT_EMBEDDED: &str = "no VM rootfs is embedded in this build — \
     point --vm-rootfs at one, or use a release build, which carries its own";

/// Returns whether this binary carries a rootfs at all.
///
/// Only the tests ask: a build with no rootfs in it behaves the same way as
/// one that cannot unpack the one it has, and both are reported by
/// [`ensure_extracted`].
#[cfg(test)]
pub fn is_embedded() -> bool {
    !ARCHIVE.is_empty()
}

/// Unpacks the embedded rootfs if it is not already unpacked, and returns the
/// directory holding it.
///
/// The result is cached under the user's data directory, keyed by the digest
/// of the archive, so this costs an extraction once per version rather than
/// once per build.
///
/// # Errors
///
/// Returns a message pointing at `--vm-rootfs` when none is embedded, and
/// otherwise whatever went wrong while unpacking.
pub fn ensure_extracted() -> Result<PathBuf, String> {
    install(ARCHIVE, DIGEST, &cache_root()?)
}

/// `<data dir>/openshell-image-builder/vm-rootfs`, the parent of every
/// extracted version.
fn cache_root() -> Result<PathBuf, String> {
    dirs::data_dir()
        .map(|dir| dir.join("openshell-image-builder").join("vm-rootfs"))
        .ok_or_else(|| "cannot locate a data directory to unpack the VM rootfs into".to_string())
}

/// The body of [`ensure_extracted`], with the archive and its destination
/// passed in so it can be tested without touching the real data directory.
fn install(archive: &[u8], digest: &str, root: &Path) -> Result<PathBuf, String> {
    if archive.is_empty() {
        return Err(NOT_EMBEDDED.to_string());
    }

    let dir = root.join(VERSION);
    let key = cache_key(digest);

    // The marker alone is not enough: the extracted tree sits in a directory
    // the user can reach, so check that it still looks like a rootfs rather
    // than trusting a file written the last time it did.
    if marker_matches(&dir, &key) && VmConfig::new(&dir).check_rootfs().is_ok() {
        return Ok(dir);
    }

    let staging = root.join(format!("{STAGING_PREFIX}{VERSION}-{}", std::process::id()));
    if let Err(err) = stage(archive, &key, &staging) {
        let _ = fs::remove_dir_all(&staging);
        return Err(err);
    }

    // Another process may have installed this same rootfs while we were
    // unpacking. Its tree came from the archive ours did and it may already be
    // booting a VM from it, so keep it and drop our copy — swapping an
    // identical tree in underneath a live VM gains nothing and can break it.
    if marker_matches(&dir, &key) && VmConfig::new(&dir).check_rootfs().is_ok() {
        let _ = fs::remove_dir_all(&staging);
        return Ok(dir);
    }

    if dir.exists()
        && let Err(err) = fs::remove_dir_all(&dir)
    {
        let _ = fs::remove_dir_all(&staging);
        return Err(format!("remove stale VM rootfs {}: {err}", dir.display()));
    }
    if let Err(err) = fs::rename(&staging, &dir) {
        let _ = fs::remove_dir_all(&staging);
        return Err(format!("install VM rootfs into {}: {err}", dir.display()));
    }

    sweep_old_versions(root, &dir);

    Ok(dir)
}

/// Unpacks `archive` into a directory of its own and marks it complete.
///
/// Extraction lands here rather than in the final directory so that an
/// interrupted or failed unpack cannot leave a half-written rootfs behind that
/// a later run would boot from.
fn stage(archive: &[u8], key: &str, staging: &Path) -> Result<(), String> {
    if staging.exists() {
        fs::remove_dir_all(staging).map_err(|e| format!("remove {}: {e}", staging.display()))?;
    }
    fs::create_dir_all(staging).map_err(|e| format!("create {}: {e}", staging.display()))?;

    let decoder =
        zstd::Decoder::new(archive).map_err(|e| format!("read the embedded VM rootfs: {e}"))?;
    // The defaults match what `make-rootfs.sh` does on the machine that built
    // the archive: modes are restored (the `vm-build` helper has to stay
    // executable), while setuid bits and ownership are not — neither survives
    // an unprivileged extraction, and the VM runs as root anyway.
    tar::Archive::new(decoder).unpack(staging).map_err(|e| {
        format!(
            "unpack the embedded VM rootfs into {}: {e}",
            staging.display()
        )
    })?;

    fs::write(staging.join(MARKER), key)
        .map_err(|e| format!("write {}: {e}", staging.join(MARKER).display()))
}

/// Ties an extracted tree to the archive it came from. The version is part of
/// it so that a rebuild at the same version with a different rootfs is still
/// seen as stale.
fn cache_key(digest: &str) -> String {
    format!("{VERSION}-{}", digest.trim())
}

fn marker_matches(dir: &Path, key: &str) -> bool {
    fs::read_to_string(dir.join(MARKER)).is_ok_and(|found| found.trim() == key)
}

/// Removes rootfs trees left by other versions of this binary.
///
/// Each is around 80 MiB, and nothing else ever deletes them. Failures are
/// ignored: another process may be booting a VM from one right now, and a
/// rootfs that could not be swept is not a reason to fail the build the user
/// asked for.
///
/// A directory still being unpacked into is left alone. It belongs to whichever
/// process is writing it — the pid in its name says which — and removing one
/// would fail that process's extraction rather than reclaim anything: it is
/// about to become an installed rootfs or be cleaned up by its owner.
fn sweep_old_versions(root: &Path, current: &Path) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path == current || !path.is_dir() {
            continue;
        }
        if entry
            .file_name()
            .to_string_lossy()
            .starts_with(STAGING_PREFIX)
        {
            continue;
        }
        let _ = fs::remove_dir_all(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::io::Write;

    use tempfile::TempDir;

    /// Builds a tarball holding the one file `VmConfig::check_rootfs` looks
    /// for, so an extraction of it passes as a rootfs.
    fn archive_with(contents: &str) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());

        let mut header = tar::Header::new_gnu();
        header.set_size(contents.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        builder
            .append_data(&mut header, "usr/local/bin/vm-build", contents.as_bytes())
            .expect("append the helper");

        let tarball = builder.into_inner().expect("finish the tarball");
        zstd::encode_all(tarball.as_slice(), 1).expect("compress the tarball")
    }

    fn digest_of(archive: &[u8]) -> String {
        use sha2::{Digest, Sha256};

        Sha256::digest(archive)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    #[test]
    fn install_unpacks_the_archive() {
        let root = TempDir::new().expect("temp dir");
        let archive = archive_with("#!/bin/sh\n");

        let dir = install(&archive, &digest_of(&archive), root.path()).expect("install");

        let helper = dir.join("usr/local/bin/vm-build");
        assert_eq!(
            fs::read_to_string(&helper).expect("read the helper"),
            "#!/bin/sh\n"
        );
        assert!(
            VmConfig::new(&dir).check_rootfs().is_ok(),
            "the extracted tree should pass as a rootfs"
        );
    }

    // `check_rootfs` requires the execute bit, as libkrun exec's the helper.
    // Only Unix has one, and only there does the check run.
    #[test]
    #[cfg(unix)]
    fn install_keeps_the_helper_executable() {
        use std::os::unix::fs::PermissionsExt;

        let root = TempDir::new().expect("temp dir");
        let archive = archive_with("#!/bin/sh\n");

        let dir = install(&archive, &digest_of(&archive), root.path()).expect("install");

        let mode = fs::metadata(dir.join("usr/local/bin/vm-build"))
            .expect("stat the helper")
            .permissions()
            .mode();
        assert_ne!(mode & 0o111, 0, "mode was {mode:o}");
    }

    #[test]
    fn install_reuses_an_already_extracted_rootfs() {
        let root = TempDir::new().expect("temp dir");
        let archive = archive_with("#!/bin/sh\n");
        let digest = digest_of(&archive);

        let dir = install(&archive, &digest, root.path()).expect("first install");
        let sentinel = dir.join("sentinel");
        fs::write(&sentinel, b"kept").expect("write the sentinel");

        let again = install(&archive, &digest, root.path()).expect("second install");

        assert_eq!(again, dir);
        assert!(
            sentinel.exists(),
            "a cache hit should not unpack the archive again"
        );
    }

    #[test]
    fn install_replaces_a_rootfs_from_another_archive() {
        let root = TempDir::new().expect("temp dir");
        let old = archive_with("#!/bin/sh\n");
        let dir = install(&old, &digest_of(&old), root.path()).expect("first install");
        fs::write(dir.join("sentinel"), b"stale").expect("write the sentinel");

        let new = archive_with("#!/bin/sh\n# newer\n");
        let dir = install(&new, &digest_of(&new), root.path()).expect("second install");

        assert_eq!(
            fs::read_to_string(dir.join("usr/local/bin/vm-build")).expect("read the helper"),
            "#!/bin/sh\n# newer\n"
        );
        assert!(
            !dir.join("sentinel").exists(),
            "a stale rootfs should be replaced, not merged into"
        );
    }

    #[test]
    fn install_re_extracts_when_the_rootfs_was_damaged() {
        let root = TempDir::new().expect("temp dir");
        let archive = archive_with("#!/bin/sh\n");
        let digest = digest_of(&archive);

        let dir = install(&archive, &digest, root.path()).expect("first install");
        fs::remove_file(dir.join("usr/local/bin/vm-build")).expect("damage the rootfs");

        let dir = install(&archive, &digest, root.path()).expect("second install");

        assert!(
            VmConfig::new(&dir).check_rootfs().is_ok(),
            "a damaged rootfs should be unpacked again"
        );
    }

    #[test]
    fn install_sweeps_other_versions() {
        let root = TempDir::new().expect("temp dir");
        let stale = root.path().join("0.0.1-old");
        fs::create_dir_all(&stale).expect("create a stale version");
        let archive = archive_with("#!/bin/sh\n");

        install(&archive, &digest_of(&archive), root.path()).expect("install");

        assert!(!stale.exists(), "an older version should be swept");
    }

    #[test]
    fn install_leaves_another_process_staging_directory_alone() {
        let root = TempDir::new().expect("temp dir");
        // Named for a pid that is not ours, as a concurrent first-ever build
        // unpacking into the same cache would be. Sweeping it would fail that
        // build's extraction rather than reclaim anything.
        let staging = root.path().join(format!(
            "{STAGING_PREFIX}{VERSION}-{}",
            std::process::id() + 1
        ));
        fs::create_dir_all(&staging).expect("create a staging directory");
        let archive = archive_with("#!/bin/sh\n");

        install(&archive, &digest_of(&archive), root.path()).expect("install");

        assert!(
            staging.exists(),
            "a staging directory in flight should not be swept"
        );
    }

    #[test]
    fn install_without_an_embedded_archive_points_at_the_flag() {
        let root = TempDir::new().expect("temp dir");

        let err = install(&[], "", root.path()).expect_err("no archive is embedded");

        assert!(err.contains("--vm-rootfs"), "error was: {err}");
    }

    #[test]
    fn install_leaves_nothing_behind_when_the_archive_is_corrupt() {
        let root = TempDir::new().expect("temp dir");
        let mut corrupt = archive_with("#!/bin/sh\n");
        corrupt.truncate(corrupt.len() / 2);

        let err = install(&corrupt, &digest_of(&corrupt), root.path())
            .expect_err("a truncated archive cannot be unpacked");

        assert!(err.contains("VM rootfs"), "error was: {err}");
        let leftovers: Vec<_> = fs::read_dir(root.path())
            .expect("read the cache root")
            .flatten()
            .map(|entry| entry.file_name())
            .collect();
        assert!(leftovers.is_empty(), "left behind: {leftovers:?}");
    }

    /// Unpacks what this binary actually carries, when it carries anything.
    ///
    /// The synthetic archives above prove the mechanism; this proves the
    /// payload — that whatever was staged at build time is a rootfs the VM can
    /// boot, rather than some other tarball. It unpacks into a temporary
    /// directory rather than through [`ensure_extracted`], so running the tests
    /// never writes 80 MiB into the data directory of whoever ran them.
    #[test]
    fn the_embedded_archive_is_a_build_rootfs() {
        if !is_embedded() {
            return;
        }

        let root = TempDir::new().expect("temp dir");
        let dir = install(ARCHIVE, DIGEST, root.path()).expect("unpack the embedded rootfs");

        VmConfig::new(&dir)
            .check_rootfs()
            .expect("the embedded rootfs should be usable");
    }

    #[test]
    fn cache_key_covers_the_version_and_the_digest() {
        let key = cache_key("  abc123\n");

        assert_eq!(key, format!("{VERSION}-abc123"));
    }

    #[test]
    fn marker_matches_only_the_recorded_key() {
        let dir = TempDir::new().expect("temp dir");
        let mut marker = fs::File::create(dir.path().join(MARKER)).expect("create the marker");
        marker.write_all(b"1.0.0-abc\n").expect("write the marker");

        assert!(marker_matches(dir.path(), "1.0.0-abc"));
        assert!(!marker_matches(dir.path(), "1.0.0-def"));
    }

    #[test]
    fn marker_matches_is_false_without_a_marker() {
        let dir = TempDir::new().expect("temp dir");

        assert!(!marker_matches(dir.path(), "1.0.0-abc"));
    }
}
