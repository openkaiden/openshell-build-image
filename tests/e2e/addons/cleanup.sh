#!/usr/bin/env bash
# Copyright (C) 2026 Red Hat, Inc.
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
# http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.
#
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail
if [[ $# -ne 1 || ! $1 =~ ^(podman|docker|container)$ ]]; then
  echo "usage: $0 {podman|docker|container}" >&2
  exit 2
fi
: "${E2E_DIR:?}"
runtime=$1
failed=0
if [[ -n ${E2E_NAME:-} ]] && "$runtime" inspect "$E2E_NAME-addon-check" > /dev/null 2>&1; then
  "$runtime" rm --force "$E2E_NAME-addon-check" || failed=1
fi
if [[ -f $E2E_DIR/addon-built-images ]]; then
  while IFS= read -r image; do
    if "$runtime" image inspect "$image" > /dev/null 2>&1; then
      "$runtime" image rm --force "$image" || failed=1
    fi
  done < <(sort -u "$E2E_DIR/addon-built-images")
fi
if [[ -f $E2E_DIR/addon-fixture-refs ]]; then
  while IFS= read -r image; do
    if "$runtime" image inspect "$image" > /dev/null 2>&1; then
      "$runtime" image rm --force "$image" || failed=1
    fi
  done < <(sort -u "$E2E_DIR/addon-fixture-refs")
fi
exit "$failed"
