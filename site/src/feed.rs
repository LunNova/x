// SPDX-FileCopyrightText: 2025 LunNova
//
// SPDX-License-Identifier: MIT

use crate::config::BlogConfig;
use crate::pages::PageMetadata;
use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use gray_matter::Pod;
use std::collections::BTreeMap;

// TODO: Make this configurable in site.toml
const FEED_ITEM_LIMIT: usize = 1000;

struct FeedItem {
	date: String,
	title: String,
	description: String,
	link: String,
	categories_rss: String,
	categories_atom: String,
}

fn format_rfc2822_date(date_str: &str) -> String {
	if let Ok(naive_date) = NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
		let datetime = naive_date.and_hms_opt(0, 0, 0).unwrap();
		let utc_datetime = Utc.from_utc_datetime(&datetime);
		return utc_datetime.format("%a, %d %b %Y %H:%M:%S GMT").to_string();
	}

	if DateTime::parse_from_rfc2822(date_str).is_ok() {
		return date_str.to_string();
	}
	date_str.to_string()
}

fn format_iso8601_date(date_str: &str) -> String {
	if let Ok(naive_date) = NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
		let datetime = naive_date.and_hms_opt(0, 0, 0).unwrap();
		let utc_datetime = Utc.from_utc_datetime(&datetime);
		return utc_datetime.format("%Y-%m-%dT%H:%M:%SZ").to_string();
	}

	if date_str.contains('T') {
		date_str.to_string()
	} else {
		format!("{date_str}T00:00:00Z")
	}
}

fn collect_feed_items(config: &BlogConfig, pages_metadata: &BTreeMap<String, PageMetadata>) -> Vec<FeedItem> {
	let mut dated_pages: Vec<_> = pages_metadata
		.iter()
		.filter_map(|(path, metadata)| {
			if let Some(Pod::Hash(fm)) = &metadata.front_matter
				&& let Some(Pod::String(date)) = fm.get("date")
			{
				let title = metadata.title.as_ref().unwrap_or(path);
				let description = fm
					.get("description")
					.and_then(|d| if let Pod::String(s) = d { Some(s.as_str()) } else { None })
					.unwrap_or(&metadata.content[..metadata.content.len().min(200)]);

				let sort_key = crate::pages::PageSortKey::from_metadata(path, metadata);
				return Some((sort_key, date, path, title, description));
			}
			None
		})
		.collect();

	dated_pages.sort_by(|a, b| a.0.cmp(&b.0));

	dated_pages
		.iter()
		.take(FEED_ITEM_LIMIT)
		.map(|(_sort_key, date, path, title, description)| {
			let link = format!("{}/{}", config.site.base_url.trim_end_matches('/'), path);

			let (categories_rss, categories_atom) = if let Some(metadata) = pages_metadata.get(*path) {
				let mut rss_cats = String::new();
				let mut atom_cats = String::new();
				for tag_name in metadata.get_tags() {
					rss_cats.push_str(&format!("\t\t\t<category>{}</category>\n", crate::escape_html_attribute(tag_name)));
					atom_cats.push_str(&format!("\t\t<category term=\"{}\"/>\n", crate::escape_html_attribute(tag_name)));
				}
				(rss_cats, atom_cats)
			} else {
				(String::new(), String::new())
			};

			FeedItem {
				date: date.to_string(),
				title: title.to_string(),
				description: description.to_string(),
				link,
				categories_rss,
				categories_atom,
			}
		})
		.collect()
}

pub fn generate_rss_feed(config: &BlogConfig, pages_metadata: &BTreeMap<String, PageMetadata>) -> String {
	let feed_items = collect_feed_items(config, pages_metadata);
	let mut items = String::new();

	for item in feed_items {
		items.push_str(&format!(
			r#"		<item>
			<title>{}</title>
			<link>{}</link>
			<description>{}</description>
			<pubDate>{}</pubDate>
			<guid>{}</guid>
{}		</item>
"#,
			crate::escape_html_attribute(&item.title),
			crate::escape_html_attribute(&item.link),
			crate::escape_html_attribute(&item.description),
			format_rfc2822_date(&item.date),
			crate::escape_html_attribute(&item.link),
			item.categories_rss
		));
	}

	let feed_url = format!("{}/rss.xml", config.site.base_url.trim_end_matches('/'));

	format!(
		r#"<?xml version="1.0" encoding="UTF-8"?>
<?xml-stylesheet type="text/xsl" href="/feed.xsl"?>
<rss version="2.0" xmlns:atom="http://www.w3.org/2005/Atom">
	<channel>
		<title>{}</title>
		<link>{}</link>
		<description>{}</description>
		<language>en-us</language>
		<atom:link href="{}" rel="self" type="application/rss+xml" />
{}	</channel>
</rss>"#,
		crate::escape_html_attribute(&config.site.title),
		crate::escape_html_attribute(&config.site.base_url),
		crate::escape_html_attribute(config.site.description.as_deref().unwrap_or("")),
		crate::escape_html_attribute(&feed_url),
		items
	)
}

pub fn generate_atom_feed(config: &BlogConfig, pages_metadata: &BTreeMap<String, PageMetadata>) -> String {
	let feed_items = collect_feed_items(config, pages_metadata);
	let mut entries = String::new();

	for item in &feed_items {
		entries.push_str(&format!(
			r#"	<entry>
		<title>{}</title>
		<link href="{}"/>
		<id>{}</id>
		<updated>{}</updated>
		<summary>{}</summary>
{}	</entry>
"#,
			crate::escape_html_attribute(&item.title),
			crate::escape_html_attribute(&item.link),
			crate::escape_html_attribute(&item.link),
			format_iso8601_date(&item.date),
			crate::escape_html_attribute(&item.description),
			item.categories_atom
		));
	}

	let updated = feed_items
		.first()
		.map(|item| format_iso8601_date(&item.date))
		.unwrap_or_else(|| "2024-01-01T00:00:00Z".to_string());

	let atom_feed_url = format!("{}/atom.xml", config.site.base_url.trim_end_matches('/'));

	format!(
		r#"<?xml version="1.0" encoding="UTF-8"?>
<?xml-stylesheet type="text/xsl" href="/feed.xsl"?>
<feed xmlns="http://www.w3.org/2005/Atom">
	<title>{}</title>
	<link href="{}"/>
	<link href="{}" rel="self" type="application/atom+xml"/>
	<updated>{}</updated>
	<author>
		<name>{}</name>
	</author>
	<id>{}</id>
{}
</feed>"#,
		crate::escape_html_attribute(&config.site.title),
		crate::escape_html_attribute(&config.site.base_url),
		crate::escape_html_attribute(&atom_feed_url),
		updated,
		crate::escape_html_attribute(
			config
				.extra
				.as_ref()
				.and_then(|e| e.get("author"))
				.and_then(|a| a.as_str())
				.unwrap_or("Unknown")
		),
		crate::escape_html_attribute(&config.site.base_url),
		entries
	)
}
