<!--
SPDX-FileCopyrightText: 2026 LunNova
SPDX-License-Identifier: MIT
-->

# cargo-shipshape

Keeps a rust project in good condition.

- Sort top level items/declarations in rust files by type and name
- Extract overly long mod blocks to separate files

## CLI

```
Usage: cargo-shipshape [<paths...>] [-c] [--diff] [-n] [-r] [--no-extract] [--extract-threshold <extract-threshold>]

Sort Rust file items by type and name

Positional Arguments:
  paths             files or directories to process (defaults to current
                    directory)

Options:
  -c, --check       check mode - exit 1 if files need sorting (for CI)
  --diff            show diff of what would change
  -n, --dry-run     don't write changes, just report
  -r, --recursive   process all .rs files in directory recursively
  --no-extract      disable automatic extraction of large inline modules
  --extract-threshold
                    line threshold for module extraction (default: 100)
  --help, help      display usage information
```

## Detailed Behavior

- Sorts top-level items in Rust files by type and name
  - Order: extern crate -> mod -> use -> const -> static -> type alias -> macro_rules -> macro calls -> trait -> struct -> enum -> union -> fn -> impl -> inline mod blocks
  - Within each category, sorted by name
  - Preserves attached attributes and doc comments
  - Adds blank lines between different item types
- Extracts large inline modules to separate files
  - Default threshold: 100 lines
  - Cargo-aware placement: sibling files for crate roots, subdirectories for non-roots
  - Uses mod.rs form in tests/examples/benches to avoid Cargo autodiscovery creating new binaries

## Planned? features

- Config
  - .editorconfig? separate toml file?
- Opinionated lints
  - Error handling patterns (e.g. bare unwrap usage, error type choices)
  - Dependency usage (detecting unused deps, suggesting alternatives)
  - Coverage requirements
- Automatically run rustfmt after organizing?
