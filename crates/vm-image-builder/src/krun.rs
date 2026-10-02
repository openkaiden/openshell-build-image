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

//! libkrun FFI backend.
//!
//! Compiled only with the `krun` feature on macOS/aarch64; everything here is
//! unreachable on other targets.
//!
//! # Why this forks
//!
//! `krun_start_enter` does not return on success. libkrun takes over the
//! calling process and calls `exit()` with the workload's exit code once the
//! microVM shuts down. Calling it directly from a library function would end
//! the host program mid-build: destructors would not run, temporary build
//! contexts would leak, and the caller could not report on the result.
//!
//! So the VM is entered in a forked child and the parent reaps it, turning the
//! VM's exit code into a `Result`. Forking without an immediate `exec` is only
//! safe in a single-threaded process, because a lock held by another thread at
//! fork time would be held forever in the child. The CLI is single-threaded, but
//! this crate is a library and cannot assume its callers are, so
//! [`check_single_threaded`] verifies it immediately before the fork.
//!
//! [`check_single_threaded`]: crate::check_single_threaded

use std::ffi::CString;
use std::os::raw::c_char;
use std::ptr;

use krun_sys::{
    krun_add_virtiofs, krun_create_ctx, krun_set_exec, krun_set_log_level, krun_set_vm_config,
    krun_set_workdir, krun_start_enter,
};

use crate::{
    TAG_CONTEXT, TAG_OUTPUT, TAG_ROOT, VM_BUILD_SCRIPT, VM_DNS_ENV, VmBuild, VmBuildError,
};

/// Child exit code when VM configuration failed before boot.
const EXIT_SETUP_FAILED: i32 = 119;
/// Child exit code when `krun_start_enter` returned instead of entering the VM.
const EXIT_ENTER_FAILED: i32 = 118;

/// libkrun's most verbose log level, used when the host is running at debug.
const KRUN_LOG_LEVEL_TRACE: u32 = 5;

/// `PATH` for the build helper inside the VM. libkrun's init starts the
/// workload with an empty environment, so `buildah` and its helpers would not
/// be found without this.
const VM_PATH: &str = "PATH=/bin:/usr/bin:/usr/local/bin:/sbin:/usr/sbin";

/// Boots a microVM for `build` and waits for it to finish.
pub fn run(build: &VmBuild) -> Result<(), VmBuildError> {
    // Everything fallible that can be done on the host is done here, before the
    // fork, so failures surface as ordinary errors.
    let args = Args::new(build)?;

    // The fork below is only sound in a single-threaded process. Checking it
    // here — rather than trusting the caller — turns what would otherwise be a
    // silent deadlock in the child into an error the caller can report.
    crate::check_single_threaded()?;

    // SAFETY: `check_single_threaded` just established that no other thread can
    // be holding a lock, so the child inherits a consistent address space.
    let pid = unsafe { libc::fork() };
    match pid {
        -1 => Err(VmBuildError::Io(std::io::Error::last_os_error())),
        0 => {
            // Child. `enter_vm` never returns: either libkrun exits with the
            // workload's status, or we exit with a sentinel code.
            let code = enter_vm(&args, build.cpus, build.memory_mib);
            // SAFETY: `_exit` skips atexit handlers and buffer flushes, which
            // belong to the parent's copy of the runtime, not ours.
            unsafe { libc::_exit(code) }
        }
        _ => wait_for(pid),
    }
}

/// Waits for the VM child and maps its exit status onto a [`VmBuildError`].
fn wait_for(pid: libc::pid_t) -> Result<(), VmBuildError> {
    let mut status: libc::c_int = 0;
    // EINTR is expected whenever a signal lands while we are blocked here.
    loop {
        // SAFETY: `pid` is our own child and `status` is a valid out-pointer.
        let ret = unsafe { libc::waitpid(pid, &mut status, 0) };
        if ret == -1 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(VmBuildError::Io(err));
        }
        break;
    }

    if libc::WIFSIGNALED(status) {
        return Err(VmBuildError::Failed { exit_code: None });
    }

    match libc::WEXITSTATUS(status) {
        0 => Ok(()),
        EXIT_SETUP_FAILED => Err(VmBuildError::Krun {
            call: "krun_* configuration",
            code: -1,
        }),
        EXIT_ENTER_FAILED => Err(VmBuildError::Krun {
            call: "krun_start_enter",
            code: -1,
        }),
        code => Err(VmBuildError::Failed {
            exit_code: Some(code),
        }),
    }
}

/// Configures and enters the microVM. Returns only on failure, with the exit
/// code the child should use.
fn enter_vm(args: &Args, cpus: u8, memory_mib: u32) -> i32 {
    if log::log_enabled!(log::Level::Debug) {
        // SAFETY: sets a global log level; valid to call before any context exists.
        unsafe { krun_set_log_level(KRUN_LOG_LEVEL_TRACE) };
    }

    // SAFETY: every pointer below comes from a `CString` owned by `args`, which
    // outlives this call. `krun_set_exec` stores the argv/envp pointers, and
    // `krun_start_enter` consumes the context built from them.
    unsafe {
        let ctx = krun_create_ctx();
        if ctx < 0 {
            eprintln!("libkrun: krun_create_ctx failed (code {ctx})");
            return EXIT_SETUP_FAILED;
        }
        let ctx = ctx as u32;

        let steps: [(&str, i32); 6] = [
            (
                "krun_set_vm_config",
                krun_set_vm_config(ctx, cpus, memory_mib),
            ),
            // TAG_ROOT is KRUN_FS_ROOT_TAG — the bundled kernel mounts it as /.
            (
                "krun_add_virtiofs(root)",
                krun_add_virtiofs(ctx, args.tag_root.as_ptr(), args.rootfs.as_ptr()),
            ),
            (
                "krun_add_virtiofs(context)",
                krun_add_virtiofs(ctx, args.tag_context.as_ptr(), args.context.as_ptr()),
            ),
            (
                "krun_add_virtiofs(output)",
                krun_add_virtiofs(ctx, args.tag_output.as_ptr(), args.output_dir.as_ptr()),
            ),
            (
                "krun_set_workdir",
                krun_set_workdir(ctx, args.workdir.as_ptr()),
            ),
            {
                // The kernel's shebang handling supplies the script path as
                // argv[0], so this array starts at what the script sees as $1.
                let argv: [*const c_char; 4] = [
                    args.containerfile.as_ptr(),
                    args.tag.as_ptr(),
                    args.output_filename.as_ptr(),
                    ptr::null(),
                ];
                // A null in the middle terminates the list, so a build with no
                // nameservers simply passes PATH alone and the guest keeps
                // whatever resolv.conf its rootfs came with.
                let dns = args.dns_env.as_ref().map_or(ptr::null(), |s| s.as_ptr());
                let envp: [*const c_char; 3] = [args.path_env.as_ptr(), dns, ptr::null()];
                (
                    "krun_set_exec",
                    krun_set_exec(ctx, args.exec.as_ptr(), argv.as_ptr(), envp.as_ptr()),
                )
            },
        ];

        for (call, ret) in steps {
            if ret < 0 {
                eprintln!("libkrun: {call} failed (code {ret})");
                return EXIT_SETUP_FAILED;
            }
        }

        let ret = krun_start_enter(ctx);
        // Only reached when libkrun rejected the configuration; on success it
        // exits the process itself with the workload's status.
        eprintln!("libkrun: krun_start_enter failed (code {ret})");
        EXIT_ENTER_FAILED
    }
}

/// The `CString`s handed to libkrun.
///
/// They are grouped in one struct so a single value keeps every pointer passed
/// to `krun_set_exec` alive until `krun_start_enter` has consumed them.
struct Args {
    rootfs: CString,
    context: CString,
    output_dir: CString,
    output_filename: CString,
    containerfile: CString,
    tag: CString,
    tag_root: CString,
    tag_context: CString,
    tag_output: CString,
    workdir: CString,
    exec: CString,
    path_env: CString,
    /// `OPENSHELL_VM_DNS=…`, absent when the build carries no nameservers.
    dns_env: Option<CString>,
}

impl Args {
    fn new(build: &VmBuild) -> Result<Self, VmBuildError> {
        Ok(Args {
            rootfs: cstring_path(&build.rootfs, "VM rootfs")?,
            context: cstring_path(&build.context, "build context")?,
            output_dir: cstring_path(&build.output_dir, "output directory")?,
            output_filename: cstring(&build.output_filename, "output filename")?,
            containerfile: cstring(&build.containerfile_vm_path(), "Containerfile path")?,
            tag: cstring(&build.tag, "image tag")?,
            tag_root: cstring(TAG_ROOT, "root tag")?,
            tag_context: cstring(TAG_CONTEXT, "context tag")?,
            tag_output: cstring(TAG_OUTPUT, "output tag")?,
            workdir: cstring("/", "workdir")?,
            exec: cstring(VM_BUILD_SCRIPT, "build helper")?,
            path_env: cstring(VM_PATH, "PATH")?,
            dns_env: build
                .dns_env_value()
                .map(|value| cstring(&format!("{VM_DNS_ENV}={value}"), "VM nameservers"))
                .transpose()?,
        })
    }
}

/// Converts a path to a `CString`, rejecting non-UTF-8 and embedded NULs.
fn cstring_path(path: &std::path::Path, what: &'static str) -> Result<CString, VmBuildError> {
    let s = path.to_str().ok_or_else(|| VmBuildError::Path {
        what,
        path: path.to_path_buf(),
        source: std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "path is not valid UTF-8 and cannot be passed to libkrun",
        ),
    })?;
    cstring(s, what).map_err(|_| VmBuildError::Path {
        what,
        path: path.to_path_buf(),
        source: std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "path contains an interior NUL byte",
        ),
    })
}

/// Converts a string to a `CString`, rejecting embedded NULs.
fn cstring(s: &str, what: &'static str) -> Result<CString, VmBuildError> {
    CString::new(s).map_err(|_| {
        VmBuildError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("{what} contains an interior NUL byte"),
        ))
    })
}
