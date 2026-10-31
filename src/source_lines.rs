use regex::Regex;
use std::sync::LazyLock;

pub fn normalize_footnote_ids(mut html: String) -> String {
    static REFERENCES: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r##"<a href="#fn-([^"]+)"[^>]*data-footnote-ref>(\d+)</a>"##).unwrap()
    });
    let mappings: Vec<_> = REFERENCES
        .captures_iter(&html)
        .map(|c| (c[1].to_owned(), c[2].to_owned()))
        .collect();
    for (name, number) in mappings {
        html = html
            .replace(&format!("id=\"fn-{name}\""), &format!("id=\"F{number}\""))
            .replace(
                &format!("href=\"#fn-{name}\""),
                &format!("href=\"#F{number}\""),
            );
    }
    html
}

static SOURCEPOS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<(p|li|table|blockquote|pre)\b([^>]*?)\sdata-sourcepos="(\d+):[^"]*"([^>]*)>"#)
        .expect("valid source position regex")
});
static UNUSED_SOURCEPOS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\sdata-sourcepos="[^"]*""#).expect("valid unused source position regex")
});
pub fn mark_source_lines(html: String) -> String {
    let html = SOURCEPOS_RE
        .replace_all(&html, |captures: &regex::Captures<'_>| {
            let line = captures[3].parse::<usize>().unwrap_or(1).max(1);
            format!(
                "<{}{} data-line=\"{}\"{}>",
                &captures[1], &captures[2], line, &captures[4]
            )
        })
        .into_owned();
    UNUSED_SOURCEPOS.replace_all(&html, "").into_owned()
}
