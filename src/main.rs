use std::collections::HashMap;
use std::path::{self, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, Result};
use axum::extract::Path;
use axum::response::Html;
use axum::{routing, Router};
use chrono::{NaiveDate, ParseError};
use comrak::ComrakOptions;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use once_cell::sync::Lazy;
use tokio::fs;
use tokio::sync::{mpsc, RwLock};

struct Post {
	path: String,
	author: String,
	date: NaiveDate,
	title: String,
	md: String,
	html: String,
	public: bool,
}

static POSTS: Lazy<Arc<RwLock<HashMap<String, Post>>>> =
	Lazy::new(|| Arc::new(RwLock::new(HashMap::new())));

#[tokio::main]
async fn main() {
	let app = Router::new()
		.route("/", routing::get(root))
		.route("/blog/", routing::get(root))
		.route("/blog/:post_name", routing::get(load_post));

	tokio::task::spawn(watch_md());

	println!("Running server!");

	axum::Server::bind(&"0.0.0.0:80".parse().unwrap())
		.serve(app.into_make_service())
		.await
		.unwrap();
}

async fn watch_md() {
	let path: PathBuf = "./md/".into();

	// On initialization, generate all posts
	let mut dir = fs::read_dir(&path).await.unwrap();

	while let Ok(Some(dir_entry)) = dir.next_entry().await {
		if let Err(err) = gen_static(dir_entry.path()).await {
			eprintln!("Error generating blog post {:?}: {err:?}", dir_entry.path());
		}
	}

	let (tx, mut rx) = mpsc::channel(1);

	let mut watcher = RecommendedWatcher::new(
		move |res| {
			futures::executor::block_on(async { tx.send(res).await.unwrap() });
		},
		notify::Config::default(),
	)
	.unwrap();

	watcher.watch(&path, RecursiveMode::Recursive).unwrap();

	while let Some(res) = rx.recv().await {
		let res = res.unwrap();

		let gen = |paths: Vec<PathBuf>| async move {
			for path in paths.into_iter() {
				if let Err(err) = gen_static(&path).await {
					eprintln!("Error generating blog post {:?}: {err:?}", path);
				}
			}
		};

		match res.kind {
			notify::EventKind::Create(_) => gen(res.paths).await,
			notify::EventKind::Modify(_) => gen(res.paths).await,
			// notify::EventKind::Remove(_) => remove_posts(res.paths).await,
			_ => (),
		}
	}
}

async fn root() -> Html<String> {
	let posts = search(SearchQuery::title("hello")).await;

	let mut s = String::new();

	let all_posts = POSTS.read().await;

	for post_path in posts {
		let post = all_posts.get(&post_path).unwrap();
		s.push_str(&format!(
			"Post: <a href = '/blog/{}'>{}</a><br>",
			post.path, post.title,
		));
	}

	Html(s)
}

async fn load_post(Path(post_name): Path<String>) -> Html<String> {
	if !post_name.is_ascii() {
		return not_found();
	}

	let invalid_name = post_name.chars().any(|char| {
		let ascii = char as u32;
		// Check that the char matches [A-Za-z0-9-_]
		!((ascii >= b'a' as u32 && ascii <= b'z' as u32)
			|| ascii == b'-' as u32
			|| ascii == b'_' as u32
			|| (ascii >= b'A' as u32 && ascii <= b'Z' as u32)
			|| (ascii >= b'0' as u32 && ascii <= b'9' as u32))
	});

	if invalid_name {
		return not_found();
	}

	let posts = POSTS.read().await;

	let post = match posts.get(&post_name) {
		Some(post) => post,
		None => return not_found(),
	};

	Html(post.html.clone())
}

fn not_found() -> Html<String> {
	Html("404".to_string())
}

#[derive(PartialEq)]
enum WriteTo {
	Beginning,
	End,
}

fn styling(_html: &str) -> (impl ToString, WriteTo) {
	// Append the css to the beginning of the string
	(include_str!("../static/style.css"), WriteTo::Beginning)
}

pub struct SearchQuery {
	title: Option<String>,
}

impl SearchQuery {
	fn title(title: impl ToString) -> Self {
		Self {
			title: Some(title.to_string()),
		}
	}
}

async fn search(mut query: SearchQuery) -> Vec<String> {
	query.title = query.title.map(|title| title.to_lowercase());

	let posts = POSTS.read().await;

	posts
		.iter()
		.filter(|(_path, post)| {
			if let Some(title) = &query.title {
				post.title.to_lowercase().contains(title)
			} else {
				false
			}
		})
		.map(|(_path, post)| post.path.clone())
		.collect()
}

async fn gen_static<P: AsRef<path::Path>>(file: P) -> Result<()> {
	let html_filters = [styling];

	let path = file.as_ref();

	let md = fs::read_to_string(&path).await?;
	let mut md_lines = md.lines();

	let path = match md_lines.next() {
		Some(line) => line.to_string(),
		None => return Err(anyhow!("Invalid markdown file: No path line")),
	};
	let title = match md_lines.next() {
		Some(line) => line.to_string(),
		None => return Err(anyhow!("Invalid markdown file: No title line")),
	};
	let author = match md_lines.next() {
		Some(line) => line.to_string(),
		None => return Err(anyhow!("Invalid markdown file: No author line")),
	};
	let date: NaiveDate = match md_lines.next() {
		Some(line) => NaiveDate::parse_from_str(line, "%m/%d/%Y").map_err(|err: ParseError| {
			anyhow!("error creating date from {line}: {}", err.to_string())
		})?,
		None => return Err(anyhow!("Invalid markdown file: No date line")),
	};

	let public: bool = match md_lines.next() {
		Some(line) => line == "public",
		None => return Err(anyhow!("Invalid markdown file: No date line")),
	};

	let md: String = md_lines.fold(String::new(), |s, l| s + l + "\n");
	let mut html = markdown_to_html(&md);

	let (beginning, end): (Vec<_>, Vec<_>) = html_filters
		.iter()
		.map(|filter| filter(&html))
		.partition(|(_, write_to)| *write_to == WriteTo::Beginning);

	let beginning: String = beginning.into_iter().map(|(s, _)| s.to_string()).collect();
	let end: String = end.into_iter().map(|(s, _)| s.to_string()).collect();

	html = format!("{}{}{}", beginning, html, end);

	let post = Post {
		author,
		date,
		path: path.clone(),
		title,
		public,
		md,
		html,
	};
	POSTS.write().await.insert(path, post);

	Ok(())
}

fn markdown_to_html(md: &str) -> String {
	let html = comrak::markdown_to_html(&md, &ComrakOptions::default());
	String::from_utf8(minify_html::minify(
		html.as_bytes(),
		&minify_html::Cfg::new(),
	))
	.unwrap()
}
