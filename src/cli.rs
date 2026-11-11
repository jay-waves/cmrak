use std::path::PathBuf;

pub const HELP: &str = r#"cmrak — compile Markdown or source code to static HTML

Usage: cmrak <compile> [OPTIONS] <INPUT> [OUTPUT]
       cmrak svg <INPUT.mmd|INPUT.js> [OUTPUT.svg] [--root DIR]
       cmrak serve [INPUT --root DIR] [--site-root DIR] [--port PORT]
       cmrak stop

Options:
  -o, --output <PATH>                 Output HTML (default: input with .html)
      --root <DIR>                    Allowed local attachment root (default: current directory)
      --images <reference|embed>  Local image handling (default: embed; reference in site mode)
      --allow-missing-images          Warn and continue when local images are missing
      --standalone                    Inline CSS, fonts and local images (default)
      --site-root <DIR>               Site output directory (typically _site); preserve root-relative paths
      --color <never|auto>             Output color (default: never)
      --title <TEXT>                  Document title (default: input filename)
  -h, --help                         Show help
  -V, --version                      Show version

Site resources are versioned under <site-root>/_shared/cmrak. Existing matching files are reused.
Repository and documentation: https://github.com/jay-waves/cmrak
Remote image URLs are preserved in all modes and loaded by the browser.
Recognized source files (C/C++, BibTeX, Python, Lua, shell, JS/TS, Rust, etc.)
are highlighted with highlight.js using the current code theme and fonts.

Chartist charts:
  <chartist src="chart.js" width="75%" caption="Description"></chartist>
  The JS module path is relative to the Markdown file and must be inside --root.
  width and caption are optional. Supported imports are chartist, chartist/server,
  and @chartist-js/essentials; use Chartist layers for common chart features.
  Default-export a synchronous function that renders SVG into its container.
  Set explicit dimensions: compile-time rendering has no browser layout,
  network, canvas or timers. Give the SVG an accessible label.
  Optionally export hydrate(container) to add browser interaction to the existing
  SVG without redrawing it. Verify rendering and interactions in a browser.
  Choose a Paul Tol Muted series color with className: 'tol-muted-rose'. Names:
  indigo, cyan, teal, green, olive, sand, rose, wine, purple, grey.
  See the bundled examples/viz/ and README.md for examples.

Direct SVG rendering:
  cmrak svg charts/flow.mmd
  cmrak svg charts/sales.js charts/sales.svg --root .
  Accepts Mermaid .mmd sources and Chartist .js modules; output defaults to
  the input path with an .svg extension.
"#;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Images {
    Reference,
    Embed,
}

#[derive(Debug, Clone)]
pub struct Args {
    pub site_root: Option<PathBuf>,
    pub input: PathBuf,
    pub output: PathBuf,
    pub root: Option<PathBuf>,
    pub images: Images,
    pub allow_missing_images: bool,
    pub standalone: bool,
    pub asset_dir: Option<PathBuf>,
    pub title: Option<String>,
}

pub fn parse(values: impl IntoIterator<Item = String>) -> Result<Option<Args>, String> {
    let mut values = values.into_iter().peekable();
    if values.peek().is_some_and(|s| s == "compile") {
        values.next();
    }
    let mut paths = Vec::new();
    let mut output = None;
    let mut root = None;
    let mut images = Images::Reference;
    let mut standalone = false;
    let mut allow_missing_images = false;
    let mut site_root = None;
    let mut asset_dir = None;
    let mut title = None;
    let mut positional = false;
    while let Some(value) = values.next() {
        if positional {
            paths.push(PathBuf::from(value));
            continue;
        }
        let (flag, inline) = value
            .split_once('=')
            .map_or((value.as_str(), None), |(a, b)| (a, Some(b)));
        let mut argument = || {
            inline
                .map(str::to_owned)
                .or_else(|| values.next())
                .ok_or_else(|| format!("missing value for {flag}"))
        };
        match flag {
            "--" => positional = true,
            "-h" | "--help" => {
                print!("{HELP}");
                return Ok(None);
            }
            "-V" | "--version" => {
                println!(
                    "cmrak {} (commit {})",
                    env!("CARGO_PKG_VERSION"),
                    env!("CMRAK_BUILD_COMMIT")
                );
                return Ok(None);
            }
            "-o" | "--output" => output = Some(PathBuf::from(argument()?)),
            "--site-root" => site_root = Some(PathBuf::from(argument()?)),
            "--root" => root = Some(PathBuf::from(argument()?)),
            "--asset-dir" => asset_dir = Some(PathBuf::from(argument()?)),
            "--title" => title = Some(argument()?),
            "--allow-missing-images" if inline.is_none() => allow_missing_images = true,
            "--standalone" if inline.is_none() => standalone = true,
            "--color" => match argument()?.as_str() {
                "never" | "auto" => {}
                _ => return Err("--color must be never or auto".into()),
            },
            "--images" => {
                images = match argument()?.as_str() {
                    "reference" => Images::Reference,
                    "embed" => Images::Embed,
                    _ => return Err("--images must be reference or embed".into()),
                }
            }
            "--diagnostic-format" => {
                return Err("--diagnostic-format is no longer supported".into());
            }
            s if s.starts_with('-') => return Err(format!("unknown option {s}")),
            _ => paths.push(PathBuf::from(value)),
        }
    }
    if paths.is_empty() {
        return Err("missing input; use --help for usage".into());
    }
    if paths.len() > 2 || (paths.len() == 2 && output.is_some()) {
        return Err("provide one input and one output".into());
    }
    if site_root.is_some() && (paths.len() == 2 || output.is_some()) {
        return Err("--site-root determines output paths; omit OUTPUT and --output".into());
    }
    if asset_dir.is_some() {
        return Err("--asset-dir is no longer needed; use --site-root for shared resources".into());
    }
    let mut paths = paths.into_iter();
    let input = paths.next().unwrap();
    if standalone && site_root.is_some() {
        return Err("--standalone conflicts with --site-root".into());
    }
    let standalone = site_root.is_none();
    let output = output
        .or_else(|| paths.next())
        .unwrap_or_else(|| input.with_extension("html"));
    let output = if let Some(site) = &site_root {
        let root = dunce::canonicalize(root.as_deref().unwrap_or(std::path::Path::new(".")))
            .map_err(|e| e.to_string())?;
        let input_path = dunce::canonicalize(&input).map_err(|e| e.to_string())?;
        crate::site::page_path(
            &site.join(
                input_path
                    .strip_prefix(&root)
                    .map_err(|_| "input must be inside --root")?,
            ),
        )
    } else {
        output
    };
    Ok(Some(Args {
        allow_missing_images,
        site_root,
        input,
        output,
        root,
        images: if standalone { Images::Embed } else { images },
        standalone,
        asset_dir,
        title,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn image_modes() {
        let parse_args = |values: &[&str]| parse(values.iter().map(|value| (*value).to_owned()));
        assert_eq!(
            parse_args(&["a.md"]).unwrap().unwrap().images,
            Images::Embed
        );
        assert_eq!(
            parse_args(&["a.md", "--images=embed"])
                .unwrap()
                .unwrap()
                .images,
            Images::Embed
        );
        for values in [
            vec!["a.md", "--standalone", "--images=reference"],
            vec!["a.md", "--images=reference", "--standalone"],
        ] {
            assert_eq!(parse_args(&values).unwrap().unwrap().images, Images::Embed);
        }
        assert!(parse_args(&["a.md", "--images=copy"]).is_err());
    }

    #[test]
    fn parses_compile_arguments() {
        let args =
            parse(["compile", "a.md", "b.html", "--root=..", "--standalone"].map(String::from))
                .unwrap()
                .unwrap();
        assert_eq!(args.output, PathBuf::from("b.html"));
        assert_eq!(args.images, Images::Embed);
        assert!(parse(["a.md", "--diagnostic-format", "short"].map(String::from)).is_err());
        assert!(parse(["a.md", "b.html", "-o", "c.html"].map(String::from)).is_err());
        assert!(parse(["watch", "a.md", "b.html"].map(String::from)).is_err());
    }
}
