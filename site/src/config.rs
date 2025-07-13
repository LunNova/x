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
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct FeaturesConfig {
	pub wiki_links: Option<bool>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ThemeConfig {
	pub dir: String,
}
