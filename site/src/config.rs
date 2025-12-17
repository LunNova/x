// SPDX-FileCopyrightText: 2025 LunNova
//
// SPDX-License-Identifier: MIT

use argh::FromArgs;
use serde::{Deserialize, Serialize};

#[derive(FromArgs)]
/// A simple blog engine
pub struct Args {
	#[argh(subcommand)]
	pub command: Command,
}

#[derive(FromArgs)]
#[argh(subcommand)]
pub enum Command {
	Serve(ServeArgs),
	Render(RenderArgs),
}

#[derive(FromArgs)]
#[argh(subcommand, name = "serve")]
/// Serve the blog
pub struct ServeArgs {
	#[argh(positional)]
	/// path to the blog directory
	pub blog_dir: String,
	#[argh(switch)]
	/// show draft pages (normally hidden in production)
	pub show_drafts: bool,
	#[argh(option)]
	/// override the domain name (default: http://127.0.0.1:3030)
	pub domain: Option<String>,
}

#[derive(FromArgs)]
#[argh(subcommand, name = "render")]
/// Render the blog to static files
pub struct RenderArgs {
	#[argh(positional)]
	/// path to the blog directory
	pub blog_dir: String,
	#[argh(positional)]
	/// path to the output directory
	pub output_dir: String,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct BlogConfig {
	pub site: SiteConfig,
	pub features: Option<FeaturesConfig>,
	pub theme: Option<ThemeConfig>,
	pub extra: Option<serde_json::Value>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct SiteConfig {
	pub title: String,
	pub base_url: String,
	pub pages_dir: String,
	pub description: Option<String>,
	pub baseline_date: Option<String>,
	pub embed_images_dir: Option<String>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct FeaturesConfig {
	pub wiki_links: Option<bool>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ThemeConfig {
	pub dir: String,
}
