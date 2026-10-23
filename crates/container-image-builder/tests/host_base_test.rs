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

use std::process::Command;

use container_image_builder::base::{Libc, prepare_base};
use container_image_builder::{ContainerCli, ContainerRunner, build_with_local_images};

#[test]
#[ignore = "requires a running Podman machine and a cached base image"]
fn podman_prepares_and_builds_from_the_same_local_base() {
    let cli = ContainerCli::Podman;
    let runner = ContainerRunner;
    let base = std::env::var("OSBI_BASE_SMOKE_IMAGE")
        .unwrap_or_else(|_| "docker.io/library/alpine:3.24.2".to_owned());
    let expected_libc = match std::env::var("OSBI_BASE_SMOKE_LIBC").as_deref() {
        Ok("glibc") => Libc::Glibc,
        Ok("musl") | Err(_) => Libc::Musl,
        Ok(other) => panic!("unsupported OSBI_BASE_SMOKE_LIBC value: {other}"),
    };
    let prepared = prepare_base(&base, &cli, &runner, true).expect("prepare the base image");
    assert_eq!(prepared.libc, Some(expected_libc));

    let context = tempfile::Builder::new()
        .prefix("osbi-smoke-")
        .tempdir()
        .expect("create build context");
    let tag = format!(
        "localhost/osbi-base-smoke:{}",
        context.path().file_name().unwrap().to_string_lossy()
    );
    let containerfile = format!("FROM {}\nRUN true\n", prepared.local_reference);
    let build_result = build_with_local_images(&containerfile, &tag, &cli, &runner, context.path());
    let _ = Command::new("podman").args(["image", "rm", &tag]).status();
    build_result.expect("build from the inspected local base without pulling");
}
