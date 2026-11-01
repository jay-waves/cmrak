//! Recognized source files bypass Markdown parsing and use the fenced-code CSS.
use std::path::Path;

use crate::{assets::escape, engine::Engine};

pub(crate) fn language(path: &Path) -> Option<&'static str> {
    let name = path.file_name()?.to_str()?.to_ascii_lowercase();
    match name.as_str() {
        "makefile" | "gnumakefile" => return Some("makefile"),
        "dockerfile" | "containerfile" => return Some("dockerfile"),
        "cmakelists.txt" => return Some("cmake"),
        ".bashrc" | ".bash_profile" | ".zshrc" => return Some("bash"),
        _ => {}
    }
    Some(
        match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "c" | "h" => "c",
            "cc" | "cpp" | "cxx" | "hh" | "hpp" | "hxx" => "cpp",
            "bib" => "bibtex",
            "py" | "pyw" | "pyi" => "python",
            "lua" => "lua",
            "sh" | "bash" | "zsh" => "bash",
            "js" | "mjs" | "cjs" | "jsx" => "javascript",
            "ts" | "mts" | "cts" | "tsx" => "typescript",
            "rs" => "rust",
            "go" => "go",
            "java" => "java",
            "kt" | "kts" => "kotlin",
            "cs" => "csharp",
            "rb" | "rake" => "ruby",
            "php" => "php",
            "pl" | "pm" => "perl",
            "r" => "r",
            "swift" => "swift",
            "sql" => "sql",
            "json" | "jsonc" => "json",
            "yaml" | "yml" => "yaml",
            "toml" | "ini" | "cfg" | "conf" => "ini",
            "xml" | "svg" | "html" | "htm" | "xhtml" => "xml",
            "css" => "css",
            "scss" => "scss",
            "less" => "less",
            "tex" | "sty" | "cls" => "latex",
            "typ" => "typst",
            "ps1" | "psm1" | "psd1" => "powershell",
            "cmake" => "cmake",
            "mk" => "makefile",
            "dockerfile" => "dockerfile",
            "diff" | "patch" => "diff",
            "graphql" | "gql" => "graphql",
            "http" => "http",
            "txt" => "plaintext",
            _ => return None,
        },
    )
}

pub(crate) fn render(source: &str, language: &str, engine: &Engine) -> Result<String, String> {
    let html = engine
        .render("code", source, language, 0)?
        .unwrap_or_else(|| escape(source));
    Ok(format!(
        "<div class=\"code-wrap\" data-line=\"1\"><pre><code class=\"hljs language-{}\">{html}</code></pre></div>\n",
        escape(language)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_extensions_and_special_names() {
        for (name, expected) in [
            ("main.c", "c"),
            ("api.h", "c"),
            ("API.HPP", "cpp"),
            ("refs.bib", "bibtex"),
            ("main.PY", "python"),
            ("init.lua", "lua"),
            ("build.sh", "bash"),
            ("Makefile", "makefile"),
            ("Dockerfile", "dockerfile"),
            ("CMakeLists.txt", "cmake"),
            (".zshrc", "bash"),
        ] {
            assert_eq!(language(Path::new(name)), Some(expected), "{name}");
        }
        for name in [
            "note.md",
            "note.markdown",
            "image.png",
            "unknown",
            "data.bin",
        ] {
            assert_eq!(language(Path::new(name)), None, "{name}");
        }
    }
}
