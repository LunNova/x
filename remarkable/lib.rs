use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use ssh2::Session;
use std::fmt::Write as FmtWrite;
use std::fs;
use std::io::Read;
use std::io::Write;
use std::net::TcpStream;
use std::path::Path;

#[derive(Serialize, Deserialize)]
struct Metadata {
	#[serde(rename = "createdTime")]
	created_time: String,
	#[serde(rename = "lastModified")]
	last_modified: String,
	#[serde(rename = "lastOpened")]
	last_opened: String,
	#[serde(rename = "lastOpenedPage")]
	last_opened_page: u32,
	parent: String,
	pinned: bool,
	#[serde(rename = "type")]
	doc_type: String,
	#[serde(rename = "visibleName")]
	visible_name: String,
}

#[derive(Serialize, Deserialize)]
struct Content {
	#[serde(rename = "fileType")]
	file_type: String,
}

pub struct RemarkableSync {
	session: Session,
	remote_path: String,
}

impl RemarkableSync {
	pub fn new(host: &str) -> Result<Self> {
		let tcp = TcpStream::connect(format!("{}:22", host)).context("Failed to connect to reMarkable")?;
		let _ = tcp.set_nodelay(true);

		use nix::sys::socket::{setsockopt, sockopt};

		// let socket_fd = tcp.as_raw_fd();
		setsockopt(&tcp, sockopt::TcpMaxSeg, &1400)?;
		let mut session = Session::new()?;
		session.set_tcp_stream(tcp);
		session.handshake()?;

		session.userauth_agent("root")?;

		Ok(Self {
			session,
			remote_path: String::from("/home/root/.local/share/remarkable/xochitl"),
		})
	}

	pub fn sync_and_restart(&self) -> Result<()> {
		self.execute_command("sync")?;
		self.execute_command("sleep 3")?;
		self.execute_command("sync")?;
		self.execute_command("sleep 3")?;
		self.execute_command("systemctl restart xochitl")?;
		self.execute_command("sleep 3")?;
		Ok(())
	}

	pub fn sync_document(&self, local_path: &Path) -> Result<()> {
		let filename = local_path.file_name().context("Invalid filename")?.to_string_lossy();

		let doc_id_no_ext = local_path
			.file_stem()
			.unwrap()
			.to_string_lossy()
			.chars()
			.filter(|c| c.is_ascii() && (c.is_alphanumeric() || *c == '_'))
			.take(20)
			.collect::<String>();

		// let doc_id_no_ext = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, local_path.to_string_lossy().as_bytes()).to_string();

		let doc_id = format!("{}.{}", doc_id_no_ext, local_path.extension().unwrap().to_string_lossy());
		// Create metadata
		let now = std::time::SystemTime::now()
			.duration_since(std::time::UNIX_EPOCH)
			.unwrap()
			.as_millis()
			.to_string();
		let metadata = Metadata {
			created_time: now.clone(),
			last_modified: now.clone(),
			last_opened: now.clone(),
			last_opened_page: 1,
			parent: String::new(),
			pinned: false,
			doc_type: String::from("DocumentType"),
			visible_name: filename.to_string(),
		};

		// Create content
		let content = Content {
			file_type: if filename.ends_with(".pdf") {
				"pdf".to_string()
			} else if filename.ends_with(".epub") {
				"epub".to_string()
			} else {
				return Err(anyhow::anyhow!("Unsupported file type"));
			},
		};

		// Check if document already exists
		println!("Checking if document {} already exists", local_path.display());
		let check_path = format!("{}/{}", self.remote_path, doc_id);
		let remote_file = self.session.scp_recv(Path::new(&check_path));
		let status = match remote_file {
			Ok(_) => 0,
			Err(_) => 1,
		};

		if status == 0 {
			println!("Document {} already exists as {}, skipping", local_path.display(), doc_id);
			return Ok(());
		}

		// Upload files
		self.upload_file(local_path, &format!("{}/{}", self.remote_path, doc_id))?;
		self.upload_json(&metadata, &format!("{}/{}.metadata", self.remote_path, doc_id_no_ext))?;
		self.upload_json(&content, &format!("{}/{}.content", self.remote_path, doc_id_no_ext))?;

		let local_content = r#"{
    "contentFormatVersion": 1
}"#;
		self.upload_string(local_content, &format!("{}/{}.local", self.remote_path, doc_id_no_ext))?;

		// Touch files and sync
		self.execute_command(&format!("mkdir -p {}/{}", self.remote_path, doc_id_no_ext))?;
		self.execute_command(&format!("touch {}/{}", self.remote_path, doc_id))?;
		self.execute_command(&format!("touch {}/{}.metadata", self.remote_path, doc_id_no_ext))?;
		self.execute_command(&format!("touch {}/{}.local", self.remote_path, doc_id_no_ext))?;
		self.execute_command(&format!("touch {}/{}.content", self.remote_path, doc_id_no_ext))?;

		println!("Synced {} as {}", local_path.display(), doc_id);

		Ok(())
	}

	fn upload_file(&self, local_path: &Path, remote_path: &str) -> Result<()> {
		// Implementation for uploading file via SFTP
		let mut remote_file = self
			.session
			.scp_send(Path::new(remote_path), 0o755, fs::metadata(local_path)?.len(), None)?;

		let contents = fs::read(local_path)?;
		remote_file.write_all(&contents)?;
		remote_file.send_eof()?;
		remote_file.wait_eof()?;
		remote_file.close()?;
		remote_file.wait_close()?;

		Ok(())
	}

	fn upload_string(&self, content: &str, remote_path: &str) -> Result<()> {
		let mut remote_file = self.session.scp_send(Path::new(remote_path), 0o755, content.len() as u64, None)?;
		remote_file.write_all(content.as_bytes())?;
		remote_file.send_eof()?;
		remote_file.wait_eof()?;
		remote_file.close()?;
		remote_file.wait_close()?;
		Ok(())
	}

	fn upload_json<T: Serialize>(&self, data: &T, remote_path: &str) -> Result<()> {
		let mut json = serde_json::to_string_pretty(data)?;

		writeln!(json)?;
		self.upload_string(&json, remote_path)
	}

	fn execute_command(&self, command: &str) -> Result<()> {
		let mut channel = self.session.channel_session()?;
		channel.exec(command)?;
		let mut output = String::new();
		channel.read_to_string(&mut output)?;
		let exit_status = channel.exit_status()?;
		if exit_status != 0 {
			return Err(anyhow::anyhow!("Command failed with status {}: {}", exit_status, output));
		}
		if !output.is_empty() {
			eprintln!("{}", output);
		}
		channel.close()?;
		channel.wait_close()?;
		Ok(())
	}
}
