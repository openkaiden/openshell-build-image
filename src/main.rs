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

mod backend;
mod host_addon;
mod registry;
mod vm_rootfs;

use std::net::IpAddr;
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;

use clap::Parser;
use container_image_builder::{ContainerCli, ContainerRunner};
use log::LevelFilter;
use openshell_build_image::containerfile;
use vm_image_builder::{KrunRunner, VmConfig, VmRunner};

use crate::backend::{
    AppleContainerBackend, BuildError, BuildRequest, BuildResult, DockerBackend, ImageBackend,
    PodmanBackend, VmBackend,
};

/// Selects how images are built.
///
/// The first three variants drive a container CLI installed on the host; `Vm`
/// builds inside a microVM instead and produces a rootfs tarball rather than an
/// image in a local image store.
///
/// This enum is local to the binary so that the library crates
/// (`container-image-builder`, `vm-image-builder`) have no dependency on
/// `clap`. [`Runtime::container_cli`] converts to [`ContainerCli`] after
/// argument parsing.
#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
enum Runtime {
    Podman,
    Docker,
    /// Apple's `container` CLI (macOS only).
    #[value(name = "container")]
    MacOsContainer,
    /// A libkrun microVM (macOS on Apple Silicon only).
    Vm,
}

impl Runtime {
    /// Returns the container CLI this runtime drives, or `None` for
    /// [`Runtime::Vm`], which builds in a microVM instead of shelling out.
    fn container_cli(self) -> Option<ContainerCli> {
        match self {
            Runtime::Podman => Some(ContainerCli::Podman),
            Runtime::Docker => Some(ContainerCli::Docker),
            Runtime::MacOsContainer => Some(ContainerCli::MacOsContainer),
            Runtime::Vm => None,
        }
    }
}

/// Test adapter that keeps runner injection concise in the CLI tests.
#[cfg(test)]
enum Backend<'a> {
    Cli(&'a ContainerCli, &'a dyn container_image_builder::Runner),
    Vm(&'a VmConfig, &'a dyn VmRunner, &'a Path),
}

#[cfg(test)]
impl ImageBackend for Backend<'_> {
    fn check_ready(&self) -> Result<(), BuildError> {
        match self {
            Backend::Cli(ContainerCli::Podman, runner) => {
                PodmanBackend { runner: *runner }.check_ready()
            }
            Backend::Cli(ContainerCli::Docker, runner) => {
                DockerBackend { runner: *runner }.check_ready()
            }
            Backend::Cli(ContainerCli::MacOsContainer, runner) => {
                AppleContainerBackend { runner: *runner }.check_ready()
            }
            Backend::Vm(config, runner, output) => VmBackend {
                config,
                runner: *runner,
                output,
            }
            .check_ready(),
        }
    }

    fn build(&self, request: &BuildRequest<'_>) -> Result<BuildResult, BuildError> {
        match self {
            Backend::Cli(ContainerCli::Podman, runner) => {
                PodmanBackend { runner: *runner }.build(request)
            }
            Backend::Cli(ContainerCli::Docker, runner) => {
                DockerBackend { runner: *runner }.build(request)
            }
            Backend::Cli(ContainerCli::MacOsContainer, runner) => {
                AppleContainerBackend { runner: *runner }.build(request)
            }
            Backend::Vm(config, runner, output) => VmBackend {
                config,
                runner: *runner,
                output,
            }
            .build(request),
        }
    }
}

/// What `--runtime` resolved to, owning the values a backend borrows.
///
/// The VM variant carries its output path because that is settled while the
/// runtime is chosen — from `--vm-output`, or derived from the tag.
enum Selected {
    Cli(ContainerCli),
    Vm(VmConfig, PathBuf),
}

#[derive(Parser)]
#[command(
    name = "openshell-build-image",
    version,
    about = "OpenShell image builder"
)]
struct Cli {
    #[arg(help = "Tag for the built image (e.g. myimage:latest)")]
    tag: String,
    #[arg(
        long,
        value_enum,
        help = "Backend to build the image with (podman, docker, container, vm)"
    )]
    runtime: Runtime,
    #[arg(
        long,
        value_name = "IMAGE",
        value_parser = containerfile::parse_image_reference,
        help = "Project image to build from (name, tag, or digest)"
    )]
    from: String,
    #[arg(
        long,
        value_name = "IMAGE",
        value_parser = containerfile::parse_image_reference,
        help = "OCI addon image to copy into the built image (repeatable, in order)"
    )]
    addon: Vec<String>,
    #[arg(
        short = 'v',
        action = clap::ArgAction::Count,
        help = "Increase log verbosity (-v info, -vv debug)"
    )]
    verbose: u8,
    #[arg(
        long,
        help = "Copy a Containerfile describing the build to $HOME/Containerfile inside the image"
    )]
    copy_containerfile: bool,
    #[arg(
        long = "vm-rootfs",
        value_name = "DIR",
        help = "Root filesystem the build VM boots from (--runtime vm only). \
                Defaults to the one embedded in this binary."
    )]
    vm_rootfs: Option<PathBuf>,
    #[arg(
        long = "vm-output",
        value_name = "FILE",
        help = "Path for the rootfs tarball produced by --runtime vm. \
                Defaults to a name derived from <TAG> in the current directory."
    )]
    vm_output: Option<PathBuf>,
    // libkrun rejects a zero vCPU count and a zero memory size, so clap turns
    // those into a flag-specific parse error rather than a generic VM
    // configuration failure after the build context has been staged.
    #[arg(
        long = "vm-cpus",
        value_name = "N",
        value_parser = clap::value_parser!(u8).range(1..),
        help = "vCPUs given to the build VM (--runtime vm only)."
    )]
    vm_cpus: Option<u8>,
    #[arg(
        long = "vm-memory",
        value_name = "MIB",
        value_parser = clap::value_parser!(u32).range(1..),
        help = "RAM in MiB given to the build VM (--runtime vm only)."
    )]
    vm_memory: Option<u32>,
    #[arg(
        long = "vm-dns",
        value_name = "ADDR",
        help = "Nameserver the build VM resolves through (--runtime vm only). \
                Repeatable. Defaults to the host's own nameservers."
    )]
    vm_dns: Vec<IpAddr>,
}

fn main() {
    let cli = Cli::parse();
    // TODO: when JSON output is added, logs written to stderr may interfere with
    // structured output — revisit whether logs should be suppressed or embedded in the JSON.
    let log_level = match cli.verbose {
        0 => LevelFilter::Warn,
        1 => LevelFilter::Info,
        _ => LevelFilter::Debug,
    };
    env_logger::Builder::new().filter_level(log_level).init();

    if let Err(e) = check_vm_flags(&cli) {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }

    // Backends borrow their VM configuration, so the selected values must
    // outlive the backend.
    let selected = match select_runtime(&cli) {
        Ok(selected) => selected,
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    };
    let backend = backend_for(&selected, &ContainerRunner, &KrunRunner);
    if let Err(e) = backend.check_ready() {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }

    let result = run(
        &cli.tag,
        &cli.from,
        &cli.addon,
        cli.copy_containerfile,
        backend.as_ref(),
    );
    let result = match result {
        Ok(result) => result,
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    };

    if let Some(summary) = build_summary(&result) {
        println!("{summary}");
    }
}

/// Rejects the `--vm-*` flags when the selected runtime is not `vm`.
///
/// They configure a backend that is not in use, so accepting them silently
/// would hide a mistake in the command line.
fn check_vm_flags(cli: &Cli) -> Result<(), String> {
    if cli.runtime == Runtime::Vm {
        return Ok(());
    }
    let given = [
        ("--vm-rootfs", cli.vm_rootfs.is_some()),
        ("--vm-output", cli.vm_output.is_some()),
        ("--vm-cpus", cli.vm_cpus.is_some()),
        ("--vm-memory", cli.vm_memory.is_some()),
        ("--vm-dns", !cli.vm_dns.is_empty()),
    ];
    match given.iter().find(|(_, present)| *present) {
        Some((flag, _)) => Err(format!("{flag} is only supported with --runtime vm")),
        None => Ok(()),
    }
}

/// Assembles the VM configuration, filling in the defaults for anything the
/// user did not pass.
///
/// Without `--vm-rootfs` the VM boots from the rootfs embedded in this binary,
/// unpacked on first use, so that `--runtime vm` needs nothing of the user
/// beyond the binary itself. Without `--vm-dns` it resolves through the host's
/// own nameservers.
///
/// # Errors
///
/// Returns the reason the embedded rootfs could not be made available, when
/// the caller passed no rootfs of their own, or the reason a `--vm-dns` address
/// cannot be used.
fn vm_config(
    rootfs: Option<PathBuf>,
    cpus: Option<u8>,
    memory: Option<u32>,
    dns: &[IpAddr],
) -> Result<VmConfig, String> {
    // Before the rootfs, which on a default run means unpacking the embedded
    // one: a mistyped --vm-dns should not first cost 80 MiB of extraction.
    let nameservers = vm_nameservers(dns)?;
    let rootfs = match rootfs {
        Some(rootfs) => rootfs,
        None => vm_rootfs::ensure_extracted()?,
    };
    Ok(VmConfig {
        rootfs,
        cpus: cpus.unwrap_or(vm_image_builder::DEFAULT_CPUS),
        memory_mib: memory.unwrap_or(vm_image_builder::DEFAULT_MEMORY_MIB),
        nameservers,
    })
}

/// Resolves the nameservers the VM uses: `--vm-dns` if given, the host's own
/// otherwise.
///
/// An address the guest cannot reach is rejected rather than dropped — the
/// user named it, so saying nothing would look like it had been honoured.
fn vm_nameservers(dns: &[IpAddr]) -> Result<Vec<IpAddr>, String> {
    if dns.is_empty() {
        return Ok(vm_image_builder::dns::default_nameservers());
    }
    if let Some(addr) = dns
        .iter()
        .find(|addr| !vm_image_builder::dns::is_reachable_from_vm(addr))
    {
        return Err(format!(
            "--vm-dns {addr} cannot be reached from inside the VM: a loopback or \
             link-local address means the VM itself, not the host"
        ));
    }
    Ok(dns.to_vec())
}

/// Derives the default tarball name for a VM build from the image tag.
///
/// A tag can hold characters that are awkward or illegal in a filename (`:` in
/// every tag, `/` in any registry-qualified name), so everything outside a
/// conservative set is replaced with `-`: `ghcr.io/me/app:1.0` becomes
/// `ghcr.io-me-app-1.0.tar`.
fn vm_output_path(tag: &str) -> PathBuf {
    let name: String = tag
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    PathBuf::from(format!("{name}.tar"))
}

/// Resolve `--runtime` and its flags before constructing a backend. VM support
/// is checked before extracting an embedded rootfs, which can be expensive.
fn select_runtime(cli: &Cli) -> Result<Selected, String> {
    match cli.runtime.container_cli() {
        Some(container_cli) => Ok(Selected::Cli(container_cli)),
        None => {
            // Before the rootfs, which on a default run means unpacking the
            // embedded one: an unsupported host, or a binary built without the
            // `vm` feature, should not first cost 80 MiB of extraction.
            KrunRunner.check_supported().map_err(|e| e.to_string())?;
            select_vm(cli)
        }
    }
}

/// Resolves what `--runtime vm` builds through, once the host is known to be
/// able to boot a VM at all.
///
/// Split out of [`select_runtime`] so that the resolution can be tested on a
/// host — or a build — where [`KrunRunner`] reports itself unsupported, which
/// is every machine that is not an Apple Silicon Mac with the `vm` feature on.
fn select_vm(cli: &Cli) -> Result<Selected, String> {
    let config = vm_config(
        cli.vm_rootfs.clone(),
        cli.vm_cpus,
        cli.vm_memory,
        &cli.vm_dns,
    )?;
    let output = cli
        .vm_output
        .clone()
        .unwrap_or_else(|| vm_output_path(&cli.tag));
    Ok(Selected::Vm(config, output))
}

/// Construct the selected complete-build implementation once.
fn backend_for<'a>(
    selected: &'a Selected,
    runner: &'a dyn container_image_builder::Runner,
    vm_runner: &'a dyn VmRunner,
) -> Box<dyn ImageBackend + 'a> {
    match selected {
        Selected::Cli(ContainerCli::Podman) => Box::new(PodmanBackend { runner }),
        Selected::Cli(ContainerCli::Docker) => Box::new(DockerBackend { runner }),
        Selected::Cli(ContainerCli::MacOsContainer) => Box::new(AppleContainerBackend { runner }),
        Selected::Vm(config, output) => Box::new(VmBackend {
            config,
            runner: vm_runner,
            output,
        }),
    }
}

/// What to report after a successful build, if anything.
///
/// A CLI build leaves a tagged image the user can look up, so there is nothing
/// to add; a VM build leaves a file, so say where it landed.
fn build_summary(result: &BuildResult) -> Option<String> {
    match result {
        BuildResult::Image { .. } => None,
        BuildResult::VmArtifact { rootfs_tar, config } => Some(match config {
            Some(config) => format!("Wrote {} and {}", rootfs_tar.display(), config.display()),
            None => format!("Wrote {}", rootfs_tar.display()),
        }),
    }
}

fn run(
    tag: &str,
    from: &str,
    addons: &[String],
    copy_containerfile: bool,
    backend: &dyn ImageBackend,
) -> Result<BuildResult, BuildError> {
    backend.build(&BuildRequest {
        from,
        addons,
        tag,
        copy_containerfile,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;
    use container_image_builder::Runner;
    use std::process::{Command, ExitStatus};

    struct FakeRunner(i32);

    impl Runner for FakeRunner {
        fn run(&self, _cmd: &mut Command) -> std::io::Result<ExitStatus> {
            Ok(Command::new("sh")
                .args(["-c", &format!("exit {}", self.0)])
                .status()?)
        }
    }

    // Reads the Containerfile written to the `-f <path>` temp file and stores its content.
    struct ContainerfileCapture(std::sync::Mutex<String>);

    impl Runner for ContainerfileCapture {
        fn run(&self, cmd: &mut Command) -> std::io::Result<ExitStatus> {
            let args: Vec<_> = cmd
                .get_args()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            if let Some(idx) = args.iter().position(|a| a == "-f") {
                if let Some(path) = args.get(idx + 1) {
                    *self.0.lock().unwrap() = std::fs::read_to_string(path)?;
                }
            }
            Ok(Command::new("sh").args(["-c", "exit 0"]).status()?)
        }
    }

    // Stands in for the microVM: records the build it was handed so tests can
    // assert on it without libkrun, which is unavailable in CI.
    struct FakeVmRunner(
        std::sync::Mutex<Option<(vm_image_builder::VmBuild, String, serde_json::Value)>>,
    );

    impl FakeVmRunner {
        fn new() -> Self {
            FakeVmRunner(std::sync::Mutex::new(None))
        }

        fn captured(&self) -> vm_image_builder::VmBuild {
            self.0
                .lock()
                .unwrap()
                .clone()
                .expect("VM runner was not called")
                .0
        }

        /// The Containerfile the VM would have read from the context share.
        fn containerfile(&self) -> String {
            self.0
                .lock()
                .unwrap()
                .clone()
                .expect("VM runner was not called")
                .1
        }

        fn request(&self) -> serde_json::Value {
            self.0
                .lock()
                .unwrap()
                .clone()
                .expect("VM runner was not called")
                .2
        }
    }

    impl VmRunner for FakeVmRunner {
        fn run(
            &self,
            build: &vm_image_builder::VmBuild,
        ) -> Result<(), vm_image_builder::VmBuildError> {
            // `run` deletes the context directory as soon as it returns, so the
            // Containerfile has to be read here, while the VM would see it.
            let containerfile = std::fs::read_to_string(build.context.join("Containerfile"))?;
            let request: serde_json::Value =
                serde_json::from_slice(&std::fs::read(build.context.join("build-request.json"))?)
                    .unwrap();
            assert_eq!(request["version"], 1);
            assert_eq!(request["tag"], build.tag);
            assert_eq!(
                build.request_vm_path().as_deref(),
                Some("/build/context/build-request.json")
            );
            if containerfile.contains("COPY build-containerfile") {
                assert_eq!(
                    std::fs::read_to_string(build.context.join("build-containerfile"))?,
                    containerfile
                );
            }
            *self.0.lock().unwrap() = Some((build.clone(), containerfile, request));
            Ok(())
        }
    }

    /// Builds a directory that passes `VmConfig::check_rootfs`.
    fn fake_vm_rootfs(dir: &Path) -> PathBuf {
        let rootfs = dir.join("vm-rootfs");
        let bin = rootfs.join("usr/local/bin");
        std::fs::create_dir_all(&bin).unwrap();
        let helper = bin.join("vm-build");
        std::fs::write(&helper, "#!/bin/sh\n").unwrap();
        let resolver = bin.join("vm-guest-helper");
        std::fs::write(&resolver, "#!/bin/sh\n").unwrap();
        // `check_rootfs` requires the execute bit, as libkrun exec's the
        // helper. Only Unix has one, and only there does the check run.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).unwrap();
            std::fs::set_permissions(&resolver, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        rootfs
    }

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    /// Parses `args` as a full command line, with the binary name prepended.
    fn parse_cli(args: &[&str]) -> Result<Cli, clap::Error> {
        let mut argv = vec!["openshell-build-image", "--from", "alpine:3.24"];
        argv.extend_from_slice(args);
        Cli::try_parse_from(argv)
    }

    #[test]
    fn version_matches_cargo_toml() {
        let cmd = Cli::command();
        assert_eq!(cmd.get_version(), Some(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn from_is_required() {
        let err = Cli::try_parse_from(["test", "--runtime", "podman", "test:latest"])
            .err()
            .expect("missing --from should fail");
        assert_eq!(err.kind(), clap::error::ErrorKind::MissingRequiredArgument);
        assert!(err.to_string().contains("--from <IMAGE>"));
    }

    #[test]
    fn from_accepts_project_images_for_every_runtime() {
        for runtime in ["podman", "docker", "container", "vm"] {
            let image = "localhost:5000/project/tools@sha256:0123456789abcdef";
            let cli =
                Cli::try_parse_from(["test", "--runtime", runtime, "--from", image, "test:latest"])
                    .unwrap();
            assert_eq!(cli.from, image);
        }
    }

    #[test]
    fn from_rejects_empty_and_multiline_values() {
        for image in ["", "alpine AS injected", "alpine\nRUN touch /injected"] {
            let err = Cli::try_parse_from([
                "test",
                "--runtime",
                "podman",
                "--from",
                image,
                "test:latest",
            ])
            .err()
            .expect("invalid --from should fail");
            assert_eq!(err.kind(), clap::error::ErrorKind::ValueValidation);
        }
    }

    #[test]
    fn addon_accepts_zero_one_or_many_images_for_every_runtime() {
        for runtime in ["podman", "docker", "container", "vm"] {
            let first = "ghcr.io/example/first@sha256:0123456789abcdef";
            let second = "ghcr.io/example/second:latest";
            let base_args = [
                "test",
                "--runtime",
                runtime,
                "--from",
                "alpine:3.24",
                "test:latest",
            ];
            assert!(Cli::try_parse_from(base_args).unwrap().addon.is_empty());

            let one = Cli::try_parse_from([
                "test",
                "--runtime",
                runtime,
                "--from",
                "alpine:3.24",
                "--addon",
                first,
                "test:latest",
            ])
            .unwrap();
            assert_eq!(one.addon, [first]);

            let many = Cli::try_parse_from([
                "test",
                "--runtime",
                runtime,
                "--from",
                "alpine:3.24",
                "--addon",
                first,
                "--addon",
                second,
                "--addon",
                first,
                "test:latest",
            ])
            .unwrap();
            assert_eq!(many.addon, [first, second, first]);
        }
    }

    #[test]
    fn addon_rejects_empty_and_multiline_values() {
        for image in ["", "addon AS injected", "addon\nRUN touch /injected"] {
            let err = Cli::try_parse_from([
                "test",
                "--runtime",
                "podman",
                "--from",
                "alpine:3.24",
                "--addon",
                image,
                "test:latest",
            ])
            .err()
            .expect("invalid --addon should fail");
            assert_eq!(err.kind(), clap::error::ErrorKind::ValueValidation);
        }
    }

    #[test]
    fn vm_addon_request_reaches_the_guest_in_order() {
        let temp = tempfile::tempdir().unwrap();
        let rootfs = fake_vm_rootfs(temp.path());
        let config = VmConfig {
            rootfs,
            cpus: 2,
            memory_mib: 1024,
            nameservers: vec![],
        };
        let output = temp.path().join("output.tar");
        let runner = FakeVmRunner::new();
        let result = run(
            "test:latest",
            "alpine:3.24",
            &[
                "ghcr.io/example/addon:latest".to_owned(),
                "ghcr.io/example/second:latest".to_owned(),
                "ghcr.io/example/addon:latest".to_owned(),
            ],
            false,
            &Backend::Vm(&config, &runner, &output),
        );
        assert_eq!(
            result.unwrap(),
            BuildResult::VmArtifact {
                rootfs_tar: output.clone(),
                config: Some(PathBuf::from(format!("{}.config.json", output.display()))),
            }
        );
        let build = runner.captured();
        assert_eq!(
            build.request_vm_path().as_deref(),
            Some("/build/context/build-request.json")
        );
        assert_eq!(
            runner.request()["addons"],
            serde_json::json!([
                "ghcr.io/example/addon:latest",
                "ghcr.io/example/second:latest",
                "ghcr.io/example/addon:latest"
            ])
        );
    }

    #[test]
    fn vm_addon_requires_a_guest_resolver_in_the_rootfs() {
        let temp = tempfile::tempdir().unwrap();
        let rootfs = fake_vm_rootfs(temp.path());
        std::fs::remove_file(rootfs.join("usr/local/bin/vm-guest-helper")).unwrap();
        let config = VmConfig::new(&rootfs);
        let output = temp.path().join("output.tar");
        let runner = FakeVmRunner::new();
        let error = run(
            "test:latest",
            "alpine:3.24",
            &["ghcr.io/example/addon:latest".to_owned()],
            false,
            &Backend::Vm(&config, &runner, &output),
        )
        .unwrap_err();
        assert!(error.to_string().contains("rebuild it to use --addon"));
    }

    #[test]
    fn run_passes_project_image_to_cli_backend() {
        let capture = ContainerfileCapture(std::sync::Mutex::new(String::new()));
        let image = "localhost:5000/project/tools:dev";
        run(
            "test:latest",
            image,
            &[],
            false,
            &Backend::Cli(&ContainerCli::Podman, &capture),
        )
        .unwrap();
        assert!(
            capture
                .0
                .into_inner()
                .unwrap()
                .contains(&format!("FROM {image} AS system"))
        );
    }

    #[test]
    fn copy_containerfile_flag_is_opt_in() {
        let cli = Cli::try_parse_from([
            "test",
            "--runtime",
            "podman",
            "--from",
            "alpine:3.24",
            "test:latest",
        ])
        .unwrap();
        assert!(!cli.copy_containerfile);
        let cli = Cli::try_parse_from([
            "test",
            "--from",
            "alpine:3.24",
            "--runtime",
            "podman",
            "--copy-containerfile",
            "test:latest",
        ])
        .unwrap();
        assert!(cli.copy_containerfile);
    }

    #[test]
    fn run_stages_exact_build_containerfile_only_when_requested() {
        struct CopyChecker(bool);

        impl Runner for CopyChecker {
            fn run(&self, cmd: &mut Command) -> std::io::Result<ExitStatus> {
                let args: Vec<_> = cmd.get_args().collect();
                let file_index = args.iter().position(|arg| *arg == "-f").unwrap() + 1;
                let used = std::fs::read_to_string(args[file_index])?;
                let context = Path::new(args.last().unwrap());
                assert!(!context.join("certs").exists());
                let staged = context.join("build-containerfile");
                assert_eq!(staged.exists(), self.0);
                if self.0 {
                    assert_eq!(std::fs::read_to_string(staged)?, used);
                    assert!(used.contains("COPY build-containerfile /tmp/build-containerfile"));
                } else {
                    assert!(!used.contains("build-containerfile"));
                }
                FakeRunner(0).run(cmd)
            }
        }

        for enabled in [false, true] {
            run(
                "test:latest",
                "ghcr.io/example/project:dev",
                &[],
                enabled,
                &Backend::Cli(&ContainerCli::Podman, &CopyChecker(enabled)),
            )
            .unwrap();
        }
    }

    // run

    #[test]
    fn run_with_base_image_succeeds() {
        let result = run(
            "test:latest",
            "ghcr.io/example/project:dev",
            &[],
            false,
            &Backend::Cli(&ContainerCli::Podman, &FakeRunner(0)),
        );
        assert!(result.is_ok(), "expected Ok, got {result:?}");
    }

    #[test]
    fn run_returns_error_when_runner_fails() {
        let result = run(
            "test:latest",
            "ghcr.io/example/project:dev",
            &[],
            false,
            &Backend::Cli(&ContainerCli::Podman, &FakeRunner(1)),
        );
        assert!(result.is_err());
    }

    #[test]
    fn docker_addon_build_uses_a_local_base_tag_and_cleans_it_up() {
        use std::cell::RefCell;
        use std::process::Output;

        struct DockerCapture {
            commands: RefCell<Vec<Vec<String>>>,
            containerfile: RefCell<String>,
            fail_build: bool,
        }

        impl Runner for DockerCapture {
            fn run(&self, cmd: &mut Command) -> std::io::Result<ExitStatus> {
                let args: Vec<String> = std::iter::once(cmd.get_program())
                    .chain(cmd.get_args())
                    .map(|arg| arg.to_string_lossy().into_owned())
                    .collect();
                if args[1] == "build" {
                    let file = args[args.iter().position(|arg| arg == "-f").unwrap() + 1].clone();
                    *self.containerfile.borrow_mut() = std::fs::read_to_string(file)?;
                }
                self.commands.borrow_mut().push(args.clone());
                FakeRunner(i32::from(args[1] == "build" && self.fail_build)).run(cmd)
            }

            fn output(&self, cmd: &mut Command) -> std::io::Result<Output> {
                let args: Vec<String> = std::iter::once(cmd.get_program())
                    .chain(cmd.get_args())
                    .map(|arg| arg.to_string_lossy().into_owned())
                    .collect();
                let digest = format!("sha256:{}", "b".repeat(64));
                let value = if args[1] == "manifest" {
                    serde_json::json!({
                        "schemaVersion": 2,
                        "mediaType": "application/vnd.oci.image.index.v1+json",
                        "manifests": [{
                            "mediaType": "application/vnd.oci.image.manifest.v1+json",
                            "digest": digest,
                            "platform": {"os": "linux", "architecture": "arm64"}
                        }]
                    })
                } else if args[3] == "example.test/base:latest" {
                    serde_json::json!([{
                        "Id": format!("sha256:{}", "a".repeat(64)),
                        "Os": "linux",
                        "Architecture": "arm64"
                    }])
                } else {
                    assert_eq!(args[3], format!("example.test/addon@{digest}"));
                    serde_json::json!([{
                        "Os": "linux",
                        "Architecture": "arm64",
                        "Config": {"Env": ["ADDON_TEST=selected"]}
                    }])
                };
                self.commands.borrow_mut().push(args);
                Ok(Output {
                    status: FakeRunner(0).run(cmd)?,
                    stdout: serde_json::to_vec(&value).unwrap(),
                    stderr: Vec::new(),
                })
            }
        }

        for fail_build in [false, true] {
            let capture = DockerCapture {
                commands: RefCell::new(Vec::new()),
                containerfile: RefCell::new(String::new()),
                fail_build,
            };
            let result = run(
                "test:latest",
                "example.test/base:latest",
                &["example.test/addon:latest".to_owned()],
                false,
                &Backend::Cli(&ContainerCli::Docker, &capture),
            );
            assert_eq!(result.is_err(), fail_build);
            let commands = capture.commands.borrow();
            let tag = commands
                .iter()
                .find(|args| args[1..3] == ["image", "tag"])
                .unwrap()
                .last()
                .unwrap();
            assert!(
                capture
                    .containerfile
                    .borrow()
                    .contains(&format!("FROM {tag} AS system"))
            );
            assert_eq!(commands.last().unwrap(), &["docker", "image", "rm", tag]);
        }
    }

    #[test]
    fn run_does_not_bundle_host_certificates() {
        let capture = ContainerfileCapture(std::sync::Mutex::new(String::new()));
        run(
            "test:latest",
            "ghcr.io/example/project:dev",
            &[],
            false,
            &Backend::Cli(&ContainerCli::Podman, &capture),
        )
        .unwrap();
        let cf = capture.0.into_inner().unwrap();
        assert!(
            !cf.contains("COPY certs/"),
            "Containerfile must not copy host CA certificates"
        );
    }

    // vm runtime

    #[test]
    fn runtime_vm_has_no_container_cli() {
        assert_eq!(Runtime::Vm.container_cli(), None);
    }

    #[test]
    fn runtime_cli_variants_map_to_their_binaries() {
        assert_eq!(Runtime::Podman.container_cli(), Some(ContainerCli::Podman));
        assert_eq!(Runtime::Docker.container_cli(), Some(ContainerCli::Docker));
        assert_eq!(
            Runtime::MacOsContainer.container_cli(),
            Some(ContainerCli::MacOsContainer)
        );
    }

    #[test]
    fn cli_accepts_vm_runtime() {
        let cli = parse_cli(&["--runtime", "vm", "test:latest"]).unwrap();
        assert_eq!(cli.runtime, Runtime::Vm);
    }

    #[test]
    fn vm_output_path_replaces_tag_separators() {
        assert_eq!(
            vm_output_path("myimage:latest"),
            PathBuf::from("myimage-latest.tar")
        );
        assert_eq!(
            vm_output_path("ghcr.io/me/app:1.0"),
            PathBuf::from("ghcr.io-me-app-1.0.tar")
        );
    }

    #[test]
    fn vm_output_path_keeps_a_plain_name() {
        assert_eq!(vm_output_path("myimage"), PathBuf::from("myimage.tar"));
    }

    #[test]
    fn vm_config_uses_defaults_when_unset() {
        let config = vm_config(Some(PathBuf::from("/tmp/rootfs")), None, None, &[]).unwrap();
        assert_eq!(config.rootfs, PathBuf::from("/tmp/rootfs"));
        assert_eq!(config.cpus, vm_image_builder::DEFAULT_CPUS);
        assert_eq!(config.memory_mib, vm_image_builder::DEFAULT_MEMORY_MIB);
        // Discovered from this host, so only the invariant can be asserted.
        assert!(!config.nameservers.is_empty());
    }

    #[test]
    fn vm_config_uses_the_given_resources() {
        let dns = [ip("10.0.0.1"), ip("10.0.0.2")];
        let config = vm_config(
            Some(PathBuf::from("/tmp/rootfs")),
            Some(8),
            Some(16384),
            &dns,
        )
        .unwrap();
        assert_eq!(config.cpus, 8);
        assert_eq!(config.memory_mib, 16384);
        assert_eq!(config.nameservers, dns);
    }

    // --- vm_nameservers ---

    #[test]
    fn vm_nameservers_falls_back_to_the_host_when_unset() {
        let nameservers = vm_nameservers(&[]).unwrap();
        assert!(!nameservers.is_empty());
        assert!(
            nameservers
                .iter()
                .all(vm_image_builder::dns::is_reachable_from_vm)
        );
    }

    #[test]
    fn vm_nameservers_keeps_what_the_user_asked_for_in_order() {
        let dns = [ip("8.8.8.8"), ip("192.168.1.254")];
        assert_eq!(vm_nameservers(&dns).unwrap(), dns);
    }

    #[test]
    fn vm_nameservers_rejects_an_address_the_guest_cannot_reach() {
        // Silently dropping it would look like the flag had been honoured.
        for addr in ["127.0.0.1", "::1", "169.254.1.1"] {
            let err = vm_nameservers(&[ip(addr)]).unwrap_err();
            assert!(err.contains(addr), "error was: {err}");
            assert!(err.contains("--vm-dns"), "error was: {err}");
        }
    }

    // Only the empty case is asserted here. Calling this on a binary that does
    // embed a rootfs would unpack 80 MiB into the data directory of whoever
    // ran the tests; the vm-runtime workflow covers that path by building an
    // image with no --vm-rootfs at all. `vm_rootfs` covers the unpacking.
    #[test]
    fn vm_config_without_a_rootfs_falls_back_to_the_embedded_one() {
        if vm_rootfs::is_embedded() {
            return;
        }

        let err =
            vm_config(None, None, None, &[]).expect_err("nothing is embedded to fall back to");

        assert!(err.contains("--vm-rootfs"), "error was: {err}");
    }

    #[test]
    fn vm_config_rejects_bad_dns_before_resolving_the_rootfs() {
        // The rootfs is the expensive half: on a default run it unpacks the
        // embedded copy. A rejected flag must not pay for that first.
        let err = vm_config(None, None, None, &[ip("127.0.0.1")]).unwrap_err();
        assert!(err.contains("--vm-dns"), "error was: {err}");
    }

    #[test]
    fn check_vm_flags_accepts_vm_flags_with_vm_runtime() {
        let cli = parse_cli(&[
            "--runtime",
            "vm",
            "--vm-cpus",
            "4",
            "--vm-memory",
            "8192",
            "--vm-dns",
            "10.0.0.1",
            "test:latest",
        ])
        .unwrap();
        assert!(check_vm_flags(&cli).is_ok());
    }

    #[test]
    fn cli_accepts_repeated_vm_dns() {
        let cli = parse_cli(&[
            "--runtime",
            "vm",
            "--vm-dns",
            "10.0.0.1",
            "--vm-dns",
            "fd00::1",
            "test:latest",
        ])
        .unwrap();
        assert_eq!(cli.vm_dns, vec![ip("10.0.0.1"), ip("fd00::1")]);
    }

    #[test]
    fn cli_rejects_a_vm_dns_that_is_not_an_address() {
        assert!(
            parse_cli(&[
                "--runtime",
                "vm",
                "--vm-dns",
                "dns.example.com",
                "test:latest"
            ])
            .is_err()
        );
    }

    #[test]
    fn check_vm_flags_accepts_other_runtimes_without_vm_flags() {
        let cli = parse_cli(&["--runtime", "podman", "test:latest"]).unwrap();
        assert!(check_vm_flags(&cli).is_ok());
    }

    #[test]
    fn check_vm_flags_rejects_vm_flags_with_other_runtimes() {
        for (flag, value) in [
            ("--vm-rootfs", "/tmp/rootfs"),
            ("--vm-output", "out.tar"),
            ("--vm-cpus", "4"),
            ("--vm-memory", "8192"),
            ("--vm-dns", "10.0.0.1"),
        ] {
            let cli = parse_cli(&["--runtime", "podman", flag, value, "test:latest"]).unwrap();
            let err = check_vm_flags(&cli).unwrap_err();
            assert!(err.contains(flag), "expected '{flag}' in: {err}");
            assert!(err.contains("--runtime vm"), "unexpected: {err}");
        }
    }

    #[test]
    fn run_with_vm_backend_succeeds() {
        let tmp = tempfile::tempdir().unwrap();
        let config = VmConfig::new(&fake_vm_rootfs(tmp.path()));
        let runner = FakeVmRunner::new();
        let output = tmp.path().join("test-latest.tar");
        let result = run(
            "test:latest",
            "ghcr.io/example/project:dev",
            &[],
            false,
            &Backend::Vm(&config, &runner, &output),
        );
        assert!(result.is_ok(), "expected Ok, got {result:?}");

        let captured = runner.captured();
        assert_eq!(captured.tag, "test:latest");
        assert_eq!(captured.output_filename, "test-latest.tar");
    }

    #[test]
    fn run_with_vm_backend_passes_the_generated_containerfile() {
        let tmp = tempfile::tempdir().unwrap();
        let config = VmConfig::new(&fake_vm_rootfs(tmp.path()));
        let runner = FakeVmRunner::new();
        run(
            "test:latest",
            "ghcr.io/example/project:dev",
            &[],
            true,
            &Backend::Vm(&config, &runner, &tmp.path().join("out.tar")),
        )
        .unwrap();

        // The VM reads the Containerfile through the context share, so it must
        // have been written into the context directory before the VM booted.
        let cf = runner.containerfile();
        assert!(cf.contains("RUN cp /tmp/build-containerfile \"$HOME/Containerfile\""));
        assert!(
            cf.contains("FROM ghcr.io/example/project:dev AS system"),
            "unexpected Containerfile: {cf}"
        );
    }

    #[test]
    fn run_with_vm_backend_propagates_errors() {
        struct FailingVmRunner;

        impl VmRunner for FailingVmRunner {
            fn run(
                &self,
                _build: &vm_image_builder::VmBuild,
            ) -> Result<(), vm_image_builder::VmBuildError> {
                Err(vm_image_builder::VmBuildError::Failed { exit_code: Some(2) })
            }
        }

        let tmp = tempfile::tempdir().unwrap();
        let config = VmConfig::new(&fake_vm_rootfs(tmp.path()));
        let result = run(
            "test:latest",
            "ghcr.io/example/project:dev",
            &[],
            false,
            &Backend::Vm(&config, &FailingVmRunner, &tmp.path().join("out.tar")),
        );
        assert!(result.is_err(), "expected Err, got {result:?}");
    }

    /// The VM half of a selection, or `None` if the CLI arm was taken.
    fn vm_parts(selected: Selected) -> Option<(VmConfig, PathBuf)> {
        match selected {
            Selected::Vm(config, output) => Some((config, output)),
            Selected::Cli(_) => None,
        }
    }

    // select_runtime

    // The gate in front of `select_vm`. Which way it goes depends on how this
    // binary was built, and both directions are worth pinning: an unsupported
    // build must refuse before unpacking a rootfs, and a `vm` build must not
    // refuse a rootfs it was handed.
    #[test]
    fn select_runtime_vm_checks_vm_support_before_the_rootfs() {
        let tmp = tempfile::tempdir().unwrap();
        let rootfs = fake_vm_rootfs(tmp.path());
        let cli = parse_cli(&[
            "--runtime",
            "vm",
            "--vm-rootfs",
            rootfs.to_str().unwrap(),
            "myimage:latest",
        ])
        .unwrap();

        let selected = select_runtime(&cli);

        match KrunRunner.check_supported() {
            Ok(()) => assert!(selected.is_ok(), "{:?}", selected.err()),
            Err(unsupported) => assert_eq!(selected.err(), Some(unsupported.to_string())),
        }
    }

    #[test]
    fn select_runtime_vm_resolves_config_and_output() {
        let tmp = tempfile::tempdir().unwrap();
        let rootfs = fake_vm_rootfs(tmp.path());
        let cli = parse_cli(&[
            "--runtime",
            "vm",
            "--vm-rootfs",
            rootfs.to_str().unwrap(),
            "--vm-cpus",
            "4",
            "--vm-memory",
            "8192",
            "myimage:latest",
        ])
        .unwrap();
        let (config, output) = vm_parts(select_vm(&cli).unwrap()).unwrap();
        assert_eq!(config.rootfs, rootfs);
        assert_eq!(config.cpus, 4);
        assert_eq!(config.memory_mib, 8192);
        // No --vm-output, so the name is derived from the tag.
        assert_eq!(output, PathBuf::from("myimage-latest.tar"));
    }

    #[test]
    fn select_runtime_vm_honours_the_output_flag() {
        let tmp = tempfile::tempdir().unwrap();
        let rootfs = fake_vm_rootfs(tmp.path());
        let out = tmp.path().join("custom.tar");
        let cli = parse_cli(&[
            "--runtime",
            "vm",
            "--vm-rootfs",
            rootfs.to_str().unwrap(),
            "--vm-output",
            out.to_str().unwrap(),
            "myimage:latest",
        ])
        .unwrap();
        let (_, output) = vm_parts(select_vm(&cli).unwrap()).unwrap();
        assert_eq!(output, out);
    }

    #[test]
    fn vm_backend_check_ready_rejects_an_unusable_rootfs() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("no-such-rootfs");
        let cli = parse_cli(&[
            "--runtime",
            "vm",
            "--vm-rootfs",
            missing.to_str().unwrap(),
            "myimage:latest",
        ])
        .unwrap();
        let selected = select_vm(&cli).unwrap();
        let vm_runner = FakeVmRunner::new();
        let backend = backend_for(&selected, &FakeRunner(0), &vm_runner);
        let err = backend.check_ready().err().unwrap().to_string();
        assert!(
            err.contains("no-such-rootfs"),
            "error should name the rootfs: {err}"
        );
    }

    #[test]
    fn select_runtime_cli_takes_the_container_cli_arm() {
        let cli = parse_cli(&["--runtime", "podman", "myimage:latest"]).unwrap();
        assert!(vm_parts(select_runtime(&cli).unwrap()).is_none());
    }

    // backend_for

    #[test]
    fn backend_for_dispatches_all_four_runtimes_and_reports_their_results() {
        use std::cell::RefCell;

        struct ProgramCapture(RefCell<Vec<String>>);

        impl Runner for ProgramCapture {
            fn run(&self, cmd: &mut Command) -> std::io::Result<ExitStatus> {
                self.0
                    .borrow_mut()
                    .push(cmd.get_program().to_string_lossy().into_owned());
                FakeRunner(0).run(cmd)
            }
        }

        let capture = ProgramCapture(RefCell::new(Vec::new()));
        let vm_runner = FakeVmRunner::new();
        for (cli, program) in [
            (ContainerCli::Podman, "podman"),
            (ContainerCli::Docker, "docker"),
            (ContainerCli::MacOsContainer, "container"),
        ] {
            let selected = Selected::Cli(cli);
            let backend = backend_for(&selected, &capture, &vm_runner);
            let result = run("test:latest", "alpine:3.24", &[], false, backend.as_ref())
                .expect("host build should succeed");
            assert_eq!(
                result,
                BuildResult::Image {
                    tag: "test:latest".to_owned()
                }
            );
            assert_eq!(capture.0.borrow().last().map(String::as_str), Some(program));
        }

        let tmp = tempfile::tempdir().unwrap();
        let rootfs = fake_vm_rootfs(tmp.path());
        let out = tmp.path().join("out.tar");
        let selected = Selected::Vm(VmConfig::new(&rootfs), out.clone());
        let backend = backend_for(&selected, &capture, &vm_runner);
        let result = run("test:latest", "alpine:3.24", &[], false, backend.as_ref())
            .expect("VM build should succeed");
        assert_eq!(
            result,
            BuildResult::VmArtifact {
                rootfs_tar: out,
                config: None,
            }
        );
        assert_eq!(vm_runner.captured().rootfs, rootfs.canonicalize().unwrap());
        assert_eq!(capture.0.borrow().len(), 3);
    }

    // build_summary

    #[test]
    fn build_summary_names_the_tarball_a_vm_build_wrote() {
        let result = BuildResult::VmArtifact {
            rootfs_tar: PathBuf::from("out/myimage-latest.tar"),
            config: None,
        };
        assert_eq!(
            build_summary(&result).as_deref(),
            Some("Wrote out/myimage-latest.tar")
        );
    }

    #[test]
    fn build_summary_is_silent_for_a_cli_build() {
        // The image lands in the CLI's own store, so there is no path to report.
        assert_eq!(
            build_summary(&BuildResult::Image {
                tag: "test:latest".to_owned()
            }),
            None
        );
    }
}
