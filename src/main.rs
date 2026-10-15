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

mod vm_rootfs;

use std::net::IpAddr;
use std::path::{Path, PathBuf};

use clap::Parser;
use container_image_builder::{ContainerCli, ContainerRunner, Runner, build};
use log::LevelFilter;
use openshell_build_image::addon::AddonError;
use openshell_build_image::containerfile;
use vm_image_builder::{KrunRunner, VmConfig, VmRunner, build as vm_build};

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

/// Where [`run`] sends the generated Containerfile to be built.
///
/// The two variants carry different runner traits because the backends differ
/// in kind, not just in configuration: one spawns a process, the other boots a
/// VM and writes its result to a path on disk.
enum Backend<'a> {
    /// Shell out to a container CLI, leaving a tagged image in its image store.
    Cli(&'a ContainerCli, &'a dyn Runner),
    /// Build in a microVM, writing a flattened rootfs tarball to the path.
    Vm(&'a VmConfig, &'a dyn VmRunner, &'a Path),
}

/// What `--runtime` resolved to, owning the values [`Backend`] borrows.
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
        help = "Copy the build Containerfile to $HOME/Containerfile inside the image"
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

    // `Backend` borrows what it points at, so the owned values have to outlive
    // it: this resolves them first, then borrows them below.
    let selected = match select_runtime(&cli) {
        Ok(selected) => selected,
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    };
    let backend = backend_for(&selected);

    if let Err(e) = run(
        &cli.tag,
        &cli.from,
        &cli.addon,
        cli.copy_containerfile,
        &backend,
    ) {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }

    if let Some(summary) = build_summary(&selected) {
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

/// Resolves `--runtime` and the flags that go with it into the backend to build
/// through, rejecting a backend that cannot run.
///
/// Each backend validates what it needs here, before any staging work happens,
/// so a container CLI missing from `PATH` or an unusable VM rootfs fails
/// immediately rather than after the build context has been assembled.
fn select_runtime(cli: &Cli) -> Result<Selected, String> {
    match cli.runtime.container_cli() {
        Some(container_cli) => {
            container_cli.check_in_path().map_err(|e| e.to_string())?;
            Ok(Selected::Cli(container_cli))
        }
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
    config.check_rootfs().map_err(|e| e.to_string())?;
    let output = cli
        .vm_output
        .clone()
        .unwrap_or_else(|| vm_output_path(&cli.tag));
    Ok(Selected::Vm(config, output))
}

/// Borrows a [`Selected`] as the [`Backend`] that [`run`] builds through.
fn backend_for(selected: &Selected) -> Backend<'_> {
    match selected {
        Selected::Cli(container_cli) => Backend::Cli(container_cli, &ContainerRunner),
        Selected::Vm(config, output) => Backend::Vm(config, &KrunRunner, output),
    }
}

/// What to report after a successful build, if anything.
///
/// A CLI build leaves a tagged image the user can look up, so there is nothing
/// to add; a VM build leaves a file, so say where it landed.
fn build_summary(selected: &Selected) -> Option<String> {
    match selected {
        Selected::Cli(_) => None,
        Selected::Vm(_, output) => Some(format!("Wrote {}", output.display())),
    }
}

fn run(
    tag: &str,
    from: &str,
    addons: &[String],
    copy_containerfile: bool,
    backend: &Backend,
) -> Result<(), Box<dyn std::error::Error>> {
    if !addons.is_empty() {
        return Err(AddonError::NotImplemented.into());
    }
    let context_dir = tempfile::Builder::new()
        .prefix("openshell-build-image")
        .tempdir()?;
    let output = containerfile::generate(from, copy_containerfile, &[])?;
    if copy_containerfile {
        std::fs::write(context_dir.path().join("build-containerfile"), &output)?;
    }
    match backend {
        Backend::Cli(cli, runner) => build(&output, tag, cli, *runner, context_dir.path())?,
        Backend::Vm(config, runner, vm_output) => {
            vm_build(&output, tag, config, *runner, context_dir.path(), vm_output)?
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;
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
    struct FakeVmRunner(std::sync::Mutex<Option<(vm_image_builder::VmBuild, String)>>);

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
    }

    impl VmRunner for FakeVmRunner {
        fn run(
            &self,
            build: &vm_image_builder::VmBuild,
        ) -> Result<(), vm_image_builder::VmBuildError> {
            // `run` deletes the context directory as soon as it returns, so the
            // Containerfile has to be read here, while the VM would see it.
            let containerfile = std::fs::read_to_string(build.context.join("Containerfile"))?;
            if containerfile.contains("COPY build-containerfile") {
                assert_eq!(
                    std::fs::read_to_string(build.context.join("build-containerfile"))?,
                    containerfile
                );
            }
            *self.0.lock().unwrap() = Some((build.clone(), containerfile));
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
        // `check_rootfs` requires the execute bit, as libkrun exec's the
        // helper. Only Unix has one, and only there does the check run.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).unwrap();
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
    fn addon_is_rejected_until_build_support_is_added() {
        let result = run(
            "test:latest",
            "alpine:3.24",
            &["ghcr.io/example/addon:latest".to_owned()],
            false,
            &Backend::Cli(&ContainerCli::Podman, &FakeRunner(0)),
        );
        assert_eq!(
            result.unwrap_err().to_string(),
            "--addon builds are not implemented yet"
        );
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

    /// The VM half of a backend, or `None` if it is a CLI backend.
    fn vm_backend_parts<'a>(backend: Backend<'a>) -> Option<(&'a VmConfig, &'a Path)> {
        match backend {
            Backend::Vm(config, _, output) => Some((config, output)),
            Backend::Cli(..) => None,
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
    fn select_runtime_vm_rejects_an_unusable_rootfs() {
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
        // `Selected` has no `Debug`, so `unwrap_err` is unavailable here.
        let err = select_vm(&cli).err().unwrap();
        assert!(
            err.contains("no-such-rootfs"),
            "error should name the rootfs: {err}"
        );
    }

    #[test]
    fn select_runtime_cli_takes_the_container_cli_arm() {
        let cli = parse_cli(&["--runtime", "podman", "myimage:latest"]).unwrap();
        // Whether podman is on PATH varies by machine, and the CLI arm runs
        // either way. A VM selection would mean the wrong arm was taken; an
        // error that does not name the binary would mean it failed elsewhere.
        let r = select_runtime(&cli);
        let cli_arm = r.map_or_else(|e| e.contains("podman"), |s| vm_parts(s).is_none());
        assert!(cli_arm, "--runtime podman must take the container CLI arm");
    }

    // backend_for

    #[test]
    fn backend_for_maps_each_selection_to_its_backend() {
        let tmp = tempfile::tempdir().unwrap();
        let config = VmConfig::new(&fake_vm_rootfs(tmp.path()));
        let vm = Selected::Vm(config, tmp.path().join("out.tar"));
        assert!(vm_backend_parts(backend_for(&vm)).is_some());
        // The CLI arm of both `backend_for` and the helper above.
        let cli = Selected::Cli(ContainerCli::Docker);
        assert!(vm_backend_parts(backend_for(&cli)).is_none());
        // And the CLI arm of `vm_parts`.
        assert!(vm_parts(cli).is_none());
    }

    #[test]
    fn backend_for_vm_selection_passes_through_the_config_and_output() {
        let tmp = tempfile::tempdir().unwrap();
        let rootfs = fake_vm_rootfs(tmp.path());
        let out = tmp.path().join("out.tar");
        let selected = Selected::Vm(VmConfig::new(&rootfs), out.clone());
        let (config, output) = vm_backend_parts(backend_for(&selected)).unwrap();
        assert_eq!(config.rootfs, rootfs);
        assert_eq!(output, out);
    }

    // build_summary

    #[test]
    fn build_summary_names_the_tarball_a_vm_build_wrote() {
        let tmp = tempfile::tempdir().unwrap();
        let config = VmConfig::new(&fake_vm_rootfs(tmp.path()));
        let selected = Selected::Vm(config, PathBuf::from("out/myimage-latest.tar"));
        assert_eq!(
            build_summary(&selected).as_deref(),
            Some("Wrote out/myimage-latest.tar")
        );
    }

    #[test]
    fn build_summary_is_silent_for_a_cli_build() {
        // The image lands in the CLI's own store, so there is no path to report.
        assert_eq!(build_summary(&Selected::Cli(ContainerCli::Podman)), None);
    }
}
