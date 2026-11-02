import { build } from 'esbuild';
import * as chartist from 'chartist';
import * as chartistServer from 'chartist/server';
import { mkdir, readFile, writeFile, readdir, cp, rm } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { createRequire } from 'node:module';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const output = resolve(root, 'generated');
const deps = dirname(dirname(createRequire(import.meta.url).resolve('katex/package.json')));
await mkdir(resolve(output, 'fonts'), { recursive: true });
await build({ entryPoints: [resolve(root, 'intl.mjs')], outfile: resolve(output, 'intl.js'), bundle: true, format: 'iife', platform: 'neutral', target: 'es2020', minifyWhitespace: true, minifyIdentifiers: true, minifySyntax: false, banner: { js: 'globalThis.Intl ??= {};' }, legalComments: 'inline' });
for (const stale of ['d3.css', 'd3-module.js', 'D3-LICENSE']) await rm(resolve(output, stale), { force: true });
await build({ entryPoints: [resolve(root, 'navigation.mjs')], outfile: resolve(output, 'navigation.js'), bundle: true, format: 'iife', platform: 'browser', target: 'es2020', minify: true });
await build({
  entryPoints: [resolve(root, 'render.mjs')],
  outfile: resolve(output, 'render.js'), bundle: true, format: 'iife',
  platform: 'neutral', mainFields: ['module', 'main'], target: 'es2020',
  // Syntax minification merges declarations into deeply nested expressions.
  // Keep the parser stack bounded when QuickJS compiles the bundle on Windows.
  minifyWhitespace: true, minifyIdentifiers: true, minifySyntax: false,
  legalComments: 'inline',
});
const shim = (module, global) => Object.keys(module).map(name => `export const ${name}=globalThis.${global}.${name};`).join('\n');
const moduleSource = shim(chartist, '__vizChartist');
const serverModuleSource = shim(chartistServer, '__vizChartistServer');
await writeFile(resolve(output, 'chartist-module.js'), moduleSource);
await writeFile(resolve(output, 'chartist-server-module.js'), serverModuleSource);
await build({
  stdin: { contents: "import * as c from 'chartist'; import * as s from 'chartist/server'; globalThis.__vizChartist = c; globalThis.__vizChartistServer = s;", resolveDir: root },
  outfile: resolve(output, 'viz.js'), bundle: true, format: 'iife', platform: 'browser',
  target: 'es2020', minify: true, legalComments: 'inline',
});
let theme = await readFile(resolve(root, 'web/markdown/theme.css'), 'utf8');
// Use system text fonts and the packaged Monaspace code font.
theme = theme.replace(/@font-face\s*\{[^}]*font-family:\s*"Lib Source Han Serif";[^}]*\}/g, '');
theme = '@font-face { font-family: "Monaspace Neon"; src: url("fonts/MonaspaceNeonVar.woff") format("woff"); font-style: normal; font-weight: 200 800; font-display: swap; }\n' + theme;
let highlight = await readFile(resolve(root, 'web/markdown/highlight.css'), 'utf8');
highlight = highlight.replace('@scope (.reader, .code-search-results)', '@scope (#content)');
const css = [theme, highlight,
  await readFile(resolve(root, 'web/markdown/heading-sections.css'), 'utf8'),
  await readFile(resolve(root, 'style.css'), 'utf8'),
].join('\n');
const sourceFonts = (await readdir(resolve(deps, 'katex/dist/fonts')))
  .filter(name => name.endsWith('.woff2'));
sourceFonts.push('MonaspaceNeonVar.woff');
// Remove only obsolete generated fonts, leaving source assets untouched.
for (const name of await readdir(resolve(output, 'fonts'))) {
  if (!sourceFonts.includes(name)) await rm(resolve(output, 'fonts', name), { recursive: true });
}
for (const name of sourceFonts) {
  if (name.endsWith('.woff2')) await cp(resolve(deps, 'katex/dist/fonts', name), resolve(output, 'fonts', name));
}
await cp(resolve(root, 'fonts/MonaspaceNeonVar.woff'), resolve(output, 'fonts/MonaspaceNeonVar.woff'));
await cp(resolve(root, 'fonts/LICENSE-Monaspace.txt'), resolve(output, 'Monaspace-LICENSE'));
// Keep only the preferred format. Every emitted font URL has an embedded asset.
const katex = await readFile(resolve(deps, 'katex/dist/katex.min.css'), 'utf8');
const compactKatex = katex.replace(/src:([^;}]+)/g, (declaration, sources) => {
  const preferred = sources.match(/url\(([^)]*\.woff2)\)/);
  return preferred ? `src:url(${preferred[1]}) format("woff2")` : declaration;
});
const compact = css;
await writeFile(resolve(output, 'style.css'), compact);
await writeFile(resolve(output, 'katex.css'), compactKatex);
const vizCss = [await readFile(resolve(root, 'chartist.css'), 'utf8'), await readFile(resolve(root, 'viz.css'), 'utf8')].join('\n');
await writeFile(resolve(output, 'viz.css'), vizCss);
const mermaidCss = await readFile(resolve(root, 'mermaid.css'), 'utf8');
await writeFile(resolve(output, 'mermaid.css'), mermaidCss);
const hash = createHash('sha256').update(compact).update(compactKatex).update(vizCss);
hash.update(moduleSource).update(serverModuleSource).update(await readFile(resolve(output, 'viz.js'))).update(mermaidCss);
hash.update(await readFile(resolve(output, 'navigation.js')));
for (const name of (await readdir(resolve(output, 'fonts'))).sort()) {
  hash.update(name).update(await readFile(resolve(output, 'fonts', name)));
}
await writeFile(resolve(output, 'version.txt'), hash.digest('hex').slice(0, 16));
for (const [name, path] of [
  ['KaTeX-LICENSE', 'node_modules/katex/LICENSE'],
  ['highlight.js-LICENSE', 'node_modules/highlight.js/LICENSE'],
  ['Chartist-LICENSE', 'node_modules/chartist/LICENSE-MIT'],
  ['LinkeDOM-LICENSE', 'node_modules/linkedom/LICENSE'],
  ['Pele-LICENSE', 'node_modules/pele/LICENSE'],
]) await cp(resolve(deps, path.replace(/^node_modules\//, '')), resolve(output, name));
console.log(`Prepared cmrak assets: ${output}`);

const formatjsPackages = ['intl-numberformat', 'intl-locale', 'intl-getcanonicallocales', 'intl-pluralrules', 'intl-localematcher', 'bigdecimal'];
const formatjsLicenses = await Promise.all(formatjsPackages.map(async name => `@formatjs/${name}\n${await readFile(resolve(root, 'node_modules/@formatjs', name, 'LICENSE.md'), 'utf8')}`));
await writeFile(resolve(output, 'FormatJS-LICENSE'), formatjsLicenses.join('\n\n'));
