# cmrak developer manual

Follow [AGENTS.md](AGENTS.md). User instructions belong in [README.md](README.md);
shared terms belong in [GLOSSARY.md](GLOSSARY.md).

## Layout and architecture

| Path | Responsibility |
| --- | --- |
| `src/` | Rust CLI, parsing, rendering orchestration, assets, and preview |
| `web/markdown/` | Markdown styling, navigation, and footnote interaction |
| `web/charts/` | Chartist and Mermaid styles |
| `web/raw/` | Code highlighting adapters |
| `web/runtime/` | Compile-time QuickJS render entry point and Intl polyfills |
| `scripts/` | Asset generation and executable packaging |
| `tests/` | JavaScript tests |
| `examples/` | Renderable examples, including Markdown fixtures |
| `vendor/` | Upstream dependencies with a narrowly scoped parser patch |
| `generated/` | Untracked bundles, styles, fonts, licenses, and content version |
| `fonts/` | Source font assets and licenses |

Rust parses Markdown with pulldown-cmark. Embedded QuickJS executes KaTeX,
highlight.js, Pele, and Chartist at compilation time. Static output does not
need rendering scripts; Chartist hydration explicitly adds browser code.
Build inputs live under `web/`, not as parallel root-level copies.
Generated filenames remain unchanged so consumers retain their asset paths.

## Build and verification

```sh
npm ci
npm run build:cmrak-assets
cargo build --release --locked -p cmrak
npm run package:cmrak
```

Packaging produces `release/cmrak-<platform>-<arch>/cmrak` (or `cmrak.exe`).
Builds require Node and a C compiler for QuickJS; the executable does not.
Rebuild assets after changes to rendering sources, themes, or npm dependencies.
Do not commit generated resources.

Use `npm test` for JavaScript tests, or select individual files with
`node --test tests/NAME.test.mjs`. Run relevant Rust tests with
`cargo test --locked -p cmrak FILTER`; use a full suite only when the change
requires it. After Rust edits, run `cargo fmt` then `cargo fmt --check`.
Before committing, inspect the staged diff and run `git diff --check`.
Reuse the global Playwright installation and system Chrome specified in
`AGENTS.md`; do not install browser tooling for ad hoc checks.

## Resource pipeline

`scripts/build-cmrak-assets.mjs` bundles the compile-time runtime, Intl
polyfills, navigation, and browser Chartist runtime. It combines Markdown CSS,
adds chart styles and required fonts/licenses, and computes a content version.
`build.rs` embeds generated assets into Rust. Code uses Monaspace Neon at 13px;
standalone output uses its CDN font with local fallbacks. KaTeX CSS and fonts
are included only when needed.

Site resources live under `_shared/cmrak/<content-version>/`. Identical assets
are reused; missing or corrupt ones are repaired without removing unrelated
files. Hydration modules are content-addressed. Preview serves these assets
with long-lived caching and ordinary files with ETag validation.

## Charts and parser integration

Chartist is an active dependency and rendering feature. Preserve its renderer,
module shims, CSS, import allowlist, hydration, and tests together.
Chart modules render synchronously and should depend only on source and
embedded data. Content changes invalidate cached results. Chart IDs depend on
the declaration and occurrence count, so editing ordinary prose does not
renumber charts; inserting an identical declaration can affect later duplicates.
Chart errors point to their source node.

Chart CSS uses serif text, compact strokes, and Paul Tol Muted colors.
Series can select `tol-muted-` classes with suffixes `indigo`, `cyan`,
`teal`, `green`, `olive`, `sand`, `rose`, `wine`, `purple`, or `grey`.
Use Chartist essentials layers for axis titles and value labels.
Only the Chartist examples listed in README are current; legacy D3 examples
are not evidence of supported imports.

### Local pulldown-cmark patch

Based on crates.io pulldown-cmark 0.13.6 (MIT; upstream copyright and license preserved).

The opt-in `cmrak-graphs` feature adds `mermaid` and `chartist` to raw HTML block opening detection in src/firstpass.rs. Their bodies therefore remain opaque until the matching closing tag, including blank lines and Markdown fences. Closing diagram tags match ASCII case-insensitively. Container termination semantics are unchanged. Missing-closing diagnostics remain the responsibility of cmrak. No global Cargo registry patch is used: only cmrak explicitly selects this path dependency and feature.

## Document behavior

Headings become native details sections with compatible anchors. Footnotes
become native details panels after paragraphs or tables. JavaScript controls
inline-reference toggles; without it, native details remain usable.
Repeated references in one paragraph share a panel, separate paragraphs get
separate panels, and cyclic notes remain reference text. Math delimiters are
`$…$` and `$$…$$`, not backslash-parenthesis or backslash-bracket pairs.

Markdown blocks carry one-based `data-line` values. Navigation finds the block
starting at or before a requested line, reveals collapsed ancestors, and
corrects scrolling after fonts and images load. Named heading anchors are
preserved. Do not transplant ypst's sparse anchor injection: cmrak already
gets source positions from its parser event stream.

Resource paths are resolved relative to input, canonicalized, and checked
against the compilation root. Site mode rewrites Markdown links without
recursively compiling notes; supported code links are compiled as literal
highlighted text and reused by canonical path. Compilation failures preserve
existing output. Diagnostics use one-based line and column positions.

## Preview and release workflows

Preview compiles under the system temporary `cmrak-preview` directory.
Processes in a project share assets, and file locks serialize compilation of
the same input. SSE notifications carry optional fragments after successful
compilation; revision polling detects other changes. The service is shared
with ypst and remains active until stopped. A conflicting listener is an error.

Branch pushes and manual workflow runs build Linux and Windows x64 rolling
releases tagged `rolling-YYYYMMDD-<sha6>` using UTC date and commit prefix.
The package version remains `0.5.0`. After successful upload and latest-release
publication, prior rolling releases/tags (including the legacy version release)
are removed. Failed uploads preserve the previous release; reruns reuse the
same date/commit tag. Actions needs contents-write permission and permission
to delete old tags. Concurrent runs cancel earlier unfinished runs.
The install action always downloads the latest binary, independently of its
selected action revision.
