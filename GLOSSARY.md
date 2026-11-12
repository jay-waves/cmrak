# Terminology and naming conventions

| Term | Agreed meaning |
| --- | --- |
| cmrak | This standalone Rust Markdown-to-HTML compiler; lowercase command and package name. |
| pulldown-cmark | The Markdown parser used by cmrak, with the local opt-in graph patch. |
| Comrak | A different Markdown parser; not cmrak's current parser. |
| entry file / INPUT | The file passed directly to compile or serve. |
| compilation root / --root | The canonical boundary for resource access, defaulting to the working directory. |
| standalone HTML | One HTML output embedding local assets; remote images and the code font can still use network URLs. |
| site output / --site-root | An output tree with relative page links, copied attachments, and shared resources. |
| fragment | A URL target following #; --fragment accepts it with or without #. |
| L<number> | Source-line navigation, such as L42, resolved to the nearest preceding Markdown block in the compiled input. |
| data-line | The one-based source start line attached to a rendered block. |
| Chartist | The supported JavaScript chart library and chartist HTML-tag renderer. |
| Mermaid | Diagram syntax accepted through tags, fences, and .mmd files. |
| Pele | The compile-time Mermaid rendering implementation. |
| QuickJS | The embedded JavaScript engine used at compilation time. |
| hydration | Optional browser interaction exported as hydrate(container) by a chart module. |
| generated assets | Rebuildable, untracked bundles under generated/, embedded into the executable. |
| content version | Asset-content hash used for shared resource paths and cache reuse. |
| rolling release | A date/commit-tagged build publication, distinct from package version 0.5.0. |
| preview service | The local HTTP/SSE service shared with ypst. |

README.md is the user manual; CMRAK.md is the developer manual; GLOSSARY.md owns
terminology. AGENTS.md defines the contribution workflow. Upstream vendor
documents and executable Markdown fixtures are not independent project guides.
