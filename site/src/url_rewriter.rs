// SPDX-FileCopyrightText: 2025 LunNova
//
// SPDX-License-Identifier: MIT

use html5ever::parse_document;
use html5ever::serialize::{SerializeOpts, serialize};
use html5ever::tendril::{StrTendril, TendrilSink};
use markup5ever_rcdom::{Handle, NodeData, RcDom, SerializableHandle};
use std::default::Default;
use url::Url;

/// Rewrite URLs in HTML content to convert relative and site-relative URLs to absolute URLs
pub fn rewrite_urls(html: &str, base_url: &str, current_path: &str) -> Result<String, Box<dyn std::error::Error>> {
	let dom = parse_document(RcDom::default(), Default::default())
		.from_utf8()
		.read_from(&mut html.as_bytes())?;

	let site_base = Url::parse(base_url)?;

	let current_url = site_base.join(current_path)?;

	walk_and_rewrite(&dom.document, &site_base, &current_url);

	let mut html_output = Vec::new();
	serialize(
		&mut html_output,
		&SerializableHandle::from(dom.document.clone()),
		SerializeOpts::default(),
	)?;

	let result = String::from_utf8(html_output)?;

	Ok(result)
}

fn walk_and_rewrite(handle: &Handle, site_base: &Url, current_url: &Url) {
	let node = handle;

	if let NodeData::Element { ref name, ref attrs, .. } = node.data {
		let tag_name = &*name.local;

		let mut attrs = attrs.borrow_mut();
		for attr in attrs.iter_mut() {
			let attr_name = &*attr.name.local;

			let should_rewrite = match attr_name {
				"href" | "src" => true,
				"action" => tag_name == "form",
				_ => false,
			};

			if should_rewrite && let Ok(rewritten) = rewrite_single_url(&attr.value, site_base, current_url) {
				attr.value = StrTendril::from(rewritten);
			}
		}
	}

	for child in node.children.borrow().iter() {
		walk_and_rewrite(child, site_base, current_url);
	}
}

fn rewrite_single_url(url_str: &str, site_base: &Url, current_url: &Url) -> Result<String, Box<dyn std::error::Error>> {
	let trimmed = url_str.trim();

	if trimmed.is_empty()
		|| trimmed.starts_with('#')
		|| trimmed.starts_with("mailto:")
		|| trimmed.starts_with("javascript:")
		|| trimmed.starts_with("data:")
		|| trimmed.starts_with("tel:")
	{
		return Ok(trimmed.to_string());
	}

	if Url::parse(trimmed).is_ok() {
		return Ok(trimmed.to_string());
	}

	if trimmed.starts_with('/') {
		let absolute = site_base.join(trimmed)?;
		return Ok(absolute.to_string());
	}

	let absolute = current_url.join(trimmed)?;
	Ok(absolute.to_string())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_site_relative_urls() {
		let html = r#"<a href="/about">About</a> <a href="/articles/post">Post</a>"#;
		let result = rewrite_urls(html, "https://example.com", "/current/").unwrap();

		assert!(result.contains(r#"href="https://example.com/about""#));
		assert!(result.contains(r#"href="https://example.com/articles/post""#));
	}

	#[test]
	fn test_relative_urls() {
		let html = r#"<img src="./image.png"> <a href="../other.html"> <img src="nested/pic.jpg">"#;
		let result = rewrite_urls(html, "https://example.com", "/articles/post/").unwrap();

		assert!(result.contains(r#"src="https://example.com/articles/post/image.png""#));
		assert!(result.contains(r#"href="https://example.com/articles/other.html""#));
		assert!(result.contains(r#"src="https://example.com/articles/post/nested/pic.jpg""#));
	}

	#[test]
	fn test_absolute_urls_unchanged() {
		let html = r#"<a href="https://external.com/page">External</a> <img src="http://cdn.example.com/image.png">"#;
		let result = rewrite_urls(html, "https://example.com", "/current/").unwrap();

		assert!(result.contains(r#"href="https://external.com/page""#));
		assert!(result.contains(r#"src="http://cdn.example.com/image.png""#));
	}

	#[test]
	fn test_special_urls_unchanged() {
		let html = r##"<a href="#section">Anchor</a> <a href="mailto:test@example.com">Email</a> <a href="javascript:void(0)">JS</a>"##;
		let result = rewrite_urls(html, "https://example.com", "/current/").unwrap();

		assert!(result.contains(r##"href="#section""##));
		assert!(result.contains("href=\"mailto:test@example.com\""));
		assert!(result.contains(r#"href="javascript:void(0)""#));
	}

	#[test]
	fn test_form_actions() {
		let html = r#"<form action="/submit">form</form> <form action="./handler.php">form2</form>"#;
		let result = rewrite_urls(html, "https://example.com", "/forms/").unwrap();

		assert!(result.contains(r#"action="https://example.com/submit""#));
		assert!(result.contains(r#"action="https://example.com/forms/handler.php""#));
	}

	#[test]
	fn test_mixed_attributes() {
		let html = r#"<a href="/page"><img src="./thumb.jpg" alt="test"></a>"#;
		let result = rewrite_urls(html, "https://example.com", "/gallery/").unwrap();

		assert!(result.contains(r#"href="https://example.com/page""#));
		assert!(result.contains(r#"src="https://example.com/gallery/thumb.jpg""#));
	}

	#[test]
	fn test_root_path() {
		let html = r#"<a href="./about.html">About</a>"#;
		let result = rewrite_urls(html, "https://example.com", "/").unwrap();

		assert!(result.contains(r#"href="https://example.com/about.html""#));
	}

	#[test]
	fn test_nested_path() {
		let html = r#"<a href="../../../root.html">Root</a>"#;
		let result = rewrite_urls(html, "https://example.com", "/a/b/c/d/").unwrap();

		assert!(result.contains(r#"href="https://example.com/a/root.html""#));
	}

	#[test]
	fn test_link_elements() {
		let html = r#"<link rel="stylesheet" href="/css/style.css"> <link rel="icon" href="./favicon.ico"> <link rel="preload" href="../fonts/font.woff2">"#;
		let result = rewrite_urls(html, "https://example.com", "/blog/post/").unwrap();

		assert!(result.contains(r#"href="https://example.com/css/style.css""#));
		assert!(result.contains(r#"href="https://example.com/blog/post/favicon.ico""#));
		assert!(result.contains(r#"href="https://example.com/blog/fonts/font.woff2""#));
	}

	#[test]
	fn test_full_html_document() {
		let html = r#"<!DOCTYPE html>
<html>
<head>
    <title>Test Page</title>
    <link rel="stylesheet" href="/css/style.css">
    <link rel="icon" href="./favicon.ico">
</head>
<body>
    <h1>Hello World</h1>
    <a href="/about">About</a>
    <img src="./image.png" alt="test">
    <form action="../submit">
        <input type="submit" value="Submit">
    </form>
</body>
</html>"#;
		let result = rewrite_urls(html, "https://example.com", "/blog/post/").unwrap();

		// Should preserve full document structure
		assert!(result.contains("<!DOCTYPE html>"));
		assert!(result.contains("<html>"));
		assert!(result.contains("<head>"));
		assert!(result.contains("<body>"));
		assert!(result.contains("</body>"));
		assert!(result.contains("</html>"));

		// Should rewrite URLs in head
		assert!(result.contains(r#"href="https://example.com/css/style.css""#));
		assert!(result.contains(r#"href="https://example.com/blog/post/favicon.ico""#));

		// Should rewrite URLs in body
		assert!(result.contains(r#"href="https://example.com/about""#));
		assert!(result.contains(r#"src="https://example.com/blog/post/image.png""#));
		assert!(result.contains(r#"action="https://example.com/blog/submit""#));
	}
}
