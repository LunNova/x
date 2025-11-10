// SPDX-FileCopyrightText: 2025 LunNova
//
// SPDX-License-Identifier: MIT

mod badges;
mod config;
mod context;
mod feed;
mod front_matter;
mod pages;
mod render;
mod semantic_web;
mod url_rewriter;
mod utils;

// hyper 1.4 imports. Don't change these, don't assume things that work in hyper 0.x
use hyper::body::{Bytes, Incoming};
use hyper::header::{HeaderName, HeaderValue, IF_MODIFIED_SINCE};
use hyper::server::conn::http1;
use hyper::{Method, Request, StatusCode};
use hyper_util::rt::TokioIo;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Seek};
use std::ops::Range;
use std::path::Path;
use tera::Tera;

use http_body_util::Full;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use opentelemetry::trace::TracerProvider as _;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;
use tokio::time::sleep;
use tracing::{debug, error, info, instrument, warn};
use tracing_subscriber::prelude::*;

use config::*;
use pages::{RenderedSite, StaticFiles, preload_pages_data, preload_static_files};
use utils::*;

#[instrument(skip(templates, rendered_site, static_files))]
fn setup_hot_reload(
	templates: Arc<RwLock<Tera>>,
	rendered_site: Arc<RwLock<RenderedSite>>,
	static_files: Arc<RwLock<StaticFiles>>,
	config: Arc<BlogConfig>,
	show_drafts: bool,
) {
	let config = config.clone();
	tokio::spawn(async move {
		let (tx, mut rx) = tokio::sync::mpsc::channel(1000);

		let mut watcher = RecommendedWatcher::new(
			move |res: Result<notify::Event, notify::Error>| {
				// Filter out Access events (opens, reads) before sending
				if let Ok(ref event) = res {
					if event.kind.is_access() {
						return;
					}
				}

				// Use try_send to avoid blocking - during initial setup the receiver
				// might not be draining fast enough and we don't want to block notify
				if let Err(e) = tx.try_send(res) {
					eprintln!("Failed to send file watch event (channel full?): {:?}", e);
				}
			},
			notify::Config::default(),
		)
		.unwrap();

		let theme_dir = config.theme.as_ref().map(|t| t.dir.as_str()).unwrap_or("templates");

		let theme_path = std::path::Path::new(theme_dir);
		if theme_path.exists() {
			match watcher.watch(theme_path, RecursiveMode::Recursive) {
				Ok(_) => info!("Watching theme directory: {}", theme_dir),
				Err(e) => error!("Failed to watch theme directory: {:?}", e),
			}
		}

		match watcher.watch(std::path::Path::new(&config.site.pages_dir), RecursiveMode::Recursive) {
			Ok(_) => info!("Watching pages directory: {}", config.site.pages_dir),
			Err(e) => {
				error!("Failed to watch pages directory '{}': {:?}", config.site.pages_dir, e);
				return;
			}
		}

		let static_dir = std::path::Path::new("static");
		if static_dir.exists() {
			match watcher.watch(static_dir, RecursiveMode::Recursive) {
				Ok(_) => info!("Watching static directory: static"),
				Err(e) => error!("Failed to watch static directory: {:?}", e),
			}
		}

		let theme_static_dir = std::path::Path::new(theme_dir).join("static");
		if theme_static_dir.exists() {
			match watcher.watch(&theme_static_dir, RecursiveMode::Recursive) {
				Ok(_) => info!("Watching theme static directory: {}", theme_static_dir.display()),
				Err(e) => error!("Failed to watch theme static directory: {:?}", e),
			}
		}

		let mut pending_events: HashSet<std::path::PathBuf> = HashSet::new();
		let mut last_event_time = std::time::Instant::now();
		let debounce_duration = Duration::from_millis(500);

		loop {
			let timeout_duration = if pending_events.is_empty() {
				Duration::from_secs(3600)
			} else {
				let elapsed = last_event_time.elapsed();
				if elapsed >= debounce_duration {
					Duration::from_millis(0)
				} else {
					debounce_duration - elapsed
				}
			};

			match tokio::time::timeout(timeout_duration, rx.recv()).await {
				Ok(Some(Ok(event))) => {
					debug!("Received file system event: kind={:?}, paths={:?}", event.kind, event.paths);

					if event.need_rescan() || event.kind.is_modify() || event.kind.is_create() || event.kind.is_remove() {
						let relevant_paths: Vec<_> = event
							.paths
							.into_iter()
							.filter(|path| {
								let path_str = path.to_string_lossy();
								!path_str.contains(".sass-cache")
									&& !path_str.contains(".tmp")
									&& !path_str.ends_with("~") && !path_str.contains("/.git/")
							})
							.collect();

						if !relevant_paths.is_empty() {
							debug!("Queuing file change events: {:?}", relevant_paths);
							for path in relevant_paths {
								pending_events.insert(path);
							}
							last_event_time = std::time::Instant::now();
						}
					}
				}
				Ok(Some(Err(e))) => error!("Watch error: {:?}", e),
				Ok(None) => {
					warn!("Watcher channel closed, retrying in 5 seconds...");
					sleep(Duration::from_secs(5)).await;
				}
				Err(_) => {
					if !pending_events.is_empty() {
						info!("Processing {} debounced file changes", pending_events.len());

						let has_static_changes = pending_events.iter().any(|path| {
							let path_str = path.to_string_lossy();
							path_str.contains("/static/") || path.starts_with(&theme_static_dir) || path.starts_with("static/")
						});

						if has_static_changes {
							info!("Reloading static files due to changes in {} files", pending_events.len());
							let new_static_files = preload_static_files(&config).await;
							info!("Loaded {} static files", new_static_files.len());
							*static_files.write().await = new_static_files;
						} else {
							info!("Reloading templates and pages due to changes in {} files", pending_events.len());
							let templates_pattern = format!("{theme_dir}/templates/**/*");
							let mut tera = Tera::new(&templates_pattern).unwrap();
							tera.register_filter("escape_html_attribute", EscapeHtmlAttribute);

							*templates.write().await = tera;
							let new_rendered_site = preload_pages_data(&mut *templates.write().await, &config, show_drafts).await;
							*rendered_site.write().await = new_rendered_site;
						}

						pending_events.clear();
					}
				}
			}
		}
	});
}

fn setup_opentelemetry() {
	use opentelemetry_otlp::WithExportConfig;
	// #[cfg(debug_assertions)]
	let use_otlp = std::env::var("OTLP_ENDPOINT").is_ok();

	// #[cfg(not(debug_assertions))]
	// let use_otlp = true;

	let subscriber = tracing_subscriber::registry().with(tracing_subscriber::fmt::layer().with_filter(tracing_subscriber::filter::filter_fn(
		|metadata| {
			let level = metadata.level();
			match (cfg!(debug_assertions), metadata.target().starts_with(env!("CARGO_PKG_NAME"))) {
				(true, true) => level <= &tracing::Level::TRACE,
				_ => level <= &tracing::Level::INFO,
			}
		},
	)));

	if use_otlp {
		let otlp_endpoint = std::env::var("OTLP_ENDPOINT").unwrap_or_else(|_| "http://log-target:3333".to_string());

		let tracer = opentelemetry_sdk::trace::SdkTracerProvider::builder()
			.with_batch_exporter(
				opentelemetry_otlp::SpanExporter::builder()
					.with_http()
					.with_endpoint(otlp_endpoint)
					.build()
					.unwrap(),
			)
			.with_resource(
				opentelemetry_sdk::Resource::builder()
					.with_service_name(env!("CARGO_PKG_NAME"))
					.build(),
			)
			.build()
			.tracer(module_path!());

		let telemetry = tracing_opentelemetry::layer().with_tracer(tracer);
		subscriber.with(telemetry).init();
	} else {
		subscriber.init();
	}
}

#[tokio::main]
async fn main() {
	setup_opentelemetry();

	let args: Args = argh::from_env();

	match args.command {
		Command::Serve(serve_args) => serve_blog(serve_args).await,
		Command::Render(render_args) => render_static(render_args).await,
	}
}

async fn load_blog_config(blog_dir: &str) -> Arc<BlogConfig> {
	// Relative paths in site.toml work only from blog directory
	let blog_dir = std::path::Path::new(blog_dir)
		.canonicalize()
		.unwrap_or_else(|e| panic!("Failed to resolve blog directory '{blog_dir}': {e}"));
	std::env::set_current_dir(&blog_dir).unwrap_or_else(|e| panic!("Failed to change to blog directory: {e}"));

	let config_content = std::fs::read_to_string("site.toml").unwrap_or_else(|e| panic!("Failed to read site.toml: {e}"));
	let config: BlogConfig = toml::from_str(&config_content).unwrap_or_else(|e| panic!("Failed to parse config: {e}"));

	Arc::from(config)
}

fn generate_redirect_html(base_url: &str, target_path: &str) -> String {
	let full_url = format!("{}/{}", base_url.trim_end_matches('/'), target_path);
	format!(
		r#"<!doctype html><meta charset=utf-8>
<link rel=canonical href={full_url}>
<meta http-equiv=refresh content="0; url={full_url}">
<title>Redirect</title>
<p><a href={full_url}>Click here</a> to be redirected.</p>"#
	)
}

fn escape_html_attribute(s: &'_ str) -> std::borrow::Cow<'_, str> {
	let mut output = String::with_capacity(s.len());
	for c in s.chars() {
		match c {
			'&' => output.push_str("&amp;"),
			'<' => output.push_str("&lt;"),
			'>' => output.push_str("&gt;"),
			'"' => output.push_str("&quot;"),
			'\'' => output.push_str("&apos;"),
			_ => output.push(c),
		}
	}
	if output.len() == s.len() {
		std::borrow::Cow::from(s)
	} else {
		std::borrow::Cow::from(output)
	}
}

struct EscapeHtmlAttribute;
impl tera::Filter for EscapeHtmlAttribute {
	fn filter(&self, value: &tera::Value, _args: &HashMap<String, tera::Value>) -> tera::Result<tera::Value> {
		let s = tera::try_get_value!("escape_html_attribute", "value", String, value);
		Ok(tera::Value::String(escape_html_attribute(&s).to_string()))
	}

	fn is_safe(&self) -> bool {
		true
	}
}

async fn setup_templates_and_data(config: &BlogConfig, show_drafts: bool) -> (Arc<RwLock<Tera>>, Arc<RwLock<RenderedSite>>) {
	let theme_dir = config.theme.as_ref().map(|t| t.dir.as_str()).unwrap_or("templates");
	let templates_pattern = format!("{theme_dir}/templates/**/*");
	let mut new_tmp = Tera::new(&templates_pattern).unwrap();
	new_tmp.register_filter("escape_html_attribute", EscapeHtmlAttribute);

	let templates = Arc::new(RwLock::new(new_tmp));

	let rendered_site = Arc::new(RwLock::new(
		preload_pages_data(&mut *templates.write().await, config, show_drafts).await,
	));

	(templates, rendered_site)
}

async fn serve_blog(serve_args: ServeArgs) {
	let show_drafts = serve_args.show_drafts;
	let mut config = load_blog_config(&serve_args.blog_dir).await;

	Arc::get_mut(&mut config).unwrap().site.base_url = "http://127.0.0.1:3030".to_string();

	info!("Starting blog engine for: {}", config.site.title);
	info!("Pages directory: {}", config.site.pages_dir);
	if show_drafts {
		info!("Draft pages will be shown");
	}

	let (templates, rendered_site) = setup_templates_and_data(&config, show_drafts).await;
	let static_files = Arc::new(RwLock::new(preload_static_files(&config).await));

	setup_hot_reload(
		templates.clone(),
		rendered_site.clone(),
		static_files.clone(),
		config.clone(),
		show_drafts,
	);

	let request_context = Arc::new(RequestContext {
		rendered_site,
		templates,
		static_files,
	});

	let addr: std::net::SocketAddr = ([127, 0, 0, 1], 3030).into();
	let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
	info!("Starting server on http://{}", addr);

	let runtime = tokio::runtime::Builder::new_multi_thread()
		.worker_threads((num_cpus::get() / 2).clamp(1, 8))
		.enable_all()
		.build()
		.unwrap();

	loop {
		let (stream, _) = listener.accept().await.unwrap();
		let io = TokioIo::new(stream);

		let request_context = request_context.clone();

		runtime.spawn(async move {
			if let Err(err) = http1::Builder::new()
				.serve_connection(
					io,
					hyper::service::service_fn(move |req| handle_request(req, request_context.clone())),
				)
				.await
			{
				eprintln!("Error serving connection: {err:?}");
			}
		});
	}
}

async fn render_static(render_args: RenderArgs) {
	let config = load_blog_config(&render_args.blog_dir).await;

	info!("Starting static rendering for: {}", config.site.title);
	info!("Pages directory: {}", config.site.pages_dir);
	info!("Output directory: {}", render_args.output_dir);

	let (_templates, rendered_site) = setup_templates_and_data(&config, false).await;
	let static_files = Arc::new(RwLock::new(preload_static_files(&config).await));

	let output_path = Path::new(&render_args.output_dir);
	fs::create_dir_all(output_path).unwrap_or_else(|e| panic!("Failed to create output directory: {e}"));

	info!("Rendering pages...");

	let rendered_site_read = rendered_site.read().await;
	let static_files_read = static_files.read().await;

	let sitemap_path = output_path.join("sitemap.xml");
	fs::write(&sitemap_path, &rendered_site_read.sitemap).unwrap_or_else(|e| panic!("Failed to write sitemap.xml: {e}"));
	info!("Generated sitemap.xml");

	let rss_path = output_path.join("rss.xml");
	fs::write(&rss_path, &rendered_site_read.rss_feed).unwrap_or_else(|e| panic!("Failed to write rss.xml: {e}"));
	info!("Generated rss.xml");

	let atom_path = output_path.join("atom.xml");
	fs::write(&atom_path, &rendered_site_read.atom_feed).unwrap_or_else(|e| panic!("Failed to write atom.xml: {e}"));
	info!("Generated atom.xml");

	for (page_key, page_data) in &rendered_site_read.pages_data {
		let page_key = if page_key == "/" { "" } else { page_key };
		let html_path = if page_key.is_empty() {
			output_path.join("index.html")
		} else {
			let page_dir = output_path.join(page_key);
			fs::create_dir_all(&page_dir).unwrap();
			page_dir.join("index.html")
		};
		fs::write(&html_path, &page_data.html_content).unwrap_or_else(|e| panic!("Failed to write {}: {e}", html_path.display()));

		let md_path = if page_key.is_empty() {
			output_path.join("index.md")
		} else {
			let page_dir = output_path.join(page_key);
			page_dir.join("index.md")
		};
		fs::write(&md_path, &page_data.content).unwrap_or_else(|e| panic!("Failed to write {}: {e}", md_path.display()));

		let txt_path = if page_key.is_empty() {
			output_path.join("index.txt")
		} else {
			let page_dir = output_path.join(page_key);
			page_dir.join("index.txt")
		};
		fs::write(&txt_path, &page_data.content).unwrap_or_else(|e| panic!("Failed to write {}: {e}", txt_path.display()));
	}

	info!("Rendered {} pages", rendered_site_read.pages_data.len());

	for (alias_path, target_path) in &rendered_site_read.aliases {
		let redirect_html = generate_redirect_html(&config.site.base_url, target_path);

		let redirect_file_path = if alias_path.ends_with('/') || alias_path.is_empty() {
			let alias_dir = if alias_path.is_empty() {
				output_path.to_path_buf()
			} else {
				output_path.join(alias_path.trim_end_matches('/'))
			};
			fs::create_dir_all(&alias_dir).unwrap();
			alias_dir.join("index.html")
		} else {
			let alias_dir = output_path.join(alias_path);
			fs::create_dir_all(&alias_dir).unwrap();
			alias_dir.join("index.html")
		};

		fs::write(&redirect_file_path, redirect_html)
			.unwrap_or_else(|e| panic!("Failed to write redirect file {}: {e}", redirect_file_path.display()));
	}

	if !rendered_site_read.aliases.is_empty() {
		info!("Generated {} redirect pages", rendered_site_read.aliases.len());
	}

	for (file_path, (content, _)) in static_files_read.iter() {
		let target_path = output_path.join(file_path);
		if let Some(parent) = target_path.parent() {
			fs::create_dir_all(parent).unwrap();
		}
		fs::write(&target_path, content).unwrap_or_else(|e| panic!("Failed to write static file {}: {e}", target_path.display()));
	}

	info!("Copied {} static files", static_files_read.len());
	info!("Static rendering complete!")
}

struct RequestContext {
	rendered_site: Arc<RwLock<RenderedSite>>,
	static_files: Arc<RwLock<StaticFiles>>,
	templates: Arc<RwLock<Tera>>,
}

use autometrics::autometrics;
#[autometrics]
async fn handle_request(
	req: Request<Incoming>,
	request_context: Arc<RequestContext>,
) -> Result<hyper::Response<http_body_util::Full<Bytes>>, hyper::Error> {
	let span = tracing::span!(
		tracing::Level::INFO,
		"handle_request",
		http.method = ?req.method(),
		url.path = ?req.uri().path(),
		url.full = ?req.uri().to_string(),
		server.address = ?req.uri().host().unwrap_or(""),
		server.port = ?req.uri().port_u16().unwrap_or(80),
		network.protocol.name = "http",
		network.protocol.version = ?req.version(),
		user_agent.original = ?req.headers().get(hyper::header::USER_AGENT).and_then(|v| v.to_str().ok()).unwrap_or("")
	);
	for (key, value) in req.headers() {
		let header_name = format!("http.request.header.{}", key.as_str().to_lowercase());
		span.record(&*header_name, tracing::field::display(value.to_str().unwrap_or("")));
	}
	let _enter = span.enter();

	match (req.method(), req.uri().path()) {
		(&Method::GET | &Method::HEAD | &Method::OPTIONS, "/sitemap.xml") => {
			let rendered_site = request_context.rendered_site.read().await;
			if let Some(resp) = check_if_modified_and_etag(rendered_site.last_modified, &req) {
				return Ok(resp);
			}
			let metadata = BodyMetadata {
				len: rendered_site.sitemap.len() as u64,
				content_type: "text/xml; charset=utf-8".parse().unwrap(),
				last_modified: rendered_site.last_modified,
				etag: None,
			};

			let response = Response::new(StatusCode::OK).with_source(BodySource::Preloaded {
				metadata: &metadata,
				content: &rendered_site.sitemap,
			});

			return Ok(response.into_response(req.method()));
		}
		(&Method::GET | &Method::HEAD | &Method::OPTIONS, "/rss.xml" | "/atom.xml") => {
			let rendered_site = request_context.rendered_site.read().await;
			if let Some(resp) = check_if_modified_and_etag(rendered_site.last_modified, &req) {
				return Ok(resp);
			}

			let content = match req.uri().path() {
				"/rss.xml" => &rendered_site.rss_feed,
				"/atom.xml" => &rendered_site.atom_feed,
				_ => unreachable!(),
			};

			let metadata = BodyMetadata {
				len: content.len() as u64,
				// non-specific type so browsers display as xml with /feed.xsl instead of download
				content_type: "application/xml; charset=utf-8".parse().unwrap(),
				last_modified: rendered_site.last_modified,
				etag: None,
			};

			let response = Response::new(StatusCode::OK).with_source(BodySource::Preloaded {
				metadata: &metadata,
				content,
			});

			return Ok(response.into_response(req.method()));
		}
		(&Method::GET | &Method::HEAD | &Method::OPTIONS, path) => {
			let trimmed_path = path.trim_start_matches('/');

			{
				let rendered_site = request_context.rendered_site.read().await;
				if let Some(target_path) = rendered_site.aliases.get(trimmed_path) {
					return Ok(hyper::Response::builder()
						.status(StatusCode::MOVED_PERMANENTLY)
						.header("Location", format!("/{target_path}"))
						.body(http_body_util::Full::new(Bytes::new()))
						.unwrap());
				}
			}

			let static_files = request_context.static_files.read().await;
			tracing::trace!("Generic GET handler for {path}", path = path);
			if path.starts_with("/static/") || static_files.contains_key(trimmed_path) {
				serve_static_file(trimmed_path, &request_context, &req).await
			} else {
				let normalized_path = normalize_path(path);
				serve_page(&normalized_path, &request_context, &req).await
			}
		}
		_ => Ok(Response::new(StatusCode::METHOD_NOT_ALLOWED).into_response(req.method())),
	}
}

#[instrument(skip(request_context, req))]
async fn serve_static_file(
	path: &str,
	request_context: &RequestContext,
	req: &Request<Incoming>,
) -> Result<hyper::Response<http_body_util::Full<Bytes>>, hyper::Error> {
	let static_files = request_context.static_files.read().await;
	let trimmed_path = path.trim_start_matches("/static/");
	debug!("Looking for static file: '{}' (trimmed: '{}')", path, trimmed_path);
	debug!("Available static files: {:?}", static_files.keys().collect::<Vec<_>>());
	if let Some((content, last_modified)) = static_files.get(trimmed_path) {
		if let Some(resp) = check_if_modified_and_etag(*last_modified, req) {
			return Ok(resp);
		}

		let content_type = mime_guess::from_path(trimmed_path)
			.first_or_octet_stream()
			.as_ref()
			.parse()
			.unwrap();

		let metadata = BodyMetadata {
			len: content.len() as u64,
			content_type,
			last_modified: *last_modified,
			etag: None, // Add ETag if needed
		};

		let mut response = Response::new(StatusCode::OK).with_source(BodySource::Preloaded {
			metadata: &metadata,
			content,
		});

		if let Some(range) = parse_range_header(req.headers(), metadata.len) {
			response = response.with_range(range);
		}

		Ok(response.into_response(req.method()))
	} else {
		Ok(Response::not_found().into_response(req.method()))
	}
}

fn create_base_response_builder() -> hyper::http::response::Builder {
	let mut builder = hyper::Response::builder();
	builder = add_security_headers(builder);
	builder
}

fn add_security_headers(mut builder: hyper::http::response::Builder) -> hyper::http::response::Builder {
	use hyper::header;

	builder = builder
		// Prevents MIME type sniffing, reducing risks of MIME confusion attacks
		.header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
		// Limits referrer information to origin for cross-origin requests, balancing functionality and privacy
		.header(header::REFERRER_POLICY, "strict-origin-when-cross-origin")
		// Enforces HTTPS for one year, including subdomains, protecting against downgrade attacks and cookie hijacking
		.header(header::STRICT_TRANSPORT_SECURITY, "max-age=31536000; includeSubDomains")
		// Allows any origin to make cross-origin requests, enabling wide embedding and integration
		.header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
		// Uses 'credentialless' to support SharedArrayBuffer without relaxing security
		// This enables use of WebAssembly threads while maintaining some cross-origin protections
		.header("Cross-Origin-Embedder-Policy", "credentialless")
		// Isolates browsing context to same origin, enhancing security against some cross-origin attacks
		.header("Cross-Origin-Opener-Policy", "same-origin")
		// Explicitly allows cross-origin resource sharing, enabling embedding and integration
		.header("Cross-Origin-Resource-Policy", "cross-origin");

	// Add Content-Security-Policy header only for HTML content
	// if mime_type.starts_with("text/html") {
	// builder = builder.header(header::CONTENT_SECURITY_POLICY, "default-src 'self'; script-src-elem 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: https:; font-src 'self'; connect-src 'self'; frame-src 'self'; frame-ancestors *; base-uri 'self'; upgrade-insecure-requests");
	// }

	builder
}

fn check_if_modified_and_etag(last_modified: SystemTime, req: &Request<Incoming>) -> Option<hyper::Response<http_body_util::Full<Bytes>>> {
	if let Some(if_modified_since) = req.headers().get(IF_MODIFIED_SINCE)
		&& let Ok(if_modified_since) = httpdate::parse_http_date(if_modified_since.to_str().unwrap())
	{
		let page_last_modified =
			SystemTime::UNIX_EPOCH + Duration::from_secs(last_modified.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs());
		if page_last_modified <= if_modified_since {
			return Some(
				hyper::Response::builder()
					.status(StatusCode::NOT_MODIFIED)
					.body(http_body_util::Full::new(Bytes::new()))
					.unwrap(),
			);
		}
	}
	// TODO: Handle ETag checking here
	None
}

#[instrument(skip(request_context, req))]
async fn serve_page(
	page: &str,
	request_context: &RequestContext,
	req: &Request<Incoming>,
) -> Result<hyper::Response<http_body_util::Full<Bytes>>, hyper::Error> {
	let rendered_site = request_context.rendered_site.read().await;
	let lookup_key_if_plain = page
		.trim_end_matches("index.md")
		.trim_end_matches(".md")
		.trim_end_matches("index.html")
		.trim_end_matches(".html")
		.trim_end_matches("index.txt")
		.trim_end_matches(".txt");
	if lookup_key_if_plain != page
		&& let Some(page_data) = rendered_site.pages_data.get(lookup_key_if_plain)
	{
		if let Some(response) = check_if_modified_and_etag(page_data.last_modified, req) {
			return Ok(response);
		}
		debug!("Serving markdown file: {}", lookup_key_if_plain);

		let metadata = BodyMetadata {
			len: page_data.content.len() as u64,
			content_type: "text/markdown; charset=utf-8".parse().unwrap(),
			last_modified: page_data.last_modified,
			etag: None,
		};

		let mut response = Response::new(StatusCode::OK).with_source(BodySource::Preloaded {
			metadata: &metadata,
			content: &page_data.content,
		});

		if let Some(range) = parse_range_header(req.headers(), metadata.len) {
			response = response.with_range(range);
		}

		return Ok(response.into_response(req.method()));
	}

	if let Some(page_data) = rendered_site.pages_data.get(page) {
		if let Some(response) = check_if_modified_and_etag(page_data.last_modified, req) {
			return Ok(response);
		}

		let metadata = BodyMetadata {
			len: page_data.html_content.len() as u64,
			content_type: "text/html; charset=utf-8".parse().unwrap(),
			last_modified: page_data.last_modified,
			etag: None,
		};

		let mut response = Response::new(StatusCode::OK).with_source(BodySource::Preloaded {
			metadata: &metadata,
			content: &page_data.html_content,
		});

		if let Some(range) = parse_range_header(req.headers(), metadata.len) {
			response = response.with_range(range);
		}

		Ok(response.into_response(req.method()))
	} else {
		Ok(Response::not_found().into_response(req.method()))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

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
}

#[derive(Clone)]
struct BodyMetadata {
	len: u64,
	content_type: HeaderValue,
	last_modified: SystemTime,
	etag: Option<HeaderValue>,
}

/// Response body source - supports multiple content delivery strategies
/// Currently only Preloaded is used, but File and Dynamic are planned for:
/// - File: Direct file serving for large assets without memory loading
/// - Dynamic: Runtime content generation (e.g., API endpoints, live data)
#[allow(dead_code)]
enum BodySource<'a> {
	/// Content pre-loaded into memory (current approach for all pages/assets)
	Preloaded { metadata: &'a BodyMetadata, content: &'a Bytes },
	/// Direct file serving without memory loading (planned for large files)
	File { path: &'a Path, metadata: &'a BodyMetadata },
	/// Runtime content generation (planned for dynamic endpoints)
	Dynamic {
		metadata: &'a BodyMetadata,
		generator: Box<dyn Fn() -> Bytes + 'a>,
	},
}

/// HTTP response builder with extensible header and content support
/// Currently only uses status, source, and range fields
struct Response<'a> {
	status: StatusCode,
	#[allow(dead_code)] // Planned for custom header support
	headers: Vec<(HeaderName, HeaderValue)>,
	source: Option<BodySource<'a>>,
	range: Option<Range<u64>>,
}

impl<'a> Response<'a> {
	fn new(status: StatusCode) -> Self {
		Self {
			status,
			headers: vec![],
			source: None,
			range: None,
		}
	}

	fn not_found() -> Self {
		Self::new(StatusCode::NOT_FOUND)
	}

	fn with_source(mut self, source: BodySource<'a>) -> Self {
		self.source = Some(source);
		self
	}

	fn with_range(mut self, range: Range<u64>) -> Self {
		self.range = Some(range);
		self
	}

	fn into_response(self, method: &Method) -> hyper::Response<http_body_util::Full<Bytes>> {
		use hyper::header::*;

		if method == Method::OPTIONS {
			return create_base_response_builder()
				.status(StatusCode::NO_CONTENT)
				.header(ALLOW, "GET, HEAD, OPTIONS")
				.body(Full::new(Bytes::new()))
				.unwrap();
		}

		let mut builder = create_base_response_builder().status(self.status);
		builder = builder.header(ACCEPT_RANGES, "bytes");

		if let Some(source) = self.source {
			let metadata = match &source {
				BodySource::Preloaded { metadata, .. } => metadata,
				BodySource::File { metadata, .. } => metadata,
				BodySource::Dynamic { metadata, .. } => metadata,
			};

			builder = builder
				.header(CONTENT_TYPE, &metadata.content_type)
				.header(LAST_MODIFIED, httpdate::fmt_http_date(metadata.last_modified));

			if let Some(etag) = &metadata.etag {
				builder = builder.header(hyper::header::ETAG, etag);
			}

			let (start, end) = if let Some(range) = self.range {
				if range.end >= metadata.len {
					return builder
						.status(StatusCode::RANGE_NOT_SATISFIABLE)
						.header(CONTENT_RANGE, format!("bytes */{}", metadata.len))
						.body(Full::new(Bytes::new()))
						.unwrap();
				}
				builder = builder
					.status(StatusCode::PARTIAL_CONTENT)
					.header(CONTENT_RANGE, format!("bytes {}-{}/{}", range.start, range.end - 1, metadata.len));
				(range.start, range.end)
			} else {
				(0, metadata.len)
			};

			builder = builder.header(CONTENT_LENGTH, end - start);

			let body = if method == Method::GET {
				match source {
					BodySource::Preloaded { content, .. } => content.slice(start as usize..end as usize),
					BodySource::File { path, .. } => {
						let mut file = fs::File::open(path).unwrap();
						let mut buffer = vec![0; (end - start) as usize];
						file.seek(std::io::SeekFrom::Start(start)).unwrap();
						file.read_exact(&mut buffer).unwrap();
						Bytes::from(buffer)
					}
					BodySource::Dynamic { generator, .. } => {
						let content = generator();
						content.slice(start as usize..end as usize)
					}
				}
			} else {
				Bytes::new()
			};

			builder.body(Full::new(body)).unwrap()
		} else {
			builder.body(Full::new(Bytes::new())).unwrap()
		}
	}
}

fn parse_range_header(headers: &hyper::HeaderMap, total_length: u64) -> Option<std::ops::Range<u64>> {
	headers.get(hyper::header::RANGE).and_then(|v| v.to_str().ok()).and_then(|v| {
		let v = v.strip_prefix("bytes=")?;
		let mut parts = v.split('-');
		let start = parts.next()?.parse::<u64>().ok()?;
		let end = parts.next().map(|v| v.parse::<u64>().ok()).unwrap_or(Some(total_length - 1))?;
		if start <= end && end < total_length {
			Some(start..end + 1)
		} else {
			None
		}
	})
}
