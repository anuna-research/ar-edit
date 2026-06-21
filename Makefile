# SPDX-License-Identifier: MIT
# Copyright (c) 2026 Anuna Research

PREFIX ?= $(HOME)/.local

# Distribution archive naming (matches install.sh / .woodpecker/release.yaml)
PROJECT := ar-edit
HOST_OS := $(shell uname -s | tr '[:upper:]' '[:lower:]' | sed 's/darwin/macos/')
HOST_ARCH := $(shell uname -m | sed 's/x86_64/x86_64/;s/arm64/arm64/;s/aarch64/arm64/')

.PHONY: all build release test clippy fmt fmt-fix check clean install uninstall dist doc doc-open help

# Default target
all: check build test

# Build all crates (debug)
build:
	cargo build --all

# Build release binary
release:
	cargo build --release --all

# Run all tests
test:
	cargo test --all

# Run clippy lints
clippy:
	cargo clippy --all -- -D warnings

# Check formatting
fmt:
	cargo fmt --all -- --check

# Format code
fmt-fix:
	cargo fmt --all

# Full CI check (fmt + clippy)
check: fmt clippy

# Install the built release binary to $(PREFIX)/bin
install: release
	install -d $(PREFIX)/bin
	install -m 755 target/release/ar-edit $(PREFIX)/bin/ar-edit
	@echo "Installed ar-edit to $(PREFIX)/bin/ar-edit"

# Remove the installed binary
uninstall:
	rm -f $(PREFIX)/bin/ar-edit

# Build a distributable tarball for the host platform
dist: release
	mkdir -p dist
	cp target/release/ar-edit dist/ar-edit
	cd dist && tar czf $(PROJECT)-$(HOST_OS)-$(HOST_ARCH).tar.gz ar-edit && rm ar-edit
	@echo "Created dist/$(PROJECT)-$(HOST_OS)-$(HOST_ARCH).tar.gz"

# Clean build artifacts
clean:
	cargo clean
	rm -rf dist

# Generate documentation
doc:
	cargo doc --all --no-deps

# Open documentation in browser
doc-open:
	cargo doc --all --no-deps --open

help:
	@echo "ar-edit - audio-region editor"
	@echo ""
	@echo "Targets:"
	@echo "  make build     - Build all crates (debug)"
	@echo "  make release   - Build release binaries"
	@echo "  make test      - Run the test suite"
	@echo "  make check     - Run fmt check and clippy"
	@echo "  make clippy    - Run clippy lints"
	@echo "  make fmt       - Check formatting"
	@echo "  make fmt-fix   - Auto-fix formatting"
	@echo "  make install   - Install release binary to \$$PREFIX/bin (default ~/.local)"
	@echo "  make uninstall - Remove the installed binary"
	@echo "  make dist      - Build a tarball for the host platform into dist/"
	@echo "  make doc       - Generate documentation"
	@echo "  make clean     - Remove build artifacts"
	@echo ""
	@echo "Options:"
	@echo "  PREFIX=<path>  - Install prefix (default: ~/.local)"
	@echo ""
	@echo "Release:"
	@echo "  ./release.sh X.Y.Z  - Tag a release; CI publishes to https://files.anuna.io/ar-edit/"
