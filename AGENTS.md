# Language and documentation

- Use English for documentation, code comments, commit messages, and agent communication. Preserve literal user content, bilingual fixtures, identifiers, and quotations where translation changes their meaning.
- The only independently maintained project documents are the top-level `README.md`, `CMRAK.md`, and `GLOSSARY.md`.
- `README.md` is the concise user manual: installation, essential usage, and user-visible limitations.
- `CMRAK.md` is the developer manual: architecture, build/test/release workflows, implementation details, and maintenance decisions.
- `GLOSSARY.md` owns terminology, keywords, and naming conventions.
- `AGENTS.md` is the workflow instruction file, not another project manual. Do not create nested guides, changelogs, or task reports.
- Preserve upstream vendor documentation, licenses, notices, and executable Markdown fixtures. Keep project-specific vendor patch explanations in `CMRAK.md`.
- Keep styles and JavaScript rendering sources under `web/`, tests under `tests/`, and build tools under `scripts/`. Generated assets belong in `generated/` and are not committed.
- Update imports, build inputs, tests, and owning documentation together when moving files or changing behavior.

# Validation

- Prefer low-cost targeted checks; run integration tests only when necessary with the smallest relevant scope.
- Reuse global Playwright at `/home/jay-waves/.local/lib/node_modules/playwright`.
- Use system Chrome with `chromium.launch({ channel: "chrome" })`.
- Do not add Playwright to projects for ad hoc checks. Do not download browsers or reinstall Playwright without user approval.
- After Rust changes, run `cargo fmt`, then `cargo fmt --check` and relevant tests.
- After rendering source or dependency changes, rebuild assets with `npm run build:cmrak-assets` before Rust compilation.

# Delivery

- Keep the existing package version `0.5.0`; do not change it as part of routine maintenance.
- Before committing, run `git diff --check` and review the staged diff; exclude secrets, generated artifacts, and unrelated files.
- When asked to push, use SSH key authentication and report the commit and verification results.
