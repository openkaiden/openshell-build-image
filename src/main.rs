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

mod agent;
mod certs;
mod config;
mod containerfile;
mod feature;
mod host;
mod inference;
mod policy;
mod vm_rootfs;
mod workspace;

use std::collections::HashMap;
use std::net::IpAddr;
use std::path::{Path, PathBuf};

const BASE_POLICY_YAML: &str = include_str!("../assets/policy.yaml");

use clap::Parser;
use container_image_builder::{ContainerCli, ContainerRunner, Runner, build};
use log::LevelFilter;
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
    name = "openshell-image-builder",
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
        env = "OPENSHELL_IMAGE_BUILDER_CONFIG",
        help = "Path to config directory (must contain config.toml)"
    )]
    config: Option<PathBuf>,
    #[arg(
        short = 'v',
        action = clap::ArgAction::Count,
        help = "Increase log verbosity (-v info, -vv debug)"
    )]
    verbose: u8,
    #[arg(long, value_enum, help = "Agent to install in the image")]
    agent: Option<agent::AgentKind>,
    #[arg(long, value_enum, help = "Inference server the agent will connect to")]
    inference: Option<inference::InferenceKind>,
    #[arg(long, help = "Override the inference provider's default endpoint URL")]
    endpoint: Option<String>,
    #[arg(long, help = "Default model for the agent to use")]
    model: Option<String>,
    #[arg(
        long,
        help = "Read .kaiden/workspace.json and apply its features, skills, and network rules"
    )]
    with_workspace_config: bool,
    #[arg(long, help = "Include OpenShell sandbox policy in the image")]
    with_policy: bool,
    #[arg(long, help = "Generate and include agent settings in the image")]
    with_agent_settings: bool,
    #[arg(
        long = "ssl-certs",
        value_name = "FILE",
        conflicts_with = "disable_ssl_certs",
        help = "Use a specific CA bundle instead of the auto-discovered one. \
                The build fails immediately if the file does not exist."
    )]
    ssl_certs: Option<String>,
    #[arg(
        long = "disable-ssl-certs",
        action = clap::ArgAction::SetTrue,
        help = "Disable bundling CA certificates into the image."
    )]
    disable_ssl_certs: bool,
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

    let ssl_certs = if cli.disable_ssl_certs {
        None
    } else {
        Some(cli.ssl_certs.map(std::path::PathBuf::from))
    };
    if let Err(e) = run(
        &cli.tag,
        cli.config,
        cli.with_workspace_config,
        cli.agent,
        cli.inference,
        cli.endpoint.as_deref(),
        cli.model.as_deref(),
        cli.with_policy,
        cli.with_agent_settings,
        ssl_certs,
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

#[allow(clippy::too_many_arguments)]
fn run(
    tag: &str,
    config_path: Option<PathBuf>,
    with_workspace_config: bool,
    agent_kind: Option<agent::AgentKind>,
    inference_kind: Option<inference::InferenceKind>,
    endpoint: Option<&str>,
    model: Option<&str>,
    with_policy: bool,
    with_agent_settings: bool,
    ssl_certs: Option<Option<PathBuf>>,
    backend: &Backend,
) -> Result<(), Box<dyn std::error::Error>> {
    if endpoint.is_some() && inference_kind == Some(inference::InferenceKind::VertexAi) {
        return Err("--endpoint is not supported for the vertexai inference provider".into());
    }
    let config = config::load(config_path.clone())?;
    let workspace = if with_workspace_config {
        workspace::load_from(Path::new("."))?
    } else {
        None
    };
    let agent = agent_kind.map(agent::from_kind);
    if let (Some(a), Some(ik)) = (agent.as_deref(), &inference_kind)
        && !a.supported_inference().contains(ik)
    {
        return Err(format!(
            "agent '{}' does not support the selected inference provider",
            a.id()
        )
        .into());
    }
    let inference = inference_kind.clone().map(inference::from_kind);
    let context_dir = tempfile::Builder::new()
        .prefix("openshell-image-builder")
        .tempdir()?;
    let features = feature::stage_all(workspace.as_ref(), context_dir.path())?;
    let has_agent_settings = if with_agent_settings {
        if let Some(a) = agent.as_deref() {
            let settings_dir = config::agent_settings_dir(config_path.as_deref(), a.id())?;
            stage_agent_settings(
                a,
                settings_dir.as_deref(),
                inference_kind.as_ref(),
                endpoint,
                model,
                context_dir.path(),
            )?
        } else {
            false
        }
    } else {
        false
    };
    let agent_env_vars = agent
        .as_deref()
        .map(|a| a.env_vars(inference_kind.as_ref(), endpoint, model))
        .unwrap_or_default();
    let base_url = resolve_base_url(inference_kind.as_ref(), endpoint);
    let skill_names = stage_skills(workspace.as_ref(), agent.as_deref(), context_dir.path())?;
    if with_policy {
        let policy_yaml = build_policy(
            BASE_POLICY_YAML,
            agent.as_deref(),
            inference.as_deref(),
            base_url.as_deref(),
            workspace.as_ref(),
        )?;
        std::fs::write(context_dir.path().join("policy.yaml"), policy_yaml)?;
    }
    let ca_certs_copied = match ssl_certs {
        None => false,
        Some(None) => certs::copy_from_paths(context_dir.path(), certs::SYSTEM_CA_CERT_PATHS)?,
        Some(Some(path)) => {
            certs::copy_from_file(context_dir.path(), &path)?;
            true
        }
    };
    let output = containerfile::generate(
        &config,
        agent.as_deref(),
        &features,
        has_agent_settings,
        &skill_names,
        &agent_env_vars,
        with_policy,
        ca_certs_copied,
    )?;
    match backend {
        Backend::Cli(cli, runner) => build(&output, tag, cli, *runner, context_dir.path())?,
        Backend::Vm(config, runner, vm_output) => {
            vm_build(&output, tag, config, *runner, context_dir.path(), vm_output)?
        }
    }
    Ok(())
}

fn copy_dir(src: &Path, dest: &Path) -> Result<(), Box<dyn std::error::Error>> {
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            return Err(format!(
                "symlinks are not allowed in agent settings: {}",
                entry.path().display()
            )
            .into());
        }
        let dest_path = dest.join(entry.file_name());
        if file_type.is_dir() {
            std::fs::create_dir_all(&dest_path)?;
            copy_dir(&entry.path(), &dest_path)?;
        } else {
            std::fs::copy(entry.path(), &dest_path)?;
        }
    }
    Ok(())
}

fn read_flat_files(dir: &Path) -> Result<HashMap<String, String>, std::io::Error> {
    let mut files = HashMap::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Ok(content) = std::fs::read_to_string(entry.path()) {
                files.insert(name, content);
            }
        }
    }
    Ok(files)
}

fn stage_skills(
    workspace: Option<&workspace::WorkspaceConfiguration>,
    agent: Option<&dyn agent::Agent>,
    context_dir: &Path,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let has_skills_dir = agent.map(|a| !a.skills_dir().is_empty()).unwrap_or(false);
    if !has_skills_dir {
        return Ok(vec![]);
    }
    let skills = match workspace {
        Some(ws) if !ws.skills.is_empty() => ws.skills.as_slice(),
        _ => return Ok(vec![]),
    };
    let dest = context_dir.join("skills");
    std::fs::create_dir_all(&dest)?;
    let mut staged = vec![];
    for skill_path in skills {
        let src = Path::new(skill_path);
        let skill_name = src
            .file_name()
            .ok_or_else(|| format!("invalid skill path: {skill_path}"))?
            .to_string_lossy()
            .into_owned();
        let dest_skill = dest.join(&skill_name);
        std::fs::create_dir_all(&dest_skill)?;
        copy_dir(src, &dest_skill)?;
        staged.push(skill_name);
    }
    Ok(staged)
}

fn resolve_base_url(
    inference: Option<&inference::InferenceKind>,
    endpoint: Option<&str>,
) -> Option<String> {
    match inference {
        Some(inference::InferenceKind::Ollama) => {
            let raw = endpoint.unwrap_or(inference::OLLAMA_DEFAULT_BASE_URL);
            Some(host::rewrite_localhost(raw))
        }
        Some(inference::InferenceKind::Anthropic) => endpoint.map(str::to_string),
        Some(inference::InferenceKind::OpenAi) => endpoint.map(host::rewrite_localhost),
        _ => None,
    }
}

fn stage_agent_settings(
    agent: &dyn agent::Agent,
    settings_dir: Option<&Path>,
    inference: Option<&inference::InferenceKind>,
    endpoint: Option<&str>,
    model: Option<&str>,
    context_dir: &Path,
) -> Result<bool, Box<dyn std::error::Error>> {
    let existing = match settings_dir {
        Some(dir) => read_flat_files(dir)?,
        None => HashMap::new(),
    };

    let base_url = resolve_base_url(inference, endpoint);
    let files = agent.skip_onboarding(existing);
    let files = agent.set_inference(files, inference, base_url.as_deref(), model);

    if settings_dir.is_none() && files.is_empty() {
        return Ok(false);
    }

    let dest = context_dir.join("agent-settings");
    std::fs::create_dir_all(&dest)?;

    if let Some(dir) = settings_dir {
        copy_dir(dir, &dest)?;
    }
    for (name, content) in &files {
        let path = dest.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, content)?;
    }
    Ok(true)
}

fn parse_workspace_host(s: &str) -> Result<(String, u16), Box<dyn std::error::Error>> {
    let url_str = if s.contains("://") {
        s.to_string()
    } else {
        format!("https://{s}")
    };
    let parsed =
        url::Url::parse(&url_str).map_err(|e| format!("invalid workspace host '{s}': {e}"))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| format!("workspace host is missing hostname: '{s}'"))?;
    Ok((host.to_string(), parsed.port().unwrap_or(443)))
}

fn workspace_hosts_policy(
    hosts: &[String],
    agent_binary: Option<&str>,
) -> Result<policy::NetworkPolicyRule, Box<dyn std::error::Error>> {
    let mut binaries = vec![
        policy::NetworkBinary::new("/bin/**"),
        policy::NetworkBinary::new("/usr/bin/**"),
        policy::NetworkBinary::new("/usr/local/bin/**"),
        policy::NetworkBinary::new("/sandbox/.local/bin/**"),
    ];
    if let Some(bin) = agent_binary {
        binaries.push(policy::NetworkBinary::new(bin));
    }
    Ok(policy::NetworkPolicyRule {
        name: "workspace".to_string(),
        endpoints: hosts
            .iter()
            .map(|s| {
                parse_workspace_host(s).map(|(host, port)| policy::NetworkEndpoint {
                    host,
                    port,
                    ..Default::default()
                })
            })
            .collect::<Result<Vec<_>, _>>()?,
        binaries,
    })
}

fn build_policy(
    base_yaml: &str,
    agent: Option<&dyn agent::Agent>,
    inference: Option<&dyn inference::Inference>,
    base_url: Option<&str>,
    workspace: Option<&workspace::WorkspaceConfiguration>,
) -> Result<String, Box<dyn std::error::Error>> {
    let mut sandbox_policy = policy::parse_sandbox_policy(base_yaml)?;
    if let (Some(inference), Some(agent)) = (inference, agent) {
        let inference_yaml = inference.policy_yaml(agent.binary_path(), base_url);
        let inference_policy = policy::parse_sandbox_policy(&inference_yaml)?;
        sandbox_policy
            .network_policies
            .extend(inference_policy.network_policies);
    }
    if let Some(agent) = agent {
        let agent_yaml = agent.policy_yaml();
        if !agent_yaml.is_empty() {
            let agent_policy = policy::parse_sandbox_policy(agent_yaml)?;
            sandbox_policy
                .network_policies
                .extend(agent_policy.network_policies);
        }
    }
    if let Some(hosts) = workspace
        .and_then(|ws| ws.network.as_ref())
        .map(|net| net.hosts.as_slice())
        .filter(|h| !h.is_empty())
    {
        let agent_binary = agent.map(|a| a.binary_path());
        sandbox_policy.network_policies.insert(
            "workspace".to_string(),
            workspace_hosts_policy(hosts, agent_binary)?,
        );
    }
    Ok(policy::serialize_sandbox_policy(&sandbox_policy)?)
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
        let mut argv = vec!["openshell-image-builder"];
        argv.extend_from_slice(args);
        Cli::try_parse_from(argv)
    }

    #[test]
    fn version_matches_cargo_toml() {
        let cmd = Cli::command();
        assert_eq!(cmd.get_version(), Some(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn build_policy_without_agent_has_no_claude_code_rule() {
        let yaml = build_policy(BASE_POLICY_YAML, None, None, None, None).unwrap();
        assert!(!yaml.contains("name: claude-code"));
    }

    #[test]
    fn build_policy_with_claude_agent_includes_claude_code_rule() {
        let yaml = build_policy(
            BASE_POLICY_YAML,
            Some(&agent::ClaudeAgent),
            None,
            None,
            None,
        )
        .unwrap();
        assert!(yaml.contains("name: claude-code"));
    }

    #[test]
    fn build_policy_without_inference_has_no_anthropic_rule() {
        let yaml = build_policy(
            BASE_POLICY_YAML,
            Some(&agent::ClaudeAgent),
            None,
            None,
            None,
        )
        .unwrap();
        assert!(!yaml.contains("api.anthropic.com"));
    }

    #[test]
    fn build_policy_with_inference_includes_anthropic_rule() {
        let yaml = build_policy(
            BASE_POLICY_YAML,
            Some(&agent::ClaudeAgent),
            Some(&inference::AnthropicInference),
            None,
            None,
        )
        .unwrap();
        assert!(yaml.contains("api.anthropic.com"));
    }

    #[test]
    fn build_policy_with_inference_uses_agent_binary() {
        let yaml = build_policy(
            BASE_POLICY_YAML,
            Some(&agent::ClaudeAgent),
            Some(&inference::AnthropicInference),
            None,
            None,
        )
        .unwrap();
        assert!(yaml.contains("/sandbox/.local/bin/claude"));
    }

    #[test]
    fn build_policy_with_vertexai_inference_includes_aiplatform_rule() {
        let yaml = build_policy(
            BASE_POLICY_YAML,
            Some(&agent::ClaudeAgent),
            Some(&inference::VertexAiInference),
            None,
            None,
        )
        .unwrap();
        assert!(yaml.contains("aiplatform.googleapis.com"));
    }

    #[test]
    fn build_policy_with_ollama_inference_includes_host_openshell_internal() {
        let yaml = build_policy(
            BASE_POLICY_YAML,
            Some(&agent::ClaudeAgent),
            Some(&inference::OllamaInference),
            None,
            None,
        )
        .unwrap();
        assert!(yaml.contains("host.openshell.internal"));
    }

    #[test]
    fn build_policy_with_anthropic_and_custom_endpoint_uses_proxy_host() {
        let yaml = build_policy(
            BASE_POLICY_YAML,
            Some(&agent::ClaudeAgent),
            Some(&inference::AnthropicInference),
            Some("https://my-anthropic-proxy.example.com"),
            None,
        )
        .unwrap();
        assert!(yaml.contains("my-anthropic-proxy.example.com"));
        assert!(!yaml.contains("api.anthropic.com"));
    }

    #[test]
    fn build_policy_with_ollama_and_custom_endpoint_uses_custom_host_and_port() {
        let yaml = build_policy(
            BASE_POLICY_YAML,
            Some(&agent::ClaudeAgent),
            Some(&inference::OllamaInference),
            Some("http://host.openshell.internal:9999/v1"),
            None,
        )
        .unwrap();
        assert!(yaml.contains("host.openshell.internal"));
        assert!(yaml.contains("9999"));
        assert!(!yaml.contains("11434"));
    }

    // copy_dir

    #[test]
    fn copy_dir_copies_file_with_content() {
        let src = tempfile::tempdir().unwrap();
        std::fs::write(src.path().join("file.txt"), "hello").unwrap();
        let dest = tempfile::tempdir().unwrap();
        copy_dir(src.path(), dest.path()).unwrap();
        assert_eq!(
            std::fs::read_to_string(dest.path().join("file.txt")).unwrap(),
            "hello"
        );
    }

    #[test]
    fn copy_dir_recurses_into_subdirectories() {
        let src = tempfile::tempdir().unwrap();
        let subdir = src.path().join("sub");
        std::fs::create_dir(&subdir).unwrap();
        std::fs::write(subdir.join("nested.txt"), "nested").unwrap();
        let dest = tempfile::tempdir().unwrap();
        copy_dir(src.path(), dest.path()).unwrap();
        assert_eq!(
            std::fs::read_to_string(dest.path().join("sub").join("nested.txt")).unwrap(),
            "nested"
        );
    }

    #[test]
    fn copy_dir_empty_source_succeeds() {
        let src = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        assert!(copy_dir(src.path(), dest.path()).is_ok());
    }

    #[test]
    fn copy_dir_fails_when_source_missing() {
        let dest = tempfile::tempdir().unwrap();
        let result = copy_dir(Path::new("/nonexistent/path"), dest.path());
        assert!(result.is_err());
    }

    #[test]
    #[cfg(unix)]
    fn copy_dir_rejects_symlinks() {
        let src = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink("/etc/passwd", src.path().join("link")).unwrap();
        let dest = tempfile::tempdir().unwrap();
        let err = copy_dir(src.path(), dest.path()).unwrap_err();
        assert!(
            err.to_string().contains("symlinks are not allowed"),
            "unexpected error: {err}"
        );
    }

    // stage_agent_settings

    #[test]
    fn stage_agent_settings_creates_agent_settings_subdir() {
        let settings = tempfile::tempdir().unwrap();
        let context = tempfile::tempdir().unwrap();
        stage_agent_settings(
            &agent::OpencodeAgent,
            Some(settings.path()),
            None,
            None,
            None,
            context.path(),
        )
        .unwrap();
        assert!(context.path().join("agent-settings").is_dir());
    }

    #[test]
    fn stage_agent_settings_copies_files_into_subdir() {
        let settings = tempfile::tempdir().unwrap();
        std::fs::write(settings.path().join("myfile"), "data").unwrap();
        let context = tempfile::tempdir().unwrap();
        stage_agent_settings(
            &agent::OpencodeAgent,
            Some(settings.path()),
            None,
            None,
            None,
            context.path(),
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(context.path().join("agent-settings").join("myfile")).unwrap(),
            "data"
        );
    }

    #[test]
    fn stage_agent_settings_preserves_nested_structure() {
        let settings = tempfile::tempdir().unwrap();
        let subdir = settings.path().join(".claude");
        std::fs::create_dir(&subdir).unwrap();
        std::fs::write(subdir.join("settings.json"), "{}").unwrap();
        let context = tempfile::tempdir().unwrap();
        stage_agent_settings(
            &agent::OpencodeAgent,
            Some(settings.path()),
            None,
            None,
            None,
            context.path(),
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(
                context
                    .path()
                    .join("agent-settings")
                    .join(".claude")
                    .join("settings.json")
            )
            .unwrap(),
            "{}"
        );
    }

    #[test]
    fn stage_agent_settings_returns_false_for_noop_agent_without_settings_dir() {
        let context = tempfile::tempdir().unwrap();
        let staged = stage_agent_settings(
            &agent::OpencodeAgent,
            None,
            None,
            None,
            None,
            context.path(),
        )
        .unwrap();
        assert!(!staged);
    }

    #[test]
    fn stage_agent_settings_creates_claude_json_for_claude_agent_without_settings_dir() {
        let context = tempfile::tempdir().unwrap();
        let staged =
            stage_agent_settings(&agent::ClaudeAgent, None, None, None, None, context.path())
                .unwrap();
        assert!(staged);
        assert!(
            context
                .path()
                .join("agent-settings")
                .join(".claude.json")
                .exists()
        );
    }

    #[test]
    fn stage_agent_settings_claude_json_has_onboarding_flags() {
        let context = tempfile::tempdir().unwrap();
        stage_agent_settings(&agent::ClaudeAgent, None, None, None, None, context.path()).unwrap();
        let content =
            std::fs::read_to_string(context.path().join("agent-settings").join(".claude.json"))
                .unwrap();
        let json: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(json["hasCompletedOnboarding"], true);
        assert_eq!(json["projects"]["/sandbox"]["hasTrustDialogAccepted"], true);
    }

    #[test]
    fn stage_agent_settings_with_ollama_creates_opencode_config() {
        let context = tempfile::tempdir().unwrap();
        let staged = stage_agent_settings(
            &agent::OpencodeAgent,
            None,
            Some(&inference::InferenceKind::Ollama),
            None,
            None,
            context.path(),
        )
        .unwrap();
        assert!(staged);
        assert!(
            context
                .path()
                .join("agent-settings")
                .join(".config")
                .join("opencode")
                .join("config.json")
                .exists()
        );
    }

    // stage_skills

    struct NoSkillsAgent;

    impl agent::Agent for NoSkillsAgent {
        fn id(&self) -> &str {
            "no-skills"
        }
        fn install(&self) -> String {
            String::new()
        }
        fn binary_path(&self) -> &str {
            "/no-skills"
        }
    }

    fn make_skill_dir(base: &Path, name: &str) -> PathBuf {
        let skill = base.join(name);
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(skill.join("SKILL.md"), format!("# {name}")).unwrap();
        skill
    }

    fn make_workspace_with_skills(skills: &[&str]) -> workspace::WorkspaceConfiguration {
        let mut ws = workspace::WorkspaceConfiguration::default();
        ws.skills = skills.iter().map(|s| s.to_string()).collect();
        ws
    }

    #[test]
    fn stage_skills_returns_empty_when_no_agent() {
        let context = tempfile::tempdir().unwrap();
        let ws = make_workspace_with_skills(&[]);
        let names = stage_skills(Some(&ws), None, context.path()).unwrap();
        assert!(names.is_empty());
    }

    #[test]
    fn stage_skills_returns_empty_when_agent_has_no_skills_dir() {
        let context = tempfile::tempdir().unwrap();
        let ws = make_workspace_with_skills(&["some-skill"]);
        let names = stage_skills(Some(&ws), Some(&NoSkillsAgent), context.path()).unwrap();
        assert!(names.is_empty());
    }

    #[test]
    fn stage_skills_returns_empty_when_workspace_has_no_skills() {
        let context = tempfile::tempdir().unwrap();
        let ws = make_workspace_with_skills(&[]);
        let names = stage_skills(Some(&ws), Some(&agent::ClaudeAgent), context.path()).unwrap();
        assert!(names.is_empty());
    }

    #[test]
    fn stage_skills_returns_empty_when_no_workspace() {
        let context = tempfile::tempdir().unwrap();
        let names = stage_skills(None, Some(&agent::ClaudeAgent), context.path()).unwrap();
        assert!(names.is_empty());
    }

    #[test]
    fn stage_skills_copies_skill_dir_to_context() {
        let src = tempfile::tempdir().unwrap();
        make_skill_dir(src.path(), "my-skill");
        let context = tempfile::tempdir().unwrap();
        let skill_path = src.path().join("my-skill").to_string_lossy().into_owned();
        let ws = make_workspace_with_skills(&[&skill_path]);
        let names = stage_skills(Some(&ws), Some(&agent::ClaudeAgent), context.path()).unwrap();
        assert_eq!(names, vec!["my-skill"]);
        assert!(context.path().join("skills").join("my-skill").is_dir());
    }

    #[test]
    fn stage_skills_copies_skill_contents() {
        let src = tempfile::tempdir().unwrap();
        make_skill_dir(src.path(), "my-skill");
        let context = tempfile::tempdir().unwrap();
        let skill_path = src.path().join("my-skill").to_string_lossy().into_owned();
        let ws = make_workspace_with_skills(&[&skill_path]);
        stage_skills(Some(&ws), Some(&agent::ClaudeAgent), context.path()).unwrap();
        let skill_md = context
            .path()
            .join("skills")
            .join("my-skill")
            .join("SKILL.md");
        assert!(skill_md.exists());
        assert_eq!(std::fs::read_to_string(skill_md).unwrap(), "# my-skill");
    }

    #[test]
    fn stage_skills_returns_all_skill_names() {
        let src = tempfile::tempdir().unwrap();
        make_skill_dir(src.path(), "skill-a");
        make_skill_dir(src.path(), "skill-b");
        let context = tempfile::tempdir().unwrap();
        let path_a = src.path().join("skill-a").to_string_lossy().into_owned();
        let path_b = src.path().join("skill-b").to_string_lossy().into_owned();
        let ws = make_workspace_with_skills(&[&path_a, &path_b]);
        let mut names = stage_skills(Some(&ws), Some(&agent::ClaudeAgent), context.path()).unwrap();
        names.sort();
        assert_eq!(names, vec!["skill-a", "skill-b"]);
    }

    // run

    #[test]
    fn run_with_no_agent_succeeds() {
        let tmp = tempfile::tempdir().unwrap();
        let result = run(
            "test:latest",
            Some(tmp.path().to_path_buf()),
            false,
            None,
            None,
            None,
            None,
            false,
            false,
            None,
            &Backend::Cli(&ContainerCli::Podman, &FakeRunner(0)),
        );
        assert!(result.is_ok(), "expected Ok, got {result:?}");
    }

    #[test]
    fn run_with_claude_agent_succeeds() {
        let tmp = tempfile::tempdir().unwrap();
        let result = run(
            "test:latest",
            Some(tmp.path().to_path_buf()),
            false,
            Some(agent::AgentKind::Claude),
            None,
            None,
            None,
            false,
            false,
            None,
            &Backend::Cli(&ContainerCli::Podman, &FakeRunner(0)),
        );
        assert!(result.is_ok(), "expected Ok, got {result:?}");
    }

    #[test]
    fn run_with_claude_agent_and_anthropic_inference_succeeds() {
        let tmp = tempfile::tempdir().unwrap();
        let result = run(
            "test:latest",
            Some(tmp.path().to_path_buf()),
            false,
            Some(agent::AgentKind::Claude),
            Some(inference::InferenceKind::Anthropic),
            None,
            None,
            false,
            false,
            None,
            &Backend::Cli(&ContainerCli::Podman, &FakeRunner(0)),
        );
        assert!(result.is_ok(), "expected Ok, got {result:?}");
    }

    #[test]
    fn run_with_claude_agent_and_ollama_inference_returns_error() {
        let tmp = tempfile::tempdir().unwrap();
        let result = run(
            "test:latest",
            Some(tmp.path().to_path_buf()),
            false,
            Some(agent::AgentKind::Claude),
            Some(inference::InferenceKind::Ollama),
            None,
            None,
            false,
            false,
            None,
            &Backend::Cli(&ContainerCli::Podman, &FakeRunner(0)),
        );
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("does not support the selected inference provider")
        );
    }

    #[test]
    fn run_returns_error_when_runner_fails() {
        let tmp = tempfile::tempdir().unwrap();
        let result = run(
            "test:latest",
            Some(tmp.path().to_path_buf()),
            false,
            None,
            None,
            None,
            None,
            false,
            false,
            None,
            &Backend::Cli(&ContainerCli::Podman, &FakeRunner(1)),
        );
        assert!(result.is_err());
    }

    #[test]
    fn run_with_endpoint_and_vertexai_returns_error() {
        let tmp = tempfile::tempdir().unwrap();
        let result = run(
            "test:latest",
            Some(tmp.path().to_path_buf()),
            false,
            None,
            Some(inference::InferenceKind::VertexAi),
            Some("https://my-vertex-proxy.example.com"),
            None,
            false,
            false,
            None,
            &Backend::Cli(&ContainerCli::Podman, &FakeRunner(0)),
        );
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("--endpoint is not supported for the vertexai inference provider")
        );
    }

    #[test]
    fn run_with_model_succeeds() {
        let tmp = tempfile::tempdir().unwrap();
        let result = run(
            "test:latest",
            Some(tmp.path().to_path_buf()),
            false,
            Some(agent::AgentKind::Claude),
            Some(inference::InferenceKind::Anthropic),
            None,
            Some("claude-opus-4-5"),
            false,
            false,
            None,
            &Backend::Cli(&ContainerCli::Podman, &FakeRunner(0)),
        );
        assert!(result.is_ok(), "expected Ok, got {result:?}");
    }

    #[test]
    fn stage_agent_settings_with_anthropic_and_model_creates_opencode_config() {
        let context = tempfile::tempdir().unwrap();
        let staged = stage_agent_settings(
            &agent::OpencodeAgent,
            None,
            Some(&inference::InferenceKind::Anthropic),
            None,
            Some("claude-opus-4-5"),
            context.path(),
        )
        .unwrap();
        assert!(staged);
        assert!(
            context
                .path()
                .join("agent-settings")
                .join(".config")
                .join("opencode")
                .join("config.json")
                .exists()
        );
    }

    #[test]
    fn stage_agent_settings_with_vertexai_and_model_creates_opencode_config() {
        let context = tempfile::tempdir().unwrap();
        let staged = stage_agent_settings(
            &agent::OpencodeAgent,
            None,
            Some(&inference::InferenceKind::VertexAi),
            None,
            Some("vertex/claude-opus-4-5"),
            context.path(),
        )
        .unwrap();
        assert!(staged);
        assert!(
            context
                .path()
                .join("agent-settings")
                .join(".config")
                .join("opencode")
                .join("config.json")
                .exists()
        );
    }

    // resolve_base_url

    #[test]
    fn resolve_base_url_ollama_default_rewrites_localhost() {
        let url = resolve_base_url(Some(&inference::InferenceKind::Ollama), None).unwrap();
        assert!(url.contains("host.openshell.internal"));
        assert!(!url.contains("localhost"));
    }

    #[test]
    fn resolve_base_url_ollama_custom_endpoint_rewrites_localhost() {
        let url = resolve_base_url(
            Some(&inference::InferenceKind::Ollama),
            Some("http://localhost:9999/v1"),
        )
        .unwrap();
        assert_eq!(url, "http://host.openshell.internal:9999/v1");
    }

    #[test]
    fn resolve_base_url_ollama_non_localhost_endpoint_unchanged() {
        let url = resolve_base_url(
            Some(&inference::InferenceKind::Ollama),
            Some("http://remote-server:11434/v1"),
        )
        .unwrap();
        assert_eq!(url, "http://remote-server:11434/v1");
    }

    #[test]
    fn resolve_base_url_returns_none_for_non_local_providers() {
        assert!(resolve_base_url(Some(&inference::InferenceKind::Anthropic), None).is_none());
        assert!(resolve_base_url(Some(&inference::InferenceKind::VertexAi), None).is_none());
        assert!(resolve_base_url(None, None).is_none());
    }

    #[test]
    fn resolve_base_url_anthropic_with_endpoint_returns_endpoint() {
        let url = resolve_base_url(
            Some(&inference::InferenceKind::Anthropic),
            Some("https://my-proxy.example.com"),
        )
        .unwrap();
        assert_eq!(url, "https://my-proxy.example.com");
    }

    #[test]
    fn resolve_base_url_anthropic_endpoint_not_rewritten() {
        let url = resolve_base_url(
            Some(&inference::InferenceKind::Anthropic),
            Some("http://localhost:8080"),
        )
        .unwrap();
        assert_eq!(url, "http://localhost:8080");
    }

    #[test]
    fn build_policy_with_openai_inference_includes_api_openai_com() {
        let yaml = build_policy(
            BASE_POLICY_YAML,
            Some(&agent::OpencodeAgent),
            Some(&inference::OpenAiInference),
            None,
            None,
        )
        .unwrap();
        assert!(yaml.contains("api.openai.com"));
    }

    #[test]
    fn resolve_base_url_returns_none_for_openai_without_endpoint() {
        assert!(resolve_base_url(Some(&inference::InferenceKind::OpenAi), None).is_none());
    }

    #[test]
    fn resolve_base_url_openai_with_endpoint_returns_endpoint() {
        let url = resolve_base_url(
            Some(&inference::InferenceKind::OpenAi),
            Some("https://my-openai-proxy.example.com/v1"),
        )
        .unwrap();
        assert_eq!(url, "https://my-openai-proxy.example.com/v1");
    }

    #[test]
    fn resolve_base_url_openai_endpoint_rewrites_localhost() {
        let url = resolve_base_url(
            Some(&inference::InferenceKind::OpenAi),
            Some("http://localhost:8080/v1"),
        )
        .unwrap();
        assert_eq!(url, "http://host.openshell.internal:8080/v1");
    }

    #[test]
    fn run_with_policy_flag_succeeds() {
        let tmp = tempfile::tempdir().unwrap();
        let result = run(
            "test:latest",
            Some(tmp.path().to_path_buf()),
            false,
            None,
            None,
            None,
            None,
            true,
            false,
            None,
            &Backend::Cli(&ContainerCli::Podman, &FakeRunner(0)),
        );
        assert!(result.is_ok(), "expected Ok, got {result:?}");
    }

    #[test]
    fn run_with_agent_settings_and_claude_agent_succeeds() {
        let tmp = tempfile::tempdir().unwrap();
        let result = run(
            "test:latest",
            Some(tmp.path().to_path_buf()),
            false,
            Some(agent::AgentKind::Claude),
            None,
            None,
            None,
            false,
            true,
            None,
            &Backend::Cli(&ContainerCli::Podman, &FakeRunner(0)),
        );
        assert!(result.is_ok(), "expected Ok, got {result:?}");
    }

    #[test]
    fn run_with_claude_agent_and_openai_inference_returns_error() {
        let tmp = tempfile::tempdir().unwrap();
        let result = run(
            "test:latest",
            Some(tmp.path().to_path_buf()),
            false,
            Some(agent::AgentKind::Claude),
            Some(inference::InferenceKind::OpenAi),
            None,
            None,
            false,
            false,
            None,
            &Backend::Cli(&ContainerCli::Podman, &FakeRunner(0)),
        );
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("does not support the selected inference provider")
        );
    }

    #[test]
    fn stage_agent_settings_with_openai_and_model_creates_opencode_config() {
        let context = tempfile::tempdir().unwrap();
        let staged = stage_agent_settings(
            &agent::OpencodeAgent,
            None,
            Some(&inference::InferenceKind::OpenAi),
            None,
            Some("gpt-4o"),
            context.path(),
        )
        .unwrap();
        assert!(staged);
        assert!(
            context
                .path()
                .join("agent-settings")
                .join(".config")
                .join("opencode")
                .join("config.json")
                .exists()
        );
    }

    // parse_workspace_host

    #[test]
    fn parse_workspace_host_defaults_to_443() {
        let (host, port) = parse_workspace_host("example.com").unwrap();
        assert_eq!(host, "example.com");
        assert_eq!(port, 443);
    }

    #[test]
    fn parse_workspace_host_respects_explicit_port() {
        let (host, port) = parse_workspace_host("example.com:8080").unwrap();
        assert_eq!(host, "example.com");
        assert_eq!(port, 8080);
    }

    #[test]
    fn parse_workspace_host_with_full_https_url() {
        let (host, port) = parse_workspace_host("https://example.com:8443").unwrap();
        assert_eq!(host, "example.com");
        assert_eq!(port, 8443);
    }

    #[test]
    fn parse_workspace_host_fails_on_invalid_input() {
        assert!(parse_workspace_host("not a valid host !!!").is_err());
    }

    // workspace_hosts_policy

    #[test]
    fn workspace_hosts_policy_creates_one_rule_with_all_endpoints() {
        let hosts = vec!["example.com".to_string(), "api.foo.com:8443".to_string()];
        let rule = workspace_hosts_policy(&hosts, None).unwrap();
        assert_eq!(rule.name, "workspace");
        assert_eq!(rule.endpoints.len(), 2);
        assert_eq!(rule.endpoints[0].host, "example.com");
        assert_eq!(rule.endpoints[0].port, 443);
        assert_eq!(rule.endpoints[1].host, "api.foo.com");
        assert_eq!(rule.endpoints[1].port, 8443);
    }

    #[test]
    fn workspace_hosts_policy_includes_glob_binaries() {
        let hosts = vec!["example.com".to_string()];
        let rule = workspace_hosts_policy(&hosts, None).unwrap();
        let paths: Vec<&str> = rule.binaries.iter().map(|b| b.path.as_str()).collect();
        assert!(paths.contains(&"/bin/**"));
        assert!(paths.contains(&"/usr/bin/**"));
        assert!(paths.contains(&"/usr/local/bin/**"));
        assert!(paths.contains(&"/sandbox/.local/bin/**"));
    }

    #[test]
    fn workspace_hosts_policy_includes_agent_binary_when_provided() {
        let hosts = vec!["example.com".to_string()];
        let rule = workspace_hosts_policy(&hosts, Some("/sandbox/.local/bin/claude")).unwrap();
        let paths: Vec<&str> = rule.binaries.iter().map(|b| b.path.as_str()).collect();
        assert!(paths.contains(&"/sandbox/.local/bin/claude"));
    }

    #[test]
    fn workspace_hosts_policy_omits_agent_binary_when_none() {
        let hosts = vec!["example.com".to_string()];
        let rule = workspace_hosts_policy(&hosts, None).unwrap();
        assert_eq!(rule.binaries.len(), 4);
    }

    #[test]
    fn workspace_hosts_policy_fails_on_invalid_host() {
        let hosts = vec!["not a valid host !!!".to_string()];
        assert!(workspace_hosts_policy(&hosts, None).is_err());
    }

    // build_policy with workspace hosts

    #[test]
    fn build_policy_with_workspace_hosts_includes_host() {
        use kdn_workspace_configuration::{NetworkConfiguration, NetworkConfigurationMode};
        let mut ws = workspace::WorkspaceConfiguration::default();
        ws.network = Some(NetworkConfiguration {
            hosts: vec!["myhost.example.com".to_string()],
            mode: NetworkConfigurationMode::Deny,
        });
        let yaml = build_policy(BASE_POLICY_YAML, None, None, None, Some(&ws)).unwrap();
        assert!(yaml.contains("myhost.example.com"));
        assert!(yaml.contains("workspace"));
    }

    #[test]
    fn build_policy_with_workspace_hosts_includes_agent_binary() {
        use kdn_workspace_configuration::{NetworkConfiguration, NetworkConfigurationMode};
        let mut ws = workspace::WorkspaceConfiguration::default();
        ws.network = Some(NetworkConfiguration {
            hosts: vec!["myhost.example.com".to_string()],
            mode: NetworkConfigurationMode::Deny,
        });
        let yaml = build_policy(
            BASE_POLICY_YAML,
            Some(&agent::ClaudeAgent),
            None,
            None,
            Some(&ws),
        )
        .unwrap();
        assert!(yaml.contains("/sandbox/.local/bin/claude"));
    }

    #[test]
    fn build_policy_with_empty_network_hosts_unchanged() {
        let ws = workspace::WorkspaceConfiguration::default();
        let yaml_no_ws = build_policy(BASE_POLICY_YAML, None, None, None, None).unwrap();
        let yaml_ws = build_policy(BASE_POLICY_YAML, None, None, None, Some(&ws)).unwrap();
        assert_eq!(yaml_no_ws, yaml_ws);
    }

    // ssl_certs / run() tests

    #[test]
    fn run_with_ssl_certs_auto_discover_no_certs_found_succeeds() {
        let tmp = tempfile::tempdir().unwrap();
        let result = run(
            "test:latest",
            Some(tmp.path().to_path_buf()),
            false,
            None,
            None,
            None,
            None,
            false,
            false,
            Some(None),
            &Backend::Cli(&ContainerCli::Podman, &FakeRunner(0)),
        );
        assert!(result.is_ok(), "expected Ok, got {result:?}");
    }

    #[test]
    fn run_with_ssl_certs_specific_file_succeeds() {
        let tmp = tempfile::tempdir().unwrap();
        let cert = tmp.path().join("bundle.crt");
        std::fs::write(&cert, b"FAKE_CERT_DATA").unwrap();
        let result = run(
            "test:latest",
            Some(tmp.path().to_path_buf()),
            false,
            None,
            None,
            None,
            None,
            false,
            false,
            Some(Some(cert)),
            &Backend::Cli(&ContainerCli::Podman, &FakeRunner(0)),
        );
        assert!(result.is_ok(), "expected Ok, got {result:?}");
    }

    #[test]
    fn run_with_ssl_certs_specific_file_missing_returns_error() {
        let tmp = tempfile::tempdir().unwrap();
        let result = run(
            "test:latest",
            Some(tmp.path().to_path_buf()),
            false,
            None,
            None,
            None,
            None,
            false,
            false,
            Some(Some(PathBuf::from("/nonexistent/bundle.crt"))),
            &Backend::Cli(&ContainerCli::Podman, &FakeRunner(0)),
        );
        assert!(result.is_err());
    }

    #[test]
    fn run_with_disable_ssl_certs_containerfile_has_no_cert_copy() {
        let tmp = tempfile::tempdir().unwrap();
        let capture = ContainerfileCapture(std::sync::Mutex::new(String::new()));
        run(
            "test:latest",
            Some(tmp.path().to_path_buf()),
            false,
            None,
            None,
            None,
            None,
            false,
            false,
            None,
            &Backend::Cli(&ContainerCli::Podman, &capture),
        )
        .unwrap();
        let cf = capture.0.into_inner().unwrap();
        assert!(
            !cf.contains("COPY certs/"),
            "Containerfile must not contain cert COPY when --disable-ssl-certs is passed"
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
            Some(tmp.path().to_path_buf()),
            false,
            None,
            None,
            None,
            None,
            false,
            false,
            None,
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
            Some(tmp.path().to_path_buf()),
            false,
            Some(agent::AgentKind::Claude),
            None,
            None,
            None,
            false,
            false,
            None,
            &Backend::Vm(&config, &runner, &tmp.path().join("out.tar")),
        )
        .unwrap();

        // The VM reads the Containerfile through the context share, so it must
        // have been written into the context directory before the VM booted.
        let cf = runner.containerfile();
        assert!(cf.contains("FROM"), "unexpected Containerfile: {cf}");
        assert!(cf.contains("claude"), "expected the agent install in: {cf}");
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
            Some(tmp.path().to_path_buf()),
            false,
            None,
            None,
            None,
            None,
            false,
            false,
            None,
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
