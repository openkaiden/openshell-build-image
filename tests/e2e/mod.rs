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

//! Assertions against the sandbox created by the E2E workflow's shell steps.

use regex::Regex;
use serde_json::Value;
use std::{
    env,
    error::Error,
    fs::{self, File},
    io,
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub type Result<T> = std::result::Result<T, Box<dyn Error>>;
const ROOT: &str = env!("CARGO_MANIFEST_DIR");
const CURL_POLICY_DENIAL_EXIT_CODES: [i32; 4] = [7, 22, 35, 56];

fn require(condition: bool, message: impl Into<String>) -> Result<()> {
    if !condition {
        return Err(io::Error::other(message.into()).into());
    }
    Ok(())
}

fn policy_with_curl_allowed(policy: &str) -> Result<String> {
    const DENIED: &str = "network_policies: {}";
    const ALLOWED: &str = concat!(
        "network_policies:\n",
        "  curl_example:\n",
        "    endpoints:\n",
        "      - host: example.com\n",
        "        port: 443\n",
        "    binaries:\n",
        "      - path: /usr/bin/curl",
    );
    require(
        policy.lines().filter(|line| *line == DENIED).count() == 1,
        "tests/e2e/policy.yaml must contain exactly one standalone \
         'network_policies: {}' line before curl can be approved",
    )?;
    Ok(policy
        .split_inclusive('\n')
        .map(|line| {
            if line.trim_end_matches(['\r', '\n']) == DENIED {
                line.replacen(DENIED, ALLOWED, 1)
            } else {
                line.to_owned()
            }
        })
        .collect())
}

fn is_policy_denial(code: i32, logs: &str) -> bool {
    // Never accept DNS (6), timeout (28), certificate (60), or missing curl (127).
    CURL_POLICY_DENIAL_EXIT_CODES.contains(&code)
        && Regex::new(r"NET:OPEN\s+\[[^]\r\n]+\]\s+DENIED\s+/usr/bin/curl\([^)]*\)\s+->\s+example\.com:443(?:\s|$)")
            .expect("valid denial pattern")
            .is_match(logs)
}

#[derive(Debug)]
struct Output {
    code: i32,
    stdout: String,
    stderr: String,
}

// Every command owns a process group, including its descendants. Drop also covers
// early returns and test panics, so a timed-out command does not outlive the assertion.
struct Process(Child);
impl Process {
    fn group_gone(&mut self) -> Result<bool> {
        // Reap the leader, but its exit does not imply that its descendants exited.
        self.0.try_wait()?;
        // SAFETY: commands start in a group whose ID is the child's PID.
        let group = -(self.0.id() as i32);
        if unsafe { libc::kill(group, 0) } == 0 {
            return Ok(false);
        }
        let error = io::Error::last_os_error();
        match error.raw_os_error() {
            Some(libc::ESRCH) => Ok(true),
            // EPERM means the group exists, although not every member can be
            // signalled. Keep trying to stop it until the cleanup deadline.
            Some(libc::EPERM) => Ok(false),
            _ => Err(io::Error::new(
                error.kind(),
                format!("checking process group {}: {error}", -group),
            )
            .into()),
        }
    }

    fn stop(&mut self) -> Result<()> {
        if self.group_gone()? {
            return Ok(());
        }
        let group = -(self.0.id() as i32);
        let mut permission_denied = false;
        for (signal, seconds) in [(libc::SIGTERM, 15), (libc::SIGKILL, 5)] {
            let deadline = Instant::now() + Duration::from_secs(seconds);
            let mut signalled = false;
            while Instant::now() < deadline {
                if self.group_gone()? {
                    return Ok(());
                }
                if !signalled {
                    // SAFETY: the child was started in its own process group; a
                    // negative PID addresses that group, not the test runner.
                    if unsafe { libc::kill(group, signal) } == 0 {
                        signalled = true;
                        permission_denied = false;
                    } else {
                        let error = io::Error::last_os_error();
                        match error.raw_os_error() {
                            Some(libc::ESRCH) => return Ok(()),
                            Some(libc::EPERM) => permission_denied = true,
                            Some(libc::EINTR) => {}
                            _ => {
                                return Err(io::Error::new(
                                    error.kind(),
                                    format!("signalling process group {}: {error}", -group),
                                )
                                .into());
                            }
                        }
                    }
                }
                thread::sleep(Duration::from_millis(100));
            }
        }
        let reason = if permission_denied {
            " (signalling was denied)"
        } else {
            ""
        };
        Err(io::Error::other(format!("process group {} did not stop{reason}", -group)).into())
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        if let Err(error) = self.stop() {
            eprintln!("Process cleanup: {error}");
        }
    }
}

struct Scenario {
    state: PathBuf,
    artifacts: PathBuf,
    launcher: PathBuf,
    name: String,
}

impl Scenario {
    fn from_env() -> Result<Self> {
        let state = fs::canonicalize(env::var("E2E_DIR")?)?;
        let artifacts = PathBuf::from(env::var("E2E_ARTIFACTS")?);
        fs::create_dir_all(&artifacts)?;
        Ok(Self {
            launcher: state.join("osenv"),
            state,
            artifacts: fs::canonicalize(artifacts)?,
            name: env::var("E2E_NAME")?,
        })
    }

    fn command_output(
        &self,
        args: &[&str],
        label: &str,
        timeout: u64,
        check: bool,
    ) -> Result<Output> {
        // File-backed output avoids filling a pipe while waiting for the command.
        let stdout = tempfile::tempfile()?;
        let stderr = tempfile::tempfile()?;
        let mut process = Process(
            Command::new(&self.launcher)
                .arg(self.state.join("bin/openshell"))
                .args(args)
                .stdin(Stdio::null())
                .process_group(0)
                .current_dir(&self.state)
                .stdout(stdout.try_clone()?)
                .stderr(stderr.try_clone()?)
                .spawn()?,
        );
        let deadline = Instant::now() + Duration::from_secs(timeout);
        let status: Result<_> = loop {
            if let Some(status) = process.0.try_wait()? {
                break Ok(status);
            }
            if Instant::now() >= deadline {
                break Err(io::Error::other(format!("{label} exceeded {timeout}s")).into());
            }
            thread::sleep(Duration::from_millis(100));
        };
        process.stop()?;
        use std::io::{Read, Seek};
        let read = |mut file: File| -> Result<String> {
            file.rewind()?;
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)?;
            Ok(String::from_utf8_lossy(&bytes).into_owned())
        };
        let output = Output {
            code: status.as_ref().ok().and_then(|s| s.code()).unwrap_or(-1),
            stdout: read(stdout)?,
            stderr: read(stderr)?,
        };
        fs::write(
            self.artifacts.join(format!("{label}.log")),
            format!(
                "exit_code={}\nstdout:\n{}\nstderr:\n{}",
                output.code, output.stdout, output.stderr
            ),
        )?;
        status?;
        require(
            !check || output.code == 0,
            format!(
                "{label} exited {}:\n{}\n{}",
                output.code, output.stdout, output.stderr
            ),
        )?;
        Ok(output)
    }

    fn cli(&self, args: &[&str], label: &str, timeout: u64, check: bool) -> Result<Output> {
        self.command_output(args, label, timeout, check)
    }

    fn poll(
        &self,
        description: &str,
        seconds: u64,
        mut callback: impl FnMut(&Self) -> Result<bool>,
    ) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(seconds);
        while Instant::now() < deadline {
            if callback(self)? {
                return Ok(());
            }
            thread::sleep(Duration::from_secs(2));
        }
        Err(io::Error::other(format!(
            "Timed out waiting for {description}; see {}",
            self.artifacts.display()
        ))
        .into())
    }

    fn exec(&self, command: &[&str], label: &str, check: bool) -> Result<Output> {
        let mut args = vec![
            "sandbox",
            "exec",
            "--name",
            &self.name,
            "--no-tty",
            "--no-login-shell",
            "--timeout",
            "30",
            "--",
        ];
        args.extend_from_slice(command);
        self.cli(&args, label, 45, check)
    }

    fn test(&self) -> Result<()> {
        let expected_version = env::var("E2E_VERSION").map_err(|error| {
            io::Error::other(format!(
                "E2E_VERSION must specify the installed OpenShell version: {error}"
            ))
        })?;
        require(
            !expected_version.is_empty(),
            "E2E_VERSION must not be empty",
        )?;
        let driver = env::var("E2E_DRIVER")?;
        require(
            matches!(driver.as_str(), "podman" | "vm"),
            "E2E_DRIVER must be podman or vm",
        )?;
        let version = self.cli(&["--version"], "openshell-version", 15, true)?;
        require(
            version.stdout.split_whitespace().last() == Some(expected_version.as_str()),
            format!("Expected OpenShell {expected_version}: {}", version.stdout),
        )?;
        let info = self.cli(
            &["gateway", "info", "--output", "json"],
            "gateway-info",
            30,
            true,
        )?;
        let data: Value = serde_json::from_str(&info.stdout)?;
        require(
            data["compute_drivers"]
                .as_array()
                .is_some_and(|drivers| drivers.len() == 1 && drivers[0]["name"] == driver),
            format!("Wrong compute driver: {}", info.stdout),
        )?;
        let policy = Path::new(ROOT).join("tests/e2e/policy.yaml");
        let allowed_policy = policy_with_curl_allowed(&fs::read_to_string(policy)?)?;
        for (flag, label) in [
            ("-u", "workload-uid"),
            ("-g", "workload-gid"),
            ("-G", "workload-groups"),
        ] {
            let identity = self.exec(&["/usr/bin/id", flag], label, true)?;
            require(
                identity.stdout.trim() == "10001",
                format!(
                    "Expected non-root workload identity 10001: {}",
                    identity.stdout
                ),
            )?;
        }
        self.exec(&["/usr/bin/curl", "--version"], "curl-version", true)?;
        let curl = [
            "/usr/bin/curl",
            "--silent",
            "--show-error",
            "--fail",
            "--connect-timeout",
            "5",
            "--max-time",
            "15",
            "https://example.com/",
        ];
        let blocked = self.exec(&curl, "curl-blocked", false)?;
        require(
            CURL_POLICY_DENIAL_EXIT_CODES.contains(&blocked.code),
            format!(
                "Expected policy rejection, got {}: {}",
                blocked.code, blocked.stderr
            ),
        )?;
        self.poll("curl policy denial for example.com:443", 45, |s| {
            let logs = s.cli(
                &["logs", &s.name, "--source", "sandbox", "-n", "500"],
                "denial-logs",
                30,
                false,
            )?;
            Ok(logs.code == 0 && is_policy_denial(blocked.code, &logs.stdout))
        })?;
        println!("Confirmed curl was denied by OpenShell policy");
        let allowed = self.state.join("allowed.yaml");
        fs::write(&allowed, allowed_policy)?;
        self.cli(
            &[
                "policy",
                "set",
                &self.name,
                "--policy",
                path(&allowed),
                "--wait",
                "--timeout",
                "120",
            ],
            "policy-allow",
            150,
            true,
        )?;
        self.cli(
            &["sandbox", "get", &self.name, "--policy-only"],
            "effective-policy",
            30,
            true,
        )?;
        let result = self.exec(&curl, "curl-allowed", true)?;
        require(
            result.stdout.contains("Example Domain"),
            "Approved curl did not fetch expected website",
        )?;
        println!("Confirmed the same curl succeeds after approval");
        Ok(())
    }
}

fn path(path: &Path) -> &str {
    path.to_str().expect("E2E paths must be UTF-8")
}

pub fn run() -> Result<()> {
    Scenario::from_env()?.test()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    const DENIED: &str = "[1234.567] [sandbox] [OCSF ] [ocsf] NET:OPEN [MED] DENIED /usr/bin/curl(4711) -> example.com:443 [reason:transparent_tcp_policy_denied]";
    #[test]
    fn allowed_policy_updates_fixture_without_rewriting_comments() -> Result<()> {
        let comment = "# Keep network_policies: {} in this comment\n";
        let fixture = include_str!("policy.yaml");
        let allowed = policy_with_curl_allowed(&format!("{comment}{fixture}"))?;
        assert!(allowed.starts_with(comment));
        assert!(allowed.contains("      - host: example.com\n        port: 443"));
        assert!(allowed.contains("      - path: /usr/bin/curl"));
        assert!(!allowed.lines().any(|line| line == "network_policies: {}"));
        Ok(())
    }
    #[test]
    fn allowed_policy_rejects_missing_or_duplicate_network_section() {
        for policy in [
            "network_policies:\n  existing_rule: {}\n",
            "# network_policies: {}\n",
            "network_policies: {}\nnetwork_policies: {}\n",
        ] {
            let error = policy_with_curl_allowed(policy).unwrap_err().to_string();
            assert!(error.contains("tests/e2e/policy.yaml"));
            assert!(error.contains("exactly one standalone 'network_policies: {}'"));
        }
    }
    #[test]
    fn matching_denial() {
        for code in [7, 22, 35, 56] {
            assert!(is_policy_denial(code, DENIED));
        }
    }
    #[test]
    fn success_is_never_a_denial() {
        assert!(!is_policy_denial(0, DENIED));
    }
    #[test]
    fn infrastructure_failures_are_never_denials() {
        for code in [6, 28, 60, 127, -15] {
            assert!(!is_policy_denial(code, DENIED));
        }
    }
    #[test]
    fn failure_without_policy_evidence() {
        assert!(!is_policy_denial(7, "Connection refused"));
    }
    #[test]
    fn other_destination_or_process() {
        for logs in [
            DENIED.replace("example.com", "other.example.com"),
            DENIED.replace(":443", ":4430"),
            DENIED.replace("/usr/bin/curl", "/usr/bin/python3"),
            DENIED.replace("DENIED", "ALLOWED"),
        ] {
            assert!(!is_policy_denial(7, &logs));
        }
    }
    #[test]
    fn destination_and_denial_must_share_one_event() {
        assert!(!is_policy_denial(
            7,
            &(DENIED.replace("example.com", "other.example.com") + "\nexample.com:443")
        ));
    }
    #[test]
    fn cleanup_stops_group_members_after_leader_exits() -> Result<()> {
        let mut leader = Process(
            Command::new("sleep")
                .arg("30")
                .process_group(0)
                .spawn()
                .map_err(|error| {
                    io::Error::new(error.kind(), format!("spawning group leader: {error}"))
                })?,
        );
        // Keep the other group member as our child so it can be reaped even on
        // systems whose init process does not promptly reap orphaned processes.
        let mut member = Command::new("sleep")
            .arg("30")
            .process_group(leader.0.id() as i32)
            .spawn()
            .map_err(|error| {
                io::Error::new(error.kind(), format!("spawning group member: {error}"))
            })?;
        leader.0.kill().map_err(|error| {
            io::Error::new(error.kind(), format!("killing group leader: {error}"))
        })?;
        leader.0.wait().map_err(|error| {
            io::Error::new(error.kind(), format!("reaping group leader: {error}"))
        })?;
        assert!(!leader.group_gone()?);
        let (sender, receiver) = mpsc::channel();
        let reaper = thread::spawn(move || {
            let _ = sender.send(member.wait());
        });
        let stopped = leader.stop();
        let exited = receiver.recv_timeout(Duration::from_secs(2));
        // Also clean up on regression: the old stop() returned without signalling.
        if exited.is_err() {
            // SAFETY: this is the isolated group created above.
            unsafe { libc::kill(-(leader.0.id() as i32), libc::SIGKILL) };
        }
        reaper.join().expect("process reaper panicked");
        stopped?;
        assert!(!exited??.success(), "group member exited without a signal");
        assert!(leader.group_gone()?);
        Ok(())
    }
    #[test]
    fn timeout_stops_command_and_retains_diagnostics() -> Result<()> {
        let state = tempfile::tempdir()?;
        let scenario = Scenario {
            state: state.path().to_owned(),
            artifacts: state.path().to_owned(),
            name: "timeout-test".into(),
            // /bin/sh uses the dummy CLI file as a script in this unit test.
            launcher: "/bin/sh".into(),
        };
        fs::create_dir(state.path().join("bin"))?;
        fs::write(
            state.path().join("bin/openshell"),
            "echo started; exec sleep 30",
        )?;
        let result = scenario.command_output(&[], "timeout-test", 1, true);
        let error = result.unwrap_err();
        assert!(
            error.to_string().contains("exceeded 1s"),
            "unexpected command error: {error}"
        );
        assert!(
            fs::read_to_string(scenario.artifacts.join("timeout-test.log"))?.contains("started")
        );
        Ok(())
    }
}
