// SPDX-FileCopyrightText: 2025 LunNova
//
// SPDX-License-Identifier: MIT

use gray_matter::Pod;
use tracing::instrument;

#[instrument(skip(content))]
pub fn parse_front_matter(content: &str) -> (String, Option<Pod>) {
	// WORKAROUND: gray_matter strips trailing spaces breaking commonmark hard break feature
	// https://github.com/the-alchemists-of-arland/gray-matter-rs/issues/23
	// Simple front matter parser that preserves trailing whitespace
	// FIXME: get this fixed upstream or stop using gray_matter's Pod type

	if content.starts_with("+++\n")
		&& let Some((front_matter_str, body)) = extract_front_matter_content(content, "+++")
		&& let Ok(toml_value) = toml::from_str::<toml::Value>(front_matter_str)
	{
		let pod = toml_value_to_pod(toml_value);
		return (trim_leading_newline(body).to_string(), Some(pod));
	}

	if content.starts_with("---\n")
		&& let Some((front_matter_str, body)) = extract_front_matter_content(content, "---")
		&& let Ok(toml_value) = toml::from_str::<toml::Value>(front_matter_str)
	{
		let pod = toml_value_to_pod(toml_value);
		return (trim_leading_newline(body).to_string(), Some(pod));
	}

	if content.starts_with("---\n")
		&& let Some((front_matter_str, body)) = extract_front_matter_content(content, "---")
		&& let Ok(yaml_value) = serde_yaml::from_str::<serde_yaml::Value>(front_matter_str)
	{
		let pod = yaml_value_to_pod(yaml_value);
		return (trim_leading_newline(body).to_string(), Some(pod));
	}

	(content.to_string(), None)
}

fn extract_front_matter_content<'a>(content: &'a str, delimiter: &str) -> Option<(&'a str, &'a str)> {
	let start_pattern = format!("{delimiter}\n");
	let end_pattern_with_newline = format!("\n{delimiter}\n");
	let end_pattern_eof = format!("\n{delimiter}");

	if !content.starts_with(&start_pattern) {
		return None;
	}

	let front_matter_start = start_pattern.len();
	let search_content = &content[front_matter_start..];

	if let Some(end_pos) = search_content.find(&end_pattern_with_newline) {
		let front_matter_str = &content[front_matter_start..front_matter_start + end_pos];
		let body = &content[front_matter_start + end_pos + end_pattern_with_newline.len()..];
		return Some((front_matter_str, body));
	}

	if let Some(end_pos) = search_content.find(&end_pattern_eof)
		&& front_matter_start + end_pos + end_pattern_eof.len() == content.len()
	{
		let front_matter_str = &content[front_matter_start..front_matter_start + end_pos];
		let body = "";
		return Some((front_matter_str, body));
	}

	None
}

fn trim_leading_newline(content: &str) -> &str {
	content.strip_prefix('\n').unwrap_or(content)
}

fn toml_value_to_pod(value: toml::Value) -> Pod {
	match value {
		toml::Value::String(s) => Pod::String(s),
		toml::Value::Integer(i) => Pod::Integer(i),
		toml::Value::Float(f) => Pod::Float(f),
		toml::Value::Boolean(b) => Pod::Boolean(b),
		toml::Value::Array(arr) => {
			let pod_vec: Vec<Pod> = arr.into_iter().map(toml_value_to_pod).collect();
			Pod::Array(pod_vec)
		}
		toml::Value::Table(table) => {
			let mut pod_map = std::collections::HashMap::new();
			for (key, value) in table {
				pod_map.insert(key, toml_value_to_pod(value));
			}
			Pod::Hash(pod_map)
		}
		toml::Value::Datetime(dt) => {
			if let Some(date) = dt.date {
				Pod::String(date.to_string())
			} else {
				Pod::String(dt.to_string())
			}
		}
	}
}

fn yaml_value_to_pod(value: serde_yaml::Value) -> Pod {
	match value {
		serde_yaml::Value::Null => Pod::Null,
		serde_yaml::Value::Bool(b) => Pod::Boolean(b),
		serde_yaml::Value::Number(n) => {
			if let Some(i) = n.as_i64() {
				Pod::Integer(i)
			} else if let Some(f) = n.as_f64() {
				Pod::Float(f)
			} else {
				Pod::String(n.to_string())
			}
		}
		serde_yaml::Value::String(s) => Pod::String(s),
		serde_yaml::Value::Sequence(seq) => {
			let pod_vec: Vec<Pod> = seq.into_iter().map(yaml_value_to_pod).collect();
			Pod::Array(pod_vec)
		}
		serde_yaml::Value::Mapping(map) => {
			let mut pod_map = std::collections::HashMap::new();
			for (key, value) in map {
				if let serde_yaml::Value::String(key_str) = key {
					pod_map.insert(key_str, yaml_value_to_pod(value));
				}
			}
			Pod::Hash(pod_map)
		}
		serde_yaml::Value::Tagged(tagged) => yaml_value_to_pod(tagged.value),
	}
}

#[instrument(skip(pod))]
pub fn pod_to_json_value(pod: &Pod) -> serde_json::Value {
	match pod {
		Pod::Null => serde_json::Value::Null,
		Pod::String(s) => serde_json::Value::String(s.clone()),
		Pod::Integer(i) => serde_json::Value::Number((*i).into()),
		Pod::Float(f) => {
			if let Some(n) = serde_json::Number::from_f64(*f) {
				serde_json::Value::Number(n)
			} else {
				serde_json::Value::from(f64::NAN)
			}
		}
		Pod::Boolean(b) => serde_json::Value::Bool(*b),
		Pod::Array(arr) => {
			let vec: Vec<_> = arr.iter().map(pod_to_json_value).collect();
			serde_json::Value::Array(vec)
		}
		Pod::Hash(map) => {
			let obj: serde_json::Map<String, serde_json::Value> = map.iter().map(|(k, v)| (k.clone(), pod_to_json_value(v))).collect();
			serde_json::Value::Object(obj)
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn assert_front_matter_hash(front_matter: Option<Pod>) -> std::collections::HashMap<String, Pod> {
		assert!(front_matter.is_some());
		if let Some(Pod::Hash(map)) = front_matter {
			map
		} else {
			panic!("Front matter should be a hash: {front_matter:?}");
		}
	}

	fn assert_clean_date(map: &std::collections::HashMap<String, Pod>, expected: &str) {
		if let Some(Pod::String(date_str)) = map.get("date") {
			assert_eq!(date_str, expected);
			assert!(
				!date_str.contains("$__toml_private_datetime"),
				"Date should not contain TOML internal representation"
			);
		} else {
			panic!("Date should be parsed as a string: {:?}", map.get("date"));
		}
	}

	fn assert_taxonomies_with_tags(map: &std::collections::HashMap<String, Pod>, expected_tags: &[&str]) {
		if let Some(Pod::Hash(taxonomies)) = map.get("taxonomies") {
			if let Some(Pod::Array(tags_array)) = taxonomies.get("tags") {
				assert_eq!(tags_array.len(), expected_tags.len());
				for (i, expected_tag) in expected_tags.iter().enumerate() {
					assert_eq!(tags_array[i], Pod::String(expected_tag.to_string()));
				}
			} else {
				panic!("Tags should be parsed as an array: {:?}", taxonomies.get("tags"));
			}
		} else {
			panic!("Taxonomies should be parsed as a hash: {:?}", map.get("taxonomies"));
		}
	}

	#[test]
	fn test_parse_front_matter_toml() {
		let content = r#"+++
title = "Test Page"
description = "A test page"
in_nav = true
+++

This is the content."#;

		let (body, front_matter) = parse_front_matter(content);

		assert_eq!(body.trim(), "This is the content.");
		let map = assert_front_matter_hash(front_matter);
		assert_eq!(map.get("title"), Some(&Pod::String("Test Page".to_string())));
		assert_eq!(map.get("description"), Some(&Pod::String("A test page".to_string())));
		assert_eq!(map.get("in_nav"), Some(&Pod::Boolean(true)));
	}

	#[test]
	fn test_parse_front_matter_yaml() {
		let content = r#"---
title: "Test Page"
description: "A test page"
in_nav: true
---

This is the content."#;

		let (body, front_matter) = parse_front_matter(content);

		assert_eq!(body.trim(), "This is the content.");
		let map = assert_front_matter_hash(front_matter);
		assert_eq!(map.get("title"), Some(&Pod::String("Test Page".to_string())));
		assert_eq!(map.get("description"), Some(&Pod::String("A test page".to_string())));
		assert_eq!(map.get("in_nav"), Some(&Pod::Boolean(true)));
	}

	#[test]
	fn test_parse_front_matter_real_toml() {
		let content = r#"+++
sort_by = "date"
template = "section.html"
page_template = "page.html"
title = "articles"
weight = 0
in_nav = true
+++"#;

		let (body, front_matter) = parse_front_matter(content);

		assert_eq!(body.trim(), "");
		let map = assert_front_matter_hash(front_matter);
		assert_eq!(map.get("title"), Some(&Pod::String("articles".to_string())));
		assert_eq!(map.get("in_nav"), Some(&Pod::Boolean(true)));
	}

	#[test]
	fn test_custom_front_matter_preserves_trailing_spaces() {
		let content = "+++\ntitle = \"Test\"\n+++\n\nLine with trailing spaces.  \nNext line.";

		let (body, front_matter) = parse_front_matter(content);

		assert!(
			body.contains("spaces.  \n"),
			"Custom front matter parser should preserve trailing spaces"
		);
		assert!(front_matter.is_some(), "Should parse front matter");

		println!("Input: {content:?}");
		println!("Our parser output: {body:?}");
		println!("Front matter: {front_matter:?}");
	}

	#[test]
	fn test_parse_front_matter_real_qrh() {
		let content = r#"---
title = "qrh"
description = "Quick Reference Handbook"
# template = "page.html"
# sort_by = "title"
# render = true
in_nav = true
---
asd"#;

		let (body, front_matter) = parse_front_matter(content);

		assert_eq!(body.trim(), "asd");
		let map = assert_front_matter_hash(front_matter);
		assert_eq!(map.get("title"), Some(&Pod::String("qrh".to_string())));
		assert_eq!(map.get("description"), Some(&Pod::String("Quick Reference Handbook".to_string())));
		assert_eq!(map.get("in_nav"), Some(&Pod::Boolean(true)));
	}

	#[test]
	fn test_parse_front_matter_with_date_and_tags() {
		let content = r#"---
title: "test-post: example pattern types in 2025"
description: Testing front matter parsing with dates and tags.
date: 2025-07-06

taxonomies:
  tags:
    - rust
    - testing
---

This is the content body."#;

		let (body, front_matter) = parse_front_matter(content);

		assert_eq!(body.trim(), "This is the content body.");
		let map = assert_front_matter_hash(front_matter);
		assert_eq!(
			map.get("title"),
			Some(&Pod::String("test-post: example pattern types in 2025".to_string()))
		);
		assert_eq!(
			map.get("description"),
			Some(&Pod::String("Testing front matter parsing with dates and tags.".to_string()))
		);
		assert_clean_date(&map, "2025-07-06");
		assert_taxonomies_with_tags(&map, &["rust", "testing"]);
	}

	#[test]
	fn test_parse_front_matter_toml_with_date_and_tags() {
		let content = r#"+++
title = "test-post: example pattern types in 2025"
description = "Testing front matter parsing with dates and tags."
date = 2025-07-06

[taxonomies]
tags = ["rust", "testing"]
+++

This is the content body."#;

		let (body, front_matter) = parse_front_matter(content);

		assert_eq!(body.trim(), "This is the content body.");
		let map = assert_front_matter_hash(front_matter);
		assert_eq!(
			map.get("title"),
			Some(&Pod::String("test-post: example pattern types in 2025".to_string()))
		);
		assert_eq!(
			map.get("description"),
			Some(&Pod::String("Testing front matter parsing with dates and tags.".to_string()))
		);
		if let Some(Pod::String(date_str)) = map.get("date") {
			println!("DEBUG: TOML date parsed as: {date_str:?}");
		}
		assert_clean_date(&map, "2025-07-06");
		assert_taxonomies_with_tags(&map, &["rust", "testing"]);
	}

	#[test]
	fn test_leading_newline_trimming() {
		let content_with_leading_newline = "+++\ntitle = \"Test\"\n+++\n\nContent starts here.";
		let content_without_leading_newline = "+++\ntitle = \"Test\"\n+++\nContent starts here.";

		let (body1, _) = parse_front_matter(content_with_leading_newline);
		let (body2, _) = parse_front_matter(content_without_leading_newline);

		assert_eq!(body1, "Content starts here.");
		assert_eq!(body2, "Content starts here.");
		assert_eq!(body1, body2);

		let content_multiple_newlines = "+++\ntitle = \"Test\"\n+++\n\n\nContent with multiple newlines.";
		let (body3, _) = parse_front_matter(content_multiple_newlines);
		assert_eq!(body3, "\nContent with multiple newlines.");

		let yaml_content = "---\ntitle: Test\n---\n\nYAML content.";
		let (body4, _) = parse_front_matter(yaml_content);
		assert_eq!(body4, "YAML content.");
	}

	#[test]
	fn test_toml_front_matter_with_yaml_delimiter() {
		let content = r#"---
title = "nixpkgs treefmt.withConfig example"
draft = true
---
Lorem ipsum dolor sit amet, consectetur adipiscing elit."#;

		let (body, front_matter) = parse_front_matter(content);

		assert_eq!(body.trim(), "Lorem ipsum dolor sit amet, consectetur adipiscing elit.");
		let map = assert_front_matter_hash(front_matter);
		assert_eq!(
			map.get("title"),
			Some(&Pod::String("nixpkgs treefmt.withConfig example".to_string()))
		);
		assert_eq!(map.get("draft"), Some(&Pod::Boolean(true)));
	}
}
