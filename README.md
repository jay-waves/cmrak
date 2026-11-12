# cmrak

A standalone Markdown-to-HTML compiler. cmrak renders math, highlighted code,
Mermaid diagrams, and Chartist charts during compilation. The executable needs
neither Node.js nor a backend service at runtime.

## Install or update

Linux:

```sh
curl -fsSL https://raw.githubusercontent.com/jay-waves/cmrak/main/install.sh | sh
```

Windows PowerShell:

```powershell
irm https://raw.githubusercontent.com/jay-waves/cmrak/main/install.ps1 | iex
```

Installers download the latest rolling release. Linux installs into
`~/.local/bin`; Windows uses `%LOCALAPPDATA%/Programs/cmrak`.
Add that directory to PATH if needed. Set `INSTALL_DIR` (PowerShell:
`$env:INSTALL_DIR`) to override it. Rerun to update; the package version
remains `0.5.0`.

## Compile

```sh
cmrak compile note.md
cmrak compile note.md output.html --root .
cmrak compile note.md --site-root _site
cmrak compile main.c main.html
cmrak svg charts/flow.mmd
cmrak svg charts/sales.js charts/sales.svg --root .
```

The default output is a same-named `.html` file. `compile` may be omitted;
`-o` selects an output path. Run `cmrak --help` for all options.
Recognized code files are highlighted as literal text, without executing their
Markdown or HTML. Unknown extensions use Markdown parsing. HTML input requires
a different output path.

Standalone output embeds CSS, local images, and required math/chart resources.
Its code font loads from jsDelivr, with local monospace fallbacks. Remote images
retain their URLs. Site mode writes paths such as
`_site/notes/topic.md/index.html`, copies attachments, and shares resources.
Deploy the entire site directory. Markdown links target corresponding HTML
paths but do not recursively compile other notes; links to supported code files
do compile those targets. Use `--images embed` to embed site images.
Do not combine `--site-root` with an output path or `--standalone`.

Paths are relative to the input file; `--root` defaults to the current directory
and bounds resource access after resolving symlinks. In site mode, leading
`/` paths refer to that root. Required resources must exist within it.
Nonportable ordinary links receive warnings and lose their targets.
Errors go to stderr as `path:line:column: error: message`; failed compilation
does not replace existing output.

## Preview and fragments

```sh
cmrak serve note.md --root . --fragment L42
cmrak serve note.md --fragment section
cmrak serve --site-root _site --port 8765
cmrak stop
```

`serve INPUT` compiles once, prints a preview URL, and starts or reuses the
background service on `127.0.0.1:8765`, shared with ypst. Invoke it again after
edits; open pages reload after successful compilation. Failed compilation sends
no notification. This form chooses its own output location and does not accept
an output path, `--site-root`, or `--standalone`.
Directory serving runs in the foreground until Ctrl+C.

`#L42` addresses source line 42 of the compiled input file. Navigation selects
the nearest block starting at or before that line, falling back to the first
block; lines beyond the document select the last block. Named heading anchors
remain available. Source-line navigation needs JavaScript; exact heading
anchors also work without it. Code files are not Markdown block documents.

## Math and charts

Use `$…$` or `$$…$$` for math. Headings collapse and footnotes expand.
Chart tags must occupy their own Markdown block and include closing tags:

```html
<mermaid src="flow.mmd" caption="Flow"></mermaid>

<chartist src="chart.js" width="75%" caption="Values"></chartist>
```

Mermaid also supports inline tag bodies and fenced `mermaid` blocks.
Chartist remains supported. Its JavaScript modules may import `chartist`,
`chartist/server`, and `@chartist-js/essentials`:

```js
import { lineChartConfig } from 'chartist';
import { toScene, renderToString } from 'chartist/server';

export default function render(container) {
  const config = lineChartConfig({ labels: ['A', 'B'], series: [[1, 3]] });
  container.innerHTML = renderToString(toScene(config, { width: 600, height: 300 }));
}
```

Rendering is synchronous and cached by script content; avoid time-dependent or
random output. Optional `hydrate(container)` adds browser interaction.
See `examples/viz/bars.js`, `line.js`, `pie.js`, and `interactive-bars.js`.
Standalone SVG compilation accepts only `.mmd` and `.js` inputs.

## GitHub Actions

```yaml
steps:
  - uses: actions/checkout@v7
  - uses: jay-waves/cmrak@main
  - run: cmrak compile note.md output.html --root .
```

The action supports Linux and Windows x64 and needs GitHub CLI. It downloads
the latest rolling binary independently of the selected action revision.

See [CMRAK.md](CMRAK.md) for development and
[GLOSSARY.md](GLOSSARY.md) for naming conventions.
