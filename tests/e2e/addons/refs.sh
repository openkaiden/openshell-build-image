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

# Published by addon-fixtures.yml on main at f324fd76894a5d86942e7ef619b4dd89a0ee6091.
# Update these immutable references only after a new main run publishes fixtures.
ADDON_SINGLE_AMD64=ghcr.io/openkaiden/tests/addon-single@sha256:3855dcd6711aac4708973ae1c7e5f687c93750f69f351701de8a69de89e95a7c
ADDON_SINGLE_ARM64=ghcr.io/openkaiden/tests/addon-single@sha256:e4397b4df12df10f0cd03150220b1f9af5d21d50847990c076056cb63fd59bde
ADDON_PLATFORMS=ghcr.io/openkaiden/tests/addon-platforms@sha256:2bc8a19319e371d2c2d1361a784e915b51759d80a4c906e542c2a2b0d99c8d21
ADDON_PLATFORMS_LIBC=ghcr.io/openkaiden/tests/addon-platforms-libc@sha256:93554c9df264d6a0e02d40dd5bcdfc6e6ed38b5b188a201a7d372efd018e3df1

# Selected child manifests, used to assert that COPY never names an index.
ADDON_PLATFORMS_AMD64=sha256:dbf1c914be9f9134cf12129526d199152bec8cb2a78be1e3b72f5d531d867f59
ADDON_PLATFORMS_ARM64=sha256:5cb4e60eb0b9fcf647d00af952771800bac6ab8e988e3754bcf9cdbe1f348fdd
ADDON_PLATFORMS_LIBC_AMD64_GLIBC=sha256:2d6779560cc7801eb52e47a5db2da772576fdeaa99bbe0be3f92ab3d7e10d4cc
ADDON_PLATFORMS_LIBC_AMD64_MUSL=sha256:f9875227b7d867fa9839b09217c060f7abd18375758c2171d64d2f1244cc66aa
ADDON_PLATFORMS_LIBC_ARM64_GLIBC=sha256:4c14e50b88b0ee8a5d12d863f2abe08dd6ed78873734403d5ff2451cd97ed474
ADDON_PLATFORMS_LIBC_ARM64_MUSL=sha256:3cdec17a00867fcc8f5c2a4f5b5b296d9b0100cf3d76ae766f8a13f535114ac2
