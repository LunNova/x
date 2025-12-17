// SPDX-FileCopyrightText: 2025 LunNova
//
// SPDX-License-Identifier: MIT

use gray_matter::Pod;
use itertools::Itertools;
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use pulldown_cmark_escape::{escape_html, escape_html_body_text};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::SystemTime;
use syntect::highlighting::{Color, Theme, ThemeSet};
use syntect::html::{IncludeBackground, styled_line_to_highlighted_html};
use syntect::parsing::SyntaxSet;
use tracing::{instrument, warn};

use crate::front_matter::parse_front_matter;
use crate::utils::slugify_tag;

static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
static THEME_SET: OnceLock<ThemeSet> = OnceLock::new();

fn get_syntax_set() -> &'static SyntaxSet {
	SYNTAX_SET.get_or_init(two_face::syntax::extra_newlines)
}

fn get_theme_set() -> &'static ThemeSet {
	THEME_SET.get_or_init(ThemeSet::load_defaults)
}

fn create_custom_theme(base_theme: &Theme) -> Theme {
	let mut theme = base_theme.clone();

	// Make comments less grayed out - use a lighter color
	// Find comment scopes and update their color
	for theme_item in theme.scopes.iter_mut() {
		let scope_str = format!("{:?}", theme_item.scope);
		if scope_str.to_lowercase().contains("comment") {
			theme_item.style.foreground = Some(Color {
				r: 140, // Lighter than typical gray
				g: 140,
				b: 176,
				a: 255,
			});
		}
	}

	theme
}

#[instrument(skip(markdown))]
pub fn markdown_to_html(markdown: &str) -> String {
	let mut options = Options::empty();
	options.insert(Options::ENABLE_STRIKETHROUGH);
	options.insert(Options::ENABLE_TABLES);
	options.insert(Options::ENABLE_FOOTNOTES);
	options.insert(Options::ENABLE_HEADING_ATTRIBUTES);
	let parser = Parser::new_ext(markdown, options);
	let mut html_output = String::new();

	let syntax_set = get_syntax_set();
	let theme_set = get_theme_set();

	// Use a dark theme that works better with our dark background
	let base_theme = theme_set
		.themes
		.get("base16-ocean.dark")
		.or_else(|| theme_set.themes.get("base16-eighties.dark"))
		.or_else(|| theme_set.themes.get("Solarized (dark)"))
		.unwrap();

	// Create our custom theme with lighter comments
	let theme = create_custom_theme(base_theme);

	// FIXME: we need to generate header IDs for headers with none
	// Header tags come with fields for id but one isn't automatically set if {# header syntax} isn't used

	// Create an iterator adapter that processes our special cases
	let processed_parser = parser.map(|event| match event {
		// If we want to make all breaks br we can, but we shouldn't need to
		// https://spec.commonmark.org/0.17/#hard-line-breaks
		// Event::SoftBreak => Event::HardBreak,
		Event::FootnoteReference(name) => {
			let mut html = String::new();
			html.push_str("<sup class=\"footnote-reference\" id=\"");
			escape_html(&mut html, &name).unwrap();
			html.push_str("-ref\"><a href=\"#");
			escape_html(&mut html, &name).unwrap();
			html.push_str("\">");
			escape_html(&mut html, &name).unwrap();
			html.push_str("</a></sup>");
			Event::Html(html.into())
		}
		Event::Start(Tag::FootnoteDefinition(name)) => {
			let mut html = String::new();
			html.push_str("<div class=\"footnote-definition\" id=\"");
			escape_html(&mut html, &name).unwrap();
			html.push_str("\"><sup class=\"footnote-definition-label\"><a href=\"#");
			escape_html(&mut html, &name).unwrap();
			html.push_str("-ref\">");
			escape_html(&mut html, &name).unwrap();
			html.push_str("</a></sup> ");
			Event::Html(html.into())
		}
		Event::End(TagEnd::FootnoteDefinition) => Event::Html("</div>".into()),
		_ => event,
	});

	// Process events efficiently with peeking_take_while
	let mut events_iter = processed_parser.peekable();

	loop {
		// Process normal events until we hit a code block or header
		pulldown_cmark::html::push_html(
			&mut html_output,
			events_iter.peeking_take_while(|e| !matches!(e, Event::Start(Tag::CodeBlock(_)) | Event::Start(Tag::Heading { .. }))),
		);

		// Handle code block if there is one
		match events_iter.next() {
			Some(Event::Start(Tag::CodeBlock(kind))) => {
				let code_lang = match kind {
					CodeBlockKind::Fenced(lang) => Some(lang.to_string()),
					_ => None,
				};
				let mut code_content = String::new();
				for inner_event in events_iter.by_ref() {
					match inner_event {
						Event::Text(text) => code_content.push_str(&text),
						Event::End(TagEnd::CodeBlock) => break,
						_ => {} // Ignore other events inside code blocks
					}
				}
				if let Some(lang) = &code_lang {
					if let Some(syntax) = syntax_set.find_syntax_by_token(lang) {
						let mut highlighter = syntect::easy::HighlightLines::new(syntax, &theme);
						html_output.push_str("<pre data-lang=\"");
						escape_html(&mut html_output, lang).unwrap();
						html_output.push_str("\"><code>");

						for line in code_content.lines() {
							let ranges = highlighter.highlight_line(line, syntax_set).unwrap();
							let html = styled_line_to_highlighted_html(&ranges[..], IncludeBackground::No).unwrap();
							html_output.push_str(&html);
							html_output.push('\n');
						}

						html_output.push_str("</code></pre>");
					} else {
						// Fallback for unknown languages
						html_output.push_str("<pre data-lang=\"");
						escape_html(&mut html_output, lang).unwrap();
						html_output.push_str("\"><code class=\"language-");
						escape_html(&mut html_output, lang).unwrap();
						html_output.push_str("\">");
						escape_html_body_text(&mut html_output, &code_content).unwrap();
						html_output.push_str("</code></pre>");
					}
				} else {
					// No language specified
					html_output.push_str("<pre><code>");
					escape_html_body_text(&mut html_output, &code_content).unwrap();
					html_output.push_str("</code></pre>");
				}
			}
			Some(Event::Start(Tag::Heading { level, id, classes, attrs })) => {
				// Handle heading with automatic ID generation
				let mut header_text = String::new();

				// Collect header content events and extract text
				let header_events: Vec<_> = events_iter
					.peeking_take_while(|e| !matches!(e, Event::End(TagEnd::Heading(_))))
					.inspect(|event| match event {
						Event::Text(text) => header_text.push_str(text),
						Event::Code(code) => header_text.push_str(code),
						_ => {}
					})
					.collect();

				// Consume the end event
				let end_event = events_iter.next();

				// Generate ID from header text if not provided
				let header_id = if id.is_some() {
					id
				} else {
					let generated_id = slugify_tag(&header_text);
					Some(generated_id.into())
				};

				// Add copy link button before the end tag
				let link_url = format!("#{}", header_id.as_ref().map(|id| id.as_ref()).unwrap_or(""));
				let copy_link = Event::Start(Tag::Link {
					link_type: pulldown_cmark::LinkType::Inline,
					dest_url: link_url.into(),
					title: "Copy link to this section".into(),
					id: "".into(),
				});
				let copy_icon = Event::Text("§".into());
				let copy_link_end = Event::End(pulldown_cmark::TagEnd::Link);

				// Emit the header with generated ID and all its content through normal renderer
				let header_start = Event::Start(Tag::Heading {
					level,
					id: header_id,
					classes,
					attrs,
				});
				pulldown_cmark::html::push_html(
					&mut html_output,
					std::iter::once(header_start)
						.chain(header_events)
						.chain([copy_link, copy_icon, copy_link_end])
						.chain(end_event),
				);
			}
			_ => {
				// No more events, we're done
				break;
			}
		}
	}

	html_output
}

#[instrument]
pub async fn load_page_content(page: &str, pages_dir: &str) -> (String, Option<Pod>, SystemTime, String) {
	use crate::pages::PAGE_EXTENSIONS;

	for ext in PAGE_EXTENSIONS {
		let path = PathBuf::from(pages_dir).join(format!("{page}.{ext}"));
		if let Ok(content) = tokio::fs::read_to_string(&path).await {
			let last_modified = tokio::fs::metadata(&path)
				.await
				.and_then(|metadata| metadata.modified())
				.unwrap_or_else(|e| {
					warn!("Failed to read metadata or modification time for '{}': {:?}", page, e);
					SystemTime::UNIX_EPOCH
				});

			let (content_body, front_matter) = parse_front_matter(&content);
			return (content_body, front_matter, last_modified, ext.to_string());
		}
	}

	warn!("Failed to find page content for '{}'", page);
	("Page not found".to_string(), None, SystemTime::UNIX_EPOCH, "md".to_string())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_hard_line_breaks() {
		// Test the CommonMark spec 6.9 - Hard line breaks
		// A line break preceded by two or more spaces should render as <br />
		let markdown = "First paragraph with some text.\n\nSecond paragraph with  \nhard break in middle.\n\nThird paragraph ends here.";

		// Use raw pulldown-cmark with default config
		let parser = Parser::new(markdown);
		let mut html = String::new();
		pulldown_cmark::html::push_html(&mut html, parser);

		// Should contain a <br> tag where the double space + newline is
		assert!(html.contains("<br"), "HTML should contain a <br> tag for hard line break");

		// Should have three paragraph blocks
		let p_count = html.matches("<p>").count();
		assert_eq!(p_count, 3, "Should have exactly 3 paragraphs, found: {p_count}");

		// Print the actual output for debugging
		println!("Markdown input:\n{markdown}");
		println!("HTML output:\n{html}");
	}

	#[test]
	fn test_pattern_wishcast_section() {
		// Test the specific section from pattern-wishcast.md that's having issues
		let markdown = "In the above syntax the pattern after `is` acts as a predicate constraining which values of the supertype are valid members of the pattern type.  \nPattern types are a form of predicate subtyping[^pr_st]; they are limited to predicates that Rust's patterns can express.  \nPattern types are described as refinement types in the WIP RFC body, but are less powerful than refinement types[^ref_st] as typically described in the literature.";

		// Use raw pulldown-cmark with default config
		let parser = Parser::new(markdown);
		let mut html = String::new();
		pulldown_cmark::html::push_html(&mut html, parser);

		// Should contain <br> tags where the double spaces + newlines are
		let br_count = html.matches("<br").count();
		assert_eq!(
			br_count, 2,
			"Should have exactly 2 <br> tags for the two hard line breaks, found: {br_count}"
		);

		// Print the actual output for debugging
		println!("Raw pulldown-cmark:");
		println!("Markdown input:\n{markdown:?}");
		println!("HTML output:\n{html}");
	}

	#[test]
	fn test_pattern_wishcast_section_our_renderer() {
		// Test the same section using our custom markdown_to_html function
		let markdown = "In the above syntax the pattern after `is` acts as a predicate constraining which values of the supertype are valid members of the pattern type.  \nPattern types are a form of predicate subtyping[^pr_st]; they are limited to predicates that Rust's patterns can express.  \nPattern types are described as refinement types in the WIP RFC body, but are less powerful than refinement types[^ref_st] as typically described in the literature.";

		// Use our custom markdown_to_html function
		let html = markdown_to_html(markdown);

		// Should contain <br> tags where the double spaces + newlines are
		let br_count = html.matches("<br").count();
		assert_eq!(
			br_count, 2,
			"Should have exactly 2 <br> tags for the two hard line breaks, found: {br_count}"
		);

		// Print the actual output for debugging
		println!("Our custom renderer:");
		println!("Markdown input:\n{markdown:?}");
		println!("HTML output:\n{html}");
	}

	#[test]
	fn test_header_id_generation() {
		// Test automatic ID generation for headers without IDs
		let markdown = "# Hello World\n\n## Testing Headers\n\n### Multiple Words Here";
		let html = markdown_to_html(markdown);

		// Should generate IDs for all headers and include copy links
		assert!(html.contains("<h1 id=\"hello-world\">Hello World<a href=\"#hello-world\" title=\"Copy link to this section\">§</a></h1>"));
		assert!(
			html.contains(
				"<h2 id=\"testing-headers\">Testing Headers<a href=\"#testing-headers\" title=\"Copy link to this section\">§</a></h2>"
			)
		);
		assert!(html.contains(
			"<h3 id=\"multiple-words-here\">Multiple Words Here<a href=\"#multiple-words-here\" title=\"Copy link to this section\">§</a></h3>"
		));
	}

	#[test]
	fn test_manual_header_ids() {
		// Test manual ID specification using the {#id} syntax
		let markdown = "# Custom Header {#my-custom-id}\n\n## Another Header {#another-id}";
		let html = markdown_to_html(markdown);

		// Should use the manually specified IDs and include copy links
		assert!(html.contains("<h1 id=\"my-custom-id\">Custom Header<a href=\"#my-custom-id\" title=\"Copy link to this section\">§</a></h1>"));
		assert!(html.contains("<h2 id=\"another-id\">Another Header<a href=\"#another-id\" title=\"Copy link to this section\">§</a></h2>"));
	}

	#[test]
	fn test_mixed_header_ids() {
		// Test mix of manual and automatic ID generation
		let markdown = "# Manual ID {#custom}\n\n## Auto Generated\n\n### Another Manual {#specific-id}\n\n#### Auto Again";
		let html = markdown_to_html(markdown);

		// Should use manual IDs where specified, generate for others, all with copy links
		assert!(html.contains("<h1 id=\"custom\">Manual ID<a href=\"#custom\" title=\"Copy link to this section\">§</a></h1>"));
		assert!(
			html.contains("<h2 id=\"auto-generated\">Auto Generated<a href=\"#auto-generated\" title=\"Copy link to this section\">§</a></h2>")
		);
		assert!(html.contains("<h3 id=\"specific-id\">Another Manual<a href=\"#specific-id\" title=\"Copy link to this section\">§</a></h3>"));
		assert!(html.contains("<h4 id=\"auto-again\">Auto Again<a href=\"#auto-again\" title=\"Copy link to this section\">§</a></h4>"));
	}

	#[test]
	fn test_header_id_with_special_chars() {
		// Test ID generation with special characters and spaces
		let markdown = "# Hello, World! & More\n\n## Testing_Underscores-And-Dashes";
		let html = markdown_to_html(markdown);

		// Should clean up special characters and normalize spaces/underscores, with copy links
		assert!(html.contains(
			"<h1 id=\"hello-world-more\">Hello, World! &amp; More<a href=\"#hello-world-more\" title=\"Copy link to this section\">§</a></h1>"
		));
		assert!(html.contains("<h2 id=\"testing-underscores-and-dashes\">Testing_Underscores-And-Dashes<a href=\"#testing-underscores-and-dashes\" title=\"Copy link to this section\">§</a></h2>"));
	}

	#[test]
	fn test_header_with_code() {
		// Test headers containing inline code
		let markdown = "# Using `code` in headers\n\n## The `main` function";
		let html = markdown_to_html(markdown);

		// Should include code content in ID generation and copy links
		assert!(html.contains("<h1 id=\"using-code-in-headers\">Using <code>code</code> in headers<a href=\"#using-code-in-headers\" title=\"Copy link to this section\">§</a></h1>"));
		assert!(html.contains(
			"<h2 id=\"the-main-function\">The <code>main</code> function<a href=\"#the-main-function\" title=\"Copy link to this section\">§</a></h2>"
		));
	}
}
