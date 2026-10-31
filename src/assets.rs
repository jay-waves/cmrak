use base64::{Engine, engine::general_purpose::STANDARD};
use std::{
    fs,
    path::{Path, PathBuf},
};
use url::Url;

include!(concat!(env!("OUT_DIR"), "/assets.rs"));
const VERSION: &str = include_str!("../generated/version.txt");
const KATEX: &str = include_str!("../generated/katex.css");
const STYLE: &str = include_str!("../generated/style.css");

pub fn escape(value: &str) -> String {
    value
        .chars()
        .fold(String::with_capacity(value.len()), |mut html, ch| {
            match ch {
                '&' => html.push_str("&amp;"),
                '<' => html.push_str("&lt;"),
                '>' => html.push_str("&gt;"),
                '"' => html.push_str("&quot;"),
                '\'' => html.push_str("&#39;"),
                _ => html.push(ch),
            }
            html
        })
}

pub fn file_url(path: &Path) -> Result<Url, String> {
    Url::from_file_path(path).map_err(|_| format!("cannot form file URL for {}", path.display()))
}

pub fn data_url(bytes: &[u8], mime: &str) -> String {
    format!("data:{mime};base64,{}", STANDARD.encode(bytes))
}

pub fn shared_base() -> Result<PathBuf, String> {
    #[cfg(target_os = "windows")]
    let base = Some(windows_local_app_data()?);
    #[cfg(target_os = "macos")]
    let base = std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Library/Caches"));
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")));
    application_assets(base)
}

fn application_assets(base: Option<PathBuf>) -> Result<PathBuf, String> {
    base.filter(|path| path.is_absolute())
        .map(|base| base.join("lib.comrak").join("assets"))
        .ok_or_else(|| {
            "cannot locate an absolute user resource directory; pass --asset-dir or --standalone"
                .into()
        })
}

#[cfg(windows)]
fn windows_local_app_data() -> Result<PathBuf, String> {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt, ptr};
    use windows_sys::Win32::{
        System::Com::CoTaskMemFree,
        UI::Shell::{FOLDERID_LocalAppData, KF_FLAG_DONT_VERIFY, SHGetKnownFolderPath},
    };

    let mut path = ptr::null_mut();
    // Ask Windows for the current user's configured folder, including any
    // redirection. Do not trust LOCALAPPDATA, guess a profile path, or fall
    // back to the executable/current/temporary directory. Querying creates
    // nothing; resource extraction creates only our application subdirectory.
    // SAFETY: Valid GUID and writable out pointer; a null token means current user.
    let status = unsafe {
        SHGetKnownFolderPath(
            &FOLDERID_LocalAppData,
            KF_FLAG_DONT_VERIFY as u32,
            ptr::null_mut(),
            &mut path,
        )
    };
    let result = if status < 0 || path.is_null() {
        Err(format!(
            "cannot locate Windows LocalAppData (HRESULT 0x{:08X}); pass --asset-dir or --standalone",
            status as u32
        ))
    } else {
        // SAFETY: On success the API returns a NUL-terminated UTF-16 allocation
        // that remains valid until CoTaskMemFree below. Preserve Unicode paths.
        let value = unsafe {
            let mut length = 0;
            while *path.add(length) != 0 {
                length += 1;
            }
            OsString::from_wide(std::slice::from_raw_parts(path, length))
        };
        Ok(PathBuf::from(value))
    };
    // SAFETY: The API requires freeing the out allocation even on failure.
    // CoTaskMemFree accepts null. The returned PathBuf owns its own copy.
    unsafe { CoTaskMemFree(path.cast()) };
    result
}

#[cfg(test)]
pub fn stylesheet(standalone: bool, directory: Option<&Path>) -> Result<String, String> {
    stylesheet_for_math(standalone, directory, true)
}

pub fn stylesheet_for_math(
    standalone: bool,
    directory: Option<&Path>,
    math: bool,
) -> Result<String, String> {
    if standalone {
        let mut css = STYLE.to_owned();
        if math {
            css.push_str(KATEX);
        }
        for (name, bytes) in ASSETS.iter().filter(|(name, _)| name.starts_with("fonts/")) {
            let mime = if name.ends_with(".otf") {
                "font/otf"
            } else {
                "font/woff2"
            };
            css = css.replace(name, &data_url(bytes, mime));
        }
        return Ok(format!("<style>{css}</style>"));
    }
    let directory = directory
        .map(Path::to_path_buf)
        .map_or_else(shared_base, Ok)?
        .join(VERSION.trim());
    for (name, bytes) in ASSETS {
        let destination = directory.join(name);
        // Repair incomplete or modified resources; old versions stay available.
        if fs::read(&destination).ok().as_deref() != Some(*bytes) {
            fs::create_dir_all(destination.parent().unwrap()).map_err(|e| {
                format!(
                    "cannot create shared resource directory {}: {e}",
                    destination.parent().unwrap().display()
                )
            })?;
            atomic_write(&destination, bytes).map_err(|e| {
                format!(
                    "cannot install shared resource {}: {e}",
                    destination.display()
                )
            })?;
        }
    }
    let absolute = dunce::canonicalize(directory.join("style.css")).map_err(|e| e.to_string())?;
    let mut links = format!(
        "<link rel=\"stylesheet\" href=\"{}\">",
        escape(file_url(&absolute)?.as_str())
    );
    if math {
        links.push_str(&format!(
            "<link rel=\"stylesheet\" href=\"{}\">",
            escape(file_url(&directory.join("katex.css"))?.as_str())
        ));
    }
    Ok(links)
}

pub fn viz_stylesheet(standalone: bool, directory: Option<&Path>) -> Result<String, String> {
    if standalone {
        return Ok(format!(
            "<style>{}</style>",
            include_str!("../generated/viz.css")
        ));
    }
    let directory = directory
        .map(Path::to_path_buf)
        .map_or_else(shared_base, Ok)?;
    let path = dunce::canonicalize(directory.join(VERSION.trim()).join("viz.css"))
        .map_err(|e| e.to_string())?;
    Ok(format!(
        "<link rel=\"stylesheet\" href=\"{}\">",
        escape(file_url(&path)?.as_str())
    ))
}

pub fn mermaid_stylesheet(standalone: bool, directory: Option<&Path>) -> Result<String, String> {
    if standalone {
        return Ok(format!(
            "<style>{}</style>",
            include_str!("../generated/mermaid.css")
        ));
    }
    let directory = directory
        .map(Path::to_path_buf)
        .map_or_else(shared_base, Ok)?;
    let path = dunce::canonicalize(directory.join(VERSION.trim()).join("mermaid.css"))
        .map_err(|e| e.to_string())?;
    Ok(format!(
        "<link rel=\"stylesheet\" href=\"{}\">",
        escape(file_url(&path)?.as_str())
    ))
}

pub fn viz_runtime(standalone: bool, directory: Option<&Path>) -> Result<String, String> {
    let source = if standalone {
        data_url(include_bytes!("../generated/viz.js"), "text/javascript")
    } else {
        let directory = directory
            .map(Path::to_path_buf)
            .map_or_else(shared_base, Ok)?;
        file_url(
            &dunce::canonicalize(directory.join(VERSION.trim()).join("viz.js"))
                .map_err(|e| e.to_string())?,
        )?
        .to_string()
    };
    let module = data_url(
        include_bytes!("../generated/chartist-module.js"),
        "text/javascript",
    );
    let server = data_url(
        include_bytes!("../generated/chartist-server-module.js"),
        "text/javascript",
    );
    Ok(format!(
        "<script src=\"{}\"></script><script type=\"importmap\">{{\"imports\":{{\"chartist\":\"{module}\",\"chartist/server\":\"{server}\"}}}}</script>",
        escape(&source)
    ))
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let name = path
        .file_name()
        .ok_or("output must be a file")?
        .to_string_lossy();
    let temporary = path.with_file_name(format!(
        ".{name}.{}-{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|e| e.to_string())?;
        file.write_all(bytes).map_err(|e| e.to_string())?;
        drop(file);
        fs::rename(&temporary, path).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_escaping_preserves_unicode_and_escapes_once() {
        assert_eq!(
            escape("中文<&>\"'&amp;"),
            "中文&lt;&amp;&gt;&quot;&#39;&amp;amp;"
        );
    }

    #[test]
    fn default_resource_base_never_uses_relative_or_empty_paths() {
        for base in [
            None,
            Some(PathBuf::new()),
            Some(PathBuf::from("relative-cache")),
        ] {
            assert!(application_assets(base).is_err());
        }
        let temp = tempfile::tempdir().unwrap();
        assert_eq!(
            application_assets(Some(temp.path().to_owned())).unwrap(),
            temp.path().join("lib.comrak/assets")
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_defaults_to_the_current_users_known_folder() {
        let local = windows_local_app_data().unwrap();
        assert!(local.is_absolute());
        assert_eq!(
            shared_base().unwrap(),
            local.join("lib.comrak").join("assets")
        );
        // A drive-relative path must never turn a default cache into a write
        // relative to whichever directory happens to be active on that drive.
        assert!(application_assets(Some(PathBuf::from(r"C:cache"))).is_err());
        assert!(application_assets(Some(PathBuf::from(r"\cache"))).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn windows_file_urls_preserve_unicode_and_support_redirected_paths() {
        for path in [
            r"C:\Users\张 三\AppData\Local\lib.comrak\assets\v1\style.css",
            r"\\?\C:\Users\张 三\AppData\Local\lib.comrak\assets\v1\style.css",
            r"\\server\profiles\张 三\Local\lib.comrak\assets\v1\style.css",
        ] {
            let url = file_url(Path::new(path)).unwrap();
            assert_eq!(url.scheme(), "file");
            assert!(!url.as_str().contains(' '));
            assert!(!url.as_str().contains('\\'));
            assert!(
                url.to_file_path()
                    .unwrap()
                    .ends_with(r"lib.comrak\assets\v1\style.css")
            );
        }
    }

    #[test]
    fn font_rules_survive_packaging_and_standalone_has_no_font_files() {
        let fonts = ASSETS
            .iter()
            .filter(|(name, _)| name.starts_with("fonts/"))
            .count();
        assert!(fonts > 10);
        assert_eq!(KATEX.matches("@font-face").count(), fonts);
        assert!(!STYLE.contains(".woff)"));
        assert!(!STYLE.contains(".ttf)"));
        let css = stylesheet(true, None).unwrap();
        assert_eq!(
            css.matches("data:font/woff2;base64,").count()
                + css.matches("data:font/otf;base64,").count(),
            fonts
        );
        assert_eq!(css.matches("data:font/otf;base64,").count(), 0);
        assert!(!css.contains("url(fonts/"));
    }

    #[test]
    fn pages_without_math_omit_katex_and_fonts() {
        let css = stylesheet_for_math(true, None, false).unwrap();
        assert!(!css.contains("data:font/"));
        assert!(!css.contains("@font-face"));
        assert!(css.len() < 30_000);
        let temp = tempfile::tempdir().unwrap();
        let links = stylesheet_for_math(false, Some(temp.path()), false).unwrap();
        assert!(!links.contains("katex.css"));
        let links = stylesheet_for_math(false, Some(temp.path()), true).unwrap();
        assert!(links.contains("katex.css"));
    }

    #[test]
    fn shared_resources_are_versioned_and_repaired() {
        let temp = tempfile::tempdir().unwrap();
        let link = stylesheet(false, Some(temp.path())).unwrap();
        assert!(link.contains("file:///"));
        let path = temp.path().join(VERSION.trim()).join("style.css");
        assert_eq!(fs::read_to_string(&path).unwrap(), STYLE);
        fs::write(&path, "damaged").unwrap();
        assert_eq!(stylesheet(false, Some(temp.path())).unwrap(), link);
        assert_eq!(fs::read_to_string(path).unwrap(), STYLE);
    }
}
