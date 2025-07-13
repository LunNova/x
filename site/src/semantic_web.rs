// SPDX-FileCopyrightText: 2025 LunNova
//
// SPDX-License-Identifier: MIT

use gray_matter::Pod;
use std::collections::{BTreeMap, HashMap};

use crate::config::BlogConfig;
use crate::context::generate_breadcrumbs_from_metadata;
use crate::pages::PageMetadata;

fn build_author_object(config: &BlogConfig) -> Option<serde_json::Value> {
	if let Some(extra) = &config.extra
		&& let Some(author_name) = extra.get("author").and_then(|a| a.as_str())
	{
		if let Some(github) = extra.get("github").and_then(|g| g.as_str()) {
			return Some(serde_json::json!({
				"@id": format!("https://github.com/{}", github),
				"@type": "Person",
				"name": author_name,
				"url": format!("https://github.com/{}", github)
			}));
		} else {
			return Some(serde_json::json!({
				"@type": "Person",
				"name": author_name
			}));
		}
	}
	None
}

fn build_breadcrumb_ldjson(current_page: &str, pages_metadata: &BTreeMap<String, PageMetadata>, base_url: &str) -> Option<serde_json::Value> {
	let breadcrumbs = generate_breadcrumbs_from_metadata(current_page, pages_metadata, base_url);
	if breadcrumbs.is_empty() {
		return None;
	}

	let items: Vec<serde_json::Value> = breadcrumbs
		.iter()
		.enumerate()
		.map(|(i, crumb)| {
			serde_json::json!({
				"@type": "ListItem",
				"position": i + 1,
				"name": crumb.title,
				"item": crumb.url
			})
		})
		.collect();

	Some(serde_json::json!({
		"@context": "https://schema.org",
		"@type": "BreadcrumbList",
		"itemListElement": items
	}))
}

pub fn generate_ldjson_impl(
	args: &HashMap<String, tera::Value>,
	config: &BlogConfig,
	pages_metadata: &BTreeMap<String, PageMetadata>,
) -> tera::Result<tera::Value> {
	let data_type = args
		.get("type")
		.and_then(|v| v.as_str())
		.ok_or_else(|| tera::Error::msg("generate_ldjson requires 'type' parameter"))?;

	let current_page = args
		.get("current_page")
		.and_then(|v| v.as_str())
		.ok_or_else(|| tera::Error::msg("generate_ldjson requires 'current_page' parameter"))?;
	let current_page = current_page.trim_start_matches('/');

	match data_type {
		"breadcrumb" => {
			if let Some(breadcrumb_json) = build_breadcrumb_ldjson(current_page, pages_metadata, &config.site.base_url) {
				Ok(tera::Value::String(breadcrumb_json.to_string()))
			} else {
				Ok(tera::Value::String("".to_string()))
			}
		}
		"site_navigation" => {
			let mut names = Vec::new();
			let mut urls = Vec::new();

			for (path, page_metadata) in pages_metadata {
				if let Some(Pod::Hash(front_matter)) = &page_metadata.front_matter
					&& let Some(Pod::Boolean(true)) = front_matter.get("in_nav")
					&& let Some(title) = &page_metadata.title
				{
					names.push(title.clone());
					urls.push(format!("/{path}"));
				}
			}

			let json = serde_json::json!({
				"@context": "https://schema.org",
				"@type": "SiteNavigationElement",
				"name": names,
				"url": urls
			});
			Ok(tera::Value::String(json.to_string()))
		}
		"website" => {
			let now = chrono::Utc::now();
			let mut json = serde_json::json!({
				"@context": "https://schema.org",
				"@type": "WebSite",
				"name": config.site.title,
				"url": config.site.base_url,
				"inLanguage": "en",
				"copyrightYear": now.format("%Y").to_string()
				// TODO: use config.site_published_date for datePublished
				// TODO: use RenderedSite.last_modified for site dateModified
			});

			if let Some(description) = &config.site.description {
				json["description"] = serde_json::Value::String(description.clone());
				json["abstract"] = serde_json::Value::String(description.clone());
			}

			if let Some(author) = build_author_object(config) {
				json["author"] = author;
			}

			if let Some(extra) = &config.extra {
				if let Some(license_url) = extra.get("license_url").and_then(|v| v.as_str()) {
					json["license"] = serde_json::Value::String(license_url.to_string());
				}

				if let Some(nav_items) = extra.get("nav_items").and_then(|v| v.as_array()) {
					let items: Vec<serde_json::Value> = nav_items
						.iter()
						.enumerate()
						.map(|(i, item)| {
							let mut list_item = serde_json::json!({
								"@type": "ListItem",
								"position": i + 1,
								"name": item.get("title").and_then(|v| v.as_str()).unwrap_or(""),
								"url": item.get("url").and_then(|v| v.as_str()).unwrap_or("")
							});
							list_item["desc"] = serde_json::Value::String("".to_string());
							list_item
						})
						.collect();

					json["mainEntity"] = serde_json::json!({
						"@type": "ItemList",
						"itemListElement": items
					});
				}

				if let Some(breadcrumb_json) = build_breadcrumb_ldjson(current_page, pages_metadata, &config.site.base_url) {
					json["breadcrumb"] = breadcrumb_json;
				}
			}

			Ok(tera::Value::String(json.to_string()))
		}
		"article" => {
			let page_metadata = pages_metadata.get(current_page);
			let page_title = page_metadata
				.and_then(|m| m.title.as_ref())
				.map(|s| s.as_str())
				.unwrap_or(current_page);

			let page_url = format!("{}/{}", config.site.base_url.trim_end_matches('/'), current_page);

			let mut json = serde_json::json!({
				"@context": "https://schema.org",
				"@id": page_url,
				"@type": "BlogPosting",
				"headline": page_title,
				"name": page_title,
				"url": page_url
			});

			if let Some(author) = build_author_object(config) {
				json["author"] = author;
			}

			if let Some(description) = page_metadata
				.and_then(|m| m.front_matter.as_ref())
				.and_then(|fm| if let Pod::Hash(map) = fm { map.get("description") } else { None })
				.and_then(|d| if let Pod::String(s) = d { Some(s.as_str()) } else { None })
			{
				json["description"] = serde_json::Value::String(description.to_string());
			}

			if let Some(date) = page_metadata
				.and_then(|m| m.front_matter.as_ref())
				.and_then(|fm| if let Pod::Hash(map) = fm { map.get("date") } else { None })
				.and_then(|d| if let Pod::String(s) = d { Some(s.as_str()) } else { None })
			{
				json["datePublished"] = serde_json::Value::String(format!("{date}T00:00:00Z"));
			}

			if let Some(categories) = page_metadata
				.and_then(|m| m.front_matter.as_ref())
				.and_then(|fm| if let Pod::Hash(map) = fm { map.get("categories") } else { None })
				.and_then(|c| if let Pod::Array(arr) = c { arr.first() } else { None })
				.and_then(|c| if let Pod::String(s) = c { Some(s.as_str()) } else { None })
			{
				json["articleSection"] = serde_json::Value::String(categories.to_string());
			}

			if let Some(metadata) = page_metadata {
				let keywords: Vec<serde_json::Value> = metadata.get_tags().map(|tag| serde_json::Value::String(tag.to_string())).collect();
				if !keywords.is_empty() {
					json["keywords"] = serde_json::Value::Array(keywords);
				}
			}

			Ok(tera::Value::String(json.to_string()))
		}
		_ => Err(tera::Error::msg(format!("Unknown JSON-LD type: {data_type}"))),
	}
}
