//! Materialize local references using the notebook's directory layout.
use crate::assets;
use std::{fs, path::Path};

pub(crate) fn page_path(path: &Path) -> std::path::PathBuf {
    path.with_extension("").join("index.html")
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
    let mut error = None;
    let result = pattern.replace_all(html, |captures: &regex::Captures<'_>| {
        let original = &captures[0];
        let mapped = (|| -> Result<String, String> {
            let url =
                url::Url::parse(&original.replace("&amp;", "&")).map_err(|e| e.to_string())?;
            let source = url.to_file_path().map_err(|_| "invalid local URL")?;
            let destination = if source.starts_with(&shared) {
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
        assert!(html.contains("href=\"../b/index.html?x=1&amp;y=2#part\""));
        assert!(html.contains("href=\"../../other/index.html#top\""));
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
        assert_eq!(args.output, site.join("ai/chatgpt/note/index.html"));
        assert!(html.contains("../../../assets/example.webp"));
        assert!(html.contains("../next/index.html#part"));
        assert!(html.contains("../../../_shared/lib-comrak/"));
        assert!(!html.contains("file:"));
        assert_eq!(
            fs::read(site.join("assets/example.webp")).unwrap(),
            b"image"
        );
        let shared = fs::read_dir(site.join("_shared/lib-comrak"))
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
