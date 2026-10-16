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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Libc {
    Glibc,
    Musl,
}

impl Libc {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Glibc => "glibc",
            Self::Musl => "musl",
        }
    }
}

/// Identify a libc marker by its path relative to an image root filesystem.
/// Tar entries and mounted guest filesystems use the same marker rules.
pub fn libc_marker(path: &Path) -> Option<Libc> {
    let components: Vec<_> = path
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .filter(|component| *component != ".")
        .collect();
    let in_library_dir = matches!(components.first(), Some(&"lib" | &"lib64"))
        || matches!(components.as_slice(), ["usr", "lib" | "lib64", ..]);
    if !in_library_dir {
        return None;
    }
    let name = components.last()?;
    if *name == "libc.so.6" || (name.starts_with("ld-linux") && name.contains(".so.")) {
        Some(Libc::Glibc)
    } else if (name.starts_with("ld-musl-") || name.starts_with("libc.musl-"))
        && name.contains(".so.1")
    {
        Some(Libc::Musl)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers_are_scoped_to_image_library_directories() {
        assert_eq!(libc_marker(Path::new("lib/libc.so.6")), Some(Libc::Glibc));
        assert_eq!(
            libc_marker(Path::new("usr/lib/aarch64-linux-gnu/ld-linux-aarch64.so.1")),
            Some(Libc::Glibc)
        );
        assert_eq!(
            libc_marker(Path::new("lib/ld-musl-aarch64.so.1")),
            Some(Libc::Musl)
        );
        assert_eq!(libc_marker(Path::new("tmp/libc.so.6")), None);
    }
}
