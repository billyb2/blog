use crate::WriteTo;

pub fn styling(_html: &str) -> (String, WriteTo) {
	// Append the css to the beginning of the string
	(
		"<meta name='viewport' content='width=device-width, initial-scale=1'>".to_owned()
			+ "<link rel='stylesheet' href='/style.css'>",
		WriteTo::Beginning,
	)
}

pub fn add_homepage(_html: &str) -> (String, WriteTo) {
	(
		"<br><a href='/'>Back to home page</a>".to_owned(),
		WriteTo::End,
	)
}

pub type HtmlFilterFn = fn(&str) -> (String, WriteTo);

pub fn apply_html_filters(html: &mut String, filters: &[HtmlFilterFn]) {
	let (beginning, end): (Vec<_>, Vec<_>) = filters
		.iter()
		.map(|filter| filter(html))
		.partition(|(_, write_to)| *write_to == WriteTo::Beginning);

	let mut beginning: String = beginning.into_iter().map(|(s, _)| s).collect();
	let end: String = end.into_iter().map(|(s, _)| s).collect();

	beginning.push_str(html);
	beginning.push_str(&end);

	*html = String::from_utf8(minify_html::minify(
		beginning.as_bytes(),
		&minify_html::Cfg::new(),
	))
	.unwrap()
}
