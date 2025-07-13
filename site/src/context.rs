// SPDX-FileCopyrightText: 2025 LunNova
//
// SPDX-License-Identifier: MIT

use gray_matter::Pod;
use hyper::body::Bytes;
use rand::seq::SliceRandom;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use tera::{Context, Tera};
use tracing::instrument;

use crate::config::BlogConfig;
use crate::front_matter::pod_to_json_value;
use crate::pages::{PageData, PageMetadata};
use crate::utils::{slugify, slugify_tag, stable_string_hash};

// Context generation aims for Zola compatibility with unified page model:
// everything is a page, so templates don't need separate handling for
// sections, individual pages, and edge cases like 404 pages.

#[derive(Serialize)]
pub struct BreadcrumbItem {
	pub title: String,
	pub url: String,
	pub is_current: bool,
}

#[instrument(skip(content, front_matter))]
pub fn generate_page_context(title: &str, content: &Bytes, front_matter: Option<&Pod>) -> Context {
	let mut context = Context::new();
	context.insert("title", title);
	context.insert("content", String::from_utf8_lossy(content).as_ref());

	let mut alternates = Vec::new();

	use serde_json::json;
	alternates.push(json!({
		"type": "text/markdown",
		"title": "Markdown version",
		"url": format!("/{}index.md", title)
	}));

	alternates.push(json!({
		"type": "text/plain",
		"title": "Plain text version",
		"url": format!("/{}index.txt", title)
	}));

	alternates.push(json!({
		"type": "application/rss+xml",
		"title": "RSS Feed",
		"url": "/rss.xml"
	}));

	alternates.push(json!({
		"type": "application/atom+xml",
		"title": "Atom Feed",
		"url": "/atom.xml"
	}));

	context.insert("alternates", &alternates);

	if let Some(Pod::Hash(data)) = front_matter {
		for (key, value) in data {
			match value {
				Pod::Null => context.insert(key, &()),
				Pod::String(s) => context.insert(key, &s),
				Pod::Integer(i) => context.insert(key, &i),
				Pod::Float(f) => context.insert(key, &f),
				Pod::Boolean(b) => context.insert(key, &b),
				Pod::Array(arr) => {
					let vec: Vec<_> = arr.iter().map(pod_to_json_value).collect();
					context.insert(key, &vec);
				}
				Pod::Hash(map) => {
					let obj: HashMap<_, _> = map.iter().map(|(k, v)| (k, pod_to_json_value(v))).collect();
					context.insert(key, &obj);
				}
			}
		}
	}

	context
}

pub fn generate_breadcrumbs_from_metadata(page: &str, pages_metadata: &BTreeMap<String, PageMetadata>, base_url: &str) -> Vec<BreadcrumbItem> {
	if page.trim_end_matches('/').is_empty() {
		return vec![];
	}

	let mut breadcrumbs = Vec::new();

	let root_title = pages_metadata
		.get("/")
		.and_then(|m| m.title.as_ref())
		.map(|s| s.as_str())
		.unwrap_or("~");

	breadcrumbs.push(BreadcrumbItem {
		title: root_title.to_string(),
		url: base_url.trim_end_matches('/').to_string() + "/",
		is_current: page.is_empty(),
	});

	let parts: Vec<&str> = page.split('/').filter(|p| !p.is_empty()).collect();
	for (i, _part) in parts.iter().enumerate() {
		let ancestor_path = parts[..=i].join("/");
		let title = pages_metadata
			.get(&ancestor_path)
			.and_then(|m| m.title.as_ref())
			.map(|s| s.as_str())
			.unwrap_or(&ancestor_path);

		let is_current = i == parts.len() - 1;
		if !is_current {
			breadcrumbs.push(BreadcrumbItem {
				title: title.to_string(),
				url: format!("{}/{}/", base_url.trim_end_matches('/'), ancestor_path),
				is_current: false,
			});
		}
	}

	breadcrumbs
}

#[instrument(skip(page_data, templates, metadata, config))]
pub fn context_and_render_page(
	page: &str,
	page_data: &PageData,
	templates: &Tera,
	metadata: &crate::pages::PreloadedMetadata,
	config: &BlogConfig,
	file_extension: &str,
) -> Result<String, tera::Error> {
	let (html_content_for_context, is_template) = if file_extension == "md" {
		(
			crate::render::markdown_to_html(&String::from_utf8_lossy(&page_data.html_content)),
			false,
		)
	} else {
		(String::new(), true)
	};

	let mut context = generate_page_context(page, &Bytes::from(html_content_for_context), page_data.front_matter.as_ref());
	let mut badges_shuffled = HashMap::new();
	for (name, badges) in metadata.badges.iter() {
		let mut shuffled = badges.clone();
		let seed = stable_string_hash(page).wrapping_mul(stable_string_hash(name));
		let mut rand = <rand::rngs::StdRng as rand::SeedableRng>::seed_from_u64(seed);
		shuffled.shuffle(&mut rand);
		badges_shuffled.insert(name.to_string(), shuffled);
	}
	context.insert("badges", &badges_shuffled);
	context.insert("config", config);
	let current_page = format!("/{}", page.trim_start_matches("/"));
	context.insert("current_page", &current_page);

	let breadcrumbs = generate_breadcrumbs_from_metadata(page, &metadata.pages_metadata, &config.site.base_url);
	context.insert("breadcrumbs", &breadcrumbs);

	context.insert("nav_items", &metadata.nav_items);

	context.insert("all_pages", &metadata.pages_summaries);

	let mut page_obj = serde_json::Map::new();
	page_obj.insert(
		"title".to_string(),
		serde_json::Value::String(
			page_data
				.front_matter
				.as_ref()
				.and_then(|fm| if let Pod::Hash(map) = fm { map.get("title") } else { None })
				.and_then(|t| if let Pod::String(s) = t { Some(s.clone()) } else { None })
				.unwrap_or_else(|| page.to_string()),
		),
	);

	let page_deslashed = page.trim_end_matches('/');
	let slug = page_deslashed[page_deslashed.rfind('/').map(|x| x + 1).unwrap_or(0)..].to_string();
	page_obj.insert("slug".to_string(), serde_json::Value::String(slug));

	page_obj.insert(
		"content".to_string(),
		serde_json::Value::String(String::from_utf8_lossy(&page_data.content).to_string()),
	);

	page_obj.insert(
		"permalink".to_string(),
		serde_json::Value::String(format!(
			"{}/{}",
			config.site.base_url.trim_end_matches('/'),
			page.trim_start_matches('/')
		)),
	);

	if let Some(relative_path) = metadata.page_paths.get(page) {
		page_obj.insert(
			"relative_path".to_string(),
			serde_json::Value::String(format!("{relative_path}.md")),
		);
	}

	if let Some(description) = page_data
		.front_matter
		.as_ref()
		.and_then(|fm| if let Pod::Hash(map) = fm { map.get("description") } else { None })
		.and_then(|d| if let Pod::String(s) = d { Some(s.clone()) } else { None })
	{
		page_obj.insert("description".to_string(), serde_json::Value::String(description));
	}

	if let Some(Pod::Hash(fm_map)) = &page_data.front_matter {
		for (key, value) in fm_map {
			if !page_obj.contains_key(key) {
				page_obj.insert(key.clone(), pod_to_json_value(value));
			}
		}

		if let Some(Pod::Hash(taxonomies)) = fm_map.get("taxonomies")
			&& let Some(Pod::Array(categories)) = taxonomies.get("categories")
		{
			let category_objects: Vec<serde_json::Value> = categories
				.iter()
				.filter_map(|c| {
					if let Pod::String(cat_name) = c {
						let cat_slug = slugify(cat_name);
						Some(serde_json::json!({
							"name": cat_name,
							"slug": cat_slug,
							"permalink": format!("{}/categories/{}/", config.site.base_url.trim_end_matches('/'), cat_slug)
						}))
					} else {
						None
					}
				})
				.collect();
			page_obj.insert("categories".to_string(), serde_json::Value::Array(category_objects));
		}

		if let Some(page_metadata) = metadata.pages_metadata.get(page) {
			let tag_objects: Vec<serde_json::Value> = page_metadata
				.get_tags()
				.map(|tag_name| {
					let tag_slug = slugify_tag(tag_name);
					serde_json::json!({
						"name": tag_name,
						"slug": tag_slug,
						"permalink": format!("{}/tags/#{}", config.site.base_url.trim_end_matches('/'), tag_slug)
					})
				})
				.collect();
			if !tag_objects.is_empty() {
				page_obj.insert("tags".to_string(), serde_json::Value::Array(tag_objects));
			}
		}

		if let Some(Pod::Array(categories)) = fm_map.get("categories") {
			let category_objects: Vec<serde_json::Value> = categories
				.iter()
				.filter_map(|c| {
					if let Pod::String(cat_name) = c {
						let cat_slug = slugify(cat_name);
						Some(serde_json::json!({
							"name": cat_name,
							"slug": cat_slug,
							"permalink": format!("{}/categories/{}/", config.site.base_url.trim_end_matches('/'), cat_slug)
						}))
					} else {
						None
					}
				})
				.collect();
			page_obj.insert("categories".to_string(), serde_json::Value::Array(category_objects));
		}
	}

	let page_deslashed = page.trim_end_matches('/');
	let prefix = if let Some(last_slash) = page_deslashed.rfind('/') {
		page[..last_slash].to_string()
	} else {
		String::new()
	};

	if let Some(siblings) = metadata.sibling_orders.get(&prefix)
		&& let Some(current_index) = siblings.iter().position(|p| p == page)
	{
		if current_index > 0 {
			let prev_page = &siblings[current_index - 1];
			if let Some(prev_metadata) = metadata.pages_metadata.get(prev_page) {
				let prev_title = prev_metadata.title.as_ref().unwrap_or(prev_page);
				page_obj.insert(
					"higher".to_string(),
					serde_json::json!({
						"title": prev_title,
						"permalink": format!("{}/{}", config.site.base_url.trim_end_matches('/'), prev_page)
					}),
				);
			}
		}

		if current_index + 1 < siblings.len() {
			let next_page = &siblings[current_index + 1];
			if let Some(next_metadata) = metadata.pages_metadata.get(next_page) {
				let next_title = next_metadata.title.as_ref().unwrap_or(next_page);
				page_obj.insert(
					"lower".to_string(),
					serde_json::json!({
						"title": next_title,
						"permalink": format!("{}/{}", config.site.base_url.trim_end_matches('/'), next_page)
					}),
				);
			}
		}
	}

	if let Some(children) = metadata.sibling_orders.get(page_deslashed) {
		let child_objects: Vec<serde_json::Value> = children
			.iter()
			.rev()
			.filter_map(|child_page| {
				metadata.pages_metadata.get(child_page).map(|child_metadata| {
					let child_title = child_metadata.title.as_ref().unwrap_or(child_page);

					let (description, date, updated, summary) = if let Some(Pod::Hash(map)) = &child_metadata.front_matter {
						let description = map
							.get("description")
							.and_then(|d| if let Pod::String(s) = d { Some(s.as_str()) } else { None });
						let date = map
							.get("date")
							.and_then(|d| if let Pod::String(s) = d { Some(s.as_str()) } else { None });
						let updated = map
							.get("updated")
							.and_then(|d| if let Pod::String(s) = d { Some(s.as_str()) } else { None });
						let summary = map
							.get("summary")
							.and_then(|d| if let Pod::String(s) = d { Some(s.as_str()) } else { None });
						(description, date, updated, summary)
					} else {
						(None, None, None, None)
					};

					let reading_time = child_metadata.reading_time;

					serde_json::json!({
						"title": child_title,
						"permalink": format!("{}/{}", config.site.base_url.trim_end_matches('/'), child_page),
						"slug": child_page,
						"description": description,
						"date": date,
						"updated": updated,
						"summary": summary,
						"reading_time": reading_time
					})
				})
			})
			.collect();

		if !child_objects.is_empty() {
			page_obj.insert("children".to_string(), serde_json::Value::Array(child_objects));
		}
	}

	context.insert("page", &page_obj);

	let template_name = page_data
		.front_matter
		.as_ref()
		.and_then(|fm| if let Pod::Hash(map) = fm { map.get("template") } else { None })
		.and_then(|t| if let Pod::String(s) = t { Some(s.as_str()) } else { None })
		.unwrap_or("page.html");

	if is_template {
		let mut temp_templates = templates.clone();
		let content_template_name = format!("_content_{}", page.replace('/', "_"));
		temp_templates.add_raw_template(&content_template_name, &String::from_utf8_lossy(&page_data.html_content))?;
		let rendered_content = temp_templates.render(&content_template_name, &context)?;

		context.insert("content", &rendered_content);
	}

	templates.render(template_name, &context)
}
