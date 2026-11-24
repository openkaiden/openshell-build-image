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

//! Read OCI metadata for Apple's container backend without pulling image layers.

use std::io::Read;
use std::process::Command;
use std::time::Duration;

use container_image_builder::Runner;
use reqwest::blocking::{Client, Response};
use reqwest::header::{ACCEPT, WWW_AUTHENTICATE};
use reqwest::{StatusCode, Url};
use serde_json::Value;
use sha2::{Digest, Sha256, Sha512};

const MANIFEST_ACCEPT: &str = "application/vnd.oci.image.index.v1+json, application/vnd.docker.distribution.manifest.list.v2+json, application/vnd.oci.image.manifest.v1+json, application/vnd.docker.distribution.manifest.v2+json";
const MAX_METADATA_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug)]
pub struct RegistryError(String);

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for RegistryError {}

fn error(message: impl Into<String>) -> RegistryError {
    RegistryError(message.into())
}

pub struct RegistryDocument {
    pub json: String,
    pub digest: String,
}

struct Reference {
    registry: String,
    endpoint: String,
    repository: String,
    selector: String,
    scheme: &'static str,
}

impl Reference {
    fn parse(image: &str) -> Result<Self, RegistryError> {
        let (name, selector) = if let Some((name, digest)) = image.split_once('@') {
            (split_tag(name).0, digest)
        } else {
            let (name, tag) = split_tag(image);
            (name, tag.unwrap_or("latest"))
        };
        let first = name.split('/').next().unwrap_or(name);
        let explicit_registry = first == "localhost"
            || first.contains('.')
            || first.contains(':')
            || first.starts_with('[');
        let (registry, repository) = if explicit_registry {
            name.split_once('/')
                .ok_or_else(|| error("addon image has no repository after registry host"))?
        } else {
            ("docker.io", name)
        };
        if repository.is_empty()
            || repository
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
            || selector.is_empty()
        {
            return Err(error("invalid addon registry reference"));
        }
        let repository = if registry == "docker.io" && !repository.contains('/') {
            format!("library/{repository}")
        } else {
            repository.to_owned()
        };
        let endpoint = if registry == "docker.io" {
            "registry-1.docker.io"
        } else {
            registry
        };
        let scheme = if endpoint == "localhost"
            || endpoint.starts_with("localhost:")
            || endpoint == "127.0.0.1"
            || endpoint.starts_with("127.0.0.1:")
            || endpoint.starts_with("[::1]")
        {
            "http"
        } else {
            "https"
        };
        Ok(Self {
            registry: registry.to_owned(),
            endpoint: endpoint.to_owned(),
            repository,
            selector: selector.to_owned(),
            scheme,
        })
    }

    fn url(&self, kind: &str, selector: &str) -> Result<Url, RegistryError> {
        let mut url = Url::parse(&format!("{}://{}/", self.scheme, self.endpoint))
            .map_err(|_| error("invalid addon registry host"))?;
        url.path_segments_mut()
            .map_err(|_| error("invalid addon registry URL"))?
            .push("v2")
            .extend(self.repository.split('/'))
            .push(kind)
            .push(selector);
        Ok(url)
    }
}

fn split_tag(image: &str) -> (&str, Option<&str>) {
    let last = image.rsplit('/').next().unwrap_or(image);
    match last.rfind(':') {
        Some(offset) => (
            &image[..image.len() - last.len() + offset],
            Some(&last[offset + 1..]),
        ),
        None => (image, None),
    }
}

pub struct RegistryClient<'a> {
    image: Reference,
    client: Client,
    runner: &'a dyn Runner,
}

impl<'a> RegistryClient<'a> {
    pub fn new(image: &str, runner: &'a dyn Runner) -> Result<Self, RegistryError> {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|err| error(format!("cannot create OCI registry client: {err}")))?;
        Ok(Self {
            image: Reference::parse(image)?,
            client,
            runner,
        })
    }

    pub fn manifest(&self) -> Result<RegistryDocument, RegistryError> {
        let url = self.image.url("manifests", &self.image.selector)?;
        let response = self.get(url, MANIFEST_ACCEPT)?;
        let digest = response
            .headers()
            .get("docker-content-digest")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let bytes = bounded_body(response)?;
        if let Some(header) = &digest {
            verify_digest(&bytes, header)?;
        }
        if self.image.selector.starts_with("sha256:") || self.image.selector.starts_with("sha512:")
        {
            verify_digest(&bytes, &self.image.selector)?;
        }
        let digest = digest.unwrap_or_else(|| format!("sha256:{:x}", Sha256::digest(&bytes)));
        let json = String::from_utf8(bytes)
            .map_err(|_| error("OCI registry returned non-UTF-8 manifest metadata"))?;
        Ok(RegistryDocument { json, digest })
    }

    pub fn config(&self, digest: &str) -> Result<String, RegistryError> {
        let url = self.image.url("blobs", digest)?;
        let bytes = bounded_body(self.get(url, "application/octet-stream")?)?;
        verify_digest(&bytes, digest)?;
        String::from_utf8(bytes).map_err(|_| error("OCI registry returned non-UTF-8 image config"))
    }

    fn get(&self, url: Url, accept: &str) -> Result<Response, RegistryError> {
        let first = self
            .client
            .get(url.clone())
            .header(ACCEPT, accept)
            .send()
            .map_err(|err| error(format!("OCI registry request failed: {err}")))?;
        if first.status().is_success() {
            return Ok(first);
        }
        if first.status() != StatusCode::UNAUTHORIZED {
            return Err(error(format!(
                "OCI registry returned HTTP {} for addon metadata",
                first.status()
            )));
        }
        let challenge = first
            .headers()
            .get(WWW_AUTHENTICATE)
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| error("OCI registry did not provide an authentication challenge"))?;
        let credentials = apple_credentials(&self.image, self.runner);
        let request = if challenge.starts_with("Bearer ") {
            let fields = parse_challenge(challenge)?;
            let realm = fields
                .iter()
                .find(|(key, _)| key == "realm")
                .map(|(_, value)| value.as_str())
                .ok_or_else(|| error("OCI registry bearer challenge has no realm"))?;
            let mut token_url = Url::parse(realm)
                .map_err(|_| error("OCI registry bearer challenge has an invalid realm"))?;
            if token_url.scheme() != "https" && self.image.scheme != "http" {
                return Err(error("OCI registry bearer realm must use HTTPS"));
            }
            {
                let mut query = token_url.query_pairs_mut();
                if let Some(service) = fields
                    .iter()
                    .find(|(key, _)| key == "service")
                    .map(|(_, value)| value.as_str())
                {
                    query.append_pair("service", service);
                }
                let scope = fields
                    .iter()
                    .find(|(key, _)| key == "scope")
                    .map(|(_, value)| value.to_owned())
                    .unwrap_or_else(|| format!("repository:{}:pull", self.image.repository));
                query.append_pair("scope", &scope);
            }
            let mut token_request = self.client.get(token_url);
            if let Some((username, password)) = &credentials {
                token_request = token_request.basic_auth(username, Some(password));
            }
            let token_response = token_request
                .send()
                .map_err(|err| error(format!("OCI registry token request failed: {err}")))?;
            if !token_response.status().is_success() {
                return Err(error(format!(
                    "OCI registry token request returned HTTP {}; check container registry login",
                    token_response.status()
                )));
            }
            let token: Value = serde_json::from_slice(&bounded_body(token_response)?)
                .map_err(|_| error("OCI registry returned an invalid bearer token"))?;
            let token = token
                .get("token")
                .or_else(|| token.get("access_token"))
                .and_then(Value::as_str)
                .filter(|token| !token.is_empty())
                .ok_or_else(|| error("OCI registry returned no bearer token"))?;
            self.client
                .get(url)
                .header(ACCEPT, accept)
                .bearer_auth(token)
        } else if challenge.starts_with("Basic ") {
            let (username, password) = credentials.ok_or_else(|| {
                error("OCI registry needs credentials; use container registry login")
            })?;
            self.client
                .get(url)
                .header(ACCEPT, accept)
                .basic_auth(username, Some(password))
        } else {
            return Err(error("unsupported OCI registry authentication challenge"));
        };
        let response = request
            .send()
            .map_err(|err| error(format!("OCI registry request failed: {err}")))?;
        if response.status().is_success() {
            Ok(response)
        } else {
            Err(error(format!(
                "OCI registry returned HTTP {} for addon metadata",
                response.status()
            )))
        }
    }
}

fn bounded_body(response: Response) -> Result<Vec<u8>, RegistryError> {
    if response
        .content_length()
        .is_some_and(|size| size > MAX_METADATA_BYTES as u64)
    {
        return Err(error("OCI metadata exceeds the 8 MiB limit"));
    }
    let mut bytes = Vec::new();
    response
        .take(MAX_METADATA_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|err| error(format!("cannot read OCI registry response: {err}")))?;
    if bytes.len() > MAX_METADATA_BYTES {
        return Err(error("OCI metadata exceeds the 8 MiB limit"));
    }
    Ok(bytes)
}

fn verify_digest(bytes: &[u8], expected: &str) -> Result<(), RegistryError> {
    let matches = if let Some(hex) = expected.strip_prefix("sha256:") {
        format!("{:x}", Sha256::digest(bytes)) == hex
    } else if let Some(hex) = expected.strip_prefix("sha512:") {
        format!("{:x}", Sha512::digest(bytes)) == hex
    } else {
        return Err(error("unsupported OCI registry digest"));
    };
    if matches {
        Ok(())
    } else {
        Err(error(
            "OCI registry response digest does not match metadata",
        ))
    }
}

fn parse_challenge(header: &str) -> Result<Vec<(String, String)>, RegistryError> {
    let params = header
        .strip_prefix("Bearer ")
        .ok_or_else(|| error("invalid OCI registry bearer challenge"))?;
    let mut fields = Vec::new();
    let mut start = 0;
    let mut quoted = false;
    for (offset, character) in params.char_indices() {
        if character == '"' {
            quoted = !quoted;
        } else if character == ',' && !quoted {
            fields.push(&params[start..offset]);
            start = offset + 1;
        }
    }
    fields.push(&params[start..]);
    fields
        .into_iter()
        .map(|field| {
            let (key, value) = field
                .trim()
                .split_once('=')
                .ok_or_else(|| error("invalid OCI registry bearer challenge field"))?;
            let value = value.trim().trim_matches('"');
            if key.is_empty() || value.is_empty() {
                return Err(error("empty OCI registry bearer challenge field"));
            }
            Ok((key.to_owned(), value.to_owned()))
        })
        .collect()
}

fn apple_credentials(image: &Reference, runner: &dyn Runner) -> Option<(String, String)> {
    let mut list = Command::new("container");
    list.args(["registry", "list", "--format", "json"]);
    let output = runner.output(&mut list).ok()?;
    if !output.status.success() {
        return None;
    }
    let registries: Value = serde_json::from_slice(&output.stdout).ok()?;
    let registry = registries.as_array()?.iter().find(|registry| {
        let name = registry.get("name").and_then(Value::as_str);
        name == Some(&image.endpoint) || name == Some(&image.registry)
    })?;
    let username = registry.get("username")?.as_str()?.to_owned();
    let host = registry.get("name")?.as_str()?;
    let mut security = Command::new("security");
    security.args([
        "find-internet-password",
        "-d",
        "com.apple.container.registry",
        "-s",
        host,
        "-a",
        &username,
        "-w",
    ]);
    let password = runner.output(&mut security).ok()?;
    if !password.status.success() {
        return None;
    }
    let password = String::from_utf8(password.stdout).ok()?;
    Some((username, password.trim_end_matches('\n').to_owned()))
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::process::{Command, ExitStatus};
    use std::thread;

    use super::*;

    struct NoCommands;

    impl Runner for NoCommands {
        fn run(&self, _: &mut Command) -> std::io::Result<ExitStatus> {
            panic!("anonymous registry lookup must not call a container CLI")
        }

        fn output(&self, _: &mut Command) -> std::io::Result<std::process::Output> {
            panic!("anonymous registry lookup must not call a container CLI")
        }
    }

    #[test]
    fn docker_hub_and_local_registry_references_are_parsed() {
        let hub = Reference::parse("alpine:3.24").unwrap();
        assert_eq!(hub.endpoint, "registry-1.docker.io");
        assert_eq!(hub.repository, "library/alpine");
        assert_eq!(hub.selector, "3.24");
        let local = Reference::parse("localhost:5000/team/addon@sha256:abc").unwrap();
        assert_eq!(local.endpoint, "localhost:5000");
        assert_eq!(local.repository, "team/addon");
        assert_eq!(local.selector, "sha256:abc");
        assert_eq!(local.scheme, "http");
        let pinned = Reference::parse("localhost:5000/team/addon:v1@sha256:abc").unwrap();
        assert_eq!(pinned.endpoint, "localhost:5000");
        assert_eq!(pinned.repository, "team/addon");
        assert_eq!(pinned.selector, "sha256:abc");
    }

    #[test]
    fn bearer_challenge_parses_quoted_scope() {
        let fields = parse_challenge(
            "Bearer realm=\"https://auth.example/token\",service=\"registry\",scope=\"repository:a/b:pull\"",
        )
        .unwrap();
        assert_eq!(
            fields[0],
            ("realm".to_owned(), "https://auth.example/token".to_owned())
        );
        assert_eq!(
            fields[2],
            ("scope".to_owned(), "repository:a/b:pull".to_owned())
        );
    }

    #[test]
    fn retrieves_single_manifest_and_config_from_local_registry() {
        let manifest = br#"{"schemaVersion":2,"mediaType":"application/vnd.oci.image.manifest.v1+json","config":{"digest":"ignored"}}"#.to_vec();
        let config =
            br#"{"os":"linux","architecture":"arm64","config":{"Env":["TEST=ok"]}}"#.to_vec();
        let config_digest = format!("sha256:{:x}", Sha256::digest(&config));
        let manifest_digest = format!("sha256:{:x}", Sha256::digest(&manifest));
        let listener = match TcpListener::bind("127.0.0.1:0") {
            Ok(listener) => listener,
            // Some build sandboxes disallow even loopback listeners. The
            // regular unit suite can still run there; CI exercises this path.
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
            Err(error) => panic!("cannot bind test registry: {error}"),
        };
        let port = listener.local_addr().unwrap().port();
        let expected_paths = [
            "/v2/team/addon/manifests/latest".to_owned(),
            format!("/v2/team/addon/blobs/{config_digest}"),
        ];
        let server = thread::spawn(move || {
            for (expected, body) in expected_paths.into_iter().zip([manifest, config]) {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0u8; 4096];
                let size = stream.read(&mut request).unwrap();
                assert!(
                    String::from_utf8_lossy(&request[..size])
                        .starts_with(&format!("GET {expected} HTTP/1.1"))
                );
                let headers = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                stream.write_all(headers.as_bytes()).unwrap();
                stream.write_all(&body).unwrap();
            }
        });

        let client =
            RegistryClient::new(&format!("127.0.0.1:{port}/team/addon:latest"), &NoCommands)
                .unwrap();
        let document = client.manifest().unwrap();
        assert_eq!(document.digest, manifest_digest);
        assert!(document.json.contains("schemaVersion"));
        let config = client.config(&config_digest).unwrap();
        assert!(config.contains("TEST=ok"));
        server.join().unwrap();
    }
}
