use cmrak::{cli, document};
use std::{fs, io::Write, path::PathBuf};

fn compile_svg(arguments: &[String]) -> Result<(), String> {
    // Like document::compile, reserve a native stack larger than QuickJS's
    // 4 MiB budget. Standalone SVG compilation also initializes the JS bundles.
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("cmrak-svg-compiler".into())
            .stack_size(16 * 1024 * 1024)
            .spawn_scoped(scope, || compile_svg_on_worker(arguments))
            .map_err(|error| format!("cannot start SVG compiler thread: {error}"))?
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
    })
}

fn compile_svg_on_worker(arguments: &[String]) -> Result<(), String> {
    let mut positional = Vec::new();
    let mut root = None;
    let mut args = arguments.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--root" => {
                root = Some(PathBuf::from(
                    args.next().ok_or("--root requires a directory")?,
                ))
            }
            "-h" | "--help" => {
                println!(
                    "Usage: cmrak svg INPUT [OUTPUT] [--root DIR]\n\nCompile a .mmd diagram or Chartist .js module to standalone SVG."
                );
                return Ok(());
            }
            flag if flag.starts_with('-') => return Err(format!("unknown svg option {flag}")),
            path => positional.push(PathBuf::from(path)),
        }
    }
    if !(1..=2).contains(&positional.len()) {
        return Err("usage: cmrak svg INPUT [OUTPUT] [--root DIR]".into());
    }
    let input = &positional[0];
    let input =
        fs::canonicalize(input).map_err(|e| format!("cannot read {}: {e}", input.display()))?;
    let root =
        fs::canonicalize(root.unwrap_or(std::env::current_dir().map_err(|e| e.to_string())?))
            .map_err(|e| format!("invalid --root: {e}"))?;
    if !input.starts_with(&root) {
        return Err("input is outside --root".into());
    }
    let output = positional
        .get(1)
        .cloned()
        .unwrap_or_else(|| input.with_extension("svg"));
    if fs::canonicalize(&output).ok().as_ref() == Some(&input) {
        return Err("output would overwrite the input".into());
    }
    let extension = input.extension().and_then(|ext| ext.to_str()).unwrap_or("");
    let source =
        fs::read_to_string(&input).map_err(|e| format!("cannot read {}: {e}", input.display()))?;
    let svg = match extension.to_ascii_lowercase().as_str() {
        "mmd" => cmrak::engine::Engine::default()
            .render("diagram", &source, "", 0)
            .map_err(|e| format!("Mermaid: {e}"))?
            .ok_or("Mermaid returned no SVG")?,
        "js" => cmrak::engine::Engine::default()
            .render_chart_svg(&source)
            .map_err(|e| format!("Chartist: {e}"))?,
        _ => return Err("cmrak svg accepts only .mmd and .js input files".into()),
    };

    // Standalone SVGs cannot inherit the HTML document's theme.
    let svg = if extension.eq_ignore_ascii_case("mmd") {
        svg.replacen("style=\"", "style=\"color-scheme:light dark;--pele-bg:light-dark(#fdfdfc,#212830);--pele-fg:light-dark(#363733,#f0f6fc);--pele-muted:light-dark(#73746e,#9198a1);--pele-line:light-dark(#73746e,#9198a1);--pele-surface:light-dark(#f4f5f4,#262c36);--pele-surface-alt:light-dark(#e9ecea,#3d444d);--pele-border:light-dark(#73746e,#9198a1);--pele-accent:light-dark(#777c78,#478be6);", 1)
    } else {
        svg
    };
    // Replace only after a complete write; preserve existing output on failure
    // and do not follow output hard links when writing SVG bytes.
    let output = std::path::absolute(&output).map_err(|e| e.to_string())?;
    let parent = output.parent().ok_or("output must be a file")?;
    let write = || -> std::io::Result<()> {
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        temporary.write_all(svg.as_bytes())?;
        temporary.persist(&output).map_err(|e| e.error)?;
        Ok(())
    };
    write().map_err(|e| format!("cannot write {}: {e}", output.display()))
}

fn is_source_diagnostic(message: &str) -> bool {
    regex::Regex::new(r"^.+:[0-9]+:[0-9]+: (?:error|warning):")
        .unwrap()
        .is_match(message)
}

fn main() -> std::process::ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let result = match arguments.first().map(String::as_str) {
        Some("svg") => Some(compile_svg(&arguments[1..])),
        Some("watch") => Some(Err("watch has been removed; use serve".into())),
        Some("preview") => Some(Err("unknown command preview; use serve".into())),
        Some("serve")
            if arguments.get(1).is_some_and(|arg| !arg.starts_with('-'))
                || arguments
                    .iter()
                    .skip(1)
                    .any(|arg| arg == "--root" || arg.starts_with("--root=")) =>
        {
            Some(cmrak::site_preview::compile(arguments[1..].to_vec()))
        }
        Some("stop") if arguments.len() == 1 => Some(cmrak::site_preview::stop()),
        Some("__site_preview_server") if arguments.len() == 3 => Some(
            arguments[2]
                .parse::<u16>()
                .map_err(|e| e.to_string())
                .and_then(|port| {
                    cmrak::site_preview::serve(std::path::Path::new(&arguments[1]), port)
                }),
        ),
        Some("serve")
            if arguments
                .get(1)
                .is_some_and(|arg| arg == "--help" || arg == "-h") =>
        {
            println!(
                "Usage: cmrak serve INPUT [--root DIR] [--fragment FRAGMENT]\n       cmrak serve [--site-root DIR] [--port PORT]"
            );
            Some(Ok(()))
        }
        Some("serve") => {
            let mut dir = std::path::PathBuf::from(".");
            let mut port = 8765;
            let mut i = 1;
            let mut error = None;
            while i < arguments.len() {
                let flag = &arguments[i];
                i += 1;
                if let Some(value) = arguments.get(i) {
                    match flag.as_str() {
                        "--site-root" | "--dir" => dir = value.into(),
                        "--port" => match value.parse::<u16>() {
                            Ok(n) => port = n,
                            Err(e) => error = Some(e.to_string()),
                        },
                        _ => error = Some(format!("unknown option {flag}")),
                    };
                    i += 1;
                } else {
                    error = Some(format!("missing value for {flag}"));
                    break;
                }
            }
            Some(if let Some(error) = error {
                Err(error)
            } else {
                cmrak::site_preview::serve(&dir, port)
            })
        }
        _ => None,
    };
    if let Some(result) = result {
        return match result {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(error) => {
                if is_source_diagnostic(&error) {
                    eprintln!("{error}");
                } else {
                    eprintln!("error: {error}");
                }
                std::process::ExitCode::FAILURE
            }
        };
    }
    let args = match cli::parse(std::env::args().skip(1)) {
        Ok(Some(args)) => args,
        Ok(None) => return std::process::ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            return std::process::ExitCode::FAILURE;
        }
    };
    match document::compile(&args) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(diagnostics) => {
            for diagnostic in diagnostics {
                eprintln!(
                    "{}:{}:{}: error: {}",
                    args.input.display(),
                    diagnostic.line,
                    diagnostic.column,
                    diagnostic.message
                );
            }
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn svg_output_preserves_sources_and_previous_output() {
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("flow.mmd");
        let output = temp.path().join("flow.svg");
        let source = "flowchart LR\n Start --> End\n";
        let compile = |output: &std::path::Path| {
            super::compile_svg(&[
                input.to_string_lossy().into_owned(),
                output.to_string_lossy().into_owned(),
                "--root".into(),
                temp.path().to_string_lossy().into_owned(),
            ])
        };
        std::fs::write(&input, source).unwrap();
        assert!(compile(&input).unwrap_err().contains("overwrite the input"));
        assert_eq!(std::fs::read_to_string(&input).unwrap(), source);

        #[cfg(unix)]
        {
            let alias = temp.path().join("alias.svg");
            std::os::unix::fs::symlink(&input, &alias).unwrap();
            assert!(compile(&alias).unwrap_err().contains("overwrite the input"));
        }
        std::fs::hard_link(&input, &output).unwrap();
        compile(&output).unwrap();
        assert_eq!(std::fs::read_to_string(&input).unwrap(), source);
        let previous = std::fs::read(&output).unwrap();
        assert!(String::from_utf8_lossy(&previous).contains("<svg"));
        std::fs::write(&input, "not a diagram").unwrap();
        assert!(compile(&output).is_err());
        assert_eq!(std::fs::read(&output).unwrap(), previous);
    }

    #[test]
    fn svg_compilation_does_not_depend_on_callers_stack_size() {
        std::thread::Builder::new()
            .stack_size(512 * 1024)
            .spawn(|| {
                let temp = tempfile::tempdir().unwrap();
                for (name, source, marker) in [
                    ("flow.mmd", "flowchart LR\n Start --> End\n", "Start"),
                    (
                        "sequence.mmd",
                        "sequenceDiagram\n Alice->>Bob: Hello\n",
                        "Alice",
                    ),
                    (
                        "chart.js",
                        include_str!("../examples/viz/bars.js"),
                        "ct-bar",
                    ),
                ] {
                    let input = temp.path().join(name);
                    std::fs::write(&input, source).unwrap();
                    super::compile_svg(&[
                        input.to_str().unwrap().to_owned(),
                        "--root".into(),
                        temp.path().to_str().unwrap().to_owned(),
                    ])
                    .unwrap();
                    let svg = std::fs::read_to_string(input.with_extension("svg")).unwrap();
                    assert!(svg.contains("<svg") && svg.contains(marker), "{name}");
                }
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn source_diagnostics_remain_parseable() {
        assert!(super::is_source_diagnostic(
            "C:\\notes\\a.md:12:3: error: invalid"
        ));
        assert!(!super::is_source_diagnostic("cannot start server"));
    }
}
