//! Text-based fragment compilation shared by Markdown adapters.
use crate::{
    assets::{self, escape},
    document::Attachments,
    engine::Engine,
};
use std::fs;
use url::Url;

// Compile code and diagram fragments from plain text.
pub(crate) fn render_code_block(
    info: &str,
    literal: &str,
    engine: &Engine,
    attachments: &Attachments,
    index: usize,
    preview: bool,
) -> Result<Option<String>, String> {
    let language = info.split_whitespace().next().unwrap_or("");
    match language {
        "mermaid" => engine.render("diagram", literal, "", index).map(|svg| {
            svg.map(|svg| format!("<div class=\"diagram\" id=\"graph-{index}\">{svg}</div>\n"))
        }),
        "math" => engine.render("display", literal, "", index),
        "viz" => {
            let mut width = None;
            for option in info.split_whitespace().skip(1) {
                let value = option.strip_prefix("width=").ok_or_else(|| {
                    format!("unknown viz option {option:?}; expected width=NN% or width=NNpx")
                })?;
                if width.is_some() {
                    return Err("viz width may only be specified once".into());
                }
                width = Some(format!(
                    " style=\"--viz-width:{}\"",
                    validate_viz_width(value)?
                ));
            }
            let reference = literal.trim();
            if reference.is_empty() || reference.lines().count() != 1 {
                return Err("viz requires one local JavaScript file path".into());
            }
            attachments.track(reference);
            let url =
                Url::parse(&attachments.resolve(reference, false)?).map_err(|e| e.to_string())?;
            if url.query().is_some() || url.fragment().is_some() {
                return Err("viz file path cannot contain a query or fragment".into());
            }
            let path = url
                .to_file_path()
                .map_err(|_| "viz requires a local file")?;
            let source = fs::read_to_string(&path)
                .map_err(|e| format!("cannot read viz {reference:?}: {e}"))?;
            let (html, interactive) = engine
                .viz(&source, index)
                .map_err(|e| format!("Chartist {reference:?}: {e}"))?;
            let module = if interactive {
                Some(attachments.chart_module(&source)?)
            } else {
                None
            };
            let metadata = if preview {
                module
                    .as_ref()
                    .map(|url| format!(" data-viz-module=\"{}\"", escape(url)))
                    .unwrap_or_default()
            } else {
                String::new()
            };
            let script = if !preview {
                module.as_ref().map(|module| format!("<script type=\"module\">import {{hydrate}} from '{module}';try{{hydrate(document.getElementById('viz-{index}'))}}catch(error){{console.error('viz-{index}',error)}}</script>")).unwrap_or_default()
            } else {
                String::new()
            };
            Ok(Some(format!(
                "<div class=\"viz\" id=\"viz-{index}\"{}{}>{html}</div>{script}\n",
                width.unwrap_or_default(),
                metadata
            )))
        }

        _ => {
            let html = if language.is_empty() {
                None
            } else {
                engine.render("code", literal, language, index)?
            };
            let class = if language.is_empty() {
                "hljs".to_owned()
            } else {
                format!("hljs language-{}", escape(language))
            };
            Ok(Some(format!(
                "<div class=\"code-wrap\"><pre><code class=\"{class}\">{}</code></pre></div>\n",
                html.unwrap_or_else(|| escape(literal))
            )))
        }
    }
}

fn validate_viz_width(value: &str) -> Result<String, String> {
    let (digits, unit, limit) = if let Some(n) = value.strip_suffix('%') {
        (n, "%", 100)
    } else if let Some(n) = value.strip_suffix("px") {
        (n, "px", 10000)
    } else {
        return Err("viz width must use % or px".into());
    };
    let number = digits
        .parse::<u32>()
        .map_err(|_| "viz width must be a positive integer")?;
    if number == 0 || number > limit || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!("viz width must be between 1 and {limit}{unit}"));
    }
    Ok(format!("{number}{unit}"))
}

pub(crate) fn graph_tag_name(input: &str) -> Option<&'static str> {
    let input = input.trim_start().as_bytes();
    ["mermaid", "chartist"].into_iter().find(|name| {
        input.first() == Some(&b'<')
            && input
                .get(1..name.len() + 1)
                .is_some_and(|part| part.eq_ignore_ascii_case(name.as_bytes()))
            && input
                .get(name.len() + 1)
                .is_some_and(|b| b.is_ascii_whitespace() || matches!(b, b'>' | b'/'))
    })
}

pub(crate) fn render_graph_tag(
    html: &str,
    engine: &Engine,
    attachments: &Attachments,
    index: usize,
    preview: bool,
) -> Result<Option<String>, String> {
    use quick_xml::{Reader, events::Event};
    let input = html.trim();
    let Some(tag) = graph_tag_name(input) else {
        return Ok(None);
    };
    let mut reader = Reader::from_str(input);
    let start = match reader
        .read_event()
        .map_err(|e| format!("invalid {tag} tag: {e}"))?
    {
        Event::Start(start) => start,
        _ => return Err(format!("{tag} requires a complete opening and closing tag")),
    };
    let mut src = None;
    let mut width = None;
    let mut caption = None;
    for item in start.attributes() {
        let attr = item.map_err(|e| format!("invalid {tag} attribute: {e}"))?;
        let value = attr
            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .map_err(|e| e.to_string())?
            .into_owned();
        match attr.key.as_ref() {
            "src" => src = Some(value),
            "width" => width = Some(validate_viz_width(&value)?),
            "caption" => caption = Some(value),
            _ => return Err(format!("unsupported {tag} attribute")),
        }
    }
    let body_start = reader.buffer_position() as usize;
    let closing = format!("</{tag}>");
    let end = input.as_bytes()[body_start..]
        .windows(closing.len())
        .position(|part| part.eq_ignore_ascii_case(closing.as_bytes()))
        .map(|at| body_start + at)
        .ok_or_else(|| format!("{tag} requires a closing tag </{tag}>"))?;
    // Only attributes use XML syntax. Mermaid source is literal, including
    // blank lines, HTML labels and ampersands.
    let body = input[body_start..end].to_owned();
    if !input[end + closing.len()..].trim().is_empty() {
        return Err(format!("place each {tag} on its own Markdown block"));
    }
    if src.is_some() && !body.trim().is_empty() {
        return Err(format!("{tag} cannot combine src and inline content"));
    }
    let source_path = src.clone();
    let source = if let Some(path) = src {
        attachments.track(&path);
        let url = Url::parse(&attachments.resolve(&path, false)?).map_err(|e| e.to_string())?;
        if url.query().is_some() || url.fragment().is_some() {
            return Err(format!(
                "{tag} file path cannot contain a query or fragment"
            ));
        }
        let path = url
            .to_file_path()
            .map_err(|_| format!("{tag} requires a local file"))?;
        if tag == "chartist" {
            // Reuse the same path resolution and module rendering as fenced viz.
            assets::file_url(&path)?.to_string()
        } else {
            fs::read_to_string(&path)
                .map_err(|e| format!("cannot read mermaid file {}: {e}", path.display()))?
        }
    } else if tag == "mermaid" {
        body
    } else {
        return Err("chartist requires src".into());
    };
    let style = width
        .as_ref()
        .map(|w| format!(" style=\"--viz-width:{w}\""))
        .unwrap_or_default();
    let content = if tag == "chartist" {
        render_code_block("viz", &source, engine, attachments, index, preview)
            .map_err(|e| match &source_path {
                Some(path) => format!("chartist source {path:?}: {e}"),
                None => e,
            })?
            .ok_or("cannot render chartist")?
    } else {
        let svg = engine
            .render("diagram", &source, "", index)
            .map_err(|e| match &source_path {
                Some(path) => format!("mermaid {path:?}: {e}"),
                None => e,
            })?
            .ok_or("cannot render mermaid")?;
        format!("<div class=\"diagram\">{svg}</div>")
    };
    let caption = caption
        .map(|c| format!("<figcaption>{}</figcaption>", escape(&c)))
        .unwrap_or_default();
    let dependency = if preview {
        source_path
            .map(|path| format!(" data-graph-source=\"{}\"", escape(&path)))
            .unwrap_or_default()
    } else {
        String::new()
    };
    Ok(Some(format!(
        "<figure class=\"graph-figure\" id=\"graph-{index}\"{style}{dependency}>{content}{caption}</figure>\n"
    )))
}
