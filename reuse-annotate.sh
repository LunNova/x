#!/usr/bin/env bash

# SPDX-FileCopyrightText: 2025 LunNova
#
# SPDX-License-Identifier: CC0-1.0

set -euo pipefail
shopt -s globstar

CR="LunNova"
METADATA_LICENSE=CC0-1.0
LOCK_LICENSE=$METADATA_LICENSE
EXPECT_LICENSE=$METADATA_LICENSE
EXAMPLES_LICENSE=MIT
LICENSE=MIT
reuse annotate **.lock --copyright $CR --license $LOCK_LICENSE --fallback-dot-license
reuse annotate $0 renovate.json5 .editorconfig .gitignore flake.nix *.toml Cargo.toml --copyright $CR --license $METADATA_LICENSE --fallback-dot-license

reuse annotate pattern-wishcast/**/*.stderr --copyright "$CR" --license $EXPECT_LICENSE --fallback-dot-license
reuse annotate pattern-wishcast/**/Cargo.toml pattern-wishcast/**/tests/**.rs pattern-wishcast/**/src/**.rs \
	--copyright "$CR" --license $LICENSE
reuse annotate pattern-wishcast/**/examples/**.rs --copyright "$CR" --license $LICENSE

exec reuse lint
