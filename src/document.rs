use crate::{
    assets::{self, escape},
    cli::{Args, Images},
    engine::Engine,
    headings::HeadingIds,
};
use comrak::{
    Arena, Options, format_html_with_plugins, nodes::NodeValue, options::Plugins, parse_document,
};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    rc::Rc,
};
use url::Url;

// IDs describe the graph declaration rather than its position in the AST.
// Occurrence numbers keep repeated declarations unique. The hash is explicit
// and deterministic across processes; collisions are resolved within a document.
#[derive(Default)]
struct GraphIds {
    occurrences: HashMap<String, u32>,
    used: HashSet<u32>,
}

impl GraphIds {
    fn for_node<'a>(&mut self, node: &'a comrak::nodes::AstNode<'a>) -> Option<usize> {
        fn is_graph(s: &str) -> bool {
            let s = s.trim();
            s.starts_with("<chartist ")
                || s.starts_with("<chartist>")
                || s.starts_with("<mermaid ")
                || s.starts_with("<mermaid>")
        }
        let data = node.data.borrow();
        let identity = match &data.value {
            NodeValue::CodeBlock(code)
                if matches!(code.info.split_whitespace().next(), Some("viz" | "mermaid")) =>
            {
                format!("fence:{}\n{}", code.info.trim(), code.literal.trim())
            }
            NodeValue::HtmlBlock(html) if is_graph(&html.literal) => html.literal.trim().to_owned(),
            NodeValue::Paragraph => {
                let markup: Option<String> = node
                    .children()
                    .map(|child| match &child.data.borrow().value {
                        NodeValue::HtmlInline(text) => Some(text.clone()),
                        NodeValue::Text(text) => Some(text.to_string()),
                        _ => None,
                    })
                    .collect();
                let markup = markup?;
                if !is_graph(&markup) {
                    return None;
                }
                markup.trim().to_owned()
            }
            _ => return None,
        };
        let occurrence = self.occurrences.entry(identity.clone()).or_default();
        let mut hash = 2166136261u32;
        for byte in identity.bytes().chain(occurrence.to_le_bytes()) {
            hash = (hash ^ u32::from(byte)).wrapping_mul(16777619);
        }
        *occurrence += 1;
        while !self.used.insert(hash) {
            hash = hash.wrapping_add(1);
        }
        Some(hash as usize)
    }
}

#[derive(Debug)]
pub struct Diagnostic {
    pub line: usize,
    pub column: usize,
    pub message: String,
}

pub struct Attachments {
    input: Url,
    site: bool,
    root: PathBuf,
    mode: Images,
    private: Option<PathBuf>,
    dependencies: Rc<RefCell<HashSet<PathBuf>>>,
    site_root: Option<PathBuf>,
    compiled_code: RefCell<HashMap<PathBuf, PathBuf>>,
}

impl Attachments {
    pub fn new(
        input: &Path,
        _output: &Path,
        root: PathBuf,
        mode: Images,
        _standalone: bool,
    ) -> Result<Self, String> {
        Ok(Self {
            input: assets::file_url(input)?,
            site: false,
            root,
            mode,
            private: None,
            dependencies: Rc::default(),
            site_root: None,
            compiled_code: RefCell::default(),
        })
    }

    pub fn exclude_private(mut self, path: PathBuf) -> Self {
        self.private = Some(dunce::canonicalize(&path).unwrap_or(path));
        self
    }

    fn join(&self, original: &str) -> Result<Url, url::ParseError> {
        if self.site && original.starts_with('/') && !original.starts_with("//") {
            Url::from_directory_path(&self.root)
                .unwrap()
                .join(original.trim_start_matches('/'))
        } else {
            self.input.join(original)
        }
    }

    fn track(&self, original: &str) {
        if let Ok(url) = self.join(original)
            && let Ok(path) = url.to_file_path()
        {
            if !path.starts_with(&self.root) {
                return;
            }
            let mut dependencies = self.dependencies.borrow_mut();
            if let Ok(canonical) = dunce::canonicalize(&path)
                && canonical.starts_with(&self.root)
            {
                dependencies.insert(canonical);
            }
            dependencies.insert(path);
        }
    }

    pub(crate) fn code_link(
        &self,
        original: &str,
        engine: &Engine,
    ) -> Result<Option<String>, String> {
        let Some(site) = &self.site_root else {
            return Ok(None);
        };
        if original.starts_with('#')
            || original.starts_with("//")
            || original.to_ascii_lowercase().starts_with("file:")
        {
            return Ok(None);
        }
        let url = self
            .join(original)
            .map_err(|e| format!("invalid link {original:?}: {e}"))?;
        if url.scheme() != "file" {
            return Ok(None);
        }
        let path = url
            .to_file_path()
            .map_err(|_| format!("invalid code link {original:?}"))?;
        if crate::source_code::language(&path).is_none() {
            return Ok(None);
        }
        let input = dunce::canonicalize(&path)
            .map_err(|e| format!("code link {original:?} ({}): {e}", path.display()))?;
        if !input.starts_with(&self.root) {
            return Err(format!(
                "code link {original:?} ({}): outside root {}",
                input.display(),
                self.root.display()
            ));
        }
        if self
            .private
            .as_ref()
            .is_some_and(|private| input.starts_with(private))
        {
            return Err(format!(
                "code link {original:?} ({}): application data is private",
                input.display()
            ));
        }
        let cached = self.compiled_code.borrow().get(&input).cloned();
        let output = if let Some(output) = cached {
            output
        } else {
            let output =
                crate::site::page_path(&site.join(input.strip_prefix(&self.root).unwrap()));
            let args = Args {
                input: input.clone(),
                output: output.clone(),
                root: Some(self.root.clone()),
                site_root: Some(site.clone()),
                images: self.mode,
                standalone: false,
                asset_dir: None,
                title: None,
            };
            compile_document(&args, engine, self.dependencies.clone()).map_err(|errors| {
                format!(
                    "code link {original:?} ({}): {}",
                    input.display(),
                    errors
                        .into_iter()
                        .map(|e| e.message)
                        .collect::<Vec<_>>()
                        .join("; ")
                )
            })?;
            self.compiled_code
                .borrow_mut()
                .insert(input.clone(), output.clone());
            output
        };
        self.track(original);
        let mut target =
            Url::from_directory_path(output.parent().ok_or("code output has no parent")?)
                .map_err(|_| "cannot form code preview URL")?;
        target.set_query(url.query());
        target.set_fragment(url.fragment());
        Ok(Some(target.to_string()))
    }

    pub fn resolve(&self, original: &str, image: bool) -> Result<String, String> {
        if image {
            self.track(original);
        }
        if original.is_empty() || original.starts_with('#') {
            return Ok(original.to_owned());
        }
        let network_url = original
            .starts_with("//")
            .then(|| format!("https:{original}"));
        let url = self
            .join(network_url.as_deref().unwrap_or(original))
            .map_err(|e| format!("invalid URL {original:?}: {e}"))?;
        if url.scheme() != "file" {
            if !matches!(url.scheme(), "http" | "https" | "mailto" | "tel" | "data") {
                return Err(format!("unsupported URL scheme in {original:?}"));
            }
            if image && !matches!(url.scheme(), "http" | "https" | "data") {
                return Err(format!("unsupported image URL {original:?}"));
            }
            return Ok(original.to_owned());
        }
        let path = url
            .to_file_path()
            .map_err(|_| format!("invalid local path {original:?}"))?;
        let path = match dunce::canonicalize(&path) {
            Ok(path) => path,
            // Links do not read their target; allow documents to reference files
            // that will be supplied later. Images still require an existing file.
            Err(error) if !image && error.kind() == std::io::ErrorKind::NotFound => {
                resolve_missing_link(&path)
                    .map_err(|e| format!("cannot resolve attachment {original:?}: {e}"))?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(format!("attachment {original:?} is missing: {error}"));
            }
            Err(error) => return Err(format!("cannot read attachment {original:?}: {error}")),
        };
        if !path.starts_with(&self.root) {
            return Err(format!(
                "attachment {original:?} is outside root {}; pass a wider --root",
                self.root.display()
            ));
        }
        if self
            .private
            .as_ref()
            .is_some_and(|private| path.starts_with(private))
        {
            return Err("application data is private".into());
        }
        if image && !path.is_file() {
            return Err(format!("image {original:?} is not a file"));
        }
        if image && self.mode == Images::Embed {
            let mime = mime_guess::from_path(&path).first_or_octet_stream();
            if !mime.essence_str().starts_with("image/") {
                return Err(format!("attachment {original:?} is not a recognized image"));
            }
            let bytes = fs::read(&path).map_err(|e| e.to_string())?;
            let mut result = assets::data_url(&bytes, mime.essence_str());
            if let Some(hash) = url.fragment() {
                result.push('#');
                result.push_str(hash);
            }
            return Ok(result);
        }
        let mut target = assets::file_url(&path)?;
        target.set_query(url.query());
        target.set_fragment(url.fragment());
        // The output HTML may live in a temporary compilation directory.
        // Keep every local attachment anchored to its source path instead of
        // making it relative to the generated document.
        Ok(target.to_string())
    }
}

// Resolve the nearest existing ancestor before restoring missing components.
// A dangling symlink must fail rather than become an unchecked lexical path.
fn resolve_missing_link(path: &Path) -> std::io::Result<PathBuf> {
    let mut ancestor = path;
    let mut missing = Vec::new();
    loop {
        match dunce::canonicalize(ancestor) {
            Ok(mut resolved) => {
                if !missing.is_empty() && !resolved.is_dir() {
                    return Err(std::io::Error::other("link parent is not a directory"));
                }
                for component in missing.into_iter().rev() {
                    resolved.push(component);
                }
                return Ok(resolved);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                match fs::symlink_metadata(ancestor) {
                    Ok(_) => return Err(error),
                    Err(metadata_error)
                        if metadata_error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(metadata_error) => return Err(metadata_error),
                }
                let Some(name) = ancestor.file_name() else {
                    return Err(error);
                };
                missing.push(name.to_owned());
                ancestor = ancestor.parent().ok_or(error)?;
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
pub fn render(
    source: &str,
    engine: &Engine,
    attachments: &Attachments,
) -> Result<String, Vec<Diagnostic>> {
    render_with_warnings(source, engine, attachments, &mut Vec::new())
}

fn render_with_warnings(
    source: &str,
    engine: &Engine,
    attachments: &Attachments,
    warnings: &mut Vec<Diagnostic>,
) -> Result<String, Vec<Diagnostic>> {
    let arena = Arena::new();
    let mut options = Options::default();
    options.extension.table = true;
    options.extension.strikethrough = true;
    options.extension.autolink = true;
    options.extension.tasklist = true;
    options.extension.footnotes = true;
    options.extension.front_matter_delimiter = Some("---".into());
    options.extension.math_dollars = true;
    options.extension.math_latex = true;
    // Raw HTML is intentionally preserved for documents that use HTML elements.
    options.render.r#unsafe = true;
    options.render.sourcepos = true;
    let root = parse_document(&arena, source, &options);
    crate::markdown::ast::add_static_heading_sections(&arena, root);
    let headings = HeadingIds::default();
    let mut plugins = Plugins::default();
    plugins.render.heading_adapter = Some(&headings);
    let nodes: Vec<_> = root.descendants().collect();
    let mut graph_ids = GraphIds::default();
    let diagnostics: Vec<_> = nodes
        .into_iter()
        .enumerate()
        .filter_map(|(index, node)| {
            if !std::ptr::eq(node, root) && node.parent().is_none() {
                return None;
            }
            let index = graph_ids.for_node(node).unwrap_or(index);
            if matches!(node.data.borrow().value, NodeValue::Paragraph) {
                let children: Vec<_> = node.children().collect();
                let markup: Option<String> = children
                    .iter()
                    .map(|child| match &child.data.borrow().value {
                        NodeValue::HtmlInline(text) => Some(text.clone()),
                        NodeValue::Text(text) => Some(text.to_string()),
                        _ => None,
                    })
                    .collect();
                if let Some(markup) = markup {
                    match render_graph_tag(&markup, engine, attachments, index, false) {
                        Ok(Some(html)) => {
                            for child in children {
                                child.detach();
                            }
                            let line = node.data.borrow().sourcepos.start.line.max(1);
                            node.data.borrow_mut().value = NodeValue::Raw(html.replacen(
                                '>',
                                &format!(" data-line=\"{line}\">"),
                                1,
                            ));
                            return None;
                        }
                        Err(message) => {
                            let position = node.data.borrow().sourcepos.start;
                            return Some(Diagnostic {
                                line: position.line.max(1),
                                column: position.column.max(1),
                                message,
                            });
                        }
                        Ok(None) => {}
                    }
                }
            }
            let mut data = node.data.borrow_mut();
            let position = data.sourcepos.start;
            if let NodeValue::Link(link) = &data.value {
                let url = link.url.clone();
                match attachments.code_link(&url, engine) {
                    Ok(Some(target)) => {
                        if let NodeValue::Link(link) = &mut data.value {
                            link.url = target;
                        }
                        return None;
                    }
                    Err(message) => {
                        return Some(Diagnostic {
                            line: position.line.max(1),
                            column: position.column.max(1),
                            message,
                        });
                    }
                    Ok(None) => {}
                }
                let problem = if url.to_ascii_lowercase().starts_with("file:") {
                    Some(format!("non-portable local link {url:?} was removed"))
                } else {
                    attachments.resolve(&url, false).err().filter(|message| {
                        message.contains(" is outside root ") || message.contains(" is missing: ")
                    })
                };
                if let Some(message) = problem {
                    warnings.push(Diagnostic {
                        line: position.line.max(1),
                        column: position.column.max(1),
                        message,
                    });
                    drop(data);
                    for child in node.children().collect::<Vec<_>>() {
                        node.insert_before(child);
                    }
                    node.detach();
                    return None;
                }
            }
            let html = match &data.value {
                NodeValue::HtmlInline(html) => Some(html.as_str()),
                NodeValue::HtmlBlock(html)
                    if !html.literal.trim_start().starts_with("<chartist")
                        && !html.literal.trim_start().starts_with("<mermaid") =>
                {
                    Some(html.literal.as_str())
                }
                _ => None,
            };
            if let Some(html) = html {
                let clean = strip_script_elements(html);
                let mut local_warnings = Vec::new();
                match crate::raw_html::rewrite_with_warnings(
                    &clean,
                    attachments,
                    &mut local_warnings,
                    &mut |url| attachments.code_link(url, engine),
                ) {
                    Ok(rewritten) => {
                        for (offset, message) in local_warnings {
                            let prefix = &clean[..offset];
                            let lines = prefix.bytes().filter(|&b| b == b'\n').count();
                            let column = if lines == 0 {
                                position.column + prefix.len()
                            } else {
                                prefix.rsplit('\n').next().unwrap_or("").len() + 1
                            };
                            warnings.push(Diagnostic {
                                line: position.line + lines,
                                column,
                                message,
                            });
                        }
                        let block = matches!(&data.value, NodeValue::HtmlBlock(_));
                        data.value = NodeValue::Raw(if block {
                            rewritten.replacen(
                                '>',
                                &format!(" data-line=\"{}\">", position.line.max(1)),
                                1,
                            )
                        } else {
                            rewritten
                        });
                        return None;
                    }
                    Err(message) => {
                        return Some(Diagnostic {
                            line: position.line.max(1),
                            column: position.column.max(1),
                            message,
                        });
                    }
                }
            }
            if let NodeValue::CodeBlock(code) = &data.value
                && code.fenced
                && code.info.split_whitespace().next() == Some("mermaid")
            {
                warnings.push(Diagnostic {
                    line: position.line.max(1),
                    column: position.column.max(1),
                    message: "mermaid code fences are deprecated; use <mermaid> instead".into(),
                });
            }

            match transform(&data.value, engine, attachments, index, false) {
                Ok(Some(value)) => {
                    let is_block = matches!(
                        &data.value,
                        NodeValue::CodeBlock(_) | NodeValue::HtmlBlock(_)
                    ) || matches!(&data.value, NodeValue::Math(math) if math.display_math);
                    data.value = match value {
                        NodeValue::Raw(html) if is_block => NodeValue::Raw(html.replacen(
                            '>',
                            &format!(" data-line=\"{}\">", position.line.max(1)),
                            1,
                        )),
                        value => value,
                    };
                    None
                }
                Ok(None) => None,
                Err(message) => {
                    let diagnostic = Diagnostic {
                        line: position.line.max(1),
                        column: position.column.max(1),
                        message,
                    };
                    Some(diagnostic)
                }
            }
        })
        .collect();
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    let mut html = String::new();
    format_html_with_plugins(root, &options, &mut html, &plugins).map_err(failure)?;
    Ok(crate::source_lines::mark_source_lines(
        crate::source_lines::normalize_footnote_ids(html),
    ))
}

/// Compile formulas, highlighted code, and graphs in the reader's Comrak tree.
/// Reader-specific transformations run after these fragments are generated.
pub type CompiledFragments = (Vec<Diagnostic>, Vec<(String, String)>);

pub fn compile_fragments<'a>(
    root: &'a comrak::nodes::AstNode<'a>,
    engine: &Engine,
    attachments: &Attachments,
) -> Result<CompiledFragments, Vec<Diagnostic>> {
    let nodes: Vec<_> = root.descendants().collect();
    let mut warnings = Vec::new();
    let mut modules = Vec::new();
    let mut errors = Vec::new();
    let mut graph_ids = GraphIds::default();
    for (index, node) in nodes.into_iter().enumerate() {
        if !std::ptr::eq(node, root) && node.parent().is_none() {
            continue;
        }
        let index = graph_ids.for_node(node).unwrap_or(index);
        let position = node.data.borrow().sourcepos.start;
        // Compile formulas and ordinary code before the reader expands footnotes,
        // so the same output is used inside and outside footnote panels.
        let fragment = {
            let data = node.data.borrow();
            match &data.value {
                NodeValue::Math(math) => Some(
                    engine
                        .render(
                            if math.display_math { "display" } else { "math" },
                            &math.literal,
                            "",
                            index,
                        )
                        .map(|html| {
                            html.map(|html| {
                                if math.display_math {
                                    format!(
                                        "<span data-line=\"{}\">{html}</span>",
                                        position.line.max(1)
                                    )
                                } else {
                                    html
                                }
                            })
                        }),
                ),
                NodeValue::CodeBlock(code)
                    if code.info.split_whitespace().next() == Some("math") =>
                {
                    Some(
                        engine
                            .render("display", &code.literal, "", index)
                            .map(|html| {
                                html.map(|html| {
                                    format!(
                                        "<div class=\"math-block\" data-line=\"{}\">{html}</div>",
                                        position.line.max(1)
                                    )
                                })
                            }),
                    )
                }
                NodeValue::CodeBlock(code)
                    if !matches!(code.info.split_whitespace().next(), Some("viz" | "mermaid")) =>
                {
                    let language = code.info.split_whitespace().next().unwrap_or("");
                    let highlighted = if language.is_empty() {
                        Ok(None)
                    } else {
                        engine.render("code", &code.literal, language, index)
                    };
                    Some(highlighted.map(|html| {
                        Some(format!(
                            "<pre data-line=\"{}\"><code{}>{}</code></pre>",
                            position.line.max(1),
                            if language.is_empty() {
                                String::new()
                            } else {
                                format!(" class=\"language-{}\"", escape(language))
                            },
                            html.unwrap_or_else(|| escape(&code.literal))
                        ))
                    }))
                }
                _ => None,
            }
        };
        if let Some(fragment) = fragment {
            match fragment {
                Ok(Some(html)) => node.data.borrow_mut().value = NodeValue::Raw(html),
                Ok(None) => {}
                Err(message) => errors.push(Diagnostic {
                    line: position.line.max(1),
                    column: position.column.max(1),
                    message,
                }),
            }
            continue;
        }
        let children: Vec<_> = node.children().collect();
        let result = if matches!(node.data.borrow().value, NodeValue::Paragraph) {
            let markup: Option<String> = children
                .iter()
                .map(|child| match &child.data.borrow().value {
                    NodeValue::HtmlInline(text) => Some(text.clone()),
                    NodeValue::Text(text) => Some(text.to_string()),
                    _ => None,
                })
                .collect();
            markup.map_or(Ok(None), |markup| {
                render_graph_tag(&markup, engine, attachments, index, true)
            })
        } else {
            let data = node.data.borrow();
            match &data.value {
                NodeValue::HtmlBlock(html) => {
                    render_graph_tag(&html.literal, engine, attachments, index, true)
                }
                NodeValue::CodeBlock(code)
                    if code.info.split_whitespace().next() == Some("mermaid") =>
                {
                    warnings.push(Diagnostic {
                        line: position.line.max(1),
                        column: position.column.max(1),
                        message: "mermaid code fences are deprecated; use <mermaid> instead".into(),
                    });
                    transform(&data.value, engine, attachments, index, true).map(
                        |value| match value {
                            Some(NodeValue::Raw(html)) => Some(html),
                            _ => None,
                        },
                    )
                }
                NodeValue::CodeBlock(code)
                    if code.info.split_whitespace().next() == Some("viz") =>
                {
                    let reference = code.literal.trim();
                    transform(&data.value, engine, attachments, index, true).map(
                        |value| match value {
                            Some(NodeValue::Raw(html)) => Some(html.replacen(
                                "<div class=\"viz\"",
                                &format!(
                                    "<div class=\"viz\" data-graph-source=\"{}\"",
                                    escape(reference)
                                ),
                                1,
                            )),
                            _ => None,
                        },
                    )
                }
                _ => Ok(None),
            }
        };
        match result {
            Ok(Some(mut html)) => {
                let marker = " data-viz-module=\"";
                if let Some(start) = html.find(marker) {
                    let value_start = start + marker.len();
                    if let Some(length) = html[value_start..].find('"') {
                        modules.push((
                            format!("viz-{index}"),
                            html[value_start..value_start + length].to_owned(),
                        ));
                        html.replace_range(start..value_start + length + 1, "");
                    }
                }
                for child in children {
                    child.detach();
                }
                let html =
                    html.replacen('>', &format!(" data-line=\"{}\">", position.line.max(1)), 1);
                node.data.borrow_mut().value = NodeValue::Raw(html);
            }
            Ok(None) => {}
            Err(message) => errors.push(Diagnostic {
                line: position.line.max(1),
                column: position.column.max(1),
                message,
            }),
        }
    }
    if errors.is_empty() {
        Ok((warnings, modules))
    } else {
        Err(errors)
    }
}

fn transform(
    value: &NodeValue,
    engine: &Engine,
    attachments: &Attachments,
    index: usize,
    preview: bool,
) -> Result<Option<NodeValue>, String> {
    match value {
        NodeValue::CodeBlock(code) => {
            let language = code.info.split_whitespace().next().unwrap_or("");
            match language {
                "mermaid" => engine
                    .render("diagram", &code.literal, "", index)
                    .map(|svg| svg.map(|svg| format!("<div class=\"diagram\" id=\"graph-{index}\">{svg}</div>\n"))),
                "math" => engine.render("display", &code.literal, "", index),
                "viz" => {
                    let mut width = None;
                    for option in code.info.split_whitespace().skip(1) {
                        let value = option.strip_prefix("width=").ok_or_else(||
                            format!("unknown viz option {option:?}; expected width=NN% or width=NNpx"))?;
                        if width.is_some() {
                            return Err("viz width may only be specified once".into());
                        }
                        width = Some(format!(" style=\"--viz-width:{}\"", validate_viz_width(value)?));
                    }
                    let reference = code.literal.trim();
                    if reference.is_empty() || reference.lines().count() != 1 {
                        return Err("viz requires one local JavaScript file path".into());
                    }
                    attachments.track(reference);
                    let url = Url::parse(&attachments.resolve(reference, false)?)
                        .map_err(|e| e.to_string())?;
                    if url.query().is_some() || url.fragment().is_some() {
                        return Err("viz file path cannot contain a query or fragment".into());
                    }
                    let path = url.to_file_path().map_err(|_| "viz requires a local file")?;
                    let source = fs::read_to_string(&path)
                        .map_err(|e| format!("cannot read viz {reference:?}: {e}"))?;
                    let (html, interactive) = engine.viz(&source, index)?;
                    let module = interactive.then(|| assets::data_url(source.as_bytes(), "text/javascript"));
                    let metadata = if preview {
                        module.as_ref().map(|url| format!(" data-viz-module=\"{}\"", escape(url))).unwrap_or_default()
                    } else { String::new() };
                    let script = if !preview {
                        module.as_ref().map(|module| format!("<script type=\"module\">import {{hydrate}} from '{module}';try{{hydrate(document.getElementById('viz-{index}'))}}catch(error){{console.error('viz-{index}',error)}}</script>")).unwrap_or_default()
                    } else { String::new() };
                    Ok(Some(format!("<div class=\"viz\" id=\"viz-{index}\"{}{}>{html}</div>{script}\n", width.unwrap_or_default(), metadata)))
                }

                _ => {
                    let html = if language.is_empty() {
                        None
                    } else {
                        engine.render("code", &code.literal, language, index)?
                    };
                    let class = if language.is_empty() {
                        "hljs".to_owned()
                    } else {
                        format!("hljs language-{}", escape(language))
                    };
                    Ok(Some(format!(
                        "<div class=\"code-wrap\"><pre><code class=\"{class}\">{}</code></pre></div>\n",
                        html.unwrap_or_else(|| escape(&code.literal))
                    )))
                }
            }
        }
        NodeValue::Math(math) => engine.render(
            if math.display_math { "display" } else { "math" },
            &math.literal,
            "",
            index,
        ),
        NodeValue::Image(link) => {
            let mut link = link.clone();
            link.url = attachments.resolve(&link.url, true)?;
            return Ok(Some(NodeValue::Image(link)));
        }
        NodeValue::Link(link) => {
            let mut link = link.clone();
            link.url = attachments.resolve(&link.url, false)?;
            return Ok(Some(NodeValue::Link(link)));
        }
        NodeValue::HtmlInline(html) => {
            crate::raw_html::rewrite(&strip_script_elements(html), attachments).map(Some)
        }
        NodeValue::HtmlBlock(html) => {
            if let Some(result) = render_graph_tag(&html.literal, engine, attachments, index, preview)? {
                return Ok(Some(NodeValue::Raw(result)));
            }
            crate::raw_html::rewrite(&strip_script_elements(&html.literal), attachments).map(Some)
        }
        _ => Ok(None),
    }
    .map(|html| html.map(NodeValue::Raw))
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

fn render_graph_tag(
    html: &str,
    engine: &Engine,
    attachments: &Attachments,
    index: usize,
    preview: bool,
) -> Result<Option<String>, String> {
    use quick_xml::{Reader, events::Event};
    let input = html.trim();
    let tag = if input.starts_with("<chartist ") || input.starts_with("<chartist>") {
        "chartist"
    } else if input.starts_with("<mermaid ") || input.starts_with("<mermaid>") {
        "mermaid"
    } else {
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
    let mut body = String::new();
    loop {
        match reader
            .read_event()
            .map_err(|e| format!("invalid {tag}: {e}"))?
        {
            Event::Text(text) => body.push_str(
                &quick_xml::escape::unescape(&text.xml10_content()).map_err(|e| e.to_string())?,
            ),
            Event::CData(text) => body.push_str(&text.xml10_content()),
            Event::GeneralRef(reference) => {
                let reference = format!("&{};", reference.xml10_content());
                body.push_str(&quick_xml::escape::unescape(&reference).map_err(|e| e.to_string())?);
            }
            Event::End(end) if end.name().as_ref() == tag => break,
            Event::Eof => return Err(format!("{tag} requires a closing tag")),
            _ => return Err(format!("{tag} content must be plain text")),
        }
    }
    if !input[reader.buffer_position() as usize..].trim().is_empty() {
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
            fs::read_to_string(&path).map_err(|e| format!("cannot read mermaid file: {e}"))?
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
        let code = comrak::nodes::NodeCodeBlock {
            info: "viz".into(),
            literal: source,
            ..Default::default()
        };
        match transform(
            &NodeValue::CodeBlock(Box::new(code)),
            engine,
            attachments,
            index,
            preview,
        )? {
            Some(NodeValue::Raw(html)) => html,
            _ => return Err("cannot render chartist".into()),
        }
    } else {
        let svg = engine
            .render("diagram", &source, "", index)?
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

fn strip_script_elements(html: &str) -> String {
    let bytes = html.as_bytes();
    let mut output = String::with_capacity(html.len());
    let mut cursor = 0;
    while let Some(relative) = find_ascii_case_insensitive(bytes, b"<script", cursor) {
        let start = relative;
        let after_name = start + b"<script".len();
        if bytes
            .get(after_name)
            .is_some_and(|byte| !byte.is_ascii_whitespace() && *byte != b'>' && *byte != b'/')
        {
            output.push_str(&html[cursor..after_name]);
            cursor = after_name;
            continue;
        }
        output.push_str(&html[cursor..start]);
        let Some(open_end) = html[after_name..].find('>').map(|end| after_name + end + 1) else {
            return output;
        };
        let self_closing = bytes[start..open_end - 1]
            .iter()
            .rev()
            .find(|byte| !byte.is_ascii_whitespace())
            == Some(&b'/');
        if self_closing {
            cursor = open_end;
            continue;
        }
        let Some(close_start) = find_ascii_case_insensitive(bytes, b"</script", open_end) else {
            return output;
        };
        let Some(close_end) = html[close_start..]
            .find('>')
            .map(|end| close_start + end + 1)
        else {
            return output;
        };
        cursor = close_end;
    }
    output.push_str(&html[cursor..]);
    output
}

fn find_ascii_case_insensitive(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    haystack
        .get(from..)?
        .windows(needle.len())
        .position(|window| {
            window
                .iter()
                .zip(needle)
                .all(|(actual, expected)| actual.eq_ignore_ascii_case(expected))
        })
        .map(|offset| from + offset)
}

fn failure(message: impl ToString) -> Vec<Diagnostic> {
    vec![Diagnostic {
        line: 1,
        column: 1,
        message: message.to_string(),
    }]
}

pub fn compile(args: &Args) -> Result<(), Vec<Diagnostic>> {
    // QuickJS uses the native thread stack for parsing and execution. Reserve
    // more than its 4 MiB budget instead of relying on the platform main stack
    // (notably smaller on Windows). Create and drop the runtime on this thread.
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("comrak-compiler".into())
            .stack_size(16 * 1024 * 1024)
            .spawn_scoped(scope, || {
                compile_document(args, &Engine::default(), Rc::default()).map(|warnings| {
                    for warning in warnings {
                        eprintln!(
                            "{}:{}:{}: warning: {}",
                            args.input.display(),
                            warning.line,
                            warning.column,
                            warning.message
                        );
                    }
                })
            })
            .map_err(|error| failure(format!("cannot start compiler thread: {error}")))?
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
    })
}

pub(crate) fn compile_document(
    args: &Args,
    engine: &Engine,
    dependencies: Rc<RefCell<HashSet<PathBuf>>>,
) -> Result<Vec<Diagnostic>, Vec<Diagnostic>> {
    let input =
        dunce::canonicalize(&args.input).map_err(|e| failure(format!("cannot open input: {e}")))?;
    let _code_lock = if let Some(site) = &args.site_root
        && crate::source_code::language(&input).is_some()
    {
        use sha2::{Digest, Sha256};
        fs::create_dir_all(site).map_err(failure)?;
        let site = dunce::canonicalize(site).map_err(failure)?;
        let lock_dir = site.join("_shared/lib-comrak/.locks");
        fs::create_dir_all(&lock_dir).map_err(failure)?;
        let mut hash = Sha256::new();
        hash.update(input.to_string_lossy().as_bytes());
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_dir.join(format!("{:x}.lock", hash.finalize())))
            .map_err(failure)?;
        lock.lock().map_err(failure)?;
        Some(lock)
    } else {
        None
    };
    let source =
        fs::read_to_string(&input).map_err(|e| failure(format!("cannot read UTF-8 input: {e}")))?;
    let root = dunce::canonicalize(args.root.as_deref().unwrap_or(Path::new(".")))
        .map_err(|e| failure(format!("cannot open root: {e}")))?;
    if !root.is_dir() || !input.starts_with(&root) {
        return Err(failure("input must be inside the --root directory"));
    }
    let output = std::path::absolute(&args.output).map_err(failure)?;
    let parent = output
        .parent()
        .ok_or_else(|| failure("output must have a parent directory"))?;
    fs::create_dir_all(parent).map_err(failure)?;
    let output = dunce::canonicalize(parent).map_err(failure)?.join(
        output
            .file_name()
            .ok_or_else(|| failure("output must be a file"))?,
    );
    if output == input || dunce::canonicalize(&output).ok().as_ref() == Some(&input) {
        return Err(failure("output would overwrite the input"));
    }
    let site_assets = args
        .site_root
        .as_ref()
        .map(|site| site.join("_shared/lib-comrak"));
    let asset_dir = site_assets.as_deref().or(args.asset_dir.as_deref());
    let mut attachments =
        Attachments::new(&input, &output, root, args.images, args.standalone).map_err(failure)?;
    attachments.site = args.site_root.is_some();
    attachments.site_root = args
        .site_root
        .as_ref()
        .map(dunce::canonicalize)
        .transpose()
        .map_err(failure)?;
    attachments.dependencies = dependencies;
    let mut warnings = Vec::new();
    let body = if let Some(language) = crate::source_code::language(&input) {
        crate::source_code::render(&source, language, engine).map_err(failure)?
    } else {
        render_with_warnings(&source, engine, &attachments, &mut warnings)?
    };
    let stylesheet =
        assets::stylesheet_for_math(args.standalone, asset_dir, body.contains("class=\"katex"))
            .map_err(failure)?;
    let viz_style = if body.contains("<div class=\"viz\"") {
        assets::viz_stylesheet(args.standalone, asset_dir).map_err(failure)?
    } else {
        String::new()
    };
    let mermaid_style = if body.contains("class=\"diagram\"") {
        assets::mermaid_stylesheet(args.standalone, asset_dir).map_err(failure)?
    } else {
        String::new()
    };
    let viz_runtime = if body.contains("<script type=\"module\">") {
        assets::viz_runtime(args.standalone, asset_dir).map_err(failure)?
    } else {
        String::new()
    };
    let title = args.title.as_deref().unwrap_or_else(|| {
        input
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Document")
    });
    let html = format!(
        "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><meta name=\"generator\" content=\"comrakk {}\"><title>{}</title>\n{stylesheet}\n{viz_style}\n{mermaid_style}\n{viz_runtime}\n</head><body><main id=\"content\">\n{body}</main></body></html>\n",
        env!("CARGO_PKG_VERSION"),
        escape(title)
    );
    let navigation = include_str!("../generated/navigation.js");
    let html = html.replace("</body>", &format!("<script>{navigation}</script></body>"));
    let html = if let Some(site) = &args.site_root {
        crate::site::rewrite(
            &html,
            &input,
            &output,
            &attachments.root,
            site,
            asset_dir.unwrap(),
        )
        .map_err(failure)?
    } else {
        html
    };
    assets::atomic_write(&output, html.as_bytes()).map_err(failure)?;
    Ok(warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_files_compile_with_highlighting_without_markdown_interpretation() {
        let temp = tempfile::tempdir().unwrap();
        let cases = [
            ("sample.c", "c", "int main(void) { return 0; }\n"),
            ("sample.h", "c", "#define VALUE 42\n"),
            (
                "sample.bib",
                "bibtex",
                "@article{key, title={Example}, year=2026}\n",
            ),
            ("sample.py", "python", "def greet():\n    return True\n"),
            ("sample.lua", "lua", "local value = true\n"),
            ("sample.sh", "bash", "#!/bin/bash\necho \"hello\"\n"),
        ];
        for (name, language, source) in cases {
            let input = temp.path().join(name);
            fs::write(&input, source).unwrap();
            let args = Args {
                input,
                output: temp.path().join(format!("{name}.html")),
                root: Some(temp.path().to_path_buf()),
                site_root: None,
                images: Images::Embed,
                standalone: true,
                asset_dir: None,
                title: Some("Source <example>".into()),
            };
            compile(&args).unwrap();
            let html = fs::read_to_string(&args.output).unwrap();
            assert!(
                html.contains(&format!("class=\"hljs language-{language}\"")),
                "{name}"
            );
            assert!(html.contains("<span class=\"hljs-"), "{name}");
            assert!(html.contains("<main id=\"content\">"));
            assert!(html.contains("<title>Source &lt;example&gt;</title>"));
            assert!(html.contains("data-line=\"1\""));

            // These sequences must remain literal source, never Markdown,
            // attachment references, math, scripts, or executable diagrams.
            let literal = "# heading\n![image](missing.png)\n$\\invalidcommand$\n```mermaid\ninvalid graph\n```\n<script>alert('x')</script>\n<chartist src=\"missing.js\"></chartist>\n";
            fs::write(&args.input, literal).unwrap();
            compile(&args).unwrap();
            let html = fs::read_to_string(&args.output).unwrap();
            let body = html
                .split("<main id=\"content\">\n")
                .nth(1)
                .unwrap()
                .split("</main>")
                .next()
                .unwrap();
            assert!(!body.contains("<script>"));
            assert!(!body.contains("<img"));
            assert!(!body.contains("<h1"));
            assert!(!body.contains("<chartist"));
            assert!(body.contains("missing.png"));
            assert!(body.contains("&lt;"));

            fs::write(&args.input, "").unwrap();
            compile(&args).unwrap();
            assert!(
                fs::read_to_string(&args.output)
                    .unwrap()
                    .contains("</code></pre>")
            );
        }
    }

    #[test]
    fn repeated_compilation_tracks_graph_dependencies_and_refreshes_changed_source() {
        let temp = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(temp.path()).unwrap();
        let input = root.join("note.md");
        let chart = root.join("chart.js");
        fs::write(&input, "<chartist src=\"chart.js\"></chartist>\n\n$x^2$\n").unwrap();
        fs::write(
            &chart,
            "export default c => { c.innerHTML = '<svg><text>first</text></svg>'; }",
        )
        .unwrap();
        let args = Args {
            site_root: None,
            input: input.clone(),
            output: root.join("out.html"),
            root: Some(input.parent().unwrap().to_path_buf()),
            images: Images::Reference,
            standalone: true,
            asset_dir: None,
            title: None,
        };
        let engine = Engine::default();
        let dependencies = Rc::new(RefCell::new(HashSet::new()));
        compile_document(&args, &engine, dependencies.clone()).unwrap();
        assert!(dependencies.borrow().contains(&chart));
        assert!(fs::read_to_string(&args.output).unwrap().contains("first"));
        fs::write(
            &chart,
            "export default c => { c.innerHTML = '<svg><text>second</text></svg>'; }",
        )
        .unwrap();
        compile_document(&args, &engine, Rc::default()).unwrap();
        assert!(fs::read_to_string(&args.output).unwrap().contains("second"));
        fs::remove_file(&chart).unwrap();
        let missing = Rc::new(RefCell::new(HashSet::new()));
        assert!(compile_document(&args, &engine, missing.clone()).is_err());
        assert!(missing.borrow().contains(&chart));
        assert!(fs::read_to_string(&args.output).unwrap().contains("second"));
    }

    #[test]
    fn outside_root_link_warns_and_does_not_abort_rendering() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("notes");
        fs::create_dir(&root).unwrap();
        fs::write(temp.path().join("outside.md"), "outside").unwrap();
        let root = dunce::canonicalize(root).unwrap();
        let attachments = Attachments::new(
            &root.join("note.md"),
            &root.join("out.html"),
            root,
            Images::Reference,
            false,
        )
        .unwrap();
        let mut warnings = Vec::new();
        let html = render_with_warnings(
            "[outside](../outside.md)\n\n![missing](missing.png)",
            &Engine::default(),
            &attachments,
            &mut warnings,
        );
        assert!(html.is_err());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("outside root"));

        warnings.clear();
        let html = render_with_warnings(
            "[outside](../outside.md)",
            &Engine::default(),
            &attachments,
            &mut warnings,
        )
        .unwrap();
        assert!(html.contains("outside"));
        assert!(!html.contains("../outside.md"));
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn local_links_warn_with_positions_and_required_images_fail() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        fs::create_dir(&root).unwrap();
        let input = root.join("note.md");
        fs::write(&input, "Intro\n\n[local](file:///elsewhere/note.pdf)\n\n<a href=\"file:///elsewhere/a.pdf\">raw</a>\n").unwrap();
        let args = Args {
            input,
            output: root.join("site/note/index.html"),
            root: Some(root.clone()),
            site_root: Some(root.join("site")),
            images: Images::Reference,
            standalone: false,
            asset_dir: None,
            title: None,
        };
        let warnings = compile_document(&args, &Engine::default(), Rc::default()).unwrap();
        assert_eq!(warnings.len(), 2);
        assert_eq!(warnings[0].line, 3);
        assert_eq!(warnings[1].line, 5);
        assert!(warnings[0].message.contains("file:///elsewhere/note.pdf"));
        assert!(warnings[1].message.contains("file:///elsewhere/a.pdf"));
        let html = fs::read_to_string(&args.output).unwrap();
        assert!(html.contains("local"));
        assert!(html.contains("raw"));
        assert!(!html.contains("file:///elsewhere/"));
        let outside = temp.path().join("outside.png");
        fs::write(&outside, "image").unwrap();
        fs::write(&args.input, "text\n\n![image](../outside.png)\n").unwrap();
        let errors = compile_document(&args, &Engine::default(), Rc::default()).unwrap_err();
        assert_eq!(errors[0].line, 3);
        assert!(errors[0].message.contains("../outside.png"));
        assert!(errors[0].message.contains("outside root"));
    }

    #[test]
    fn graph_tags_and_mermaid_fence_warning() {
        let temp = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(temp.path()).unwrap();
        fs::write(
            root.join("chart.js"),
            "import { barChartConfig } from 'chartist'; import { toScene, renderToString } from 'chartist/server'; export default c => { c.innerHTML = renderToString(toScene(barChartConfig({labels:['A'],series:[[1]]}),{width:500,height:300})); }",
        )
        .unwrap();
        fs::write(root.join("flow.mmd"), "flowchart LR\n A --> B\n").unwrap();
        let attachments = Attachments::new(
            &root.join("note.md"),
            &root.join("out.html"),
            root,
            Images::Reference,
            false,
        )
        .unwrap();
        let engine = Engine::default();
        let source = "<chartist src=\"chart.js\" width=\"75%\" caption=\"A &amp; B\"></chartist>\n\n<mermaid src=\"flow.mmd\" width=\"520px\" caption=\"Flow\"></mermaid>\n\n<mermaid>\nflowchart LR\n C --> D\n</mermaid>\n\n```mermaid\nflowchart LR\n X --> Y\n```\n";
        let mut warnings = Vec::new();
        let html = render_with_warnings(source, &engine, &attachments, &mut warnings).unwrap();
        assert!(
            html.contains("<figcaption>A &amp; B</figcaption>"),
            "{html}"
        );
        assert!(html.contains("<figcaption>Flow</figcaption>"));
        assert!(html.contains("--viz-width:75%"));
        assert!(html.contains("--viz-width:520px"));
        assert_eq!(html.matches("<svg").count(), 4);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].line, 10);
    }

    #[test]
    fn viz_compiles_modules_and_preserves_interactive_code_only_when_requested() {
        let temp = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(temp.path()).unwrap();
        let input = root.join("note.md");
        let output = root.join("note.html");
        fs::write(root.join("chart.js"), r#"
import { barChartConfig } from 'chartist';
import { toScene, renderToString } from 'chartist/server';
export default function(container) {
  container.innerHTML = renderToString(toScene(barChartConfig({labels:['A','B','C'],series:[[1,2,3]]}), {width:300,height:100}));
}
"#).unwrap();
        let mut args = crate::cli::parse(
            [
                input.to_str().unwrap(),
                output.to_str().unwrap(),
                "--standalone",
            ]
            .map(str::to_owned),
        )
        .unwrap()
        .unwrap();
        args.root = Some(temp.path().to_path_buf());
        fs::write(&input, "```viz\nchart.js\n```\n").unwrap();
        compile(&args).unwrap();
        let html = fs::read_to_string(&output).unwrap();
        assert!(html.contains("<svg"));
        assert!(html.contains("ct-label"));
        assert!(html.contains("ct-bar"));
        assert!(!html.contains("<script type=\"module\""));
        for (option, expected) in [("width=75%", "75%"), ("width=520px", "520px")] {
            fs::write(&input, format!("```viz {option}\nchart.js\n```\n")).unwrap();
            compile(&args).unwrap();
            let sized = fs::read_to_string(&output).unwrap();
            assert!(sized.contains(&format!("style=\"--viz-width:{expected}\"")));
        }
        for option in [
            "width=0%",
            "width=101%",
            "width=5em",
            "width=1px;width=2px",
            "wide",
        ] {
            fs::write(&input, format!("```viz {option}\nchart.js\n```\n")).unwrap();
            assert!(compile(&args).is_err(), "accepted {option}");
        }
        fs::write(&input, "```viz\nchart.js\n```\n").unwrap();
        let script = fs::read_to_string(root.join("chart.js")).unwrap();
        fs::write(
            root.join("chart.js"),
            format!("{script}\nexport function hydrate(container) {{}}"),
        )
        .unwrap();
        compile(&args).unwrap();
        let html = fs::read_to_string(&output).unwrap();
        assert!(html.contains("type=\"importmap\""));
        assert!(html.contains("type=\"module\""));
        assert!(html.contains("--viz-accent-2:"));
        assert!(!html.contains("file:///"));
        args.standalone = false;
        args.asset_dir = Some(root.join("shared"));
        compile(&args).unwrap();
        let html = fs::read_to_string(&output).unwrap();
        assert!(html.contains("/viz.js\""));
        assert!(html.contains("/viz.css\""));
        fs::write(
            root.join("chart.js"),
            "export default () => { throw Error('broken chart') }",
        )
        .unwrap();
        let errors = compile(&args).unwrap_err();
        assert_eq!(errors[0].line, 1);
        assert!(errors[0].message.contains("broken chart"));
        assert_eq!(fs::read_to_string(&output).unwrap(), html);
        fs::write(root.join("chart.js"), "export default async () => {}").unwrap();
        assert!(
            compile(&args).unwrap_err()[0]
                .message
                .contains("synchronous")
        );
        fs::write(
            root.join("chart.js"),
            "import x from './other.js'; export default () => {}",
        )
        .unwrap();
        assert!(compile(&args).is_err());
        fs::write(&input, "```viz\nhttps://example.com/chart.js\n```\n").unwrap();
        assert!(compile(&args).is_err());
    }

    #[test]
    fn compilation_does_not_depend_on_callers_stack_size() {
        std::thread::Builder::new()
            .stack_size(512 * 1024)
            .spawn(|| {
                let temp = tempfile::tempdir().unwrap();
                let input = temp.path().join("note.md");
                let output = temp.path().join("note.html");
                fs::write(
                    &input,
                    "$x^2$\n\n```js\nconst x = 1;\n```\n\n```mermaid\nflowchart LR\n A --> B\n```\n",
                )
                .unwrap();
                let mut args = crate::cli::parse([
                    input.to_str().unwrap().to_owned(),
                    output.to_str().unwrap().to_owned(),
                    "--standalone".to_owned(),
                ])
                .unwrap()
                .unwrap();
                args.root = Some(temp.path().to_path_buf());
                compile(&args).unwrap();
                let html = fs::read_to_string(output).unwrap();
                for marker in ["katex", "hljs-keyword", "<svg"] {
                    assert!(html.contains(marker), "missing renderer output: {marker}");
                }
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn figure_parent_image_is_anchored_to_source_outside_preview_directory() {
        let temp = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(temp.path()).unwrap();
        fs::create_dir_all(root.join("notes")).unwrap();
        fs::create_dir_all(root.join("assets/vision")).unwrap();
        let image = root.join("assets/vision/waveform-spectrogram.webp");
        fs::write(&image, b"webp fixture").unwrap();
        let input = root.join("notes/speech.md");
        let output = root.join("Temp/nvim-compile/speech.html");
        fs::write(&input, "<figure>\n  <img src=\"../assets/vision/waveform-spectrogram.webp\">\n  <figcaption>Source: <a href=\"https://speechprocessingbook.aalto.fi/representations/spectrogram-and-the-stft/\">Tom Bäckström</a>, Waveform &amp; Spectrogram</figcaption>\n</figure>\n").unwrap();
        let args = Args {
            site_root: None,
            input,
            output: output.clone(),
            root: Some(root.clone()),
            images: Images::Reference,
            standalone: false,
            asset_dir: Some(root.join("shared")),
            title: None,
        };
        compile(&args).unwrap();
        let html = fs::read_to_string(output).unwrap();
        let url = assets::file_url(&image).unwrap();
        assert!(html.contains(&format!("<img src=\"{url}\">")));
        assert!(!html.contains("src=\"../assets/"));
        assert!(
            html.contains("<figcaption>Source: <a href=\"https://speechprocessingbook.aalto.fi/")
        );
    }

    #[test]
    fn compile_references_images_and_preserves_output_on_error() {
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("note.md");
        let output = temp.path().join("out/note.html");
        fs::write(temp.path().join("image.png"), b"image bytes").unwrap();
        fs::write(
            &input,
            "# Example\n\n$x^2$\n\n![image](image.png)\n\n<figure>\n<img src=\"image.png\">\n</figure>\n",
        )
        .unwrap();
        let mut args = Args {
            site_root: None,
            input: input.clone(),
            output: output.clone(),
            root: Some(input.parent().unwrap().to_path_buf()),
            images: Images::Reference,
            standalone: false,
            asset_dir: Some(temp.path().join("shared")),
            title: Some("<Example>".into()),
        };
        compile(&args).unwrap();
        let shared = fs::read_to_string(&output).unwrap();
        assert!(shared.contains("<title>&lt;Example&gt;</title>"));
        assert!(shared.contains("file:///"));
        // Attachment resolution canonicalizes the path. On Windows that can
        // normalize the temporary directory's path representation, so build
        // the expected URL from the same canonical path.
        let image_path = dunce::canonicalize(temp.path().join("image.png")).unwrap();
        let image_url = assets::file_url(&image_path).unwrap();
        assert_eq!(shared.matches(&format!("src=\"{image_url}\"")).count(), 2);
        assert!(!temp.path().join("out/note.assets").exists());
        assert_eq!(
            fs::read(temp.path().join("image.png")).unwrap(),
            b"image bytes"
        );
        args.standalone = true;
        args.images = Images::Embed;
        compile(&args).unwrap();
        let standalone = fs::read_to_string(&output).unwrap();
        assert!(standalone.contains("data:image/png;base64,"));
        assert_eq!(standalone.matches("data:image/png;base64,").count(), 2);
        assert!(standalone.contains("data:font/woff2;base64,"));
        assert!(!standalone.contains("file:///"));
        assert!(!standalone.contains("<script type=\"module\""));
        fs::write(&input, "# Broken\n\n$\\unknowncommand$\n").unwrap();
        let errors = compile(&args).unwrap_err();
        assert_eq!(errors[0].line, 3);
        assert_eq!(fs::read_to_string(&output).unwrap(), standalone);
        args.output = input.clone();
        assert!(compile(&args).unwrap_err()[0].message.contains("overwrite"));
    }
    #[test]
    fn root_relative_images_and_output_paths() {
        let temp = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(temp.path()).unwrap();
        fs::create_dir_all(root.join("notes/topic")).unwrap();
        fs::create_dir_all(root.join("out")).unwrap();
        fs::write(root.join("a b.png"), b"png").unwrap();
        let mut attachments = Attachments::new(
            &root.join("notes/topic/a.md"),
            &root.join("out/a.html"),
            root.clone(),
            Images::Reference,
            false,
        )
        .unwrap();
        assert_eq!(
            attachments
                .resolve("../../a%20b.png?size=2#test", true)
                .unwrap(),
            format!(
                "{}?size=2#test",
                assets::file_url(&root.join("a b.png")).unwrap()
            )
        );
        assert_eq!(
            attachments.resolve("../../a%20b.png", true).unwrap(),
            assets::file_url(&root.join("a b.png")).unwrap().to_string()
        );
        assert_eq!(
            attachments
                .resolve("license.md?download=1#license", false)
                .unwrap(),
            format!(
                "{}?download=1#license",
                assets::file_url(&root.join("notes/topic/license.md")).unwrap()
            )
        );
        assert!(attachments.resolve("license.md", true).is_err());
        assert!(attachments.resolve("../../../missing.md", false).is_err());
        attachments.mode = Images::Embed;
        assert_eq!(
            attachments.resolve("../../a%20b.png#part", true).unwrap(),
            "data:image/png;base64,cG5n#part"
        );
        assert_eq!(
            attachments
                .resolve("../../a%20b.png?size=2#part", false)
                .unwrap(),
            format!(
                "{}?size=2#part",
                assets::file_url(&root.join("a b.png")).unwrap()
            )
        );
        attachments.root = root.join("notes");
        assert!(
            attachments
                .resolve("../../a%20b.png", true)
                .unwrap_err()
                .contains("outside root")
        );
        for original in [
            "https://example.com/a.png",
            "http://example.com/a.png",
            "//example.com/a.png",
        ] {
            assert_eq!(attachments.resolve(original, true).unwrap(), original);
        }
    }

    #[cfg(unix)]
    #[test]
    fn symlink_cannot_escape_root() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("secret.png"), b"image").unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("linked")).unwrap();
        let attachments = Attachments::new(
            &root.path().join("a.md"),
            &root.path().join("a.html"),
            root.path().to_owned(),
            Images::Reference,
            false,
        )
        .unwrap();
        assert!(
            attachments
                .resolve("linked/missing/note.md", false)
                .unwrap_err()
                .contains("outside root")
        );
        std::os::unix::fs::symlink(root.path().join("absent"), root.path().join("dangling"))
            .unwrap();
        assert!(attachments.resolve("dangling/note.md", false).is_err());
        fs::create_dir(root.path().join("inside")).unwrap();
        std::os::unix::fs::symlink(root.path().join("inside"), root.path().join("alias")).unwrap();
        assert_eq!(
            attachments
                .resolve("alias/missing/note.md#part", false)
                .unwrap(),
            format!(
                "{}#part",
                assets::file_url(&root.path().join("inside/missing/note.md")).unwrap()
            )
        );
        assert!(
            attachments
                .resolve("linked/secret.png", true)
                .unwrap_err()
                .contains("outside root")
        );
    }

    #[test]
    fn static_document_and_diagnostic_positions() {
        let temp = tempfile::tempdir().unwrap();
        let attachments = Attachments::new(
            &temp.path().join("a.md"),
            &temp.path().join("a.html"),
            temp.path().to_owned(),
            Images::Reference,
            false,
        )
        .unwrap();
        let engine = Engine::default();
        let source = "# Hello\n\n$x^2$\n\n```js\nconst x = 1;\n```\n\n```mermaid\nflowchart LR\n A --> B\n```\n\nNote[^n]\n\n[^n]: Visible footnote\n\n<script>alert(1)</script>\n";
        let html = render(source, &engine, &attachments).unwrap();
        assert!(html.contains("katex"));
        assert!(html.contains("hljs-keyword"));
        assert!(html.contains("<svg"));
        assert!(html.contains("Visible footnote"));
        assert!(!html.contains("<script>"));
        assert!(!html.contains(" hidden"));
        let mut warnings = Vec::new();
        let html = render_with_warnings(
            "# Hi\n\n![missing](missing.png)",
            &engine,
            &attachments,
            &mut warnings,
        )
        .unwrap_err();
        assert_eq!(html[0].line, 3);
        assert!(html[0].message.contains("missing.png"));
    }
}
