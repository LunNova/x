// SPDX-FileCopyrightText: 2025 LunNova
//
// SPDX-License-Identifier: MIT

//! badges.toml configuration format:
//! ```toml
//! [[badge]]
//! filename = "example.png"  # or "left/example.png" for subdirectory badges
//! url = "https://custom-url.com"
//! order = 1
//! id = "custom-id"  # optional, defaults to filename without extension
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use tokio::fs;
use tracing::{info, warn};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Badge {
	pub filename: String,
	pub url: String,
	pub order: Option<i32>,
	pub id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BadgeConfig {
	badge: Vec<Badge>,
}

pub async fn load_badges() -> HashMap<String, Vec<Badge>> {
	let badges_dir = Path::new("static/badges");
	let mut badges_by_dir: HashMap<String, Vec<Badge>> = HashMap::new();

	let badge_config = load_badge_config().await;
	let mut config_map: HashMap<String, Badge> = HashMap::new();

	if let Some(config) = badge_config {
		for entry in config.badge {
			config_map.insert(entry.filename.clone(), entry);
		}
	}

	if let Err(e) = scan_badges_dir(badges_dir, badges_dir, &mut badges_by_dir, &mut config_map).await {
		if e.kind() == std::io::ErrorKind::NotFound {
			info!("No badges directory found at {}, skipping badges", badges_dir.display());
		} else {
			warn!("Error scanning badges directory {}: {}", badges_dir.display(), e);
		}
	}

	for badges in badges_by_dir.values_mut() {
		badges.sort_by(|a, b| match a.order.cmp(&b.order) {
			std::cmp::Ordering::Equal => a.filename.cmp(&b.filename),
			other => other,
		});
	}

	let total_badges: usize = badges_by_dir.values().map(|v| v.len()).sum();
	info!("Loaded {} badges across {} directories", total_badges, badges_by_dir.len());
	badges_by_dir
}

async fn scan_badges_dir(
	dir: &Path,
	base_dir: &Path,
	badges_by_dir: &mut HashMap<String, Vec<Badge>>,
	config_map: &mut HashMap<String, Badge>,
) -> Result<(), std::io::Error> {
	let mut entries = fs::read_dir(dir).await?;
	let mut dir_badges = Vec::new();

	while let Some(entry) = entries.next_entry().await? {
		let path = entry.path();

		if path.is_dir() {
			Box::pin(scan_badges_dir(&path, base_dir, badges_by_dir, config_map)).await?;
		} else if let Some(filename) = path.file_name().and_then(|f| f.to_str()) {
			if !filename.ends_with(".png")
				&& !filename.ends_with(".gif")
				&& !filename.ends_with(".jpg")
				&& !filename.ends_with(".jpeg")
				&& !filename.ends_with(".webp")
				&& !filename.ends_with(".svg")
			{
				continue;
			}

			let id = filename.trim_start_matches('_');
			let id = &id[0..id.find('.').unwrap_or(id.len())];

			// Try to find config by full path relative to badges dir
			let relative_path = path.strip_prefix(base_dir).ok().and_then(|p| p.to_str()).unwrap_or(filename);

			let badge = if let Some(mut badge) = config_map.remove(relative_path) {
				if badge.id.is_none() {
					badge.id = Some(id.to_owned());
				}
				badge
			} else if let Some(mut badge) = config_map.remove(filename) {
				// Fallback to just filename for backwards compatibility
				if badge.id.is_none() {
					badge.id = Some(id.to_owned());
				}
				badge
			} else if let Some(url) = filename_to_url(filename) {
				Badge {
					filename: relative_path.to_string(),
					url,
					order: None,
					id: Some(id.to_owned()),
				}
			} else {
				continue;
			};

			dir_badges.push(badge);
		}
	}

	if !dir_badges.is_empty() {
		let dir_name = if dir == base_dir {
			"root".to_string()
		} else {
			dir.strip_prefix(base_dir)
				.ok()
				.and_then(|p| p.to_str())
				.unwrap_or("unknown")
				.to_string()
		};

		badges_by_dir.insert(dir_name, dir_badges);
	}

	Ok(())
}

async fn load_badge_config() -> Option<BadgeConfig> {
	let config_path = Path::new("badges.toml");

	match fs::read_to_string(config_path).await {
		Ok(content) => match toml::from_str(&content) {
			Ok(config) => {
				info!("Loaded badge configuration from badges.toml");
				Some(config)
			}
			Err(e) => {
				warn!("Failed to parse badges.toml: {}", e);
				None
			}
		},
		Err(_) => {
			info!("No badges.toml found, using filename-based URLs");
			None
		}
	}
}

fn filename_to_url(filename: &str) -> Option<String> {
	filename.rsplit_once('.').map(|(name, _)| {
		if name.contains(".") {
			format!("https://{name}")
		} else {
			"#".to_string()
		}
	})
}
