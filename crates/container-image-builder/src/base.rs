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

//! Prepare a base image in the same runtime that will build the final image.

use std::fs::File;
use std::path::Path;
use std::process::{Command, ExitStatus, Output};

use serde_json::Value;

use crate::{ContainerCli, Runner};
pub use addon_rules::libc::Libc;
use addon_rules::libc::libc_marker;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BasePlatform {
    pub os: String,
    pub architecture: String,
    pub variant: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedBase {
    /// A local reference to the image selected in the runtime's store.
    /// Podman and Docker use the immutable image ID for preparation. Docker
    /// addon builds give that ID a temporary tag for BuildKit's `FROM` lookup.
    /// Apple's `container` keeps the inspected name because a digest added to
    /// a tag may bypass its store.
    pub local_reference: String,
    pub platform: BasePlatform,
    /// Present only when the caller requested libc detection.
    pub libc: Option<Libc>,
}

#[derive(Debug)]
pub enum PrepareError {
    Io(std::io::Error),
    Command {
        operation: &'static str,
        exit_code: Option<i32>,
        stderr: String,
    },
    Inspection(String),
    LibcUnclear {
        image: String,
        glibc_found: bool,
        musl_found: bool,
    },
    ProbeAndCleanup {
        probe: Box<Self>,
        cleanup: Box<Self>,
    },
}

impl std::fmt::Display for PrepareError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "base image preparation I/O error: {error}"),
            Self::Command {
                operation,
                exit_code,
                stderr,
            } => {
                write!(f, "{operation} failed")?;
                if let Some(code) = exit_code {
                    write!(f, " with exit code {code}")?;
                }
                if !stderr.trim().is_empty() {
                    write!(f, ": {}", stderr.trim())?;
                }
                Ok(())
            }
            Self::Inspection(reason) => write!(f, "invalid base image inspection: {reason}"),
            Self::LibcUnclear {
                image,
                glibc_found,
                musl_found,
            } => write!(
                f,
                "cannot determine whether base image {image:?} uses glibc or musl \
                 (glibc marker: {glibc_found}, musl marker: {musl_found})"
            ),
            Self::ProbeAndCleanup { probe, cleanup } => {
                write!(f, "{probe}; cleanup also failed: {cleanup}")
            }
        }
    }
}

impl std::error::Error for PrepareError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::ProbeAndCleanup { probe, .. } => Some(probe),
            _ => None,
        }
    }
}

impl From<std::io::Error> for PrepareError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// Reuse a local base, or pull it once into `cli`'s image store. Inspection
/// reports the platform actually selected by that runtime. When `detect_libc`
/// is true, inspect a stopped container's flattened filesystem without running
/// any command inside the base image. The returned reference is suitable for
/// `FROM` in the selected runtime's local store.
pub fn prepare_base(
    image: &str,
    cli: &ContainerCli,
    runner: &dyn Runner,
    detect_libc: bool,
) -> Result<PreparedBase, PrepareError> {
    let first = inspect_image(image, cli, runner)?;
    let inspection = if first.status.success() {
        first.stdout
    } else {
        pull_image(image, cli, runner)?;
        checked_output("base image inspect", inspect_image(image, cli, runner)?)?
    };
    let (local_reference, platform) = parse_inspection(&inspection, image, cli)?;
    let libc = detect_libc
        .then(|| probe_libc(image, &local_reference, cli, runner))
        .transpose()?;
    Ok(PreparedBase {
        local_reference,
        platform,
        libc,
    })
}

fn inspect_image(
    image: &str,
    cli: &ContainerCli,
    runner: &dyn Runner,
) -> Result<Output, PrepareError> {
    let mut command = Command::new(cli.binary());
    command.args(["image", "inspect", image]);
    runner.output(&mut command).map_err(PrepareError::Io)
}

fn pull_image(image: &str, cli: &ContainerCli, runner: &dyn Runner) -> Result<(), PrepareError> {
    let mut command = Command::new(cli.binary());
    if *cli == ContainerCli::MacOsContainer {
        command.args(["image", "pull", image]);
    } else {
        command.args(["pull", image]);
    }
    checked_status("base image pull", runner.run(&mut command)?)
}

fn checked_output(operation: &'static str, output: Output) -> Result<Vec<u8>, PrepareError> {
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(PrepareError::Command {
            operation,
            exit_code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

fn checked_status(operation: &'static str, status: ExitStatus) -> Result<(), PrepareError> {
    if status.success() {
        Ok(())
    } else {
        Err(PrepareError::Command {
            operation,
            exit_code: status.code(),
            stderr: String::new(),
        })
    }
}

fn parse_inspection(
    json: &[u8],
    requested_image: &str,
    cli: &ContainerCli,
) -> Result<(String, BasePlatform), PrepareError> {
    let value: Value = serde_json::from_slice(json)
        .map_err(|error| PrepareError::Inspection(format!("invalid JSON: {error}")))?;
    let image = value
        .as_array()
        .and_then(|images| (images.len() == 1).then(|| &images[0]))
        .ok_or_else(|| PrepareError::Inspection("expected one inspected image".to_owned()))?;
    if *cli == ContainerCli::MacOsContainer {
        parse_apple_inspection(image, requested_image)
    } else {
        let id = required_string(image, &["Id", "ID"])?;
        validate_sha256_id(id)?;
        let platform = BasePlatform {
            os: required_string(image, &["Os", "OS"])?.to_owned(),
            architecture: required_string(image, &["Architecture", "Arch"])?.to_owned(),
            variant: optional_string(image, "Variant"),
        };
        Ok((id.to_owned(), platform))
    }
}

fn parse_apple_inspection(
    image: &Value,
    requested_image: &str,
) -> Result<(String, BasePlatform), PrepareError> {
    let architecture = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "amd64",
        other => other,
    };
    let variants = image
        .get("variants")
        .and_then(Value::as_array)
        .ok_or_else(|| PrepareError::Inspection("Apple image has no variants".to_owned()))?;
    let selected: Vec<_> = variants
        .iter()
        .filter(|variant| {
            variant
                .get("platform")
                .and_then(|platform| platform.get("os"))
                .and_then(Value::as_str)
                == Some("linux")
                && variant
                    .get("platform")
                    .and_then(|platform| platform.get("architecture"))
                    .and_then(Value::as_str)
                    == Some(architecture)
        })
        .collect();
    let [variant] = selected.as_slice() else {
        return Err(PrepareError::Inspection(format!(
            "expected exactly one linux/{architecture} variant, found {}",
            selected.len()
        )));
    };
    let digest = required_string(variant, &["digest"])?;
    validate_digest(digest)?;
    let platform = variant
        .get("platform")
        .ok_or_else(|| PrepareError::Inspection("variant has no platform".to_owned()))?;
    let platform = BasePlatform {
        os: "linux".to_owned(),
        architecture: architecture.to_owned(),
        variant: optional_string(platform, "variant"),
    };
    Ok((requested_image.to_owned(), platform))
}

fn required_string<'a>(value: &'a Value, keys: &[&str]) -> Result<&'a str, PrepareError> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .filter(|value| !value.is_empty())
        .ok_or_else(|| PrepareError::Inspection(format!("missing {}", keys.join(" or "))))
}

fn optional_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn validate_sha256_id(id: &str) -> Result<(), PrepareError> {
    let hex = id.strip_prefix("sha256:").unwrap_or(id);
    if hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(PrepareError::Inspection("invalid image ID".to_owned()))
    }
}

fn validate_digest(digest: &str) -> Result<(), PrepareError> {
    let hex = digest
        .strip_prefix("sha256:")
        .ok_or_else(|| PrepareError::Inspection("unsupported image digest".to_owned()))?;
    if hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(PrepareError::Inspection("invalid image digest".to_owned()))
    }
}

fn probe_libc(
    image: &str,
    local_reference: &str,
    cli: &ContainerCli,
    runner: &dyn Runner,
) -> Result<Libc, PrepareError> {
    let temp = tempfile::Builder::new().prefix("osbi-libc-").tempdir()?;
    let name = temp
        .path()
        .file_name()
        .expect("tempdir has a final component")
        .to_string_lossy();
    let mut create = Command::new(cli.binary());
    create.arg("create");
    match cli {
        ContainerCli::Podman | ContainerCli::Docker => {
            create.arg("--pull=never");
            create.args(["--name", &name, local_reference, "/bin/true"]);
        }
        ContainerCli::MacOsContainer => {
            // Apple creates rootfs.ext4 only when the container first starts.
            // /dev/null is a device and cannot execute image-controlled code.
            create.args([
                "--entrypoint",
                "/dev/null",
                "--name",
                &name,
                local_reference,
            ]);
        }
    }
    if let Err(error) = runner
        .run(&mut create)
        .map_err(PrepareError::Io)
        .and_then(|status| checked_status("base probe container create", status))
    {
        let _ = remove_probe_container(&name, cli, runner);
        return Err(error);
    }

    let result = if *cli == ContainerCli::MacOsContainer {
        materialize_apple_rootfs(&name, runner)
            .and_then(|()| export_and_scan(image, &name, temp.path(), cli, runner))
    } else {
        export_and_scan(image, &name, temp.path(), cli, runner)
    };
    let cleanup = remove_probe_container(&name, cli, runner);
    match (result, cleanup) {
        (Ok(libc), Ok(())) => Ok(libc),
        (Err(error), Ok(())) | (Ok(_), Err(error)) => Err(error),
        (Err(probe), Err(cleanup)) => Err(PrepareError::ProbeAndCleanup {
            probe: Box::new(probe),
            cleanup: Box::new(cleanup),
        }),
    }
}

fn materialize_apple_rootfs(name: &str, runner: &dyn Runner) -> Result<(), PrepareError> {
    let mut start = Command::new("container");
    start.args(["start", name]);
    let output = runner.output(&mut start)?;
    if output.status.success() {
        return Err(PrepareError::Inspection(
            "Apple base probe unexpectedly started its non-executable /dev/null entrypoint"
                .to_owned(),
        ));
    }
    // A failed process start still materializes the rootfs. Export it after
    // the VM stops; a separate export error will report a failed VM setup.
    Ok(())
}

fn remove_probe_container(
    name: &str,
    cli: &ContainerCli,
    runner: &dyn Runner,
) -> Result<(), PrepareError> {
    let mut remove = Command::new(cli.binary());
    if *cli == ContainerCli::MacOsContainer {
        remove.args(["delete", "--force", name]);
    } else {
        remove.args(["rm", "--volumes", name]);
    }
    runner
        .run(&mut remove)
        .map_err(PrepareError::Io)
        .and_then(|status| checked_status("base probe container removal", status))
}

fn export_and_scan(
    image: &str,
    container_name: &str,
    temp_dir: &Path,
    cli: &ContainerCli,
    runner: &dyn Runner,
) -> Result<Libc, PrepareError> {
    let archive = temp_dir.join("rootfs.tar");
    let mut export = Command::new(cli.binary());
    if *cli == ContainerCli::MacOsContainer {
        export.arg("export").arg("-o");
    } else {
        export.arg("export").arg("--output");
    }
    export.arg(&archive).arg(container_name);
    checked_status("base probe container export", runner.run(&mut export)?)?;
    detect_libc_in_archive(image, &archive)
}

fn detect_libc_in_archive(image: &str, archive: &Path) -> Result<Libc, PrepareError> {
    let file = File::open(archive)?;
    let mut tar = tar::Archive::new(file);
    let mut glibc_found = false;
    let mut musl_found = false;
    for entry in tar.entries()? {
        let entry = entry?;
        if !entry.header().entry_type().is_file()
            && !entry.header().entry_type().is_symlink()
            && !entry.header().entry_type().is_hard_link()
        {
            continue;
        }
        let path = entry.path()?;
        let path = path.as_ref();
        match libc_marker(path) {
            Some(Libc::Glibc) => glibc_found = true,
            Some(Libc::Musl) => musl_found = true,
            None => {}
        }
    }
    match (glibc_found, musl_found) {
        (true, false) => Ok(Libc::Glibc),
        (false, true) => Ok(Libc::Musl),
        _ => Err(PrepareError::LibcUnclear {
            image: image.to_owned(),
            glibc_found,
            musl_found,
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::fs::File;
    use std::io;
    use std::process::{Command, ExitStatus, Output};

    use super::*;

    struct FakeRunner {
        inspections: RefCell<VecDeque<Output>>,
        commands: RefCell<Vec<Vec<String>>>,
        markers: Vec<&'static str>,
        fail_create: bool,
        fail_export: bool,
        apple_start_succeeds: bool,
    }

    impl FakeRunner {
        fn new(inspections: Vec<Output>, markers: Vec<&'static str>) -> Self {
            Self {
                inspections: RefCell::new(inspections.into()),
                commands: RefCell::new(Vec::new()),
                markers,
                fail_create: false,
                fail_export: false,
                apple_start_succeeds: false,
            }
        }

        fn commands(&self) -> Vec<Vec<String>> {
            self.commands.borrow().clone()
        }
    }

    impl Runner for FakeRunner {
        fn run(&self, cmd: &mut Command) -> io::Result<ExitStatus> {
            let args = command_args(cmd);
            if args[1] == "export" && !self.fail_export {
                let archive = Path::new(&args[3]);
                let mut builder = tar::Builder::new(File::create(archive)?);
                for path in &self.markers {
                    let mut header = tar::Header::new_gnu();
                    header.set_size(0);
                    header.set_mode(0o644);
                    header.set_cksum();
                    builder.append_data(&mut header, path, io::empty())?;
                }
                builder.finish()?;
            }
            self.commands.borrow_mut().push(args.clone());
            Ok(exit_with(i32::from(
                (args[1] == "create" && self.fail_create)
                    || (args[1] == "export" && self.fail_export),
            )))
        }

        fn output(&self, cmd: &mut Command) -> io::Result<Output> {
            let args = command_args(cmd);
            self.commands.borrow_mut().push(args.clone());
            if args[1] == "start" {
                return Ok(Output {
                    status: exit_with(i32::from(!self.apple_start_succeeds)),
                    stdout: Vec::new(),
                    stderr: b"/dev/null is not executable".to_vec(),
                });
            }
            self.inspections
                .borrow_mut()
                .pop_front()
                .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "no inspection"))
        }
    }

    fn command_args(cmd: &Command) -> Vec<String> {
        std::iter::once(cmd.get_program())
            .chain(cmd.get_args())
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    fn exit_with(code: i32) -> ExitStatus {
        Command::new("sh")
            .args(["-c", &format!("exit {code}")])
            .status()
            .unwrap()
    }

    fn inspection(value: Value, code: i32) -> Output {
        Output {
            status: exit_with(code),
            stdout: serde_json::to_vec(&value).unwrap(),
            stderr: Vec::new(),
        }
    }

    fn local_inspection(id: &str, architecture: &str) -> Output {
        inspection(
            serde_json::json!([{
                "Id": id,
                "Os": "linux",
                "Architecture": architecture
            }]),
            0,
        )
    }

    #[test]
    fn reuses_local_base_without_libc_probe() {
        let id = "a".repeat(64);
        let runner = FakeRunner::new(vec![local_inspection(&id, "arm64")], vec![]);

        let prepared = prepare_base("example:latest", &ContainerCli::Podman, &runner, false)
            .expect("local base is prepared");

        assert_eq!(prepared.local_reference, id);
        assert_eq!(prepared.platform.os, "linux");
        assert_eq!(prepared.platform.architecture, "arm64");
        assert_eq!(prepared.libc, None);
        assert_eq!(
            runner.commands(),
            vec![vec!["podman", "image", "inspect", "example:latest"]]
        );
    }

    #[test]
    fn pulls_missing_base_once_and_probes_glibc_without_running_it() {
        let id = "b".repeat(64);
        let runner = FakeRunner::new(
            vec![inspection(Value::Null, 1), local_inspection(&id, "amd64")],
            vec!["usr/lib/x86_64-linux-gnu/libc.so.6"],
        );

        let prepared = prepare_base("example:latest", &ContainerCli::Podman, &runner, true)
            .expect("pulled base is prepared");

        assert_eq!(prepared.libc, Some(Libc::Glibc));
        assert_eq!(prepared.local_reference, id);
        let commands = runner.commands();
        assert_eq!(commands.len(), 6);
        assert_eq!(
            commands[0],
            ["podman", "image", "inspect", "example:latest"]
        );
        assert_eq!(commands[1], ["podman", "pull", "example:latest"]);
        assert_eq!(commands[2], commands[0]);
        assert_eq!(
            &commands[3][..4],
            ["podman", "create", "--pull=never", "--name"]
        );
        assert_eq!(commands[3][5], id);
        assert_eq!(commands[3][6], "/bin/true");
        assert_eq!(&commands[4][..3], ["podman", "export", "--output"]);
        assert_eq!(commands[4][4], commands[3][4]);
        assert_eq!(commands[5], ["podman", "rm", "--volumes", &commands[3][4]]);
        assert!(!Path::new(&commands[4][3]).exists());
    }

    #[test]
    fn docker_probe_detects_musl_and_removes_container() {
        let id = format!("sha256:{}", "c".repeat(64));
        let runner = FakeRunner::new(
            vec![local_inspection(&id, "amd64")],
            vec!["lib/ld-musl-x86_64.so.1"],
        );

        let prepared = prepare_base("example:latest", &ContainerCli::Docker, &runner, true)
            .expect("Docker base is prepared");

        assert_eq!(prepared.libc, Some(Libc::Musl));
        let commands = runner.commands();
        assert_eq!(&commands[1][..3], ["docker", "create", "--pull=never"]);
        assert_eq!(commands[3], ["docker", "rm", "--volumes", &commands[1][4]]);
    }

    #[test]
    fn apple_keeps_local_name_and_uses_local_export() {
        let digest = format!("sha256:{}", "d".repeat(64));
        let architecture = match std::env::consts::ARCH {
            "aarch64" => "arm64",
            "x86_64" => "amd64",
            other => other,
        };
        let runner = FakeRunner::new(
            vec![
                inspection(Value::Null, 1),
                inspection(
                    serde_json::json!([{
                        "variants": [{
                            "digest": digest,
                            "platform": {"os": "linux", "architecture": architecture}
                        }]
                    }]),
                    0,
                ),
            ],
            vec!["lib/ld-musl-aarch64.so.1"],
        );

        let prepared = prepare_base(
            "registry.example:5000/base:latest",
            &ContainerCli::MacOsContainer,
            &runner,
            true,
        )
        .expect("Apple base is prepared");

        assert_eq!(
            prepared.local_reference,
            "registry.example:5000/base:latest"
        );
        assert_eq!(prepared.platform.architecture, architecture);
        assert_eq!(prepared.libc, Some(Libc::Musl));
        let commands = runner.commands();
        assert_eq!(
            commands[0],
            [
                "container",
                "image",
                "inspect",
                "registry.example:5000/base:latest"
            ]
        );
        assert_eq!(
            commands[1],
            [
                "container",
                "image",
                "pull",
                "registry.example:5000/base:latest"
            ]
        );
        assert_eq!(commands[2], commands[0]);
        assert_eq!(
            &commands[3][..5],
            ["container", "create", "--entrypoint", "/dev/null", "--name"]
        );
        assert_eq!(commands[4], ["container", "start", &commands[3][5]]);
        assert_eq!(&commands[5][..3], ["container", "export", "-o"]);
        assert_eq!(
            commands[6],
            ["container", "delete", "--force", &commands[3][5]]
        );
    }

    #[test]
    fn apple_probe_rejects_an_entrypoint_that_starts() {
        let architecture = match std::env::consts::ARCH {
            "aarch64" => "arm64",
            "x86_64" => "amd64",
            other => other,
        };
        let digest = format!("sha256:{}", "d".repeat(64));
        let mut runner = FakeRunner::new(
            vec![inspection(
                serde_json::json!([{
                    "variants": [{
                        "digest": digest,
                        "platform": {"os": "linux", "architecture": architecture}
                    }]
                }]),
                0,
            )],
            vec![],
        );
        runner.apple_start_succeeds = true;

        let error = prepare_base(
            "example:latest",
            &ContainerCli::MacOsContainer,
            &runner,
            true,
        )
        .expect_err("an executable probe entrypoint must be rejected");

        assert!(matches!(error, PrepareError::Inspection(_)));
        let commands = runner.commands();
        assert_eq!(commands[2][..2], ["container", "start"]);
        assert_eq!(commands[3][..2], ["container", "delete"]);
    }

    #[test]
    fn ambiguous_libc_still_removes_probe_container() {
        let id = "e".repeat(64);
        let runner = FakeRunner::new(
            vec![local_inspection(&id, "amd64")],
            vec!["lib/libc.so.6", "lib/ld-musl-x86_64.so.1"],
        );

        let error = prepare_base("example:latest", &ContainerCli::Podman, &runner, true)
            .expect_err("mixed libc markers are ambiguous");

        assert!(matches!(
            error,
            PrepareError::LibcUnclear {
                glibc_found: true,
                musl_found: true,
                ..
            }
        ));
        let commands = runner.commands();
        assert_eq!(commands.last().unwrap()[..2], ["podman", "rm"]);
    }

    #[test]
    fn failed_export_still_removes_probe_container() {
        let id = "f".repeat(64);
        let mut runner = FakeRunner::new(vec![local_inspection(&id, "amd64")], vec![]);
        runner.fail_export = true;

        let error = prepare_base("example:latest", &ContainerCli::Podman, &runner, true)
            .expect_err("export failure is returned");

        assert!(matches!(
            error,
            PrepareError::Command {
                operation: "base probe container export",
                ..
            }
        ));
        let commands = runner.commands();
        assert_eq!(commands.last().unwrap()[..2], ["podman", "rm"]);
    }

    #[test]
    fn failed_create_attempts_to_remove_partial_container() {
        let id = "f".repeat(64);
        let mut runner = FakeRunner::new(vec![local_inspection(&id, "amd64")], vec![]);
        runner.fail_create = true;

        let error = prepare_base("example:latest", &ContainerCli::Podman, &runner, true)
            .expect_err("create failure is returned");

        assert!(matches!(
            error,
            PrepareError::Command {
                operation: "base probe container create",
                ..
            }
        ));
        let commands = runner.commands();
        assert_eq!(commands.len(), 3);
        assert_eq!(commands[2], ["podman", "rm", "--volumes", &commands[1][4]]);
    }
}
