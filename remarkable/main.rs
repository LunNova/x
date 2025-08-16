use anyhow::Result;
use remarkable::RemarkableSync;
use std::env;
use std::path::PathBuf;

fn main() -> Result<()> {
	let args: Vec<String> = env::args().collect();

	if args.len() < 3 {
		eprintln!("Usage: {} <remarkable_host> <file_path1> [file_path2 ...]", args[0]);
		std::process::exit(1);
	}

	let host = &args[1];
	let file_paths = args[2..].iter().map(PathBuf::from).collect::<Vec<_>>();

	let remarkable = RemarkableSync::new(host)?;

	for file_path in &file_paths {
		remarkable.sync_document(file_path)?;
	}

	remarkable.sync_and_restart()?;

	println!("Successfully synced {} documents to reMarkable", file_paths.len());
	Ok(())
}
