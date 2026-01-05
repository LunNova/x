# SPDX-FileCopyrightText: 2026 LunNova
#
# SPDX-License-Identifier: MIT

{
  lib,
  rustPlatform,
}:

rustPlatform.buildRustPackage rec {
  pname = "cargo-shipshape";
  version = "0.0.1-pre.1";

  src = ./..;

  buildAndTestSubdir = "cargo-shipshape";

  cargoLock = {
    lockFile = ../Cargo.lock;
  };

  meta = {
    description = "Cargo subcommand to sort Rust file items by type and name";
    mainProgram = "cargo-shipshape";
    homepage = "https://github.com/LunNova/rust-monorepo";
    license = lib.licenses.mit;
    maintainers = with lib.maintainers; [ LunNova ];
  };
}
