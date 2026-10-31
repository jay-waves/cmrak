use lib_comrak::{cli, document};

fn is_source_diagnostic(message: &str) -> bool {
    regex::Regex::new(r"^.+:[0-9]+:[0-9]+: (?:error|warning):")
        .unwrap()
        .is_match(message)
}

fn main() -> std::process::ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let result = match arguments.first().map(String::as_str) {
        Some("watch") => Some(Err("watch has been removed; use serve".into())),
        Some("preview") => Some(Err("unknown command preview; use serve".into())),
        Some("serve")
            if arguments.get(1).is_some_and(|arg| !arg.starts_with('-'))
                || arguments
                    .iter()
                    .skip(1)
                    .any(|arg| arg == "--root" || arg.starts_with("--root=")) =>
        {
            Some(lib_comrak::site_preview::compile(arguments[1..].to_vec()))
        }
        Some("stop") if arguments.len() == 1 => Some(lib_comrak::site_preview::stop()),
        Some("__site_preview_server") if arguments.len() == 3 => Some(
            arguments[2]
                .parse::<u16>()
                .map_err(|e| e.to_string())
                .and_then(|port| {
                    lib_comrak::site_preview::serve(std::path::Path::new(&arguments[1]), port)
                }),
        ),
        Some("serve")
            if arguments
                .get(1)
                .is_some_and(|arg| arg == "--help" || arg == "-h") =>
        {
            println!(
                "Usage: comrakk serve INPUT [--root DIR]\n       comrakk serve [--site-root DIR] [--port PORT]"
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
                lib_comrak::site_preview::serve(&dir, port)
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
    fn source_diagnostics_remain_parseable() {
        assert!(super::is_source_diagnostic(
            "C:\\notes\\a.md:12:3: error: invalid"
        ));
        assert!(!super::is_source_diagnostic("cannot start server"));
    }
}
