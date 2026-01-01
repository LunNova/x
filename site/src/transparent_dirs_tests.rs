// SPDX-FileCopyrightText: 2025 LunNova
//
// SPDX-License-Identifier: MIT

use crate::EscapeHtmlAttribute;
use crate::config::BlogConfig;
use crate::pages;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use tera::Tera;

fn fixture_path() -> std::path::PathBuf {
	std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/transparent_dirs")
}

fn load_test_config() -> BlogConfig {
	let fixture = fixture_path();
	let config_path = fixture.join("site.toml");
	let config_content =
		std::fs::read_to_string(&config_path).unwrap_or_else(|e| panic!("Failed to read test config at {}: {}", config_path.display(), e));
	let mut config: BlogConfig = toml::from_str(&config_content).unwrap();
	config.site.pages_dir = fixture.join("content").to_string_lossy().to_string();
	config
}

#[test]
fn test_transparent_dirs_page_urls() {
	let config = load_test_config();
	let pages_dir = Path::new(&config.site.pages_dir);

	let all_pages = pages::get_all_pages(pages_dir);
	let slugs: HashSet<String> = all_pages.iter().map(|(slug, _)| slug.clone()).collect();

	assert!(
		slugs.contains("articles/first-post/"),
		"first-post should be at articles/first-post/, got: {:?}",
		slugs
	);
	assert!(
		slugs.contains("articles/second-post/"),
		"second-post should be at articles/second-post/"
	);
	assert!(slugs.contains("articles/old-post/"), "old-post should be at articles/old-post/");
	assert!(
		slugs.contains("articles/regular/normal-page/"),
		"normal-page should keep regular/ in path"
	);
	assert!(slugs.contains("/"), "root index should exist");
	assert!(slugs.contains("articles/"), "articles index should exist");
	assert!(slugs.contains("pages/about/"), "about page should be at pages/about/");

	for slug in &slugs {
		assert!(!slug.contains("_2024"), "slug should not contain _2024: {}", slug);
		assert!(!slug.contains("_2023"), "slug should not contain _2023: {}", slug);
	}
}

#[tokio::test]
async fn test_transparent_dirs_static_files() {
	let config = load_test_config();

	let original_dir = std::env::current_dir().unwrap();
	std::env::set_current_dir(fixture_path()).unwrap();

	let static_files = pages::preload_static_files(&config).await;

	std::env::set_current_dir(original_dir).unwrap();

	assert!(
		static_files.contains_key("articles/first-post/image.png"),
		"image should be at articles/first-post/image.png, got keys: {:?}",
		static_files.keys().collect::<Vec<_>>()
	);

	for key in static_files.keys() {
		assert!(!key.contains("_2024"), "static file path should not contain _2024: {}", key);
	}
}

#[tokio::test]
async fn test_transparent_dirs_metadata_loading() {
	let config = load_test_config();
	let pages_dir = Path::new(&config.site.pages_dir);

	let metadata = pages::load_pages_metadata(pages_dir, false, None).await;

	assert!(metadata.contains_key("articles/first-post/"), "first-post metadata should exist");
	assert!(metadata.contains_key("articles/old-post/"), "old-post metadata should exist");

	let first_post = metadata.get("articles/first-post/").unwrap();
	assert_eq!(first_post.title.as_deref(), Some("First Post 2024"));

	let old_post = metadata.get("articles/old-post/").unwrap();
	assert_eq!(old_post.title.as_deref(), Some("Old Post 2023"));
}

#[tokio::test]
async fn test_transparent_dirs_static_render() {
	use tempfile::TempDir;

	let config = load_test_config();
	let output_dir = TempDir::new().unwrap();

	let original_dir = std::env::current_dir().unwrap();
	std::env::set_current_dir(fixture_path()).unwrap();

	let theme_dir = config.theme.as_ref().map(|t| t.dir.as_str()).unwrap_or("templates");
	let templates_pattern = format!("{theme_dir}/templates/**/*");
	let mut templates = Tera::new(&templates_pattern).unwrap();
	templates.register_filter("escape_html_attribute", EscapeHtmlAttribute);

	let rendered_site = pages::preload_pages_data(&mut templates, &config, false).await;
	let static_files = pages::preload_static_files(&config).await;

	std::env::set_current_dir(original_dir).unwrap();

	let output_path = output_dir.path();

	for (page_key, page_data) in &rendered_site.pages_data {
		let page_key = if page_key == "/" { "" } else { page_key };
		let html_path = if page_key.is_empty() {
			output_path.join("index.html")
		} else {
			let page_dir = output_path.join(page_key);
			fs::create_dir_all(&page_dir).unwrap();
			page_dir.join("index.html")
		};
		fs::write(&html_path, &page_data.html_content).unwrap();
	}

	for (file_path, (content, _)) in static_files.iter() {
		let target_path = output_path.join(file_path);
		if let Some(parent) = target_path.parent() {
			fs::create_dir_all(parent).unwrap();
		}
		fs::write(&target_path, content).unwrap();
	}

	assert!(
		output_path.join("articles/first-post/index.html").exists(),
		"first-post should be rendered at articles/first-post/"
	);
	assert!(
		output_path.join("articles/second-post/index.html").exists(),
		"second-post should be rendered at articles/second-post/"
	);
	assert!(
		output_path.join("articles/old-post/index.html").exists(),
		"old-post should be rendered at articles/old-post/"
	);
	assert!(
		output_path.join("articles/regular/normal-page/index.html").exists(),
		"normal-page should keep regular/ in path"
	);
	assert!(
		output_path.join("articles/first-post/image.png").exists(),
		"image should be at articles/first-post/image.png"
	);
	assert!(
		!output_path.join("articles/_2024").exists(),
		"_2024 directory should not exist in output"
	);
	assert!(
		!output_path.join("articles/_2023").exists(),
		"_2023 directory should not exist in output"
	);
}

#[tokio::test]
async fn test_transparent_dirs_sitemap_urls() {
	let config = load_test_config();

	let original_dir = std::env::current_dir().unwrap();
	std::env::set_current_dir(fixture_path()).unwrap();

	let theme_dir = config.theme.as_ref().map(|t| t.dir.as_str()).unwrap_or("templates");
	let templates_pattern = format!("{theme_dir}/templates/**/*");
	let mut templates = Tera::new(&templates_pattern).unwrap();
	templates.register_filter("escape_html_attribute", EscapeHtmlAttribute);

	let rendered_site = pages::preload_pages_data(&mut templates, &config, false).await;

	std::env::set_current_dir(original_dir).unwrap();

	let sitemap = String::from_utf8_lossy(&rendered_site.sitemap);

	assert!(
		sitemap.contains("https://example.com/articles/first-post/"),
		"sitemap should have first-post URL"
	);
	assert!(
		sitemap.contains("https://example.com/articles/old-post/"),
		"sitemap should have old-post URL"
	);
	assert!(!sitemap.contains("_2024"), "sitemap should not contain _2024");
	assert!(!sitemap.contains("_2023"), "sitemap should not contain _2023");
}
