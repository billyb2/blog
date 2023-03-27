use std::cmp::Ordering;

use serde::Deserialize;

use crate::{Post, POSTS};

#[derive(Deserialize)]
pub enum SortBy {
	Newest,
}

#[derive(Deserialize)]
pub struct SearchQuery {
	title: Option<String>,
	sort_by: Option<SortBy>,
}

impl SearchQuery {
	pub const fn empty() -> Self {
		Self {
			title: None,
			sort_by: None,
		}
	}
}

pub async fn search_posts(mut query: SearchQuery) -> Vec<String> {
	query.title = query.title.map(|title| title.to_lowercase());

	let posts = POSTS.read().await;

	let mut posts: Vec<(&String, &Post)> = posts
		.iter()
		.filter(|(_path, post)| post.public)
		.filter(|(_path, post)| match &query.title {
			Some(title) => post.title.to_lowercase().contains(title),
			None => true,
		})
		.collect();

	let sort_by = query.sort_by.unwrap_or(SortBy::Newest);
	let sort_fn = match sort_by {
		SortBy::Newest => |a: &(&String, &Post), b: &(&String, &Post)| {
			b.1.date.partial_cmp(&a.1.date).unwrap_or(Ordering::Equal)
		},
	};

	posts.sort_unstable_by(sort_fn);
	posts
		.into_iter()
		.map(|(path, _post)| path.clone())
		.collect()
}
