use crate::{
    assets::{self, escape},
    cli::{Args, Images},
    engine::Engine,
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
pub(crate) struct GraphIds {
    occurrences: HashMap<String, u32>,
    used: HashSet<u32>,
}

impl GraphIds {
    pub(crate) fn for_identity(&mut self, identity: String) -> usize {
        let occurrence = self.occurrences.entry(identity.clone()).or_default();
        let mut hash = 2166136261u32;
        for byte in identity.bytes().chain(occurrence.to_le_bytes()) {
            hash = (hash ^ u32::from(byte)).wrapping_mul(16777619);
        }
        *occurrence += 1;
        while !self.used.insert(hash) {
            hash = hash.wrapping_add(1);
        }
        hash as usize
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
    allow_missing_images: bool,
    missing_images: RefCell<Vec<String>>,
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
            allow_missing_images: false,
            missing_images: RefCell::default(),
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

    pub(crate) fn track(&self, original: &str) {
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

    pub(crate) fn chart_module(&self, source: &str) -> Result<String, String> {
        use sha2::{Digest, Sha256};
        let Some(site) = &self.site_root else {
            return Ok(assets::data_url(source.as_bytes(), "text/javascript"));
        };
        let mut directory = site.clone();
        for component in ["_shared", "cmrak", "charts"] {
            directory.push(component);
            fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
            directory = dunce::canonicalize(directory).map_err(|e| e.to_string())?;
            if !directory.starts_with(site) {
                return Err("chart assets directory is outside site root".into());
            }
        }
        let path = directory.join(format!("{:x}.mjs", Sha256::digest(source.as_bytes())));
        if fs::read(&path).ok().as_deref() != Some(source.as_bytes()) {
            assets::atomic_write(&path, source.as_bytes())?;
        }
        Ok(assets::file_url(&dunce::canonicalize(path).map_err(|e| e.to_string())?)?.to_string())
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
                allow_missing_images: false,
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

    pub(crate) fn take_image_warnings(&self) -> Vec<String> {
        self.missing_images
            .borrow_mut()
            .drain(..)
            .map(|original| {
                format!("attachment {original:?} is missing; preserving image reference")
            })
            .collect()
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
        let mut missing = false;
        let path = match dunce::canonicalize(&path) {
            Ok(path) => path,
            // Links do not read their target; allow documents to reference files
            // that will be supplied later. Images still require an existing file.
            Err(error)
                if (!image || self.allow_missing_images)
                    && error.kind() == std::io::ErrorKind::NotFound =>
            {
                missing = image;
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
        if missing {
            self.missing_images.borrow_mut().push(original.to_owned());
            let mut target = assets::file_url(&path)?;
            target.set_query(url.query());
            target.set_fragment(url.fragment());
            return Ok(target.to_string());
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
    crate::markdown_render::render(source, engine, attachments, warnings)
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
            .name("cmrak-compiler".into())
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
        let lock_dir = site.join("_shared/cmrak/.locks");
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
        .map(|site| site.join("_shared/cmrak"));
    let asset_dir = site_assets.as_deref().or(args.asset_dir.as_deref());
    let mut attachments =
        Attachments::new(&input, &output, root, args.images, args.standalone).map_err(failure)?;
    attachments.allow_missing_images = args.allow_missing_images;
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
        assets::viz_runtime(args.standalone, asset_dir, site_assets.is_some()).map_err(failure)?
    } else {
        String::new()
    };
    let title = args.title.as_deref().unwrap_or_else(|| {
        input
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Document")
    });
    let navigation = if let Some(directory) = site_assets.as_deref() {
        let path = dunce::canonicalize(
            directory
                .join(include_str!("../generated/version.txt").trim())
                .join("navigation.js"),
        )
        .map_err(failure)?;
        format!(
            "<script src=\"{}\"></script>",
            escape(assets::file_url(&path).map_err(failure)?.as_str())
        )
    } else {
        format!(
            "<script>{}</script>",
            include_str!("../generated/navigation.js")
        )
    };
    let html = format!(
        "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><meta name=\"generator\" content=\"cmrak {}\"><title>{}</title>\n{stylesheet}\n{viz_style}\n{mermaid_style}\n{viz_runtime}\n</head><body><main id=\"content\">\n{body}</main>{navigation}</body></html>\n",
        env!("CARGO_PKG_VERSION"),
        escape(title)
    );
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
    fn missing_image_warnings_point_to_each_use() {
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("note.md");
        fs::write(&input, "missing.png is mentioned here.\n\n![one](missing.png)\n\n![two](missing.png)\n\n<img src=\"a&amp;b.png\">\n\n<img srcset=\"small.png 1x, large.png 2x\">\n").unwrap();
        let mut args = crate::cli::parse([
            input.to_string_lossy().into_owned(),
            "--root".into(),
            temp.path().to_string_lossy().into_owned(),
            "--allow-missing-images".into(),
        ])
        .unwrap()
        .unwrap();
        let warnings = compile_document(&args, &Engine::default(), Rc::default()).unwrap();
        assert_eq!(
            warnings
                .iter()
                .map(|d| (d.line, d.column))
                .collect::<Vec<_>>(),
            [(3, 1), (5, 1), (7, 1), (9, 1), (9, 1)]
        );
        assert!(warnings.iter().all(|d| d.message.contains("is missing")));
        assert!(warnings[2].message.contains("a&b.png"));
        let html = fs::read(&args.output).unwrap();
        args.allow_missing_images = false;
        assert!(compile_document(&args, &Engine::default(), Rc::default()).is_err());
        assert_eq!(fs::read(&args.output).unwrap(), html);
    }

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
                allow_missing_images: false,
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
            allow_missing_images: false,
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
            allow_missing_images: false,
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
    fn fragment_errors_point_into_markdown_source() {
        let temp = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(temp.path()).unwrap();
        let attachments = Attachments::new(
            &root.join("note.md"),
            &root.join("out.html"),
            root.clone(),
            Images::Reference,
            false,
        )
        .unwrap();
        let engine = Engine::default();
        for (source, expected) in [
            (
                "before\n\n<mermaid>\nsequenceDiagram\nAlice ->> Bob: hi\nfoo\n</mermaid>\n",
                6,
            ),
            (
                "before\n\n```mermaid\nsequenceDiagram\nAlice ->> Bob: hi\nfoo\n```\n",
                6,
            ),
            ("before\n\n$x + \\invalidcommand$\n", 3),
            (
                "> ```mermaid\n> sequenceDiagram\n> Alice ->> Bob: hi\n> foo\n> ```\n",
                4,
            ),
        ] {
            let errors = render(source, &engine, &attachments).unwrap_err();
            assert_eq!(errors[0].line, expected, "{errors:?}");
            assert!(errors[0].column > 1, "{errors:?}");
            assert!(!errors[0].message.contains('\u{1e}'));
            if source.contains("mermaid") {
                assert!(errors[0].message.starts_with("Pele syntax: "));
                assert!(
                    errors[0]
                        .message
                        .lines()
                        .skip(1)
                        .all(|line| line.starts_with("  "))
                );
            }
        }
        fs::write(
            root.join("broken.mmd"),
            "sequenceDiagram\nAlice ->> Bob: hi\nfoo\n",
        )
        .unwrap();
        let errors = render(
            "<mermaid src=\"broken.mmd\"></mermaid>\n",
            &engine,
            &attachments,
        )
        .unwrap_err();
        let first = errors[0].message.lines().next().unwrap();
        assert!(
            first.contains("broken.mmd") && first.contains("fragment line 3, column 4"),
            "{errors:?}"
        );
        assert!(
            errors[0]
                .message
                .lines()
                .skip(1)
                .all(|line| line.starts_with("  "))
        );
        let errors = render(
            "<mermaid>xychart-beta\ntitle test</mermaid>\n",
            &engine,
            &attachments,
        )
        .unwrap_err();
        assert!(
            errors[0].message.starts_with("Pele semantic: "),
            "{errors:?}"
        );
    }

    #[test]
    fn native_graphs_preserve_context_and_diagnostics() {
        let temp = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(temp.path()).unwrap();
        let attachments = Attachments::new(
            &root.join("note.md"),
            &root.join("out.html"),
            root,
            Images::Embed,
            true,
        )
        .unwrap();
        let engine = Engine::default();
        for source in [
            "```html\n<mermaid>\n```\n\n`<mermaid>`\n\n<mermaid>\nflowchart LR\n\n A --> B\n</mermaid>\n<mermaid>\nflowchart LR\n X --> Y\n</mermaid>\n\nAfter\n",
            "> <mermaid>\n> flowchart LR\n>\n> A --> B\n> </mermaid>\n> <mermaid>\n> flowchart LR\n> X --> Y\n> </mermaid>\n>\n> After\n",
            "- Before\n\n  <mermaid>\n  flowchart LR\n\n  A --> B\n  </mermaid>\n  <mermaid>\n  flowchart LR\n  X --> Y\n  </mermaid>\n\n  After\n",
        ] {
            let html = render(source, &engine, &attachments).unwrap();
            assert_eq!(html.matches("<svg").count(), 2, "{html}");
            assert!(html.contains("After"));
            assert!(!html.contains("cmrak-graph"));
        }
        let errors = render(
            "Before\n\n<mermaid>\nflowchart LR\n A --> B\n",
            &engine,
            &attachments,
        )
        .unwrap_err();
        assert_eq!((errors[0].line, errors[0].column), (3, 1));
        assert!(errors[0].message.contains("</mermaid>"));
        let errors = render(
            "<mermaid>flowchart LR; A --> B</mermaid> trailing\n",
            &engine,
            &attachments,
        )
        .unwrap_err();
        assert!(errors[0].message.contains("own Markdown block"));
        let errors = render(
            "<mermaid>\nflowchart LR\n\n A --> B\n</mermaid>\n\n![missing](missing.png)\n",
            &engine,
            &attachments,
        )
        .unwrap_err();
        assert_eq!(errors[0].line, 7);
    }

    #[test]
    fn inline_mermaid_preserves_blank_lines_and_literal_html() {
        let temp = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(temp.path()).unwrap();
        let attachments = Attachments::new(
            &root.join("note.md"),
            &root.join("out.html"),
            root,
            Images::Embed,
            true,
        )
        .unwrap();
        let source = "Before\n\n<mermaid caption=\"A &amp; B\">\nflowchart LR\n A[\"A & B<br/>next\"]\n\n A --> C\n\n C --> D\n</mermaid>\n\nAfter\n";
        let html = render(source, &Engine::default(), &attachments).unwrap();
        assert_eq!(html.matches("<svg").count(), 1);
        assert!(html.contains("Before"));
        assert!(html.contains("After"));
        assert!(html.contains("<figcaption>A &amp; B</figcaption>"));
        assert!(!html.contains("<mermaid"));
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
        assert!(html.contains("/chartist.css\""));
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
            allow_missing_images: false,
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
            allow_missing_images: false,
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
