use crate::WriteTo;

pub fn add_metadata(_md: &str, _path: &str, title: &str, date: &str) -> (String, WriteTo) {
	(
		format!("## **{title}**\n### {date}\n### bootlegbilly"),
		WriteTo::Beginning,
	)
}

pub fn apply_md_filters(
	md: &mut String, path: &str, title: &str, date: &str,
	filters: &[fn(&str, &str, &str, &str) -> (String, WriteTo)],
) {
	let (beginning, end): (Vec<_>, Vec<_>) = filters
		.iter()
		.map(|filter| filter(md, path, title, date))
		.partition(|(_, write_to)| *write_to == WriteTo::Beginning);

	let mut beginning: String = beginning.into_iter().map(|(s, _)| s.to_string()).collect();
	let end: String = end.into_iter().map(|(s, _)| s.to_string()).collect();

	beginning.push_str(md);
	beginning.push_str(&end);

	*md = beginning;
}
