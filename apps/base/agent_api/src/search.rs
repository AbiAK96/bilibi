use std::collections::BTreeSet;
pub fn tokens(text: &str) -> BTreeSet<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| {
            !word.is_empty()
                && ![
                    "a", "an", "the", "of", "for", "to", "in", "on", "and", "or", "by", "with",
                    "my", "me", "i", "please",
                ]
                .contains(word)
        })
        .map(stem)
        .collect()
}
fn stem(word: &str) -> String {
    let mut word = word.to_owned();
    let len = word.chars().count();
    if len > 4 && word.ends_with("ies") {
        word.truncate(word.len() - 3);
        word.push('y');
    } else if len > 4
        && ["sses", "uses", "xes", "ches", "shes"]
            .iter()
            .any(|s| word.ends_with(s))
    {
        word.truncate(word.len() - 2);
    } else if len > 3 && word.ends_with('s') && !word.ends_with("ss") && !word.ends_with("us") {
        word.pop();
    }
    if word.chars().count() > 5 && word.ends_with("ing") {
        word.truncate(word.len() - 3);
    }
    if word.chars().count() > 3 && word.ends_with('e') {
        word.pop();
    }
    word
}
/// Integer half-points avoid float ordering; each field scores a word once.
pub fn score(
    query: &BTreeSet<String>,
    key: &str,
    title: &str,
    keywords: &[&str],
    summary: &str,
    body: Option<&str>,
) -> u64 {
    let keywords = keywords.join(" ");
    [
        (key, 6),
        (title, 6),
        (&keywords, 4),
        (summary, 2),
        (body.unwrap_or(""), 1),
    ]
    .into_iter()
    .map(|(text, weight)| (tokens(text).intersection(query).count() as u64) * weight)
    .sum()
}
