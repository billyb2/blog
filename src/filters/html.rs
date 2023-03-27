use crate::WriteTo;

pub fn styling(_html: &str) -> (String, WriteTo) {
	// Append the css to the beginning of the string
	(
		"<meta name='viewport' content='width=device-width, initial-scale=1'>".to_owned()
			+ include_str!("../../static/style.css"),
		WriteTo::Beginning,
	)
}

pub fn add_homepage(_html: &str) -> (String, WriteTo) {
	(
		"<br><a href='/'>Back to home page</a>".to_owned(),
		WriteTo::End,
	)
}

pub fn apply_html_filters(html: &mut String, filters: &[fn(&str) -> (String, WriteTo)]) {
	let (beginning, end): (Vec<_>, Vec<_>) = filters
		.iter()
		.map(|filter| filter(&html))
		.partition(|(_, write_to)| *write_to == WriteTo::Beginning);

	let mut beginning: String = beginning.into_iter().map(|(s, _)| s.to_string()).collect();
	let end: String = end.into_iter().map(|(s, _)| s.to_string()).collect();

	beginning.push_str(html);
	beginning.push_str(&end);

	*html = String::from_utf8(minify_html::minify(
		beginning.as_bytes(),
		&minify_html::Cfg::new(),
	))
	.unwrap()
}
