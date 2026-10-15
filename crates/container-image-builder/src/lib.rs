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

//! Build container images from an in-memory Containerfile.
//!
//! This crate abstracts the container CLI (Podman, Docker, or the macOS
//! `container` tool) behind a common interface. All three CLIs accept the same
//! `build -f <containerfile> -t <tag> <context-dir>` invocation, so the
//! build logic is identical regardless of which runtime is selected.
//!
//! # Supported runtimes
//!
//! | Variant | Binary |
//! |---|---|
//! | [`ContainerCli::Podman`] | `podman` |
//! | [`ContainerCli::Docker`] | `docker` |
//! | [`ContainerCli::MacOsContainer`] | `container` |
//!
//! # Quick start
//!
//! ```no_run
//! use container_image_builder::{build, ContainerCli, ContainerRunner};
//! use std::path::Path;
//!
//! let cli = ContainerCli::Podman;
//!
//! // Fail fast if the binary is not installed.
//! cli.check_in_path()?;
//!
//! // Build an image from an in-memory Containerfile.
//! build(
//!     "FROM ubuntu:24.04\nRUN echo hello",
//!     "myimage:latest",
//!     &cli,
//!     &ContainerRunner,
//!     Path::new("."),
//! )?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # Testability
//!
//! [`Runner`] is a trait so that tests can inject a [`FakeRunner`] that returns
//! a controlled [`ExitStatus`] without spawning a real container CLI process.
//! See the trait documentation for an example.
//!
//! [`FakeRunner`]: https://docs.rs/container-image-builder/latest/container_image_builder/trait.Runner.html

use std::io::Write as _;
use std::path::Path;
use std::process::{Command, ExitStatus, Output};

use which::which;

use tempfile::NamedTempFile;

pub mod base;

// ---------------------------------------------------------------------------
// ContainerCli
// ---------------------------------------------------------------------------

/// Selects which container CLI binary to invoke when building images.
///
/// All three variants support the same build command syntax:
/// `<binary> build -f <containerfile> -t <tag> <context-dir>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContainerCli {
    /// Use `podman` — the default on most Linux distributions.
    Podman,
    /// Use `docker` — Docker Desktop or the Docker Engine CLI.
    Docker,
    /// Use `container` — Apple's container CLI (macOS only).
    MacOsContainer,
}

impl ContainerCli {
    /// Returns the name of the binary that this variant invokes.
    ///
    /// # Examples
    ///
    /// ```
    /// use container_image_builder::ContainerCli;
    ///
    /// assert_eq!(ContainerCli::Podman.binary(), "podman");
    /// assert_eq!(ContainerCli::Docker.binary(), "docker");
    /// assert_eq!(ContainerCli::MacOsContainer.binary(), "container");
    /// ```
    pub fn binary(&self) -> &str {
        match self {
            ContainerCli::Podman => "podman",
            ContainerCli::Docker => "docker",
            ContainerCli::MacOsContainer => "container",
        }
    }

    /// Returns `Ok(())` when an executable named [`binary`] is found in
    /// `PATH`, or a [`RuntimeNotFoundError`] otherwise.
    ///
    /// Uses the [`which`](https://docs.rs/which) crate, which checks both
    /// file existence and the executable bit (Unix) / file extension (Windows).
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeNotFoundError`] if:
    /// - The `PATH` environment variable is not set.
    /// - No executable with the binary name returned by [`ContainerCli::binary`]
    ///   is found in any `PATH` directory.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use container_image_builder::ContainerCli;
    ///
    /// ContainerCli::Podman.check_in_path().expect("podman must be installed");
    /// ```
    ///
    /// [`binary`]: ContainerCli::binary
    pub fn check_in_path(&self) -> Result<(), RuntimeNotFoundError> {
        let binary = self.binary();
        if which(binary).is_ok() {
            Ok(())
        } else {
            Err(RuntimeNotFoundError(binary.to_string()))
        }
    }
}

// ---------------------------------------------------------------------------
// RuntimeNotFoundError
// ---------------------------------------------------------------------------

/// Error returned by [`ContainerCli::check_in_path`] when the selected binary
/// is absent from `PATH`.
///
/// The error message names the missing binary and suggests using `--runtime` to
/// select a different one.
#[derive(Debug)]
pub struct RuntimeNotFoundError(String);

impl std::fmt::Display for RuntimeNotFoundError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "'{}' not found in PATH — install it or choose a different --runtime",
            self.0
        )
    }
}

impl std::error::Error for RuntimeNotFoundError {}

// ---------------------------------------------------------------------------
// Runner
// ---------------------------------------------------------------------------

/// Executes a pre-built [`Command`].
///
/// The trait exists so that production code uses [`ContainerRunner`] while unit
/// tests substitute a `FakeRunner` that returns a controlled [`ExitStatus`]
/// without spawning a real container CLI process.
///
/// # Implementing a fake runner
///
/// ```
/// use container_image_builder::Runner;
/// use std::process::{Command, ExitStatus};
///
/// struct FakeRunner(i32);
///
/// impl Runner for FakeRunner {
///     fn run(&self, _cmd: &mut Command) -> std::io::Result<ExitStatus> {
///         // Spawn a trivial shell to produce the desired exit code.
///         Command::new("sh")
///             .args(["-c", &format!("exit {}", self.0)])
///             .status()
///     }
/// }
/// ```
pub trait Runner {
    /// Runs `cmd` and returns its [`ExitStatus`].
    fn run(&self, cmd: &mut Command) -> std::io::Result<ExitStatus>;

    /// Runs `cmd` and captures its output for image inspection.
    fn output(&self, cmd: &mut Command) -> std::io::Result<Output> {
        cmd.output()
    }
}

// ---------------------------------------------------------------------------
// ContainerRunner
// ---------------------------------------------------------------------------

/// The real [`Runner`] implementation: calls [`Command::status`] directly.
///
/// Use this in production code. For tests, implement [`Runner`] on a local
/// `FakeRunner` struct instead.
pub struct ContainerRunner;

impl Runner for ContainerRunner {
    fn run(&self, cmd: &mut Command) -> std::io::Result<ExitStatus> {
        cmd.status()
    }
}

// ---------------------------------------------------------------------------
// BuildError
// ---------------------------------------------------------------------------

/// Errors that can occur during [`build`].
#[derive(Debug)]
pub enum BuildError {
    /// An I/O error occurred while writing the temporary Containerfile or
    /// spawning the container CLI process.
    Io(std::io::Error),
    /// The container CLI process exited with a non-zero status.
    ///
    /// `exit_code` is `None` when the process was killed by a signal (Unix
    /// only); it is `Some(code)` otherwise.
    Failed { exit_code: Option<i32> },
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BuildError::Io(e) => write!(f, "I/O error: {e}"),
            BuildError::Failed {
                exit_code: Some(code),
            } => write!(f, "container build failed with exit code {code}"),
            BuildError::Failed { exit_code: None } => {
                write!(f, "container build was terminated by a signal")
            }
        }
    }
}

impl std::error::Error for BuildError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            BuildError::Io(e) => Some(e),
            BuildError::Failed { .. } => None,
        }
    }
}

impl From<std::io::Error> for BuildError {
    fn from(e: std::io::Error) -> Self {
        BuildError::Io(e)
    }
}

// ---------------------------------------------------------------------------
// build
// ---------------------------------------------------------------------------

/// Builds a container image from an in-memory Containerfile string.
///
/// The function writes `containerfile` to a temporary file and then invokes:
///
/// ```text
/// <cli.binary()> build -f <tmp-containerfile> -t <tag> <context_dir>
/// ```
///
/// `runner` abstracts command execution so that callers can inject a fake
/// runner in tests. Pass [`ContainerRunner`] in production.
///
/// # Arguments
///
/// - `containerfile` — the full Containerfile content (not a file path).
/// - `tag` — the image tag, e.g. `"myimage:latest"`.
/// - `cli` — which container binary to invoke.
/// - `runner` — executes the command; use [`ContainerRunner`] in production.
/// - `context_dir` — the build context directory passed as the last argument
///   to the CLI.
///
/// # Errors
///
/// - [`BuildError::Io`] if the temporary file cannot be written or the CLI
///   process cannot be spawned.
/// - [`BuildError::Failed`] if the CLI exits with a non-zero status or is
///   killed by a signal.
///
/// # Examples
///
/// ```no_run
/// use container_image_builder::{build, ContainerCli, ContainerRunner};
/// use std::path::Path;
///
/// build(
///     "FROM scratch",
///     "empty:latest",
///     &ContainerCli::Podman,
///     &ContainerRunner,
///     Path::new("."),
/// )?;
/// # Ok::<(), container_image_builder::BuildError>(())
/// ```
pub fn build(
    containerfile: &str,
    tag: &str,
    cli: &ContainerCli,
    runner: &dyn Runner,
    context_dir: &Path,
) -> Result<(), BuildError> {
    build_with_pull_policy(containerfile, tag, cli, runner, context_dir, false, None)
}

/// Builds from image references that were already prepared in the runtime's
/// local store. Podman and Docker are instructed not to pull during the build;
/// Apple's `container build` uses its local store unless `--pull` is supplied.
pub fn build_with_local_images(
    containerfile: &str,
    tag: &str,
    cli: &ContainerCli,
    runner: &dyn Runner,
    context_dir: &Path,
) -> Result<(), BuildError> {
    build_with_pull_policy(containerfile, tag, cli, runner, context_dir, true, None)
}

/// Build from already prepared images using the platform of the inspected
/// base. This matters when the base is a non-native platform image already in
/// the local store: the builder must choose addon payloads for that platform.
pub fn build_with_local_images_on_platform(
    containerfile: &str,
    tag: &str,
    cli: &ContainerCli,
    runner: &dyn Runner,
    context_dir: &Path,
    platform: &base::BasePlatform,
) -> Result<(), BuildError> {
    build_with_pull_policy(
        containerfile,
        tag,
        cli,
        runner,
        context_dir,
        true,
        Some(platform),
    )
}

fn build_with_pull_policy(
    containerfile: &str,
    tag: &str,
    cli: &ContainerCli,
    runner: &dyn Runner,
    context_dir: &Path,
    local_only: bool,
    platform: Option<&base::BasePlatform>,
) -> Result<(), BuildError> {
    let mut tmpfile = NamedTempFile::new()?;
    tmpfile.write_all(containerfile.as_bytes())?;

    let mut cmd = Command::new(cli.binary());
    cmd.arg("build");
    if local_only {
        match cli {
            ContainerCli::Podman => cmd.arg("--pull=never"),
            ContainerCli::Docker => cmd.arg("--pull=false"),
            ContainerCli::MacOsContainer => &mut cmd,
        };
    }
    if let Some(platform) = platform
        && *cli != ContainerCli::MacOsContainer
    {
        let mut value = format!("{}/{}", platform.os, platform.architecture);
        if let Some(variant) = &platform.variant {
            value.push('/');
            value.push_str(variant);
        }
        cmd.arg("--platform").arg(value);
    }
    cmd.arg("-f")
        .arg(tmpfile.path())
        .args(["-t", tag])
        .arg(context_dir);

    log::debug!("running: {cmd:?}");

    let status = runner.run(&mut cmd)?;

    if status.success() {
        Ok(())
    } else {
        Err(BuildError::Failed {
            exit_code: status.code(),
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::io;

    struct FakeRunner(fn() -> io::Result<ExitStatus>);

    impl Runner for FakeRunner {
        fn run(&self, _cmd: &mut Command) -> io::Result<ExitStatus> {
            self.0()
        }
    }

    fn exit_with(code: i32) -> ExitStatus {
        Command::new("sh")
            .args(["-c", &format!("exit {code}")])
            .status()
            .unwrap()
    }

    const CONTAINERFILE: &str = "FROM scratch";
    const TAG: &str = "test:latest";

    // --- ContainerCli::binary ---

    #[test]
    fn container_cli_binary_returns_podman() {
        assert_eq!(ContainerCli::Podman.binary(), "podman");
    }

    #[test]
    fn container_cli_binary_returns_docker() {
        assert_eq!(ContainerCli::Docker.binary(), "docker");
    }

    #[test]
    fn container_cli_binary_returns_container() {
        assert_eq!(ContainerCli::MacOsContainer.binary(), "container");
    }

    // --- ContainerCli::check_in_path ---

    #[test]
    fn check_in_path_finds_existing_binary() {
        // Use a shell guaranteed to be in PATH on every supported platform.
        #[cfg(unix)]
        let binary = "sh";
        #[cfg(windows)]
        let binary = "cmd";
        assert!(which(binary).is_ok());
    }

    #[test]
    fn check_in_path_fails_for_missing_binary() {
        assert!(which("a_really_improbable_name").is_err());
    }

    // --- RuntimeNotFoundError ---

    #[test]
    fn runtime_not_found_error_display_names_binary() {
        let err = RuntimeNotFoundError("podman".to_string());
        let msg = err.to_string();
        assert!(msg.contains("podman"), "expected 'podman' in: {msg}");
        assert!(msg.contains("PATH"), "expected 'PATH' in: {msg}");
    }

    // --- build ---

    #[test]
    fn build_succeeds() {
        let runner = FakeRunner(|| Ok(exit_with(0)));
        assert!(
            build(
                CONTAINERFILE,
                TAG,
                &ContainerCli::Podman,
                &runner,
                Path::new(".")
            )
            .is_ok()
        );
    }

    #[test]
    fn build_fails_with_exit_code() {
        let runner = FakeRunner(|| Ok(exit_with(1)));
        let err = build(
            CONTAINERFILE,
            TAG,
            &ContainerCli::Podman,
            &runner,
            Path::new("."),
        )
        .unwrap_err();
        assert!(matches!(err, BuildError::Failed { exit_code: Some(1) }));
    }

    #[test]
    fn build_propagates_io_error() {
        let runner =
            FakeRunner(|| Err(io::Error::new(io::ErrorKind::NotFound, "binary not found")));
        let err = build(
            CONTAINERFILE,
            TAG,
            &ContainerCli::Podman,
            &runner,
            Path::new("."),
        )
        .unwrap_err();
        assert!(matches!(err, BuildError::Io(_)));
    }

    #[cfg(unix)]
    #[test]
    fn build_signal_killed() {
        use std::os::unix::process::ExitStatusExt;
        let runner = FakeRunner(|| Ok(ExitStatus::from_raw(9)));
        let err = build(
            CONTAINERFILE,
            TAG,
            &ContainerCli::Podman,
            &runner,
            Path::new("."),
        )
        .unwrap_err();
        assert!(matches!(err, BuildError::Failed { exit_code: None }));
    }

    // --- BuildError display ---

    #[test]
    fn io_error_display() {
        let err = BuildError::Io(io::Error::new(io::ErrorKind::NotFound, "file not found"));
        assert_eq!(err.to_string(), "I/O error: file not found");
    }

    #[test]
    fn failed_with_exit_code_display_is_cli_agnostic() {
        let err = BuildError::Failed { exit_code: Some(1) };
        let msg = err.to_string();
        assert!(
            msg.contains("container build"),
            "expected 'container build' in: {msg}"
        );
        assert!(!msg.contains("podman"), "unexpected 'podman' in: {msg}");
        assert!(!msg.contains("docker"), "unexpected 'docker' in: {msg}");
    }

    #[test]
    fn failed_with_exit_code_display() {
        let err = BuildError::Failed { exit_code: Some(1) };
        assert_eq!(err.to_string(), "container build failed with exit code 1");
    }

    #[test]
    fn failed_without_exit_code_display() {
        let err = BuildError::Failed { exit_code: None };
        assert_eq!(
            err.to_string(),
            "container build was terminated by a signal"
        );
    }

    #[test]
    fn from_io_error_wraps_correctly() {
        let io_err = io::Error::new(io::ErrorKind::PermissionDenied, "denied");
        let build_err = BuildError::from(io_err);
        assert!(matches!(build_err, BuildError::Io(_)));
    }

    // --- build uses the cli binary name in the log ---

    #[test]
    fn build_works_with_docker_cli() {
        let runner = FakeRunner(|| Ok(exit_with(0)));
        assert!(
            build(
                CONTAINERFILE,
                TAG,
                &ContainerCli::Docker,
                &runner,
                Path::new(".")
            )
            .is_ok()
        );
    }

    #[test]
    fn build_works_with_macos_container_cli() {
        let runner = FakeRunner(|| Ok(exit_with(0)));
        assert!(
            build(
                CONTAINERFILE,
                TAG,
                &ContainerCli::MacOsContainer,
                &runner,
                Path::new(".")
            )
            .is_ok()
        );
    }

    #[test]
    fn local_builds_forbid_pulling_on_podman_and_docker() {
        struct CaptureRunner(RefCell<Vec<String>>);

        impl Runner for CaptureRunner {
            fn run(&self, cmd: &mut Command) -> io::Result<ExitStatus> {
                *self.0.borrow_mut() = cmd
                    .get_args()
                    .map(|arg| arg.to_string_lossy().into_owned())
                    .collect();
                Ok(exit_with(0))
            }
        }

        for (cli, expected_flag) in [
            (ContainerCli::Podman, Some("--pull=never")),
            (ContainerCli::Docker, Some("--pull=false")),
            (ContainerCli::MacOsContainer, None),
        ] {
            let runner = CaptureRunner(RefCell::new(Vec::new()));
            build_with_local_images(CONTAINERFILE, TAG, &cli, &runner, Path::new("."))
                .expect("local build should succeed");
            let args = runner.0.borrow();
            assert_eq!(args[0], "build");
            assert_eq!(
                args.get(1).map(String::as_str),
                expected_flag.or(Some("-f"))
            );
            assert_eq!(
                args.iter().any(|arg| arg.starts_with("--pull")),
                expected_flag.is_some()
            );
        }

        let platform = base::BasePlatform {
            os: "linux".to_owned(),
            architecture: "amd64".to_owned(),
            variant: None,
        };
        for cli in [ContainerCli::Podman, ContainerCli::Docker] {
            let runner = CaptureRunner(RefCell::new(Vec::new()));
            build_with_local_images_on_platform(
                CONTAINERFILE,
                TAG,
                &cli,
                &runner,
                Path::new("."),
                &platform,
            )
            .unwrap();
            let args = runner.0.borrow();
            assert!(
                args.windows(2)
                    .any(|pair| pair == ["--platform", "linux/amd64"])
            );
        }
    }
}
