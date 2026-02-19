.PHONY: all build release test clippy fmt fmt-fix check clean install doc doc-open

# Default target
all: check build test

# Build all crates
build:
	cargo build --all

# Build release
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

# Full CI check (fmt + clippy + test)
check: fmt clippy

# Install CLI
install:
	cargo install --path crates/ar-edit

# Clean build artifacts
clean:
	cargo clean

# Generate documentation
doc:
	cargo doc --all --no-deps

# Open documentation in browser
doc-open:
	cargo doc --all --no-deps --open
