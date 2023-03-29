mod filters;
mod search;

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::path::{self, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use axum::extract::{Path, Query, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, ETAG, IF_NONE_MATCH};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::{routing, Form, Router};
use axum_client_ip::{SecureClientIp, SecureClientIpSource};
use chrono::{prelude::*, ParseError};
use comrak::{ComrakExtensionOptions, ComrakOptions};
use log::{error, info};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use once_cell::sync::Lazy;
use serde::Serialize;
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use tokio::fs;
use tokio::sync::{mpsc, RwLock};
use tokio::time::sleep;

use filters::*;
use search::*;

struct Post {
	path: String,
	date: NaiveDate,
	title: String,
	md: String,
	html: String,
	public: bool,
	num_visits: u32,
}

static POSTS: Lazy<Arc<RwLock<HashMap<String, Post>>>> =
	Lazy::new(|| Arc::new(RwLock::new(HashMap::new())));

#[tokio::main]
async fn main() {
	fern::Dispatch::new()
		.format(|out, message, record| {
			out.finish(format_args!(
				"[{} {}] {}",
				record.level(),
				record.target(),
				message
			))
		})
		.level(log::LevelFilter::Info)
		.level_for("sqlx", log::LevelFilter::Warn)
		.chain(std::io::stdout())
		.chain(fern::log_file("server.log").unwrap())
		.apply()
		.unwrap();

	let pool = SqlitePoolOptions::new()
		.connect("sqlite://database.db")
		.await
		.unwrap();

	let app = Router::new()
		.route("/", routing::get(root).post(root))
		.route("/style.css", routing::get(style))
		.route("/blog/", routing::get(root).post(root))
		.route("/blog/:post_name", routing::get(load_post))
		.layer(SecureClientIpSource::ConnectInfo.into_extension())
		.with_state(pool.clone());

	tokio::task::spawn(watch_md(pool.clone()));
	tokio::task::spawn(update_post_stats(pool.clone()));

	info!("Running server at {}!", Utc::now());

	axum::Server::bind(&([0; 8], 80).try_into().unwrap())
		.serve(app.into_make_service_with_connect_info::<SocketAddr>())
		.await
		.unwrap();
}

async fn update_post_stats(db: SqlitePool) {
	loop {
		{
			let posts = &mut POSTS.write().await;

			for (path, post) in posts.iter_mut() {
				let num_visits: (u32,) =
					sqlx::query_as("select num_visits from visits where path = ?")
						.bind(path)
						.fetch_one(&db)
						.await
						.unwrap();
				let num_visits = num_visits.0;

				post.num_visits = num_visits;
			}
		}

		#[cfg(debug_assertions)]
		const NUM_SECONDS_BETWEEN_UDPATES: u64 = 5;
		#[cfg(not(debug_assertions))]
		const NUM_SECONDS_BETWEEN_UDPATES: u64 = 60;

		sleep(Duration::from_secs(NUM_SECONDS_BETWEEN_UDPATES)).await;
	}
}

async fn watch_md(db: SqlitePool) {
	let path: PathBuf = "./md/".into();

	// On initialization, generate all posts
	let mut dir = fs::read_dir(&path).await.unwrap();

	while let Ok(Some(dir_entry)) = dir.next_entry().await {
		if let Err(err) = gen_static(dir_entry.path(), &db).await {
			error!("Error generating blog post {:?}: {err:?}", dir_entry.path());
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

		let db_ref = &db;

		let gen = |paths: Vec<PathBuf>| async move {
			for path in paths.into_iter() {
				if let Err(err) = gen_static(&path, db_ref).await {
					error!("Error generating blog post {:?}: {err:?}", path);
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

async fn root(query: Option<Form<SearchQuery>>) -> Html<String> {
	let query = query.unwrap_or_else(|| Form(SearchQuery::empty()));
	let posts = search_posts(query.0).await;

	let mut s = String::new();

	s.push_str(
		"
		<form action='/' method='post'>
			<label>Search</label>
			<input type='text' name='title'><br>
		</form>	
		",
	);

	{
		let all_posts = POSTS.read().await;

		for post_path in posts {
			let post = all_posts.get(&post_path).unwrap();
			s.push_str(&format!(
				"Post: <a href = '/blog/{}'>{}</a><br>",
				post.path, post.title,
			));
		}
	}

	apply_html_filters(&mut s, &[styling, add_homepage]);

	Html(s)
}

#[derive(Serialize)]
struct VisitLog {
	ip: IpAddr,
	time: DateTime<Utc>,
	path: String,
}

async fn load_post(
	Path(post_path): Path<String>, Query(params): Query<HashMap<String, String>>,
	State(db): State<SqlitePool>, secure_ip: SecureClientIp,
) -> Response {
	let invalid_name = post_path.chars().any(|char| {
		let ascii = char as u32;
		// Check that the char matches [A-Za-z0-9-_]
		!((ascii >= b'a' as u32 && ascii <= b'z' as u32)
			|| ascii == b'-' as u32
			|| ascii == b'_' as u32
			|| (ascii >= b'A' as u32 && ascii <= b'Z' as u32)
			|| (ascii >= b'0' as u32 && ascii <= b'9' as u32))
	});

	if invalid_name {
		return not_found().into_response();
	}

	let ip = secure_ip.0;

	let (resp, is_html) = {
		let posts = POSTS.read().await;

		let post = match posts.get(&post_path) {
			Some(post) => post,
			None => return not_found().into_response(),
		};

		let is_html = params.get("md").is_none();
		let resp = match is_html {
			true => post.html.clone(),
			false => post.md.clone(),
		};

		(resp, is_html)
	};

	let visit_log = VisitLog {
		ip,
		time: Utc::now(),
		path: post_path.clone(),
	};

	info!(target: "visits", "{}", simd_json::to_string(&visit_log).unwrap());

	let mut transaction = db.begin().await.unwrap();
	sqlx::query("insert or ignore into visits (path, num_visits) values (?, 0)")
		.bind(&post_path)
		.execute(&mut transaction)
		.await
		.unwrap();

	sqlx::query("update visits set num_visits = num_visits + 1 where path = ?")
		.bind(&post_path)
		.execute(&mut transaction)
		.await
		.unwrap();

	transaction.commit().await.unwrap();

	match is_html {
		true => Html(resp).into_response(),
		false => resp.into_response(),
	}
}

fn not_found() -> Html<String> {
	Html("404".to_string())
}

async fn gen_static<P: AsRef<path::Path>>(file: P, db: &SqlitePool) -> Result<()> {
	let html_filters: &[HtmlFilterFn] = &[styling, add_homepage];
	let md_filters: &[MdFilterFn] = &[add_metadata];

	let path = file.as_ref();

	let md = fs::read_to_string(path).await?;
	let mut md_lines = md.lines();

	let path = match md_lines.next() {
		Some(line) => line.to_string(),
		None => return Err(anyhow!("Invalid markdown file: No path line")),
	};
	let title = match md_lines.next() {
		Some(line) => line.to_string(),
		None => return Err(anyhow!("Invalid markdown file: No title line")),
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

	let mut md: String = md_lines.fold(String::new(), |s, l| s + l + "\n");
	apply_md_filters(&mut md, &path, &title, &date.to_string(), md_filters);

	let mut html = markdown_to_html(&md);
	apply_html_filters(&mut html, html_filters);

	sqlx::query("insert or ignore into visits (path, num_visits) values (?, 0)")
		.bind(&path)
		.execute(db)
		.await
		.unwrap();

	let post = Post {
		date,
		path: path.clone(),
		title,
		public,
		md,
		html,
		num_visits: 0,
	};

	{
		POSTS.write().await.insert(path, post);
	}

	Ok(())
}

fn markdown_to_html(md: &str) -> String {
	static MARKDOWN_OPTIONS: Lazy<ComrakOptions> = Lazy::new(|| ComrakOptions {
		extension: ComrakExtensionOptions {
			strikethrough: true,
			..Default::default()
		},
		..Default::default()
	});

	let html = comrak::markdown_to_html(md, &MARKDOWN_OPTIONS);
	String::from_utf8(minify_html::minify(
		html.as_bytes(),
		&minify_html::Cfg::new(),
	))
	.unwrap()
}

async fn style(headers: HeaderMap) -> (StatusCode, HeaderMap, String) {
	let bytes = fs::read("./static/style.css").await.unwrap();
	let hash = blake3::hash(&bytes);
	let hash = format!("\"{}\"", hash);

	let mut header_map = HeaderMap::with_capacity(2);

	if let Some(hash2) = headers.get(IF_NONE_MATCH) {
		if hash2.to_str().unwrap() == hash {
			return (StatusCode::NOT_MODIFIED, header_map, String::new());
		}
	}

	header_map.insert(CONTENT_TYPE, HeaderValue::from_str("text/css").unwrap());
	header_map.insert(ETAG, HeaderValue::from_str(&hash).unwrap());

	println!("{header_map:?}");
	(
		StatusCode::OK,
		header_map,
		String::from_utf8(bytes).unwrap(),
	)
}
