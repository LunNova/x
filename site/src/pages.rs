// SPDX-FileCopyrightText: 2025 LunNova
//
// SPDX-License-Identifier: MIT

use crate::badges;
use crate::config::BlogConfig;
use crate::context::context_and_render_page;
use crate::render::load_page_content;
use crate::utils::{process_links, slugify, slugify_tag};
use gray_matter::Pod;
use hyper::body::Bytes;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;
use tracing::{info, instrument};

pub const PAGE_EXTENSIONS: &[&str] = &["md", "html"];

pub fn is_page_file(path: &Path) -> bool {
	path.extension()
		.and_then(|s| s.to_str())
		.map(|ext| PAGE_EXTENSIONS.contains(&ext))
		.unwrap_or(false)
}

pub fn get_page_extension(path: &Path) -> Option<&str> {
	path.extension()
		.and_then(|s| s.to_str())
		.filter(|ext| PAGE_EXTENSIONS.contains(ext))
}

pub type StaticFiles = HashMap<String, (Bytes, SystemTime)>;

#[derive(Debug, Clone, Serialize)]
pub struct PageSummary {
	pub title: String,
	pub permalink: String,
	pub slug: String,
	pub description: Option<String>,
	pub date: Option<String>,
	pub updated: Option<String>,
	pub summary: Option<String>,
	pub reading_time: u32,
	pub sort_key: i32,
	pub children: Vec<Arc<PageSummary>>,
}

/// Sort key for consistent page ordering across all sorting locations
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageSortKey {
	pub sort_key: i32,
	pub date: Option<String>,
	pub slug: String,
}

impl PageSortKey {
	pub fn from_metadata(slug: &str, metadata: &PageMetadata) -> Self {
		let (sort_key, date) = if let Some(Pod::Hash(map)) = &metadata.front_matter {
			let sort_key = map
				.get("sort_key")
				.and_then(|k| if let Pod::Integer(i) = k { Some(*i as i32) } else { None })
				.unwrap_or(0);
			let date = map
				.get("date")
				.and_then(|d| if let Pod::String(s) = d { Some(s.clone()) } else { None });
			(sort_key, date)
		} else {
			(0, None)
		};

		PageSortKey {
			sort_key,
			date,
			slug: slug.to_string(),
		}
	}

	pub fn from_summary(summary: &PageSummary) -> Self {
		PageSortKey {
			sort_key: summary.sort_key,
			date: summary.date.clone(),
			slug: summary.slug.clone(),
		}
	}

	/// sort_key ascending, then date descending (newest first), then slug ascending (A→Z)
	/// Dated pages always come before undated pages
	pub fn cmp(&self, other: &Self) -> std::cmp::Ordering {
		match self.sort_key.cmp(&other.sort_key) {
			std::cmp::Ordering::Equal => match (&self.date, &other.date) {
				(Some(a_d), Some(b_d)) => match b_d.cmp(a_d) {
					std::cmp::Ordering::Equal => self.slug.cmp(&other.slug),
					other => other,
				},
				(Some(_), None) => std::cmp::Ordering::Less,
				(None, Some(_)) => std::cmp::Ordering::Greater,
				(None, None) => self.slug.cmp(&other.slug),
			},
			other => other,
		}
	}
}

impl Ord for PageSortKey {
	fn cmp(&self, other: &Self) -> std::cmp::Ordering {
		PageSortKey::cmp(self, other)
	}
}

impl PartialOrd for PageSortKey {
	fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
		Some(std::cmp::Ord::cmp(self, other))
	}
}

#[derive(Clone)]
pub struct PreloadedMetadata {
	pub page_paths: HashMap<String, String>, // slugified_key -> actual_file_path
	pub pages_metadata: BTreeMap<String, PageMetadata>,
	pub pages_summaries: HashMap<String, Arc<PageSummary>>, // All pages as summaries for site-wide access
	pub nav_items: Vec<serde_json::Value>,
	pub sibling_orders: HashMap<String, Vec<String>>, // prefix -> ordered list of page slugs
	pub badges: HashMap<String, Vec<badges::Badge>>,
	pub last_modified: SystemTime,
}

#[derive(Clone)]
pub struct RenderedSite {
	pub pages_data: BTreeMap<String, PageData>,
	pub aliases: HashMap<String, String>, // alias_path -> target_path
	pub sitemap: Bytes,
	pub rss_feed: Bytes,
	pub atom_feed: Bytes,
	pub last_modified: SystemTime,
}

#[derive(Clone, Debug)]
pub struct PageData {
	pub content: Bytes,
	pub front_matter: Option<Pod>,
	pub html_content: Bytes,
	pub links: Vec<String>,
	pub last_modified: SystemTime,
}

#[derive(Debug, Clone)]
pub struct PageMetadata {
	pub front_matter: Option<Pod>,
	pub title: Option<String>,
	pub reading_time: u32,
	pub content: String,
	pub last_modified: SystemTime,
	pub file_extension: String,
}

impl PageMetadata {
	/// Get a field from the front matter, with support for nested paths like "taxonomies.tags"
	pub fn get_frontmatter_field<'a>(&'a self, path: &str) -> Option<&'a Pod> {
		let parts: Vec<&str> = path.split('.').collect();
		if parts.is_empty() {
			return None;
		}

		let mut current = self.front_matter.as_ref()?;

		for part in parts {
			match current {
				Pod::Hash(map) => {
					current = map.get(part)?;
				}
				_ => return None,
			}
		}

		Some(current)
	}

	/// Get a string field from the front matter
	pub fn get_string_field(&self, path: &str) -> Option<&str> {
		match self.get_frontmatter_field(path)? {
			Pod::String(s) => Some(s.as_str()),
			_ => None,
		}
	}

	/// Get an array field from the front matter
	pub fn get_array_field(&self, path: &str) -> Option<&Vec<Pod>> {
		match self.get_frontmatter_field(path)? {
			Pod::Array(arr) => Some(arr),
			_ => None,
		}
	}

	/// Get an iterator over string values in an array field
	pub fn iter_string_array(&self, path: &str) -> impl Iterator<Item = &str> {
		self.get_array_field(path)
			.into_iter()
			.flat_map(|arr| arr.iter())
			.filter_map(|pod| if let Pod::String(s) = pod { Some(s.as_str()) } else { None })
	}

	/// Extract tags from either taxonomies.tags or direct tags field
	pub fn get_tags(&self) -> impl Iterator<Item = &str> {
		// Try direct tags field first, then taxonomies.tags
		let tags = self.get_array_field("tags").or_else(|| {
			self.get_frontmatter_field("taxonomies")
				.and_then(|v| if let Pod::Hash(map) = v { Some(map) } else { None })
				.and_then(|map| map.get("tags"))
				.and_then(|t| if let Pod::Array(arr) = t { Some(arr) } else { None })
		});

		tags.into_iter().flat_map(|arr| arr.iter()).filter_map(|tag| {
			if let Pod::String(tag_name) = tag {
				Some(tag_name.as_str())
			} else {
				None
			}
		})
	}
}

// Helper function to check if a page is a draft
fn is_draft(front_matter: &Option<Pod>) -> bool {
	if let Some(Pod::Hash(map)) = front_matter
		&& let Some(Pod::Boolean(true)) = map.get("draft")
	{
		return true;
	}
	false
}

pub async fn load_pages_metadata(pages_dir: &Path, show_drafts: bool, embed_images_dir: Option<&str>) -> BTreeMap<String, PageMetadata> {
	let all_pages = get_all_pages(pages_dir);
	let mut metadata = BTreeMap::new();

	for (slugified_key, original_path) in all_pages {
		let (content, mut front_matter, last_modified, file_ext) = load_page_content(&original_path, pages_dir.to_str().unwrap()).await;

		if !show_drafts && is_draft(&front_matter) {
			continue;
		}

		// Auto-resolve embed_image if not set
		if let Some(embed_dir) = embed_images_dir {
			let has_embed_image = front_matter
				.as_ref()
				.and_then(|fm| if let Pod::Hash(map) = fm { map.get("embed_image") } else { None })
				.is_some();

			if !has_embed_image {
				let slug_trimmed = slugified_key.trim_end_matches('/');
				let fs_path = format!("static/{}/{}.png", embed_dir, slug_trimmed);

				if Path::new(&fs_path).exists()
					&& let Some(Pod::Hash(ref mut map)) = front_matter
				{
					let url_path = format!("/{}/{}.png", embed_dir, slug_trimmed);
					map.insert("embed_image".to_string(), Pod::String(url_path));
				}
			}
		}

		let title = front_matter
			.as_ref()
			.and_then(|fm| if let Pod::Hash(map) = fm { map.get("title") } else { None })
			.and_then(|t| if let Pod::String(s) = t { Some(s.clone()) } else { None });

		let word_count = content.split_whitespace().count();
		let reading_time = std::cmp::max(1, (word_count as f64 / 250.0).ceil() as u32);

		metadata.insert(
			slugified_key,
			PageMetadata {
				front_matter,
				title,
				reading_time,
				content,
				last_modified,
				file_extension: file_ext,
			},
		);
	}

	metadata
}

pub fn generate_tags_page_metadata(pages_metadata: &BTreeMap<String, PageMetadata>) -> Option<PageMetadata> {
	let mut all_tags: HashMap<&str, Vec<String>> = HashMap::new();
	for (slugified_key, metadata) in pages_metadata {
		let mut has_tags = false;
		for tag_name in metadata.get_tags() {
			has_tags = true;
			let tag_pages = all_tags.entry(tag_name).or_default();
			tag_pages.push(slugified_key.clone());
		}
		if !has_tags {
			all_tags.entry("~untagged").or_default().push(slugified_key.clone());
		}
	}

	if all_tags.is_empty() {
		return None;
	}

	let mut sorted_tags: Vec<_> = all_tags.into_iter().collect();
	sorted_tags.sort_by(|a, b| a.0.cmp(b.0));

	for (_, pages) in &mut sorted_tags {
		pages.sort_by(|a, b| {
			let a_title = pages_metadata.get(a).and_then(|m| m.title.as_ref()).unwrap_or(a);
			let b_title = pages_metadata.get(b).and_then(|m| m.title.as_ref()).unwrap_or(b);
			a_title.cmp(b_title)
		});
	}

	let mut tags_content = String::from("All articles organized by tags:\n\n");

	for (tag_name, tag_pages) in &sorted_tags {
		let tag_slug = slugify_tag(tag_name);
		tags_content.push_str(&format!("### {tag_name} {{#{tag_slug}}}\n\n"));

		for page_key in tag_pages {
			if let Some(metadata) = pages_metadata.get(page_key) {
				let title = metadata.title.as_ref().unwrap_or(page_key);
				tags_content.push_str(&format!("- [{}](/{page_key})\n", crate::escape_html_attribute(title)));
			}
		}
		tags_content.push('\n');
	}

	let word_count = tags_content.split_whitespace().count();
	let reading_time = std::cmp::max(1, (word_count as f64 / 250.0).ceil() as u32);

	Some(PageMetadata {
		front_matter: Some(Pod::Hash({
			let mut map = std::collections::HashMap::new();
			map.insert("title".to_string(), Pod::String("Tags".to_string()));
			map.insert("template".to_string(), Pod::String("page.html".to_string()));
			map
		})),
		title: Some("Tags".to_string()),
		reading_time,
		content: tags_content,
		last_modified: SystemTime::now(),
		file_extension: "md".to_string(),
	})
}

#[instrument]
pub fn get_all_pages(dir: &Path) -> Vec<(String, String)> {
	fn visit_dirs(dir: &Path, base: &Path, pages: &mut Vec<(String, String)>) -> std::io::Result<()> {
		if dir.is_dir() {
			for entry in fs::read_dir(dir)? {
				let entry = entry?;
				let path = entry.path();
				if path.is_dir() {
					visit_dirs(&path, base, pages)?;
				} else if is_page_file(&path)
					&& let Ok(relative) = path.strip_prefix(base)
				{
					let original_path = relative.with_extension("").to_string_lossy().replace("\\", "/");

					let page_key = original_path.clone();

					let slugified_key = slugify(&page_key);

					pages.push((slugified_key, original_path));
				}
			}
		}
		Ok(())
	}

	let mut pages = Vec::new();
	visit_dirs(dir, dir, &mut pages).unwrap();
	pages.sort_by(|a, b| a.0.cmp(&b.0));
	info!("Found {} pages", pages.len());
	pages
}

#[instrument(skip(config))]
pub async fn preload_pages_metadata(config: &BlogConfig, show_drafts: bool) -> PreloadedMetadata {
	let badges = badges::load_badges().await;
	let pages_dir = Path::new(&config.site.pages_dir);
	let all_pages = get_all_pages(pages_dir);
	let mut page_paths = HashMap::new();

	let mut pages_metadata = load_pages_metadata(pages_dir, show_drafts, config.site.embed_images_dir.as_deref()).await;

	if let Some(tags_metadata) = generate_tags_page_metadata(&pages_metadata) {
		pages_metadata.insert(slugify("tags"), tags_metadata);
	}

	for (slugified_key, original_path) in &all_pages {
		if pages_metadata.contains_key(slugified_key) {
			page_paths.insert(slugified_key.clone(), original_path.clone());
		}
	}

	let mut last_modified = SystemTime::UNIX_EPOCH;
	for metadata in pages_metadata.values() {
		if metadata.last_modified > last_modified {
			last_modified = metadata.last_modified;
		}
	}

	let mut nav_items = Vec::new();
	for (path, metadata) in &pages_metadata {
		if let Some(Pod::Hash(fm_map)) = &metadata.front_matter
			&& let Some(Pod::Boolean(true)) = fm_map.get("in_nav")
			&& let Some(title) = &metadata.title
		{
			nav_items.push(serde_json::json!({
				"title": title,
				"url": format!("/{}", path)
			}));
		}
	}

	let mut prefix_groups: HashMap<String, Vec<String>> = HashMap::new();
	for slugified_key in pages_metadata.keys() {
		let slugified_key_deslashed = slugified_key.trim_end_matches('/');
		let prefix = if let Some(last_slash) = slugified_key_deslashed.rfind('/') {
			slugified_key[..last_slash].to_string()
		} else {
			String::new() // Root level
		};

		prefix_groups.entry(prefix).or_default().push(slugified_key.to_string());
	}

	let mut sibling_orders = HashMap::new();
	for (prefix, mut pages) in prefix_groups {
		pages.sort_by(|a, b| {
			let a_key = pages_metadata.get(a).map(|m| PageSortKey::from_metadata(a, m));
			let b_key = pages_metadata.get(b).map(|m| PageSortKey::from_metadata(b, m));
			match (a_key, b_key) {
				(Some(a), Some(b)) => a.cmp(&b).reverse(),
				_ => std::cmp::Ordering::Equal,
			}
		});
		sibling_orders.insert(prefix, pages);
	}

	// Create all page summaries with empty children
	let mut all_pages: Vec<PageSummary> = pages_metadata
		.iter()
		.map(|(slug, metadata)| {
			let (description, date, updated, summary, sort_key) = if let Some(Pod::Hash(map)) = &metadata.front_matter {
				let description = map
					.get("description")
					.and_then(|d| if let Pod::String(s) = d { Some(s.clone()) } else { None });
				let date = map
					.get("date")
					.and_then(|d| if let Pod::String(s) = d { Some(s.clone()) } else { None });
				let updated = map
					.get("updated")
					.and_then(|d| if let Pod::String(s) = d { Some(s.clone()) } else { None });
				let summary = map
					.get("summary")
					.and_then(|d| if let Pod::String(s) = d { Some(s.clone()) } else { None });
				let sort_key = map
					.get("sort_key")
					.and_then(|k| if let Pod::Integer(i) = k { Some(*i as i32) } else { None })
					.unwrap_or(0);
				(description, date, updated, summary, sort_key)
			} else {
				(None, None, None, None, 0)
			};

			PageSummary {
				title: metadata.title.as_ref().unwrap_or(slug).clone(),
				permalink: format!("/{slug}"),
				slug: slug.clone(),
				description,
				date,
				updated,
				summary,
				reading_time: metadata.reading_time,
				sort_key,
				children: Vec::new(),
			}
		})
		.collect();

	// Sort by depth (deepest first) to process leaf nodes before parents
	all_pages.sort_by_key(|page| std::cmp::Reverse(page.slug.matches('/').count()));

	// Process deepest-first, building parent-child relationships
	let mut pages_summaries: HashMap<String, Arc<PageSummary>> = HashMap::new();

	for mut page in all_pages {
		// Find children from already-processed (deeper) pages
		let mut children: Vec<Arc<PageSummary>> = pages_summaries
			.values()
			.filter(|child| {
				// Check if this child is a direct child of current page
				child.slug.starts_with(&page.slug)
					&& child.slug != page.slug
					&& child.slug.matches('/').count() == page.slug.matches('/').count() + 1
			})
			.cloned() // Clone the Arc, not the PageSummary
			.collect();

		children.sort_by_key(|c| PageSortKey::from_summary(c));

		page.children = children;

		// Move page into Arc (no cloning)
		pages_summaries.insert(page.slug.clone(), Arc::new(page));
	}

	PreloadedMetadata {
		page_paths,
		pages_metadata,
		pages_summaries,
		nav_items,
		sibling_orders,
		badges,
		last_modified,
	}
}

#[instrument(skip(templates, metadata, config))]
pub async fn render_site_from_metadata(templates: &mut tera::Tera, metadata: &PreloadedMetadata, config: &BlogConfig) -> RenderedSite {
	let mut pages_data = BTreeMap::new();
	let mut aliases = HashMap::new();

	let cfg_ref = std::sync::Arc::from(config.clone());
	let metadata_ref = std::sync::Arc::new(metadata.pages_metadata.clone());
	templates.register_function("generate_ldjson", move |args: &std::collections::HashMap<String, tera::Value>| {
		crate::semantic_web::generate_ldjson_impl(args, &cfg_ref, &metadata_ref)
	});

	let mut sitemap = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?><urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">");

	for (slugified_key, page_metadata) in &metadata.pages_metadata {
		let (processed_content, links) = process_links(&page_metadata.content);

		let page_data = PageData {
			content: Bytes::from(processed_content.clone()),
			front_matter: page_metadata.front_matter.clone(),
			html_content: Bytes::from(processed_content.clone()), // Will be processed in context_and_render_page
			links: links.clone(),
			last_modified: page_metadata.last_modified,
		};

		let rendered_html = context_and_render_page(
			slugified_key,
			&page_data,
			templates,
			metadata,
			config,
			&page_metadata.file_extension,
		)
		.unwrap();

		let final_html = crate::url_rewriter::rewrite_urls(&rendered_html, &config.site.base_url, slugified_key).unwrap_or_else(|e| {
			tracing::warn!("Failed to rewrite URLs for page {}: {}", slugified_key, e);
			rendered_html
		});

		pages_data.insert(
			slugified_key.clone(),
			PageData {
				content: Bytes::from(processed_content),
				front_matter: page_metadata.front_matter.clone(),
				html_content: Bytes::from(final_html),
				links,
				last_modified: page_metadata.last_modified,
			},
		);

		// Extract aliases from front matter
		if let Some(gray_matter::Pod::Hash(fm_map)) = &page_metadata.front_matter
			&& let Some(gray_matter::Pod::Array(alias_list)) = fm_map.get("aliases")
		{
			for alias in alias_list {
				if let gray_matter::Pod::String(alias_path) = alias {
					let normalized_alias = alias_path.trim_start_matches('/');
					aliases.insert(normalized_alias.to_string(), slugified_key.clone());
				}
			}
		}

		// Add to sitemap
		let url = if slugified_key == "/" {
			config.site.base_url.trim_end_matches('/').to_string()
		} else {
			format!("{}/{}", config.site.base_url.trim_end_matches('/'), slugified_key)
		};
		sitemap.push_str(&format!("\n<url><loc>{}</loc>", url));

		// Add lastmod if available (prioritize 'updated' over 'date')
		let mut lastmod_date_str = None;

		if let Some(gray_matter::Pod::Hash(fm_map)) = &page_metadata.front_matter
			&& let Some(gray_matter::Pod::String(date)) = fm_map.get("updated").or_else(|| fm_map.get("date"))
		{
			lastmod_date_str = Some(date.clone());
		}

		// Compare with baseline_date if configured
		if let Some(baseline) = &config.site.baseline_date {
			match &lastmod_date_str {
				Some(page_date) => {
					// Compare dates and use the more recent one
					// Assuming YYYY-MM-DD format for comparison
					if baseline > page_date {
						lastmod_date_str = Some(baseline.clone());
					}
				}
				None => {
					// No page date, use baseline
					lastmod_date_str = Some(baseline.clone());
				}
			}
		}

		if let Some(date) = lastmod_date_str {
			sitemap.push_str(&format!("<lastmod>{date}</lastmod>"));
		}

		sitemap.push_str("</url>");
	}

	sitemap.push_str("\n</urlset>\n");

	// Generate RSS feed
	let rss_feed = crate::feed::generate_rss_feed(config, &metadata.pages_metadata);

	// Generate Atom feed
	let atom_feed = crate::feed::generate_atom_feed(config, &metadata.pages_metadata);

	info!(
		"Rendered {} pages (including tags index) with {} aliases",
		pages_data.len(),
		aliases.len()
	);
	RenderedSite {
		pages_data,
		aliases,
		sitemap: Bytes::from(sitemap),
		rss_feed: Bytes::from(rss_feed),
		atom_feed: Bytes::from(atom_feed),
		last_modified: metadata.last_modified,
	}
}

// Convenience function that combines both phases
#[instrument(skip(templates, config))]
pub async fn preload_pages_data(templates: &mut tera::Tera, config: &BlogConfig, show_drafts: bool) -> RenderedSite {
	let metadata = preload_pages_metadata(config, show_drafts).await;
	render_site_from_metadata(templates, &metadata, config).await
}

pub async fn preload_static_files(config: &BlogConfig) -> StaticFiles {
	let mut static_files = HashMap::new();

	fn visit_dir(dir: &Path, static_dir: &Path, static_files: &mut HashMap<String, (Bytes, SystemTime)>, is_content_dir: bool) {
		if let Ok(entries) = fs::read_dir(dir) {
			for entry in entries.filter_map(|e| e.ok()) {
				let path = entry.path();
				if path.is_dir() {
					visit_dir(&path, static_dir, static_files, is_content_dir);
				} else if path.is_file() {
					// Skip page files when loading from content directory
					if is_content_dir && is_page_file(&path) {
						continue;
					}

					let mut file_name = path.strip_prefix(static_dir).unwrap().to_str().unwrap().to_string();

					// For content directory, slugify the directory path but keep the filename
					if is_content_dir {
						let path_obj = Path::new(&file_name);
						if let Some(parent) = path_obj.parent()
							&& let Some(filename) = path_obj.file_name()
						{
							let slugified_parent = slugify(parent.to_str().unwrap());
							file_name = if slugified_parent.is_empty() {
								filename.to_str().unwrap().to_string()
							} else {
								format!("{}/{}", slugified_parent.trim_end_matches('/'), filename.to_str().unwrap())
							};
						}
					}

					if let Ok(content) = fs::read(&path)
						&& let Ok(metadata) = entry.metadata()
						&& let Ok(last_modified) = metadata.modified()
					{
						static_files.insert(file_name, (Bytes::from(content), last_modified));
					}
				}
			}
		}
	}

	// First, load theme static files as fallback
	let theme_dir = config.theme.as_ref().map(|t| t.dir.as_str()).unwrap_or("theme");
	let theme_static_dir = Path::new(theme_dir).join("static");
	if theme_static_dir.is_dir() {
		visit_dir(&theme_static_dir, &theme_static_dir, &mut static_files, false);
	}

	// Then, load content-adjacent static files (images, etc.)
	let content_dir = Path::new(&config.site.pages_dir);
	if content_dir.is_dir() {
		visit_dir(content_dir, content_dir, &mut static_files, true);
	}

	// Finally, load main static files (these override everything)
	let static_dir = Path::new("static");
	if static_dir.is_dir() {
		visit_dir(static_dir, static_dir, &mut static_files, false);
	}

	static_files
}
