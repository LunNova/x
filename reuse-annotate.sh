#!/usr/bin/env bash

# SPDX-FileCopyrightText: 2025 LunNova
#
# SPDX-License-Identifier: CC0-1.0

set -euo pipefail
shopt -s globstar nullglob

if [[ $# -lt 1 ]]; then
	echo "Usage: $0 <directory>" >&2
	exit 1
fi

dir="$1"
CR="LunNova"
LICENSE=MIT
METADATA_LICENSE=CC0-1.0
EXPECT_LICENSE=CC0-1.0

annotate() {
	local license="$1"
	local fallback="$2"
	shift 2
	local files=("$@")
	if [[ ${#files[@]} -gt 0 ]]; then
		if [[ "$fallback" == "yes" ]]; then
			reuse annotate "${files[@]}" --copyright "$CR" --license "$license" --fallback-dot-license
		else
			reuse annotate "${files[@]}" --copyright "$CR" --license "$license"
		fi
	fi
}

annotate $METADATA_LICENSE yes "$dir"/**/*.lock
annotate $METADATA_LICENSE yes "$dir"/**/*.toml
annotate $LICENSE no "$dir"/**/*.rs
annotate $EXPECT_LICENSE yes "$dir"/**/*.stderr

exec reuse lint
