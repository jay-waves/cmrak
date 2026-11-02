//! Standalone Markdown pipeline: source ranges → compiled events → HTML.
use crate::{
    assets::escape,
    document::{Attachments, Diagnostic, GraphIds},
    engine::Engine,
    fragments,
    headings::{IdState, slugify},
};
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd, html};
use std::{collections::HashMap, ops::Range};
type Ev = Event<'static>;
type Located = (Ev, Range<usize>);
fn raw(value: impl Into<String>) -> Ev {
    Event::Html(value.into().into())
}
fn options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_MATH
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
}

pub(crate) fn render(
    source: &str,
    engine: &Engine,
    attachments: &Attachments,
    warnings: &mut Vec<Diagnostic>,
) -> Result<String, Vec<Diagnostic>> {
    let mut starts = vec![0];
    starts.extend(source.match_indices('\n').map(|(at, _)| at + 1));
    let events = Parser::new_ext(source, options())
        .into_offset_iter()
        .map(|(event, range)| (event.into_static(), range));
    let mut body = Vec::new();
    let mut definitions = HashMap::new();
    let mut iter = events.into_iter();
    while let Some((e, r)) = iter.next() {
        if let Event::Start(Tag::FootnoteDefinition(name)) = e {
            let mut definition = Vec::new();
            for entry in iter.by_ref() {
                if matches!(entry.0, Event::End(TagEnd::FootnoteDefinition)) {
                    break;
                }
                definition.push(entry);
            }
            definitions.insert(name.to_string(), definition);
        } else {
            body.push((e, r));
        }
    }
    let body = crate::inline_notes::expand(&body, definitions);
    let mut renderer = Renderer {
        engine,
        attachments,
        warnings,
        source,
        starts,
        ids: IdState::default(),
        graph_ids: GraphIds::default(),
        errors: Vec::new(),
    };
    let output = renderer.events(&body, true);
    if !renderer.errors.is_empty() {
        return Err(renderer.errors);
    }
    let mut html = String::new();
    html::push_html(&mut html, output.into_iter());
    Ok(html)
}

struct Renderer<'a> {
    engine: &'a Engine,
    attachments: &'a Attachments,
    warnings: &'a mut Vec<Diagnostic>,
    source: &'a str,
    starts: Vec<usize>,
    ids: IdState,
    graph_ids: GraphIds,
    errors: Vec<Diagnostic>,
}
impl Renderer<'_> {
    fn diag(&self, offset: usize, message: String) -> Diagnostic {
        let line = self.starts.partition_point(|&s| s <= offset);
        Diagnostic {
            line,
            column: offset - self.starts[line - 1] + 1,
            message,
        }
    }
    fn line(&self, offset: usize) -> usize {
        self.starts.partition_point(|&s| s <= offset)
    }
    fn compiled(
        &mut self,
        offset: usize,
        result: Result<Option<String>, String>,
        block: bool,
        range: &Range<usize>,
    ) -> Ev {
        match result {
            Ok(Some(html)) => raw(if block {
                html.replacen('>', &format!(" data-line=\"{}\">", self.line(offset)), 1)
            } else {
                html
            }),
            Ok(None) => raw(""),
            Err(message) => {
                let (offset, message) = self.fragment_error(offset, range, message);
                let error = self.diag(offset, message);
                self.errors.push(error);
                raw("")
            }
        }
    }
    fn fragment_error(
        &self,
        fallback: usize,
        range: &Range<usize>,
        message: String,
    ) -> (usize, String) {
        let Some((message, metadata)) = message.rsplit_once('\u{1e}') else {
            return (fallback, message);
        };
        let Ok(data) = serde_json::from_str::<serde_json::Value>(metadata) else {
            return (fallback, message.to_owned());
        };
        let (Some(fragment), Some(line), Some(column)) = (
            data["source"].as_str(),
            data["line"].as_u64(),
            data["column"].as_u64(),
        ) else {
            return (fallback, message.to_owned());
        };
        let position = || {
            let part = fragment
                .split('\n')
                .nth(usize::try_from(line).ok()?.checked_sub(1)?)?;
            let line_start = part.as_ptr() as usize - fragment.as_ptr() as usize;
            let units = usize::try_from(column).ok()?.checked_sub(1)?;
            let mut consumed = 0;
            let mut bytes = 0;
            for ch in part.chars() {
                if consumed >= units {
                    break;
                }
                consumed += ch.len_utf16();
                bytes += ch.len_utf8();
            }
            if consumed != units {
                return None;
            }
            let block = self.source.get(range.clone())?;
            if let Some(start) = block.find(fragment)
                && !block[start + fragment.len()..].contains(fragment)
            {
                return Some(range.start + start + line_start + bytes);
            }
            // Markdown strips container markers and indentation from fenced code.
            let mut lines = block.split_inclusive('\n');
            let opening = lines.next()?;
            if !opening.contains("```mermaid") && !opening.contains("~~~mermaid") {
                return None;
            }
            let mut original_offset = opening.len();
            for _ in 1..line {
                original_offset += lines.next()?.len();
            }
            let original = lines.next()?.trim_end_matches(['\r', '\n']);
            let target = part.trim_end_matches('\r');
            let prefix = original.strip_suffix(target)?;
            if !prefix.chars().all(|c| matches!(c, ' ' | '\t' | '>')) {
                return None;
            }
            Some(range.start + original_offset + prefix.len() + bytes)
        };
        match position() {
            Some(at) => (at, message.to_owned()),
            None => (
                fallback,
                match message.split_once('\n') {
                    Some((first, details)) => {
                        format!("{first} (fragment line {line}, column {column})\n{details}")
                    }
                    None => format!("{message} (fragment line {line}, column {column})"),
                },
            ),
        }
    }
    fn events(&mut self, events: &[Located], sections: bool) -> Vec<Ev> {
        let mut out = Vec::new();
        let mut cursor = 0;
        let mut depth = 0;
        let mut levels: Vec<u8> = Vec::new();
        let mut links = Vec::new();
        let mut image_depth = 0;
        let mut raw_anchor = false;
        let mut note_depth = 0usize;
        let mut table: Option<(Vec<pulldown_cmark::Alignment>, usize, bool)> = None;
        while cursor < events.len() {
            let (event, range) = &events[cursor];
            let offset = range.start;
            let line = self.line(offset);
            match event {
                Event::Start(Tag::MetadataBlock(_)) => {
                    cursor += 1;
                    while cursor < events.len()
                        && !matches!(events[cursor].0, Event::End(TagEnd::MetadataBlock(_)))
                    {
                        cursor += 1;
                    }
                }
                Event::Start(Tag::CodeBlock(_)) | Event::Start(Tag::HtmlBlock) => {
                    let end = match event {
                        Event::Start(Tag::CodeBlock(_)) => TagEnd::CodeBlock,
                        _ => TagEnd::HtmlBlock,
                    };
                    let mut text = String::new();
                    let mut next = cursor + 1;
                    while next < events.len() && events[next].0 != Event::End(end) {
                        match &events[next].0 {
                            Event::Text(s) | Event::Html(s) => text.push_str(s),
                            _ => {}
                        }
                        next += 1;
                    }
                    if let Event::Start(Tag::CodeBlock(kind)) = event {
                        let info = match kind {
                            CodeBlockKind::Fenced(s) => s.as_ref(),
                            CodeBlockKind::Indented => "",
                        };
                        if info.split_whitespace().next() == Some("mermaid") {
                            let warning = self.diag(
                                offset,
                                "mermaid code fences are deprecated; use <mermaid> instead".into(),
                            );
                            self.warnings.push(warning);
                        }
                        let id =
                            if matches!(info.split_whitespace().next(), Some("mermaid" | "viz")) {
                                self.graph_ids.for_identity(format!(
                                    "fence:{}\n{}",
                                    info.trim(),
                                    text.trim()
                                ))
                            } else {
                                cursor
                            };
                        let language = info.split_whitespace().next().unwrap_or("");
                        if !language.is_empty()
                            && !matches!(
                                language,
                                "mermaid" | "viz" | "math" | "text" | "plaintext"
                            )
                            && matches!(self.engine.render("code", &text, language, id), Ok(None))
                        {
                            let warning = self.diag(offset, format!("unknown highlight.js language {language:?}; displaying plain text"));
                            self.warnings.push(warning);
                        }
                        let result = fragments::render_code_block(
                            info,
                            &text,
                            self.engine,
                            self.attachments,
                            id,
                            false,
                        );
                        let result = if !language.is_empty()
                            && !matches!(language, "mermaid" | "viz" | "math")
                        {
                            match result {
                                Err(message) => {
                                    let warning = self
                                        .diag(offset, format!("{message}; displaying plain text"));
                                    self.warnings.push(warning);
                                    Ok(Some(format!("<pre><code>{}</code></pre>", escape(&text))))
                                }
                                other => other,
                            }
                        } else {
                            result
                        };
                        out.push(self.compiled(offset, result, true, range));
                    } else if fragments::graph_tag_name(&text).is_some() {
                        let id = self.graph_ids.for_identity(text.trim().to_owned());
                        let result = fragments::render_graph_tag(
                            &text,
                            self.engine,
                            self.attachments,
                            id,
                            false,
                        );
                        out.push(self.compiled(offset, result, true, range));
                    } else {
                        out.push(self.html(&text, offset, true));
                    }
                    cursor = next;
                }
                Event::Start(Tag::Heading { level, .. }) => {
                    let mut end = cursor + 1;
                    let mut title = String::new();
                    while end < events.len()
                        && !matches!(events[end].0, Event::End(TagEnd::Heading(_)))
                    {
                        if let Event::Text(s) | Event::Code(s) | Event::InlineMath(s) =
                            &events[end].0
                        {
                            title.push_str(s)
                        }
                        end += 1;
                    }
                    let id = self.ids.next(slugify(&title));
                    let level = *level as u8;
                    if sections && depth == 0 && note_depth == 0 {
                        if !levels.is_empty() {
                            out.push(raw("</div>\n"));
                        }
                        while levels.last().is_some_and(|&n| n >= level) {
                            levels.pop();
                            out.push(raw("</details>\n"));
                        }
                        out.push(raw(format!("<details class=\"heading-section\" data-heading-level=\"{level}\" id=\"section-{id}\" open><summary class=\"heading-summary\">")));
                        levels.push(level);
                    }
                    out.push(raw(format!(
                        "<h{level} id=\"{}\" data-line=\"{line}\">",
                        escape(&id)
                    )));
                    out.extend(self.events(&events[cursor + 1..end], false));
                    out.push(raw(format!("</h{level}>")));
                    if sections && depth == 0 && note_depth == 0 {
                        out.push(raw("</summary><div class=\"heading-content\">\n"));
                    }
                    cursor = end;
                }
                Event::InlineMath(text) | Event::DisplayMath(text) => {
                    let block = matches!(event, Event::DisplayMath(_));
                    let result = self.engine.render(
                        if block { "display" } else { "math" },
                        text,
                        "",
                        cursor,
                    );
                    out.push(self.compiled(offset, result, block, range));
                }
                Event::Html(text) | Event::InlineHtml(text) => {
                    if text.starts_with("<details class=\"inline-footnote\"") {
                        note_depth += 1;
                    }
                    if text.as_ref() == "</div></details>\n" {
                        note_depth = note_depth.saturating_sub(1);
                    }
                    let tag = text.to_ascii_lowercase();
                    if tag.starts_with("<a ") || tag == "<a>" {
                        raw_anchor = true;
                    }
                    if tag.starts_with("</a>") {
                        raw_anchor = false;
                    }
                    out.push(self.html(text, offset, false));
                }
                Event::Text(text) if links.is_empty() && image_depth == 0 && !raw_anchor => {
                    let mut finder = linkify::LinkFinder::new();
                    finder.url_must_have_scheme(false);
                    let mut at = 0;
                    for link in finder.links(text) {
                        let value = link.as_str();
                        let url = if link.kind() == &linkify::LinkKind::Email {
                            format!("mailto:{value}")
                        } else if value.starts_with("www.") {
                            format!("http://{value}")
                        } else if value.starts_with("https://") || value.starts_with("http://") {
                            value.to_owned()
                        } else {
                            continue;
                        };
                        out.push(Event::Text(text[at..link.start()].to_owned().into()));
                        out.push(raw(format!(
                            "<a href=\"{}\">{}</a>",
                            escape(&url),
                            escape(value)
                        )));
                        at = link.end();
                    }
                    out.push(Event::Text(text[at..].to_owned().into()));
                }
                Event::Start(Tag::Image {
                    link_type,
                    dest_url,
                    title,
                    id,
                }) => {
                    match self.attachments.resolve(dest_url, true) {
                        Ok(url) => out.push(Event::Start(Tag::Image {
                            link_type: *link_type,
                            dest_url: url.into(),
                            title: title.clone(),
                            id: id.clone(),
                        })),
                        Err(message) => {
                            let error = self.diag(offset, message);
                            self.errors.push(error);
                            out.push(event.clone());
                        }
                    }
                    depth += 1;
                    image_depth += 1;
                }
                Event::End(TagEnd::Image) => {
                    depth -= 1;
                    image_depth -= 1;
                    out.push(event.clone());
                }
                Event::Start(Tag::Link {
                    link_type,
                    dest_url,
                    title,
                    id,
                }) => {
                    let target =
                        self.attachments
                            .code_link(dest_url, self.engine)
                            .and_then(|url| {
                                url.map_or_else(|| self.attachments.resolve(dest_url, false), Ok)
                            });
                    let problem = if dest_url.to_ascii_lowercase().starts_with("file:") {
                        Some(format!("non-portable local link {dest_url:?} was removed"))
                    } else {
                        target
                            .as_ref()
                            .err()
                            .filter(|s| {
                                s.contains(" is outside root ") || s.contains(" is missing: ")
                            })
                            .cloned()
                    };
                    if let Some(message) = problem {
                        let warning = self.diag(offset, message);
                        self.warnings.push(warning);
                        links.push(false);
                    } else {
                        links.push(true);
                        match target {
                            Ok(url) => out.push(Event::Start(Tag::Link {
                                link_type: *link_type,
                                dest_url: url.into(),
                                title: title.clone(),
                                id: id.clone(),
                            })),
                            Err(message) => {
                                let error = self.diag(offset, message);
                                self.errors.push(error);
                                out.push(event.clone());
                            }
                        }
                    }
                    depth += 1;
                }
                Event::End(TagEnd::Link) => {
                    depth -= 1;
                    if links.pop().unwrap_or(true) {
                        out.push(event.clone());
                    }
                }
                Event::Start(Tag::Table(alignments)) => {
                    depth += 1;
                    table = Some((alignments.clone(), 0, false));
                    out.push(raw(format!("<table data-line=\"{line}\">")));
                }
                Event::Start(Tag::TableHead) => {
                    depth += 1;
                    table.as_mut().unwrap().2 = true;
                    out.push(raw("<thead><tr>"));
                }
                Event::End(TagEnd::TableHead) => {
                    depth -= 1;
                    let state = table.as_mut().unwrap();
                    state.2 = false;
                    state.1 = 0;
                    out.push(raw("</tr></thead><tbody>"));
                }
                Event::Start(Tag::TableRow) => {
                    depth += 1;
                    table.as_mut().unwrap().1 = 0;
                    out.push(raw("<tr>"));
                }
                Event::End(TagEnd::TableRow) => {
                    depth -= 1;
                    out.push(raw("</tr>"));
                }
                Event::Start(Tag::TableCell) => {
                    depth += 1;
                    let (alignments, column, head) = table.as_mut().unwrap();
                    let name = if *head { "th" } else { "td" };
                    let align = match alignments.get(*column) {
                        Some(pulldown_cmark::Alignment::Left) => " style=\"text-align:left\"",
                        Some(pulldown_cmark::Alignment::Center) => " style=\"text-align:center\"",
                        Some(pulldown_cmark::Alignment::Right) => " style=\"text-align:right\"",
                        _ => "",
                    };
                    *column += 1;
                    out.push(raw(format!("<{name}{align}>")));
                }
                Event::End(TagEnd::TableCell) => {
                    depth -= 1;
                    out.push(raw(if table.as_ref().unwrap().2 {
                        "</th>"
                    } else {
                        "</td>"
                    }));
                }
                Event::End(TagEnd::Table) => {
                    depth -= 1;
                    table = None;
                    out.push(raw("</tbody></table>\n"));
                }
                Event::Start(tag) => {
                    depth += 1;
                    out.push(event.clone());
                    let name = match tag {
                        Tag::Paragraph => Some("p"),
                        Tag::Item => Some("li"),
                        Tag::BlockQuote(_) => Some("blockquote"),
                        _ => None,
                    };
                    if let Some(name) = name {
                        out.pop();
                        out.push(raw(format!("<{name} data-line=\"{line}\">")));
                    }
                }
                Event::End(end) => {
                    depth -= 1;
                    let name = match end {
                        TagEnd::Paragraph => Some("p"),
                        TagEnd::Item => Some("li"),
                        TagEnd::BlockQuote(_) => Some("blockquote"),
                        _ => None,
                    };
                    if let Some(name) = name {
                        out.push(raw(format!("</{name}>\n")))
                    } else {
                        out.push(event.clone())
                    }
                }
                _ => out.push(event.clone()),
            }
            cursor += 1;
        }
        if !levels.is_empty() {
            out.push(raw("</div>\n"));
            while levels.pop().is_some() {
                out.push(raw("</details>\n"));
            }
        }
        out
    }
    fn html(&mut self, text: &str, offset: usize, block: bool) -> Ev {
        let mut warnings = Vec::new();
        let result = crate::raw_html::rewrite_without_scripts(
            text,
            self.attachments,
            &mut warnings,
            &mut |url| self.attachments.code_link(url, self.engine),
        );
        for (at, message) in warnings {
            let error = self.diag(offset + at, message);
            self.warnings.push(error);
        }
        self.compiled(offset, result.map(Some), block, &(offset..offset))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Images;
    pub(super) fn compile(source: &str) -> String {
        let temp = tempfile::tempdir().unwrap();
        let attachments = Attachments::new(
            &temp.path().join("note.md"),
            &temp.path().join("note.html"),
            temp.path().to_owned(),
            Images::Reference,
            true,
        )
        .unwrap();
        render(source, &Engine::default(), &attachments, &mut Vec::new()).unwrap()
    }
    #[test]
    fn autolinks_do_not_rewrite_code_or_existing_links() {
        let output = compile(
            "https://example.com/path and www.example.org and a@example.net.\n\n`https://code.example` [https://label.example](https://target.example) <a href='https://raw.example'>https://inside.example</a>\n",
        );
        for target in [
            "https://example.com/path",
            "http://www.example.org",
            "mailto:a@example.net",
            "https://target.example",
            "https://raw.example",
        ] {
            assert!(output.contains(&format!("href=\"{target}\"")), "{output}");
        }
        assert!(!output.contains("href=\"https://code.example"));
        assert!(!output.contains("href=\"https://label.example"));
        assert!(!output.contains("href=\"https://inside.example"));
        assert_eq!(output.matches("<a ").count(), 5);
    }

    #[test]
    fn headings_reserve_nested_ids_with_inline_notes() {
        let output = compile(
            "# Foo\n\nText[^n].\n\n## Foo\n\n> ### Foo\n> quoted\n\n## Foo-1\n\n# Foo\n\nlast\n\n[^n]: note\n",
        );
        for id in ["foo", "foo-1", "foo-2", "foo-1-1", "foo-3"] {
            assert!(output.contains(&format!("id=\"{id}\"")), "{output}");
        }
        assert!(!output.contains("section-foo-2"));
        assert_eq!(
            output.matches("<details class=\"heading-section\"").count(),
            4
        );
        assert_eq!(output.matches("</details>").count(), 5);
        assert!(!output.contains("section class=\"footnotes\""));
        assert!(output.find("id=\"F1\"").unwrap() < output.find("id=\"section-foo-1\"").unwrap());
    }
    #[test]
    fn tight_lists_and_note_headings_keep_valid_containers() {
        let output =
            compile("# Main\n\n- item[^a]\n- second[^a]\n\n[^a]: ### Note heading\n\n    body\n");
        assert_eq!(
            output.matches("<details class=\"heading-section\"").count(),
            1
        );
        assert_eq!(
            output.matches("<details class=\"inline-footnote\"").count(),
            2
        );
        assert!(output.contains("</div></details>\n</li>"), "{output}");
        assert_eq!(output.matches("id=\"F1\"").count(), 1);
        assert!(output.contains("id=\"F1-2\""));
    }

    #[test]
    fn complex_tables_nested_notes_and_repeated_references() {
        let output = compile(
            "# 标题\n\n正文[^n]，重复[^n]，未知[^missing]。\n\n> - [x] task\n>   - nested\n\n| 名 | 注 |\n|:---|---:|\n| A | [^n] |\n\n[^n]: 第一段 *强调*，引用[^other]。\n\n    第二段 `code`。\n\n[^other]: 循环[^n]。\n",
        );
        assert!(output.contains("<table data-line=\"8\">"), "{output}");
        assert!(output.contains("<thead><tr><th style=\"text-align:left\">"));
        assert!(output.contains("<tbody><tr><td style=\"text-align:left\">"));
        assert!(output.contains("<em>强调</em>"));
        assert!(output.contains("第二段 <code>code</code>"));
        assert!(output.contains("[^missing]"));
        assert_eq!(
            output.matches("<details class=\"inline-footnote\"").count(),
            4
        );
        assert_eq!(output.matches("data-note=\"F1\"").count(), 2);
        assert!(output.contains("[^n]"));
        assert!(
            output.find("</table>").unwrap()
                < output.rfind("<details class=\"inline-footnote\"").unwrap()
        );
        assert!(!output.contains("data-footnote-backref"));
    }
}

#[cfg(test)]
mod native_graph_tests {
    use super::*;
    #[test]
    fn graph_case_attributes_and_adjacent_blocks_render_without_placeholders() {
        let output = super::tests::compile(
            "<!-- <mermaid> example -->\n\n<MERMAID\n caption='A &amp; B'>\nflowchart LR\n\n A --> B\n</MERMAID>\n<mermaid>\nflowchart LR\n C --> D\n</mermaid>\n\nafter\n",
        );
        assert_eq!(output.matches("<svg").count(), 2);
        assert!(output.contains("<figcaption>A &amp; B</figcaption>"));
        assert!(output.contains("after"));
        assert!(!output.contains("cmrak-graph"));
    }

    #[test]
    fn patched_parser_preserves_graph_blocks_and_markdown_context() {
        for source in [
            "<mermaid>\nflowchart LR\n\n```\nA --> B\n</mermaid>\n\nafter\n",
            "> <mermaid>\n> flowchart LR\n>\n> A --> B\n> </mermaid>\n>\n> after\n",
            "- before\n\n  <mermaid>\n  flowchart LR\n\n  A --> B\n  </mermaid>\n\n  after\n",
        ] {
            let events: Vec<_> = Parser::new_ext(source, options()).collect();
            assert_eq!(
                events
                    .iter()
                    .filter(|e| matches!(e, Event::Start(Tag::HtmlBlock)))
                    .count(),
                1
            );
            assert!(
                !events
                    .iter()
                    .any(|e| matches!(e, Event::Start(Tag::CodeBlock(_))))
            );
            let graph = events
                .iter()
                .filter_map(|e| {
                    if let Event::Html(text) = e {
                        Some(text.as_ref())
                    } else {
                        None
                    }
                })
                .collect::<String>();
            assert!(graph.contains("\n\n"));
            assert!(graph.contains("</mermaid>"));
            assert!(!graph.contains("> flowchart"));
            assert!(
                events
                    .iter()
                    .any(|e| matches!(e,Event::Text(text) if text.contains("after")))
            );
        }
        let source = "```html\n<mermaid>\n```\n\n`<mermaid>`\n";
        assert!(
            !Parser::new_ext(source, options()).any(|e| matches!(e, Event::Start(Tag::HtmlBlock)))
        );
    }
}
