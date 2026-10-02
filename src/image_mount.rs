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

use std::path::Path;
use std::time::Duration;

use serde::Deserialize;

/// The structure of the images-to-mount YAML file.
/// The `image` field is present in the file but is not used by this tool.
#[derive(Deserialize)]
struct ImageMountFile {
    #[allow(dead_code)]
    image: Option<String>,
    init: Option<String>,
}

/// Derives the mount name from a local path or URL.
///
/// The name is the last path component with any `.yaml` or `.yml` extension
/// stripped. For example `/some/path/curl.yaml` → `"curl"`.
pub fn mount_name(path_or_url: &str) -> Option<String> {
    // For URLs (which always use '/'), extract the last '/'-delimited segment.
    // For local filesystem paths, delegate to std::path::Path so that the
    // OS-native separator (e.g. '\' on Windows) is handled correctly.
    let last = if path_or_url.starts_with("http://") || path_or_url.starts_with("https://") {
        path_or_url.rsplit('/').next()?.to_string()
    } else {
        Path::new(path_or_url)
            .file_name()?
            .to_string_lossy()
            .into_owned()
    };
    let stem = last
        .strip_suffix(".yaml")
        .or_else(|| last.strip_suffix(".yml"))
        .unwrap_or(&last)
        .to_string();
    if stem.is_empty() { None } else { Some(stem) }
}

fn load_yaml_content(path_or_url: &str) -> Result<String, Box<dyn std::error::Error>> {
    if path_or_url.starts_with("http://") || path_or_url.starts_with("https://") {
        let agent = ureq::AgentBuilder::new()
            .timeout_read(Duration::from_secs(30))
            .timeout_write(Duration::from_secs(30))
            .build();
        Ok(agent.get(path_or_url).call()?.into_string()?)
    } else {
        Ok(std::fs::read_to_string(path_or_url)?)
    }
}

/// Loads an images-to-mount YAML file (from a local path or URL) and returns
/// the `init` value with every `$MOUNT` placeholder replaced by
/// `/sandbox/mnt/<name>`, where `<name>` is the file stem of `path_or_url`.
pub fn load_init(path_or_url: &str) -> Result<String, Box<dyn std::error::Error>> {
    let name = mount_name(path_or_url).ok_or_else(|| {
        format!("--image-mount: cannot determine mount name from '{path_or_url}'")
    })?;
    let content = load_yaml_content(path_or_url)?;
    let file: ImageMountFile = serde_yml::from_str(&content)
        .map_err(|e| format!("--image-mount: invalid YAML in '{path_or_url}': {e}"))?;
    let raw_init = file.init.unwrap_or_default();
    let mount = format!("/sandbox/mnt/{name}");
    Ok(raw_init.trim_end().replace("$MOUNT", &mount))
}

#[cfg(test)]
mod tests {
    use super::*;

    // mount_name

    #[test]
    fn mount_name_strips_yaml_extension_from_filename() {
        assert_eq!(mount_name("curl.yaml"), Some("curl".to_string()));
    }

    #[test]
    fn mount_name_strips_yml_extension_from_filename() {
        assert_eq!(mount_name("curl.yml"), Some("curl".to_string()));
    }

    #[test]
    fn mount_name_strips_extension_from_absolute_path() {
        assert_eq!(
            mount_name("/some/path/to/curl.yaml"),
            Some("curl".to_string())
        );
    }

    #[test]
    fn mount_name_strips_extension_from_url() {
        assert_eq!(
            mount_name("https://example.com/curl.yaml"),
            Some("curl".to_string())
        );
    }

    #[test]
    fn mount_name_strips_extension_from_github_raw_url() {
        assert_eq!(
            mount_name("https://raw.githubusercontent.com/feloy/images-to-mount/main/curl.yaml"),
            Some("curl".to_string())
        );
    }

    #[test]
    fn mount_name_returns_stem_without_extension() {
        assert_eq!(mount_name("my-tool"), Some("my-tool".to_string()));
    }

    #[test]
    fn mount_name_returns_none_for_empty_stem() {
        assert_eq!(mount_name(".yaml"), None);
    }

    // load_init

    #[test]
    fn load_init_replaces_mount_placeholder() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("curl.yaml");
        std::fs::write(
            &path,
            "image: docker.io/curlimages/curl:latest\ninit: export PATH=$MOUNT/usr/bin:$PATH\n",
        )
        .unwrap();
        let init = load_init(path.to_str().unwrap()).unwrap();
        assert_eq!(init, "export PATH=/sandbox/mnt/curl/usr/bin:$PATH");
    }

    #[test]
    fn load_init_uses_filename_stem_as_mount_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("my-tool.yaml");
        std::fs::write(&path, "image: example\ninit: source $MOUNT/init.sh\n").unwrap();
        let init = load_init(path.to_str().unwrap()).unwrap();
        assert_eq!(init, "source /sandbox/mnt/my-tool/init.sh");
    }

    #[test]
    fn load_init_trims_trailing_whitespace() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tool.yaml");
        std::fs::write(&path, "image: example\ninit: \"export X=$MOUNT  \"\n").unwrap();
        let init = load_init(path.to_str().unwrap()).unwrap();
        assert_eq!(init, "export X=/sandbox/mnt/tool");
    }

    #[test]
    fn load_init_handles_missing_init_field() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tool.yaml");
        std::fs::write(&path, "image: example\n").unwrap();
        let init = load_init(path.to_str().unwrap()).unwrap();
        assert_eq!(init, "");
    }

    #[test]
    fn load_init_handles_multiline_init() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tool.yaml");
        std::fs::write(
            &path,
            "image: example\ninit: |\n  export PATH=$MOUNT/bin:$PATH\n  export LD_LIBRARY_PATH=$MOUNT/lib:$LD_LIBRARY_PATH\n",
        )
        .unwrap();
        let init = load_init(path.to_str().unwrap()).unwrap();
        assert_eq!(
            init,
            "export PATH=/sandbox/mnt/tool/bin:$PATH\nexport LD_LIBRARY_PATH=/sandbox/mnt/tool/lib:$LD_LIBRARY_PATH"
        );
    }

    #[test]
    fn load_init_fails_on_missing_file() {
        let result = load_init("/nonexistent/path/tool.yaml");
        assert!(result.is_err());
    }

    #[test]
    fn load_init_fails_on_invalid_yaml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tool.yaml");
        std::fs::write(&path, "not: valid: yaml: [[[").unwrap();
        let result = load_init(path.to_str().unwrap());
        assert!(result.is_err());
    }
}
