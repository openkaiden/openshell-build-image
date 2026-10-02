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
mod image_mount;
mod inference;
mod policy;
mod workspace;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

const BASE_POLICY_YAML: &str = include_str!("../assets/policy.yaml");

use clap::Parser;
use container_image_builder::{ContainerCli, ContainerRunner, Runner, build};
use log::LevelFilter;

/// Selects which container CLI to use for building images.
///
/// This enum is local to the binary so that the library crate (`container-image-builder`)
/// has no dependency on `clap`. A `From<Runtime>` impl converts to
/// [`ContainerCli`] after argument parsing.
#[derive(clap::ValueEnum, Clone)]
enum Runtime {
    Podman,
    Docker,
    /// Apple's `container` CLI (macOS only).
    #[value(name = "container")]
    MacOsContainer,
}

impl From<Runtime> for ContainerCli {
    fn from(r: Runtime) -> Self {
        match r {
            Runtime::Podman => ContainerCli::Podman,
            Runtime::Docker => ContainerCli::Docker,
            Runtime::MacOsContainer => ContainerCli::MacOsContainer,
        }
    }
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
        help = "Container CLI to use for building images (podman, docker, container)"
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
        long,
        action = clap::ArgAction::Append,
        help = "Path or URL to a YAML file describing a container image to mount (may be repeated)"
    )]
    image_mount: Vec<String>,
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

    let container_cli = ContainerCli::from(cli.runtime);
    if let Err(e) = container_cli.check_in_path() {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }

    let ssl_certs = if cli.disable_ssl_certs {
        None
    } else {
        Some(cli.ssl_certs.map(std::path::PathBuf::from))
    };
    let image_mounts: Vec<&str> = cli.image_mount.iter().map(|s| s.as_str()).collect();
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
        &image_mounts,
        &container_cli,
        &ContainerRunner,
    ) {
        eprintln!("Error: {e}");
        std::process::exit(1);
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
    image_mounts: &[&str],
    runtime: &ContainerCli,
    runner: &dyn Runner,
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
    let image_mount_inits: Vec<String> = image_mounts
        .iter()
        .map(|path_or_url| image_mount::load_init(path_or_url))
        .collect::<Result<_, _>>()?;
    let mut seen_names = std::collections::HashSet::new();
    for path_or_url in image_mounts {
        if let Some(name) = image_mount::mount_name(path_or_url)
            && !seen_names.insert(name.clone())
        {
            return Err(format!(
                "--image-mount: duplicate mount name '{name}' derived from '{path_or_url}'"
            )
            .into());
        }
    }
    let output = containerfile::generate(
        &config,
        &containerfile::ContainerfileOptions {
            agent: agent.as_deref(),
            features: &features,
            with_agent_settings: has_agent_settings,
            skill_names: &skill_names,
            env_vars: &agent_env_vars,
            with_policy,
            with_ca_certs: ca_certs_copied,
            image_mount_inits: &image_mount_inits,
        },
    )?;
    build(&output, tag, runtime, runner, context_dir.path())?;
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
            &[],
            &ContainerCli::Podman,
            &FakeRunner(0),
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
            &[],
            &ContainerCli::Podman,
            &FakeRunner(0),
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
            &[],
            &ContainerCli::Podman,
            &FakeRunner(0),
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
            &[],
            &ContainerCli::Podman,
            &FakeRunner(0),
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
            &[],
            &ContainerCli::Podman,
            &FakeRunner(1),
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
            &[],
            &ContainerCli::Podman,
            &FakeRunner(0),
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
            &[],
            &ContainerCli::Podman,
            &FakeRunner(0),
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
            &[],
            &ContainerCli::Podman,
            &FakeRunner(0),
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
            &[],
            &ContainerCli::Podman,
            &FakeRunner(0),
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
            &[],
            &ContainerCli::Podman,
            &FakeRunner(0),
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
    fn run_with_image_mount_from_file_succeeds() {
        let yaml_dir = tempfile::tempdir().unwrap();
        let yaml_path = yaml_dir.path().join("curl.yaml");
        std::fs::write(
            &yaml_path,
            "image: docker.io/curlimages/curl:latest\ninit: export PATH=$MOUNT/usr/bin:$PATH\n",
        )
        .unwrap();
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
            &[yaml_path.to_str().unwrap()],
            &ContainerCli::Podman,
            &FakeRunner(0),
        );
        assert!(result.is_ok(), "expected Ok, got {result:?}");
    }

    #[test]
    fn run_with_multiple_image_mounts_succeeds() {
        let yaml_dir = tempfile::tempdir().unwrap();
        let curl_path = yaml_dir.path().join("curl.yaml");
        let jq_path = yaml_dir.path().join("jq.yaml");
        std::fs::write(
            &curl_path,
            "image: docker.io/curlimages/curl:latest\ninit: export PATH=$MOUNT/usr/bin:$PATH\n",
        )
        .unwrap();
        std::fs::write(
            &jq_path,
            "image: ghcr.io/jqlang/jq:latest\ninit: export PATH=$MOUNT/bin:$PATH\n",
        )
        .unwrap();
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
            &[curl_path.to_str().unwrap(), jq_path.to_str().unwrap()],
            &ContainerCli::Podman,
            &FakeRunner(0),
        );
        assert!(result.is_ok(), "expected Ok, got {result:?}");
    }

    #[test]
    fn run_with_image_mount_invalid_path_returns_error() {
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
            &["/nonexistent/path/tool.yaml"],
            &ContainerCli::Podman,
            &FakeRunner(0),
        );
        assert!(result.is_err(), "expected Err for missing image-mount file");
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
            &[],
            &ContainerCli::Podman,
            &FakeRunner(0),
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
            &[],
            &ContainerCli::Podman,
            &FakeRunner(0),
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
            &[],
            &ContainerCli::Podman,
            &FakeRunner(0),
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
            &[],
            &ContainerCli::Podman,
            &capture,
        )
        .unwrap();
        let cf = capture.0.into_inner().unwrap();
        assert!(
            !cf.contains("COPY certs/"),
            "Containerfile must not contain cert COPY when --disable-ssl-certs is passed"
        );
    }
}
