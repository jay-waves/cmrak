//! Materialize local references using the notebook's directory layout.
use crate::assets;
use std::{fs, path::Path};

pub(crate) fn page_path(path: &Path) -> std::path::PathBuf {
    path.join("index.html")
}

pub(crate) fn rewrite(
    html: &str,
    input: &Path,
    output: &Path,
    root: &Path,
    site: &Path,
    shared: &Path,
) -> Result<String, String> {
    let site = dunce::canonicalize(site).map_err(|e| e.to_string())?;
    let shared = dunce::canonicalize(shared).map_err(|e| e.to_string())?;
    let page = assets::file_url(output)?;
    let pattern = regex::Regex::new(r#"file://[^\s\"'<>]+"#).unwrap();
    // Code pages are literal source, including any file:// text inside <code>.
    let code_body = if crate::source_code::language(input).is_some() {
        html.find("<main id=\"content\">").zip(html.find("</main>"))
    } else {
        None
    };
    let mut error = None;
    let result = pattern.replace_all(html, |captures: &regex::Captures<'_>| {
        let original = &captures[0];
        if code_body.is_some_and(|(start, end)| {
            captures.get(0).unwrap().start() >= start && captures.get(0).unwrap().start() < end
        }) {
            return original.to_owned();
        }
        let mapped = (|| -> Result<String, String> {
            let url =
                url::Url::parse(&original.replace("&amp;", "&")).map_err(|e| e.to_string())?;
            let source = url.to_file_path().map_err(|_| "invalid local URL")?;
            let destination = if source.starts_with(&shared) || source.starts_with(&site) {
                source.clone()
            } else if source == input {
                output.to_path_buf()
            } else {
                let relative = source
                    .strip_prefix(root)
                    .map_err(|_| "resource outside root")?;
                let target = site.join(relative);
                if source.extension().is_some_and(|ext| {
                    ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown")
                }) {
                    page_path(&target)
                } else {
                    if source.is_file() && source != target {
                        fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
                        let parent = dunce::canonicalize(target.parent().unwrap())
                            .map_err(|e| e.to_string())?;
                        if !parent.starts_with(&site) {
                            return Err("site resource escapes output directory".into());
                        }
                        let bytes = fs::read(&source).map_err(|e| e.to_string())?;
                        if fs::read(&target).ok().as_deref() != Some(bytes.as_slice()) {
                            assets::atomic_write(&target, &bytes)?;
                        }
                    }
                    target
                }
            };
            let mut target = assets::file_url(&destination)?;
            if url.path().ends_with('/') && !target.path().ends_with('/') {
                target.set_path(&format!("{}/", target.path()));
            }
            target.set_query(url.query());
            target.set_fragment(url.fragment());
            Ok(assets::escape(
                &page
                    .make_relative(&target)
                    .ok_or("cannot form relative URL")?,
            ))
        })();
        match mapped {
            Ok(value) => value,
            Err(message) => {
                error = Some(message);
                original.to_owned()
            }
        }
    });
    if let Some(error) = error {
        Err(error)
    } else {
        Ok(result.into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn site_reuses_external_navigation_and_interactive_chart_modules() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let site = root.join("site");
        fs::write(root.join("chart.js"), "import { axisTitle } from '@chartist-js/essentials'; export default c => { c.innerHTML = '<svg></svg>' }; export function hydrate(c) { c.dataset.ready = typeof axisTitle }").unwrap();
        for name in ["a.md", "b.md"] {
            fs::write(root.join(name), "<chartist src=\"chart.js\"></chartist>\n").unwrap();
            let args = crate::cli::parse([
                root.join(name).to_string_lossy().to_string(),
                "--root".into(),
                root.to_string_lossy().to_string(),
                "--site-root".into(),
                site.to_string_lossy().to_string(),
            ])
            .unwrap()
            .unwrap();
            crate::document::compile(&args).unwrap();
            let output = fs::read_to_string(site.join(name).join("index.html")).unwrap();
            assert!(output.contains("/navigation.js\""), "{output}");
            assert!(output.contains("/chartist-module.js\""), "{output}");
            assert!(output.contains("/chartist-server-module.js\""), "{output}");
            assert!(
                output.contains("/chartist-essentials-module.js\""),
                "{output}"
            );
            assert!(output.contains("/charts/"), "{output}");
            assert!(!output.contains("data:text/javascript;base64,"));
            assert!(!output.contains("file:///"));
        }
        assert_eq!(
            fs::read_dir(site.join("_shared/cmrak/charts"))
                .unwrap()
                .count(),
            1
        );
    }

    #[test]
    fn page_paths_keep_complete_names_to_avoid_collisions() {
        assert_eq!(
            page_path(Path::new("_site/example.md")),
            Path::new("_site/example.md/index.html")
        );
        assert_eq!(
            page_path(Path::new("_site/example.py")),
            Path::new("_site/example.py/index.html")
        );
        assert_ne!(
            page_path(Path::new("example.md")),
            page_path(Path::new("example.py"))
        );
    }

    #[test]
    fn code_links_create_pages_and_preserve_urls() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("src/空 格.c"),
            "// file:///outside/not-a-resource\nint main(void) { return 0; }\n",
        )
        .unwrap();
        let input = root.join("note.md");
        fs::write(&input, "[one](src/%E7%A9%BA%20%E6%A0%BC.c?x=1&y=2#part)\n\n[two](/src/%E7%A9%BA%20%E6%A0%BC.c#end)\n\n<a href=\"src/%E7%A9%BA%20%E6%A0%BC.c\">raw</a>\n").unwrap();
        let site = root.join("_site");
        let args = crate::cli::parse([
            "compile".to_owned(),
            input.to_string_lossy().into(),
            "--root".into(),
            root.to_string_lossy().into(),
            "--site-root".into(),
            site.to_string_lossy().into(),
        ])
        .unwrap()
        .unwrap();
        crate::document::compile(&args).unwrap();
        let html = fs::read_to_string(&args.output).unwrap();
        assert!(
            html.contains("../src/%E7%A9%BA%20%E6%A0%BC.c/?x=1&amp;y=2#part"),
            "{html}"
        );
        assert!(html.contains("../src/%E7%A9%BA%20%E6%A0%BC.c/#end"));
        assert!(html.contains(">raw</a>"));
        let code = fs::read_to_string(site.join("src/空 格.c/index.html")).unwrap();
        assert!(code.contains("file:///outside/not-a-resource"));
        assert!(code.contains("hljs-keyword"));
        assert!(site.join("src/空 格.c").is_dir());
    }

    #[test]
    fn missing_code_link_is_an_error_at_reference() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let input = root.join("note.md");
        fs::write(&input, "Intro\n\n[missing](src/missing.c)\n").unwrap();
        let args = crate::cli::parse([
            "compile".to_owned(),
            input.to_string_lossy().into(),
            "--root".into(),
            root.to_string_lossy().into(),
            "--site-root".into(),
            root.join("site").to_string_lossy().into(),
        ])
        .unwrap()
        .unwrap();
        let errors = crate::document::compile(&args).unwrap_err();
        assert_eq!(errors[0].line, 3);
        assert!(errors[0].message.contains("src/missing.c"));
        assert!(errors[0].message.contains("missing.c"));
    }

    #[test]
    fn code_link_rejects_outside_and_invalid_utf8() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        fs::create_dir(&root).unwrap();
        let input = root.join("note.md");
        let outside = temp.path().join("outside.py");
        fs::write(&outside, "print(1)").unwrap();
        let args = crate::cli::parse([
            "compile".to_owned(),
            input.to_string_lossy().into(),
            "--root".into(),
            root.to_string_lossy().into(),
            "--site-root".into(),
            root.join("site").to_string_lossy().into(),
        ])
        .err();
        assert!(args.is_some());
        fs::write(&input, "\n[bad](../outside.py)\n").unwrap();
        let args = crate::cli::parse([
            "compile".to_owned(),
            input.to_string_lossy().into(),
            "--root".into(),
            root.to_string_lossy().into(),
            "--site-root".into(),
            root.join("site").to_string_lossy().into(),
        ])
        .unwrap()
        .unwrap();
        let errors = crate::document::compile(&args).unwrap_err();
        assert_eq!(errors[0].line, 2);
        assert!(errors[0].message.contains("outside.py"));
        assert!(errors[0].message.contains("outside root"));
        fs::write(root.join("bad.py"), [0xff]).unwrap();
        fs::write(&input, "[bad](bad.py)").unwrap();
        let errors = crate::document::compile(&args).unwrap_err();
        assert!(errors[0].message.contains("bad.py"));
        assert!(errors[0].message.contains("UTF-8"));
    }
    #[cfg(unix)]
    #[test]
    fn site_compiles_math_when_output_directory_uses_a_symlink() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        let actual_site = temp.path().join("actual-site");
        let linked_site = temp.path().join("linked-site");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&actual_site).unwrap();
        std::os::unix::fs::symlink(&actual_site, &linked_site).unwrap();
        let input = root.join("test.md");
        fs::write(&input, "$x$").unwrap();
        let args = crate::cli::parse([
            "compile".to_owned(),
            input.to_str().unwrap().into(),
            "--root".into(),
            root.to_str().unwrap().into(),
            "--site-root".into(),
            linked_site.to_str().unwrap().into(),
        ])
        .unwrap()
        .unwrap();
        crate::document::compile(&args).unwrap();
        let html = fs::read_to_string(&args.output).unwrap();
        assert!(html.contains("katex.css"));
        assert!(html.contains("class=\"katex"));
        assert!(!html.contains("file://"));
    }

    #[test]
    fn site_rebases_links_and_resources_from_the_source_directory() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("docs/images")).unwrap();
        fs::write(root.join("docs/images/pic.png"), b"image").unwrap();
        let input = root.join("docs/a.md");
        fs::write(&input, "[next](b.md?x=1&y=2#part)\n\n[other](../other.markdown#top)\n\n[local](#here)\n\n[external](https://example.com/b.md#part)\n\n![image](images/pic.png?size=2#view)\n\n[download](images/pic.png)\n").unwrap();
        let site = root.join("_site");
        let args = crate::cli::parse([
            "compile".to_owned(),
            input.to_str().unwrap().into(),
            "--root".into(),
            root.to_str().unwrap().into(),
            "--site-root".into(),
            site.to_str().unwrap().into(),
        ])
        .unwrap()
        .unwrap();
        crate::document::compile(&args).unwrap();
        let html = fs::read_to_string(&args.output).unwrap();
        assert!(html.contains("href=\"../b.md/index.html?x=1&amp;y=2#part\""));
        assert!(html.contains("href=\"../../other.markdown/index.html#top\""));
        assert!(html.contains("href=\"#here\""));
        assert!(html.contains("href=\"https://example.com/b.md#part\""));
        assert!(html.contains("src=\"../images/pic.png?size=2#view\""));
        assert!(html.contains("href=\"../images/pic.png\""));
        assert_eq!(
            fs::read(site.join("docs/images/pic.png")).unwrap(),
            b"image"
        );
    }
    #[test]
    fn site_preserves_layout_and_reuses_shared_resources() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("ai/chatgpt")).unwrap();
        fs::create_dir_all(root.join("assets")).unwrap();
        fs::write(root.join("assets/example.webp"), b"image").unwrap();
        let input = root.join("ai/chatgpt/note.md");
        fs::write(
            &input,
            "![image](/assets/example.webp)\n\n[next](next.md#part)",
        )
        .unwrap();
        let site = root.join("_site");
        fs::create_dir_all(site.join("_shared")).unwrap();
        fs::write(site.join("_shared/user.txt"), "keep").unwrap();
        let args = crate::cli::parse([
            "compile".to_owned(),
            input.to_str().unwrap().to_owned(),
            "--root".into(),
            root.to_str().unwrap().into(),
            "--site-root".into(),
            site.to_str().unwrap().into(),
        ])
        .unwrap()
        .unwrap();
        assert!(!args.standalone);
        crate::document::compile(&args).unwrap();
        let html = fs::read_to_string(&args.output).unwrap();
        assert_eq!(args.output, site.join("ai/chatgpt/note.md/index.html"));
        assert!(html.contains("../../../assets/example.webp"));
        assert!(html.contains("../next.md/index.html#part"));
        assert!(html.contains("../../../_shared/cmrak/"));
        assert!(!html.contains("file:"));
        assert_eq!(
            fs::read(site.join("assets/example.webp")).unwrap(),
            b"image"
        );
        let shared = fs::read_dir(site.join("_shared/cmrak"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path()
            .join("style.css");
        let modified = fs::metadata(&shared).unwrap().modified().unwrap();
        crate::document::compile(&args).unwrap();
        assert_eq!(fs::metadata(shared).unwrap().modified().unwrap(), modified);
        assert_eq!(
            fs::read_to_string(site.join("_shared/user.txt")).unwrap(),
            "keep"
        );
    }
}
