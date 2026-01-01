// SPDX-FileCopyrightText: 2025 LunNova
//
// SPDX-License-Identifier: MIT

use regex::Regex;
use std::sync::LazyLock;

static LINK_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[\[([^\]]+)\]\]").unwrap());
static TAG_CLEANUP_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^a-zA-Z0-9\-]+").unwrap());

/// Normalize a URL path to match our slug keys with trailing slash
pub fn normalize_path(path: &str) -> String {
	let mut normalized = path.trim_start_matches('/').to_string();
	// Ensure trailing slash
	if !normalized.ends_with('/') {
		// but not for alternates!
		if !normalized.ends_with(".md") && !normalized.ends_with(".txt") {
			normalized.push('/');
		}
	}
	normalized
}

pub fn slugify(s: &str) -> String {
	let mut input = s.to_string();

	// Handle index files: use parent directory name instead
	if input.ends_with("/index") {
		input = input.trim_end_matches("/index").to_string();
	} else if input.ends_with("/_index") {
		input = input.trim_end_matches("/_index").to_string();
	} else if input == "_index" || input == "index" {
		input = String::new(); // Root index becomes empty string
	}

	let mut result = input
		.split('/')
		// Transparent directories: segments starting with _ are stripped from URL
		.filter(|segment| !segment.starts_with('_'))
		.collect::<Vec<_>>()
		.join("/")
		.to_lowercase()
		.replace([' ', '_'], "-")
		.chars()
		.filter(|c| c.is_alphanumeric() || *c == '-' || *c == '/')
		.collect::<String>()
		.split('/')
		.map(|segment| {
			// Remove multiple consecutive hyphens and trim hyphens from ends
			segment.split('-').filter(|part| !part.is_empty()).collect::<Vec<_>>().join("-")
		})
		.collect::<Vec<_>>()
		.join("/");

	// Add trailing slash except for empty string (root)
	if !result.ends_with('/') {
		result.push('/');
	}

	result
}

/// Slugify a tag name for use in fragment identifiers, permalinks, etc.
/// Unlike `slugify()`, this doesn't add trailing slashes.
pub fn slugify_tag(s: &str) -> String {
	let lowercase = s.to_lowercase();
	let cleaned = TAG_CLEANUP_REGEX.replace_all(&lowercase, "-");
	cleaned.split('-').filter(|part| !part.is_empty()).collect::<Vec<_>>().join("-")
}

/// Simple, stable hash function for strings that won't change across Rust versions.
/// Uses a basic polynomial rolling hash with a fixed prime.
pub fn stable_string_hash(s: &str) -> u64 {
	let mut hash = 0u64;
	for chr in s.chars() {
		hash = hash.wrapping_mul(31).wrapping_add(chr as u64);
	}
	hash
}

pub fn process_links(content: &str) -> (String, Vec<String>) {
	let mut links = Vec::new();
	let processed = LINK_REGEX
		.replace_all(content, |caps: &regex::Captures| {
			let link = caps.get(1).unwrap().as_str();
			links.push(link.to_string());
			format!("<a href=\"/{link}\">{link}</a>")
		})
		.to_string();
	(processed, links)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_stable_string_hash() {
		assert_eq!(stable_string_hash("test"), stable_string_hash("test"));
		assert_ne!(stable_string_hash("test"), stable_string_hash("different"));
		assert_eq!(stable_string_hash(""), 0);
		assert_eq!(stable_string_hash("a"), 'a' as u64);
		assert_eq!(stable_string_hash("b"), 'b' as u64);
	}

	#[test]
	fn test_process_links_preserves_trailing_spaces() {
		let content = "In the above syntax the pattern after `is` acts as a predicate constraining which values of the supertype are valid members of the pattern type.  \nPattern types are a form of predicate subtyping; they are limited to predicates that Rust's patterns can express.  \nPattern types are described as refinement types in the WIP RFC body, but are less powerful than refinement types as typically described in the literature.";

		let (processed, _links) = process_links(content);

		// Should preserve the trailing spaces
		assert!(processed.contains("pattern type.  \n"), "Trailing spaces should be preserved");
		assert!(processed.contains("express.  \n"), "Trailing spaces should be preserved");

		println!("Original content: {content:?}");
		println!("Processed content: {processed:?}");
	}

	#[test]
	fn test_normalize_path() {
		// Root path
		assert_eq!(normalize_path("/"), "/");
		assert_eq!(normalize_path(""), "/");

		// Simple paths
		assert_eq!(normalize_path("/articles"), "articles/");
		assert_eq!(normalize_path("articles"), "articles/");
		assert_eq!(normalize_path("/articles/"), "articles/");
		assert_eq!(normalize_path("articles/"), "articles/");

		// Nested paths
		assert_eq!(normalize_path("/articles/tech/"), "articles/tech/");
		assert_eq!(normalize_path("/articles/tech"), "articles/tech/");

		// Edge cases
		assert_eq!(normalize_path("///"), "/");
		assert_eq!(normalize_path("/index"), "index/");
		assert_eq!(normalize_path("/_index"), "_index/");
	}

	#[test]
	fn test_slugify() {
		assert_eq!(slugify("Test Page"), "test-page/");
		assert_eq!(slugify("test_page"), "test-page/");
		assert_eq!(slugify("Test-Page"), "test-page/");
		assert_eq!(slugify("articles/My Article"), "articles/my-article/");
		assert_eq!(slugify(""), "/");

		// Test index file handling in slugify (these should match URL paths)
		assert_eq!(slugify("_index"), "/"); // Root _index becomes empty
		assert_eq!(slugify("articles"), "articles/");
		assert_eq!(slugify("articles/"), "articles/");
		assert_eq!(slugify("articles/_index"), "articles/");
		assert_eq!(slugify("articles/tech"), "articles/tech/");
	}

	#[test]
	fn test_slugify_transparent_dirs() {
		assert_eq!(slugify("articles/_2024/my-post"), "articles/my-post/");
		assert_eq!(slugify("articles/_2024/_drafts/my-post"), "articles/my-post/");
		assert_eq!(slugify("articles/_old/nested/page"), "articles/nested/page/");
		assert_eq!(slugify("_hidden/articles/_2024/post"), "articles/post/");
		assert_eq!(slugify("_archive/old-post"), "old-post/");

		// _index is stripped as filename, not filtered as transparent dir
		assert_eq!(slugify("articles/_index"), "articles/");
		assert_eq!(slugify("articles/_2024/_index"), "articles/");

		// underscore in filename (not directory) converts to hyphen
		assert_eq!(slugify("articles/my_post"), "articles/my-post/");
	}

	#[test]
	fn test_path_matching() {
		// Test that slugified file paths (as they come from get_all_pages) match normalized URL paths

		// Root _index.md processing: "_index" should become "" after slugify to match "/" URL
		assert_eq!(slugify("_index"), normalize_path("/"));
		assert_eq!(slugify("_index"), normalize_path(""));
		assert_eq!(normalize_path(""), normalize_path("/"));

		// Root index.md processing: "index" should become "" after slugify to match "/" URL
		assert_eq!(slugify("index"), normalize_path("/"));

		// Section _index files: "articles/_index" should become "articles/" to match "/articles/" URL
		assert_eq!(slugify("articles/_index"), normalize_path("/articles"));
		assert_eq!(slugify("articles/_index"), normalize_path("/articles/"));

		// Section index files: "articles/index" should become "articles/" to match "/articles/" URL
		assert_eq!(slugify("articles/index"), normalize_path("/articles"));
		assert_eq!(slugify("articles/index"), normalize_path("/articles/"));

		// Nested section _index: "articles/tech/_index" should become "articles/tech/"
		assert_eq!(slugify("articles/tech/_index"), normalize_path("/articles/tech"));
		assert_eq!(slugify("articles/tech/_index"), normalize_path("/articles/tech/"));

		// Regular pages should work
		assert_eq!(slugify("articles/some-post"), normalize_path("/articles/some-post"));
	}
}
