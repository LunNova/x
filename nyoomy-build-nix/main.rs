use argh::FromArgs;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

#[derive(FromArgs)]
/// nyoomy-build.nix - Nix build utility
struct NyoomyBuild {
	#[argh(subcommand)]
	command: Commands,
}

#[derive(FromArgs)]
#[argh(subcommand)]
enum Commands {
	Show(ShowCommand),
}

#[derive(FromArgs)]
#[argh(subcommand, name = "show")]
/// Display information about build targets
struct ShowCommand {
	#[argh(positional)]
	/// flake attribute paths to show (e.g., nixpkgs#hello)
	attrpaths: Vec<String>,
}

fn main() {
	let args: NyoomyBuild = argh::from_env();

	match args.command {
		Commands::Show(cmd) => show_command(cmd),
	}
}

fn show_command(cmd: ShowCommand) {
	if cmd.attrpaths.is_empty() {
		eprintln!("Error: No attribute paths provided");
		std::process::exit(1);
	}

	let output = Command::new("nix")
		.arg("derivation")
		.arg("show")
		.arg("--recursive")
		.args(&cmd.attrpaths)
		.output()
		.expect("Failed to execute nix derivation show");

	if !output.status.success() {
		eprintln!("Error running nix derivation show:");
		eprintln!("{}", String::from_utf8_lossy(&output.stderr));
		std::process::exit(1);
	}

	let json_str = String::from_utf8_lossy(&output.stdout);
	let derivations: HashMap<String, Value> = serde_json::from_str(&json_str).expect("Failed to parse JSON output from nix derivation show");

	for (_drv_path, drv_data) in derivations {
		let outputs = drv_data["outputs"].as_object().expect("Derivation missing outputs field");

		for (output_name, output_data) in outputs {
			let store_path = output_data["path"].as_str().expect("Output missing path field");

			let exists = Path::new(store_path).exists();
			let status = if exists { "built" } else { "needs building" };

			println!("{store_path} ({output_name}): {status}");
		}
	}
}
