# cmrak Chartist charts

When creating a `viz` chart for cmrak:

- Put the chart in a local JavaScript ES module. Place a complete tag in its own Markdown block, with its path relative to the Markdown file:

  ```html
  <chartist src="charts/example.js" width="75%" caption="Chart description"></chartist>
  ```

- `width` and `caption` are optional; width accepts integer percentages (1–100) or pixels (1–10000). The default is centered at up to 640px. Mermaid diagrams use `<mermaid src="charts/flow.mmd" caption="Flow"></mermaid>` or inline source between `<mermaid>` and `</mermaid>`. Mermaid code fences still work but produce a warning.
- Import only `chartist` and `chartist/server`. Inline the data; other module imports and remote modules are unsupported. Keep the chart file inside the compilation `--root`.
- Default-export a synchronous `render(container)` that draws into the supplied container. It runs at compile time in QuickJS and LinkeDOM: use explicit dimensions and an SVG `viewBox`; do not depend on layout measurement, network access, Canvas, timers, animation, or browser-only globals. Output is cached by module source, so avoid time and unseeded randomness. Keep module top-level code compatible with this environment.
- For interaction, also export `hydrate(container)`. The browser calls only `hydrate` on the pre-rendered container; attach event handlers and use browser-only APIs there. Use the existing SVG in `hydrate` and attach handlers without redrawing. Without this export, the chart is static and its JavaScript is not included in the HTML. Scope selections to `container` and prefix any SVG IDs with `container.id`.
- Use the shared `chartist.css` defaults for serif typography, compact lines, and series colors. Axis titles can use `.ct-axis-title-x` and `.ct-axis-title-y`; Chartist does not create them automatically. Match the document theme with the separate chart stylesheet's basic palette, typography, axes, grids, and common mark classes. `.series-1` through `.series-4` are subdued data colors; `.accent` / `.series-5` and `.accent-2` / `.series-6` are emphasis colors. Use emphasis selectively. `--viz-accent`, `--viz-accent-2`, `--viz-muted`, and `--viz-grid` are available. Apply `.axis`, `.grid`, `.bar`, `.point`, `.line`, `.area`, `.slice`, `.cell`, `.box`, `.whisker`, `.median`, `.outlier`, `.error-bar`, `.reference`, `.legend`, or `.caption` to suitable SVG elements. Specialized forms such as heatmaps, Sankey diagrams, violin plots, and contours should define their own geometry and any chart-specific styling in the chart module. Give charts a descriptive SVG title or accessible label, and support keyboard interaction when applicable.
- Compile the Markdown with `cmrak compile note.md output.html` (add `--root` if needed). Report compile errors and ask the user to check actual browser rendering and interactions. Use `--standalone` for a self-contained preview.

See `examples/viz/bars.js` and `examples/viz/note.md` for Chartist examples. Legacy D3 examples are not supported by the current renderer.
