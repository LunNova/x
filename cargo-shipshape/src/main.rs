// SPDX-FileCopyrightText: 2026 LunNova
//
// SPDX-License-Identifier: MIT

fn main() -> std::process::ExitCode {
	let args: Vec<String> = std::env::args().collect();
	// cargo subcommand invocation passes ["cargo-shipshape", "shipshape", ...]
	let skip = if args.get(1).is_some_and(|s| s == "shipshape") { 2 } else { 1 };
	let args_refs: Vec<&str> = args[skip..].iter().map(String::as_str).collect();
	std::process::ExitCode::from(cargo_shipshape::run(&args_refs) as u8)
}
