#!/bin/sh
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

set -eu
root=/usr/local/share/openshell-addon-test
last_fixture=$1
last_variant=$2
expected_copies=$3
expected_names=$4
expected_base=$5
shift 5
test "$(cat "$root/last-applied")" = "$last_fixture:$last_variant"
test -n "${PATH:-}"
env | grep -Fx "OSBI_TEST_VARIANT=$last_variant" > /dev/null
for fixture in single platforms platforms-libc; do
  if ! printf '%s\n' "$expected_names" | tr ' ' '\n' | grep -Fx "$fixture" > /dev/null; then
    test ! -e "$root/$fixture"
  fi
done
while test "$#" -gt 0; do
  fixture=$1
  variant=$2
  shift 2
  case $fixture in
    platforms-libc) expected_libc=${variant##*-} ;;
    *) expected_libc=none ;;
  esac
  expected_architecture=${variant#linux-}
  expected_architecture=${expected_architecture%%-*}
  expected_marker=$(printf 'fixture=%s\nos=linux\narchitecture=%s\nlibc=%s\nvariant=%s\n' \
    "$fixture" "$expected_architecture" "$expected_libc" "$variant")
  test "$(cat "$root/$fixture/selected")" = "$expected_marker"
  test "$(cat "$root/$fixture/variants/$variant")" = "$expected_marker"
  test "$(find "$root/$fixture/variants" -type f | wc -l)" -eq 1
  env_name=$(printf '%s' "$fixture" | tr '[:lower:]-' '[:upper:]_')
  env | grep -Fx "OSBI_TEST_${env_name}=$variant" > /dev/null
done
actual_copies=$(grep '^COPY --from=ghcr.io/openkaiden/tests/' /root/Containerfile || true)
test "$actual_copies" = "$expected_copies"
grep -Fx "FROM $expected_base AS system" /root/Containerfile > /dev/null
