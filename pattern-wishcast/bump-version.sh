#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 LunNova
# SPDX-License-Identifier: MIT

set -euo pipefail

if [[ $# -ne 1 ]]; then
    echo "Usage: $0 <new-version>"
    echo "Example: $0 0.0.1-pre.5"
    exit 1
fi

NEW_VERSION="$1"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

cd "$SCRIPT_DIR"

# Get current version from main Cargo.toml
CURRENT_VERSION=$(grep -m1 '^version = ' Cargo.toml | sed 's/version = "\(.*\)"/\1/')

echo "Bumping pattern-wishcast from $CURRENT_VERSION to $NEW_VERSION"

# Update versions in both Cargo.toml files
sed -i "s/^version = \"$CURRENT_VERSION\"/version = \"$NEW_VERSION\"/" Cargo.toml
sed -i "s/^version = \"$CURRENT_VERSION\"/version = \"$NEW_VERSION\"/" pattern-wishcast-macros/Cargo.toml

# Commit
git add Cargo.toml pattern-wishcast-macros/Cargo.toml
git commit -m "pattern-wishcast: bump version to $NEW_VERSION"

# Create annotated tags
git tag -a "pattern-wishcast-macros@$NEW_VERSION" -m "pattern-wishcast-macros@$NEW_VERSION"
git tag -a "pattern-wishcast@$NEW_VERSION" -m "pattern-wishcast@$NEW_VERSION"

echo ""
echo "Done. To publish:"
echo "  cargo publish -p pattern-wishcast-macros"
echo "  cargo publish -p pattern-wishcast"
echo "  git push && git push --tags"
