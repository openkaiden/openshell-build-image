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

if [[ $# -ne 2 || ! $1 =~ ^(podman|docker|container)$ || ! $2 =~ ^(ubuntu|alpine)$ ]]; then
  echo "usage: $0 {podman|docker|container} {ubuntu|alpine}" >&2
  exit 2
fi
: "${E2E_DIR:?}"
: "${E2E_ARTIFACTS:?}"
: "${E2E_NAME:?}"
: "${E2E_IMAGE:?}"
: "${E2E_BASE_IMAGE:?}"
: "${GITHUB_WORKSPACE:?}"
: "${GITHUB_ENV:?}"

runtime=$1
base_kind=$2
script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
source "$script_dir/refs.sh"
case $(uname -m) in
  x86_64) architecture=amd64; opposite_architecture=arm64 ;;
  arm64|aarch64) architecture=arm64; opposite_architecture=amd64 ;;
  *) echo "unsupported E2E host architecture: $(uname -m)" >&2; exit 1 ;;
esac
case $base_kind in
  ubuntu) libc=glibc ;;
  alpine) libc=musl ;;
esac
architecture_upper=$(printf '%s' "$architecture" | tr '[:lower:]' '[:upper:]')
opposite_architecture_upper=$(printf '%s' "$opposite_architecture" | tr '[:lower:]' '[:upper:]')
libc_upper=$(printf '%s' "$libc" | tr '[:lower:]' '[:upper:]')

case $E2E_ARTIFACTS in
  /*) artifacts="$E2E_ARTIFACTS/addons" ;;
  *) artifacts="$GITHUB_WORKSPACE/$E2E_ARTIFACTS/addons" ;;
esac
mkdir -p "$artifacts" "$E2E_DIR/addon-runtime"
cp "$script_dir/refs.sh" "$artifacts/refs.sh"
real_runtime=$(command -v "$runtime")
cat > "$E2E_DIR/addon-runtime/$runtime" <<'SH'
#!/usr/bin/env bash
printf '%s\n' "$1" >> "$OSBI_E2E_COMMAND_LOG"
exec "$OSBI_E2E_REAL_RUNTIME" "$@"
SH
chmod 700 "$E2E_DIR/addon-runtime/$runtime"

single_name="ADDON_SINGLE_$architecture_upper"
opposite_name="ADDON_SINGLE_$opposite_architecture_upper"
single_ref=${!single_name}
opposite_ref=${!opposite_name}
printf '%s\n' "$single_ref" "$opposite_ref" "$ADDON_PLATFORMS" \
  "$ADDON_PLATFORMS_LIBC" > "$E2E_DIR/addon-fixture-refs"

fixture_ref() {
  case $1 in
    single) printf '%s' "$single_ref" ;;
    platforms) printf '%s' "$ADDON_PLATFORMS" ;;
    platforms-libc) printf '%s' "$ADDON_PLATFORMS_LIBC" ;;
  esac
}

fixture_variant() {
  case $1 in
    platforms-libc) printf 'linux-%s-%s' "$architecture" "$libc" ;;
    *) printf 'linux-%s' "$architecture" ;;
  esac
}

selected_ref() {
  local fixture=$1 name digest parent
  parent=$(fixture_ref "$fixture")
  case $fixture in
    single) printf '%s' "$parent"; return ;;
    platforms) name="ADDON_PLATFORMS_$architecture_upper" ;;
    platforms-libc) name="ADDON_PLATFORMS_LIBC_${architecture_upper}_$libc_upper" ;;
  esac
  digest=${!name}
  printf '%s@%s' "${parent%@*}" "$digest"
}

image_environment() {
  "$runtime" image inspect "$1" | jq -r --arg architecture "$architecture" \
    --arg digest "${2:-}" '
    .[0] |
    if has("variants") then
      .variants[] |
      select(.platform.os == "linux" and .platform.architecture == $architecture) |
      select($digest == "" or .digest == $digest) |
      .config.config.Env[]?
    else
      .Config.Env[]?
    end
  '
}

count_probes() {
  awk '$0 == "export" { n++ } END { print n+0 }' "$1"
}

assert_probes() {
  local log=$1 expected=$2 actual
  actual=$(count_probes "$log")
  if [[ $actual -ne $expected ]]; then
    echo "expected $expected base libc probes, got $actual in $log" >&2
    exit 1
  fi
}

verify_script=$(cat "$script_dir/assert-image.sh")

check_name="$E2E_NAME-addon-check"
remove_check() {
  "$runtime" rm --force "$check_name" > /dev/null 2>&1 || true
}
trap remove_check EXIT

build_case() {
  local case_name=$1 expected_probes=$2 keep_image=$3
  shift 3
  local fixtures=("$@") image="$E2E_IMAGE-addon-$case_name"
  local options=() pairs=() copies='' fixture variant selected last_fixture='' last_variant=''
  printf '%s\n' "$image" >> "$E2E_DIR/addon-built-images"
  for fixture in "${fixtures[@]}"; do
    options+=(--addon "$(fixture_ref "$fixture")")
    variant=$(fixture_variant "$fixture")
    selected=$(selected_ref "$fixture")
    pairs+=("$fixture" "$variant")
    [[ -z $copies ]] || copies+=$'\n'
    copies+="COPY --from=$selected / /"
    printf '%s\n' "$selected" >> "$E2E_DIR/addon-fixture-refs"
    last_fixture=$fixture
    last_variant=$variant
  done
  local command_log="$artifacts/$case_name.commands.log"
  : > "$command_log"
  echo "building addon case $case_name ($runtime, $base_kind)"
  (cd "$E2E_DIR"; PATH="$E2E_DIR/addon-runtime:$PATH" \
    OSBI_E2E_REAL_RUNTIME="$real_runtime" OSBI_E2E_COMMAND_LOG="$command_log" \
    "$GITHUB_WORKSPACE/target/debug/openshell-build-image" \
      --runtime "$runtime" --from "$E2E_BASE_IMAGE" --copy-containerfile \
      "${options[@]}" -v "$image") > "$artifacts/$case_name.build.log" 2>&1
  assert_probes "$command_log" "$expected_probes"
  image_environment "$E2E_BASE_IMAGE" | sort -u > "$artifacts/base-environment.txt"
  image_environment "$image" | sort -u > "$artifacts/$case_name.environment.txt"
  test -s "$artifacts/base-environment.txt"
  local missing_base_environment expected_path actual_path addon_environment entry
  missing_base_environment=$(comm -23 \
    <(sed '/^PATH=/d' "$artifacts/base-environment.txt") \
    <(sed '/^PATH=/d' "$artifacts/$case_name.environment.txt"))
  if [[ -n $missing_base_environment ]]; then
    echo "addon case $case_name lost base environment entries: $missing_base_environment" >&2
    exit 1
  fi
  expected_path=$(sed -n 's/^PATH=//p' "$artifacts/base-environment.txt")
  for fixture in "${fixtures[@]}"; do
    selected=$(selected_ref "$fixture")
    addon_environment=$(image_environment "$selected" "${selected##*@}")
    while IFS= read -r entry; do
      if [[ $entry == PATH=* ]]; then
        expected_path+=":${entry#PATH=}"
      fi
    done <<< "$addon_environment"
  done
  actual_path=$(sed -n 's/^PATH=//p' "$artifacts/$case_name.environment.txt")
  if [[ $actual_path != "$expected_path" ]]; then
    echo "addon case $case_name PATH mismatch: expected $expected_path, got $actual_path" >&2
    exit 1
  fi
  local run_options=(--name "$check_name" --entrypoint /bin/sh)
  if [[ $runtime != container ]]; then run_options+=(--network none); fi
  "$runtime" run "${run_options[@]}" "$image" -c "$verify_script" addon-check \
    "$last_fixture" "$last_variant" "$copies" "${fixtures[*]}" "${pairs[@]}" \
    > "$artifacts/$case_name.inspect.log" 2>&1
  # Keep the platforms-libc result as E2E_SOURCE for the later OpenShell sandbox.
  # Apple container supplies a rootfs tarball; Podman and Docker supply an image tag.
  if [[ $keep_image == yes ]]; then
    if [[ $runtime == container ]]; then
      # Apple's OpenShell driver consumes a rootfs tar, and export needs a run first.
      "$runtime" export --output "$E2E_DIR/addon-image.tar" "$check_name"
      tar -tf "$E2E_DIR/addon-image.tar" > "$artifacts/image-contents.log"
      grep -Eq '(^|/)usr/local/share/openshell-addon-test/platforms-libc/selected$' \
        "$artifacts/image-contents.log"
      echo "E2E_SOURCE=$E2E_DIR/addon-image.tar" >> "$GITHUB_ENV"
      rm -f -- "$E2E_DIR/image.tar"
    else
      echo "E2E_SOURCE=$image" >> "$GITHUB_ENV"
    fi
  fi
  remove_check
  if [[ $keep_image != yes ]]; then "$runtime" image rm --force "$image"; fi
}

expect_failure() {
  local case_name=$1 from=$2 addon=$3 expected_error=$4 expected_probes=$5
  local image="$E2E_IMAGE-addon-$case_name" command_log="$artifacts/$case_name.commands.log"
  printf '%s\n' "$image" >> "$E2E_DIR/addon-built-images"
  : > "$command_log"
  if (cd "$E2E_DIR"; PATH="$E2E_DIR/addon-runtime:$PATH" \
    OSBI_E2E_REAL_RUNTIME="$real_runtime" OSBI_E2E_COMMAND_LOG="$command_log" \
    "$GITHUB_WORKSPACE/target/debug/openshell-build-image" \
      --runtime "$runtime" --from "$from" --addon "$addon" -v "$image") \
      > "$artifacts/$case_name.build.log" 2>&1; then
    echo "addon case $case_name unexpectedly succeeded" >&2
    exit 1
  fi
  grep -F "$expected_error" "$artifacts/$case_name.build.log"
  assert_probes "$command_log" "$expected_probes"
  if "$runtime" image inspect "$image" > /dev/null 2>&1; then
    echo "addon case $case_name left an output image" >&2
    exit 1
  fi
}

build_non_native() {
  local image="$E2E_IMAGE-addon-non-native"
  local command_log="$artifacts/non-native.commands.log"
  local marker="$artifacts/non-native-platforms/selected"
  printf '%s\n' "$image" >> "$E2E_DIR/addon-built-images"
  : > "$command_log"
  echo "building addon case non-native ($runtime, linux/$opposite_architecture)"
  (cd "$E2E_DIR"; PATH="$E2E_DIR/addon-runtime:$PATH" \
    OSBI_E2E_REAL_RUNTIME="$real_runtime" OSBI_E2E_COMMAND_LOG="$command_log" \
    "$GITHUB_WORKSPACE/target/debug/openshell-build-image" \
      --runtime "$runtime" --from "$opposite_ref" --addon "$ADDON_PLATFORMS" \
      -v "$image") > "$artifacts/non-native.build.log" 2>&1
  assert_probes "$command_log" 0
  "$runtime" image inspect "$image" | jq -e --arg architecture "$opposite_architecture" '
    .[0].Os == "linux" and .[0].Architecture == $architecture
  ' > /dev/null
  image_environment "$image" > "$artifacts/non-native.environment.txt"
  grep -Fx "OSBI_TEST_SINGLE=linux-$opposite_architecture" \
    "$artifacts/non-native.environment.txt"
  grep -Fx "OSBI_TEST_PLATFORMS=linux-$opposite_architecture" \
    "$artifacts/non-native.environment.txt"
  grep -Fx "OSBI_TEST_VARIANT=linux-$opposite_architecture" \
    "$artifacts/non-native.environment.txt"
  "$runtime" create --name "$check_name" --network none "$image" /bin/true \
    > "$artifacts/non-native.create.log"
  "$runtime" cp \
    "$check_name:/usr/local/share/openshell-addon-test/platforms" \
    "$artifacts/non-native-platforms"
  "$runtime" cp \
    "$check_name:/usr/local/share/openshell-addon-test/last-applied" \
    "$artifacts/non-native-last-applied"
  test "$(cat "$artifacts/non-native-last-applied")" = \
    "platforms:linux-$opposite_architecture"
  test "$(cat "$marker")" = "$(printf \
    'fixture=platforms\nos=linux\narchitecture=%s\nlibc=none\nvariant=linux-%s\n' \
    "$opposite_architecture" "$opposite_architecture")"
  test "$(cat "$artifacts/non-native-platforms/variants/linux-$opposite_architecture")" = \
    "$(cat "$marker")"
  test "$(find "$artifacts/non-native-platforms/variants" -type f | wc -l)" -eq 1
  remove_check
  "$runtime" image rm --force "$image"
}

build_case single 0 no single
build_case platforms 0 no platforms
build_case platforms-libc 1 yes platforms-libc
if [[ $base_kind == ubuntu ]]; then
  build_case forward 1 no single platforms platforms-libc
  build_case reverse 1 no platforms-libc single
  build_case repeat 1 no platforms-libc platforms-libc
  if [[ $runtime != container ]]; then build_non_native; fi
  expect_failure unsupported "$E2E_BASE_IMAGE" "$opposite_ref" \
    'no addon variant matches' 0
  expect_failure missing-libc "$single_ref" "$ADDON_PLATFORMS_LIBC" \
    'cannot determine whether base image' 1
fi
