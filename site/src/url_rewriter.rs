// SPDX-FileCopyrightText: 2025 LunNova
//
// SPDX-License-Identifier: MIT

//! URL rewriter for HTML content.
//!
//! **WARNING**: This module is tested enough to work with non-hostile data for a personal site,
//! but is not intended to handle arbitrary possibly malicious HTML. Unfortunately, we can't make
//! use of html5ever_rcdom to build a DOM and HtmlSerializer to translate it back because rcdom
//! is marked unsafe and untested, leaving us with no good way to rely on html5ever's serializer
//! that would reliably know how to turn tags back into HTML. This implementation uses the
//! tokenizer directly and manually reconstructs HTML, which works for well-formed content but
//! may have edge cases with malicious or malformed input.

use html5ever::Attribute;
use html5ever::tokenizer::{BufferQueue, EndTag, StartTag, Token, TokenSink, Tokenizer, TokenizerOpts};
use markup5ever::TokenizerResult;
use std::cell::RefCell;
use std::default::Default;
use url::Url;

/// URL rewriting token sink implementation.
///
/// Note: forced to use RefCell for interior mutability because html5ever's TokenSink trait
/// takes `&self`. Can't impl TokenSink for &mut UrlRewritingTokenSink, because we get &&mut.
struct UrlRewritingTokenSink {
	output: RefCell<String>,
	site_base: Url,
	current_url: Url,
	in_raw_tag: RefCell<bool>,
}

impl UrlRewritingTokenSink {
	fn new(site_base: Url, current_url: Url) -> Self {
		Self {
			output: RefCell::new(String::new()),
			site_base,
			current_url,
			in_raw_tag: RefCell::new(false),
		}
	}

	fn should_rewrite_attr(tag_name: &str, attr_name: &str) -> bool {
		match attr_name {
			"href" | "src" => true,
			"action" => tag_name == "form",
			_ => false,
		}
	}

	fn write_start_tag(&self, name: &str, attrs: &[Attribute], self_closing: bool) {
		let mut output = self.output.borrow_mut();
		output.push('<');
		output.push_str(name);

		for attr in attrs {
			output.push(' ');
			output.push_str(&attr.name.local);
			output.push_str("=\"");

			let value = if Self::should_rewrite_attr(name, &attr.name.local) {
				rewrite_single_url(&attr.value, &self.site_base, &self.current_url).unwrap_or_else(|_| attr.value.to_string())
			} else {
				attr.value.to_string()
			};

			output.push_str(&html_escape(&value));
			output.push('"');
		}

		if self_closing {
			output.push_str(" />");
		} else {
			output.push('>');
		}
	}

	fn write_end_tag(&self, name: &str) {
		let mut output = self.output.borrow_mut();
		output.push_str("</");
		output.push_str(name);
		output.push('>');
	}
}

fn html_escape(s: &str) -> String {
	s.replace('&', "&amp;")
		.replace('<', "&lt;")
		.replace('>', "&gt;")
		.replace('"', "&quot;")
		.replace('\'', "&#39;")
}

impl TokenSink for UrlRewritingTokenSink {
	type Handle = ();

	fn process_token(&self, token: Token, _line_number: u64) -> html5ever::tokenizer::TokenSinkResult<Self::Handle> {
		use html5ever::tokenizer::TokenSinkResult;

		match token {
			Token::TagToken(tag) => match tag.kind {
				StartTag => {
					self.write_start_tag(&tag.name, &tag.attrs, tag.self_closing);
					if &*tag.name == "script" || &*tag.name == "style" {
						*self.in_raw_tag.borrow_mut() = true;
					}
				}
				EndTag => {
					if &*tag.name == "script" || &*tag.name == "style" {
						*self.in_raw_tag.borrow_mut() = false;
					}
					self.write_end_tag(&tag.name);
				}
			},
			Token::CommentToken(comment) => {
				let mut output = self.output.borrow_mut();
				output.push_str("<!--");
				output.push_str(&comment);
				output.push_str("-->");
			}
			Token::CharacterTokens(chars) => {
				let mut output = self.output.borrow_mut();
				if *self.in_raw_tag.borrow() {
					output.push_str(&chars);
				} else {
					output.push_str(&html_escape(&chars));
				}
			}
			Token::DoctypeToken(doctype) => {
				let mut output = self.output.borrow_mut();
				output.push_str("<!DOCTYPE ");
				if let Some(name) = doctype.name {
					output.push_str(&name);
				}
				if let Some(public_id) = doctype.public_id {
					output.push_str(" PUBLIC \"");
					output.push_str(&public_id);
					output.push('"');
					if let Some(system_id) = doctype.system_id {
						output.push_str(" \"");
						output.push_str(&system_id);
						output.push('"');
					}
				} else if let Some(system_id) = doctype.system_id {
					output.push_str(" SYSTEM \"");
					output.push_str(&system_id);
					output.push('"');
				}
				output.push('>');
			}
			Token::NullCharacterToken => {}
			Token::EOFToken => {}
			Token::ParseError(err) => {
				panic!("HTML parse error: {err}");
			}
		}

		TokenSinkResult::Continue
	}
}

/// Rewrite URLs in HTML content to convert relative and site-relative URLs to absolute URLs
pub fn rewrite_urls(html: &str, base_url: &str, current_path: &str) -> Result<String, Box<dyn std::error::Error>> {
	let site_base = Url::parse(base_url)?;
	let current_url = site_base.join(current_path)?;

	let sink = UrlRewritingTokenSink::new(site_base, current_url);
	let tokenizer = Tokenizer::new(sink, TokenizerOpts::default());

	let input = BufferQueue::default();
	input.push_back(html.into());

	loop {
		match tokenizer.feed(&input) {
			TokenizerResult::Done => break,
			TokenizerResult::Script(_) => continue, // Script tokens irrelevant for URL rewriting
		}
	}

	Ok(tokenizer.sink.output.into_inner())
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
	fn test_html_entities_preserved() {
		let html = r#"<p>This has &lt;tags&gt; and &quot;quotes&quot; and &amp;amp; entities.</p>"#;
		let result = rewrite_urls(html, "https://example.com", "/").unwrap();

		assert!(result.contains("&lt;tags&gt;"));
		assert!(result.contains("&quot;quotes&quot;"));
		assert!(result.contains("&amp;amp;"));
	}

	#[test]
	fn test_mixed_quotes_in_attributes() {
		let html = r#"<div title="Compiler says &quot;error&quot; but dev's fine" data-test='JSON with "escaped" keys'></div>"#;
		let result = rewrite_urls(html, "https://example.com", "/").unwrap();

		// Should preserve escaped quotes and convert single quotes to escaped form
		assert!(result.contains("&quot;error&quot;"));
		assert!(result.contains("dev&#39;s") || result.contains("dev's"));
		assert!(result.contains("JSON with &quot;escaped&quot; keys"));
	}

	#[test]
	fn test_json_ld_script_tags() {
		let html = r#"<script type="application/ld+json">
{"@context":"https://schema.org","@type":"WebSite","name":"example.com","url":"https://example.com"}
</script>"#;
		let result = rewrite_urls(html, "https://example.com", "/").unwrap();

		assert!(result.contains(r#"{"@context":"https://schema.org""#));
		assert!(!result.contains("&quot;"));
		assert!(!result.contains("&#39;"));
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
