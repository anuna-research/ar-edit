#!/bin/bash
# SPDX-License-Identifier: MIT
# Copyright (c) 2026 Anuna Research

set -e

# ar-edit Release Script
# Usage: ./release.sh [version]
# Example: ./release.sh 0.2.0

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

info() { echo -e "${GREEN}[INFO]${NC} $1"; }
warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
error() { echo -e "${RED}[ERROR]${NC} $1"; exit 1; }

# Get version from argument or prompt
VERSION="${1:-}"

if [[ -z "$VERSION" ]]; then
  # Get current version from the workspace Cargo.toml
  CURRENT=$(grep '^version' Cargo.toml | head -1 | grep -o '"[^"]*"' | tr -d '"')
  echo "Current version: $CURRENT"
  read -p "Enter new version (without 'v' prefix): " VERSION
fi

if [[ -z "$VERSION" ]]; then
  error "Version is required"
fi

# Validate version format
if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  error "Invalid version format. Use semver: X.Y.Z"
fi

TAG="v$VERSION"

# The canonical remote is git.anuna.io (the Codeberg copy is a stale mirror).
# Prefer the remote whose URL points there; allow an override; fall back to origin.
REMOTE="${RELEASE_REMOTE:-$(git remote -v | awk '/git\.anuna\.io.*\(push\)/ {print $1; exit}')}"
REMOTE="${REMOTE:-origin}"

info "Preparing release $TAG (remote: $REMOTE)"

# Releases are cut from main
BRANCH=$(git rev-parse --abbrev-ref HEAD)
if [[ "$BRANCH" != "main" ]]; then
  error "Releases are cut from main (currently on $BRANCH)"
fi

# Check for uncommitted changes
if ! git diff --quiet || ! git diff --cached --quiet; then
  error "You have uncommitted changes. Commit or stash them first."
fi

# Check if tag already exists
if git rev-parse "$TAG" >/dev/null 2>&1; then
  error "Tag $TAG already exists"
fi

# Update the workspace version in Cargo.toml ([workspace.package] version)
info "Updating version in Cargo.toml..."
sed -i '' "s/^version = \"[^\"]*\"/version = \"$VERSION\"/" Cargo.toml

# Run tests
info "Running tests..."
cargo test --all

# Check formatting and clippy
info "Checking formatting..."
cargo fmt --all -- --check
info "Running clippy..."
cargo clippy --all -- -D warnings

# Commit version bump (a no-op when the bump was already committed)
git add Cargo.toml Cargo.lock
if git diff --cached --quiet; then
  info "Version $VERSION already committed"
else
  info "Committing version bump..."
  git commit -m "chore: bump version to $VERSION"
fi

# Create and push tag
info "Creating tag $TAG..."
git tag -a "$TAG" -m "Release $VERSION"

# Push to remote
info "Pushing to $REMOTE..."
git push "$REMOTE" main
git push "$REMOTE" "$TAG"

echo ""
info "Release $TAG published!"
echo ""
echo "Woodpecker release pipeline triggered."
echo "Release will be available at:"
echo "  https://files.anuna.io/ar-edit/$TAG/"
echo "  https://files.anuna.io/ar-edit/latest/"
