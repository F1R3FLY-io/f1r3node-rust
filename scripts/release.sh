#!/usr/bin/env bash
# Manual release script for f1r3node-rust
#
# Usage: ./scripts/release.sh [major|minor|patch]
#   Default bump type: minor
#
# This is an escape hatch for when you need a non-minor bump (e.g., major
# or patch). For normal releases, the nightly workflow auto-bumps minor
# when master has new commits.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$REPO_DIR"

git diff --quiet && git diff --cached --quiet || {
    echo "ERROR: working tree is dirty — commit or stash changes first"
    exit 1
}

source "$SCRIPT_DIR/version.sh"
bump_version "${1:-minor}"

echo "Current: ${CURRENT_VERSION} -> Next: ${NEXT_VERSION} (${TAG_NAME})"
echo ""

sed -i'' -e "0,/^version = \".*\"/s//version = \"${NEXT_VERSION}\"/" node/Cargo.toml
echo "Updated node/Cargo.toml to ${NEXT_VERSION}"

if grep -q '^LABEL version=' node/Dockerfile; then
    sed -i'' -e "s/^LABEL version=\".*\"/LABEL version=\"${NEXT_VERSION}\"/" node/Dockerfile
    echo "Updated node/Dockerfile LABEL to ${NEXT_VERSION}"
fi

cargo generate-lockfile 2>/dev/null || true
echo "Updated Cargo.lock"

if command -v git-cliff &>/dev/null; then
    git-cliff --config cliff.toml --tag "$TAG_NAME" -o CHANGELOG.md
    echo "Generated CHANGELOG.md"
else
    echo "WARNING: git-cliff not found, skipping CHANGELOG generation"
    echo "Install: cargo install git-cliff"
fi

files=(node/Cargo.toml node/Dockerfile Cargo.lock)
if [ -f CHANGELOG.md ]; then
    files+=(CHANGELOG.md)
fi

git add "${files[@]}"
git commit -m "chore(release): v${NEXT_VERSION}"
git tag -a "$TAG_NAME" -m "Release v${NEXT_VERSION}"

echo ""
echo "Release ${TAG_NAME} created."
echo ""
echo "To publish:"
echo "  git push origin $(git branch --show-current) --follow-tags"
