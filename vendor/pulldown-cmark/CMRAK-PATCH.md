Based on crates.io pulldown-cmark 0.13.6 (MIT; upstream copyright and license preserved).

The opt-in `cmrak-graphs` feature adds `mermaid` and `chartist` to raw HTML block opening detection in src/firstpass.rs. Their bodies therefore remain opaque until the matching closing tag, including blank lines and Markdown fences. Closing diagram tags match ASCII case-insensitively. Container termination semantics are unchanged. Missing-closing diagnostics remain the responsibility of cmrak. No global Cargo registry patch is used: only cmrak explicitly selects this path dependency and feature.
