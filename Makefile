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
#
# Shortcuts for the things that take more than one command, which on this
# project means everything to do with `--runtime vm`: it needs four
# environment variables, an embedded root filesystem, and a signature that
# every rebuild throws away.
#
# Targets come in pairs: build / build-vm, run / run-vm, check / check-vm.
# The -vm half compiles the libkrun FFI code, which the default feature set
# leaves out, and works only on an Apple Silicon Mac.

BINARY := target/release/openshell-image-builder
VM_ROOTFS_DIR := vm-rootfs
VM_ROOTFS_ARCHIVE := $(CURDIR)/vm-rootfs.tar
BREW_PREFIX := $(shell brew --prefix 2>/dev/null)

# Runtime for `make run`. The vm one has a target of its own.
RUNTIME ?= podman

# krun-sys generates its bindings with bindgen, which needs libclang. Prefer a
# Homebrew llvm, else the copy every Xcode Command Line Tools install carries.
ifneq ($(wildcard $(BREW_PREFIX)/opt/llvm/lib),)
LIBCLANG_PATH := $(BREW_PREFIX)/opt/llvm/lib
else
LIBCLANG_PATH := $(shell xcode-select -p 2>/dev/null)/usr/lib
endif

# Everything that compiles the FFI code needs these: bindgen has to find
# libclang, and the linker has to find libkrun through pkg-config.
VM_ENV := LIBCLANG_PATH="$(LIBCLANG_PATH)" \
	DYLD_FALLBACK_LIBRARY_PATH="$(LIBCLANG_PATH):$(BREW_PREFIX)/lib" \
	PKG_CONFIG_PATH="$(BREW_PREFIX)/lib/pkgconfig"

# Embed the rootfs when one has been built. Without it the binary still
# compiles, and then needs --vm-rootfs at run time.
ifneq ($(wildcard $(VM_ROOTFS_ARCHIVE)),)
VM_ARCHIVE_ENV := OPENSHELL_IMAGE_BUILDER_VM_ROOTFS_ARCHIVE=$(VM_ROOTFS_ARCHIVE)
endif

.PHONY: help
help:
	@echo "build              Build the binary"
	@echo "build-vm           Build with VM support and sign it, embedding the"
	@echo "                   rootfs from '$(notdir $(VM_ROOTFS_ARCHIVE))' when there is one"
	@echo "build-vm-rootfs    Build the root filesystem the build VM boots from"
	@echo "                   (needs podman on linux/arm64)"
	@echo ""
	@echo "run                Build an image: make run TAG=myimage:latest [RUNTIME=docker]"
	@echo "run-vm             The same through a VM: make run-vm TAG=myimage:latest"
	@echo "                   (the exported filesystem lands under target/)"
	@echo ""
	@echo "check              Format, lint and test — run this before committing"
	@echo "check-vm           The same suite with VM support compiled in"
	@echo "fmt                Apply rustfmt"
	@echo ""
	@echo "clean              Remove build output"
	@echo "clean-vm           Also remove the rootfs, its archive, and the unpacked copy"

.PHONY: build
build:
	cargo build --release

# Signing is part of building, not a step to remember: macOS refuses
# Hypervisor.framework to an unsigned binary, and compiling drops the
# signature every time.
.PHONY: build-vm
build-vm:
ifeq ($(VM_ARCHIVE_ENV),)
	@echo "note: no $(notdir $(VM_ROOTFS_ARCHIVE)) to embed — run 'make build-vm-rootfs' first, or pass --vm-rootfs when running"
endif
	$(VM_ARCHIVE_ENV) $(VM_ENV) cargo build --release --features vm
	codesign --sign - --entitlements entitlements.plist --force $(BINARY)
	@codesign -d --entitlements :- $(BINARY) 2>/dev/null | grep -q hypervisor \
		&& echo "signed with the hypervisor entitlement"

.PHONY: build-vm-rootfs
build-vm-rootfs:
	./crates/vm-image-builder/vm-image/make-rootfs.sh $(VM_ROOTFS_DIR) $(VM_ROOTFS_ARCHIVE)

.PHONY: run
run: build
ifndef TAG
	$(error TAG is required: make run TAG=myimage:latest)
endif
	$(BINARY) --runtime $(RUNTIME) $(ARGS) $(TAG)

# libkrun opens its kernel by name at run time, and nothing points it at
# Homebrew's directory on its own.
#
# The exported filesystem is a few hundred MB, so it goes under target/ rather
# than into the working copy, where it would sit untracked.
.PHONY: run-vm
run-vm: build-vm
ifndef TAG
	$(error TAG is required: make run-vm TAG=myimage:latest)
endif
	DYLD_LIBRARY_PATH="$(BREW_PREFIX)/lib" \
	$(BINARY) --runtime vm $(ARGS) --vm-output target/$(subst :,-,$(subst /,-,$(TAG))).tar $(TAG)

# --workspace because a plain `cargo test` runs only the root package, which
# leaves both library crates untested.
.PHONY: check
check:
	cargo fmt --check
	cargo clippy --workspace -- -D warnings
	cargo test --workspace

.PHONY: check-vm
check-vm:
	$(VM_ENV) cargo clippy --workspace --features vm -- -D warnings
	$(VM_ENV) cargo test --workspace --features vm

.PHONY: fmt
fmt:
	cargo fmt

.PHONY: clean
clean:
	cargo clean

.PHONY: clean-vm
clean-vm: clean
	rm -rf $(VM_ROOTFS_DIR) $(VM_ROOTFS_ARCHIVE)
	rm -rf "$(HOME)/Library/Application Support/openshell-image-builder/vm-rootfs"
