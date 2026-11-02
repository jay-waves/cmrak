# cmrak

独立 Markdown → HTML 编译器。Rust / pulldown-cmark 解析 Markdown，内嵌 QuickJS 在编译时执行 KaTeX、highlight.js 和 Pele。默认输出不包含渲染脚本；交互式 `<chartist>` 可显式保留初始化模块。运行不需要启动后端或安装 Node.js。

## 构建

在仓库根目录安装现有 Node 依赖，然后执行：

```sh
npm ci
npm run package:cmrak
```

产物为 `release/cmrak-<platform>-<arch>/cmrak.exe`（Windows）或 `cmrak`（其他平台）。Cargo 包名为 `cmrak`，可执行文件名为 `cmrak`。此包不依赖 lib.ts 的 HTTP 服务、Typst 或媒体处理模块。

单独构建和测试：

```sh
npm run build:cmrak-assets
cargo build --release --locked -p cmrak
cargo test --release --locked -p cmrak
```

修改前端渲染代码、主题或 npm 依赖后，重新运行资源构建脚本。生成资源不提交 Git。构建需要 Node 和 C 编译器（QuickJS）；运行 exe 不需要这些环境。

## GitHub Actions

可以在其他 workflow 中引用此仓库的 composite action，下载并安装 `cmrak`，然后在后续步骤直接使用：

```yaml
steps:
  - uses: actions/checkout@v7
  - uses: <owner>/cmrak@<ref>
  - run: cmrak compile note.md output.html --root .
```

将 `<owner>/cmrak@<ref>` 替换为实际仓库和分支、tag 或提交 SHA。Action 从 `0.5.0` release 下载预编译可执行文件，需要 GitHub CLI（GitHub 托管 runner 已预装），运行时不需要 Node.js 或 Rust。支持 Linux 和 Windows x64。该 release 随每次分支 push 更新，下载的版本不随 action 引用的提交固定。

每次分支 push 会构建两平台并更新 `0.5.0` tag 和 release；工作流也支持手动触发。仓库 Actions 需要内容写入权限，tag 保护规则必须允许更新。

## 使用

```sh
cmrak compile note.md
cmrak compile note.md output.html
cmrak compile notes/topic/note.md output.html --root .
cmrak compile note.md -o output.html
cmrak compile note.md --site-root _site
cmrak compile note.md output.html --standalone
```


图表的编译结果按脚本内容复用；请让绘图输出仅由脚本和内置数据决定，避免依赖随机数或时间。修改脚本会使缓存失效。


原生站点预览无需 Node.js：

```sh
cmrak serve note.md --root . --fragment section
cmrak stop
cmrak serve --site-root _site --port 8765
```

`serve INPUT` 将文档编译到系统临时目录 `cmrak-preview`，输出预览 URL，并启动或复用 `127.0.0.1:8765` 上的后台服务。同一项目的多个进程共享站点和静态资源；相同文件的编译通过文件锁串行化。页面在生成的 HTML 内容变化时自动刷新，服务保留在后台直到执行 `stop`。端口已被其他服务占用时会报错。`serve` 前台托管 `--site-root` 指定的已有目录（`--dir` 也可使用），按 Ctrl+C 退出；不要与共享预览服务同时占用同一端口。

`serve INPUT` 负责选择站点输出路径，不接受输出路径、`--site-root` 或 `--standalone`。每次由编辑器主动调用 `serve INPUT` 重新编译，页面随后自动刷新。可选的 `--fragment` 接受 URL fragment（可带 `#`）；编译成功后，打开对应页面的浏览器会通过 SSE 更新 fragment 并刷新。编译失败不会发送通知。

HTML 图表标签必须独占一个 Markdown 块，并提供完整闭合标签。本地 pulldown-cmark 补丁直接将图表解析为原始 HTML 块，无需 HTML tokenizer、占位替换或源码二次解析；Mermaid 内的空行、HTML 标签及实体按原文保留。真实图表缺少闭合标签时，编译在开始标签的位置报错。

图表 ID 根据声明及相同声明的出现次序生成，不随普通正文的增删而改变；JS 或 Mermaid 文件内容变化会重新渲染图表。多个完全相同的声明依靠出现次序区分，插入或删除其中一个时，其后的相同声明可能获得不同 ID。

也允许省略 `compile`。默认输出为输入文件同名 `.html`，本地图片内嵌。`--help` 列出所有选项。成功退出码为 0，编译失败为 1，参数错误为 1。支持 `--option=value` 和 `--` 后的文件名。

也支持直接编译 UTF-8 代码文件，按后缀使用 highlight.js 高亮，页面宽度、边距、明暗主题和等宽字体沿用现有 CSS：

```sh
cmrak compile main.c main.html
cmrak compile references.bib references.html
cmrak compile script.py --site-root _site --root .
cmrak serve init.lua --root .
```

支持 C/C++（`.c`、`.h`、`.cpp`、`.hpp` 等）、BibTeX、Python、Lua、Shell、JS/TS、Rust、Go、Java、Kotlin、C#、Ruby、PHP、Perl、R、Swift、SQL、JSON、YAML、TOML/INI、XML/HTML/SVG、CSS/SCSS/Less、LaTeX、Typst、PowerShell、CMake、Makefile、Dockerfile、Diff、GraphQL、HTTP 和纯文本 `.txt`。也识别 `Makefile`、`Dockerfile`、`Containerfile`、`CMakeLists.txt` 和常用 Shell 配置文件名。代码作为完整文本显示，其中的 Markdown、公式、HTML 和附件引用均不执行或展开。未识别的后缀仍沿用 Markdown 编译流程。HTML 输入须指定不同输出路径，避免覆盖源文件。

生成的 HTML 与主应用共用块级定位：`output.html#L42` 跳到起始行不超过第 42 行的最近块顶部，空行归入前一块；首块之前及文末之后分别定位首块、末块。每块只标记一个从 1 开始的 `data-line`，标题原有的 `#标题` 锚点保留。HTML 附带小型定位脚本，字体和图片加载后会校正定位；关闭 JavaScript 时仍可使用精确标题锚点。

诊断格式为 `path:line:column: error: message`，行列从 1 开始；数学或图表渲染错误、附件错误定位到对应 Markdown 节点。诊断写 stderr。编译失败不会覆盖已有 HTML。

普通超链接使用 `file://` 或指向根目录之外时，编译器输出包含链接和源位置的 warning，保留链接文字并移除不可移植的目标。图片、图表脚本等需要读取的资源必须存在且位于 `--root` 内，违反时输出 error 并停止编译。

## 输出模式

默认 standalone：内嵌基础 CSS、本地图片，以及当前页面需要的公式字体、图表资源。Monaspace Neon 代码字体固定从 jsDelivr 加载；断网或 CDN 不可用时回退到本机 Consolas、Menlo 或系统等宽字体。代码字号为 13px。无公式页面不附带 KaTeX 样式和字体。远程图片保留 URL，由浏览器加载。

显式传入 `--site-root _site` 启用站点模式：

```sh
cmrak compile ai/chatgpt/note.md --site-root _site
```

笔记库根目录默认当前工作目录，可通过 `--root DIR` 指定。示例输出 `_site/ai/chatgpt/note.md/index.html`，本地附件按笔记库目录结构复制，页面生成相对 URL。`/assets/example.webp` 表示笔记库根目录下的资源，生成 `../../../assets/example.webp`。Markdown 链接改为对应的 `文件名/index.html` 链接，但不会递归编译其他笔记。

共享资源位于 `_site/_shared/cmrak/<内容版本>/`；内容一致时复用，缺失或损坏时修复，不删除其他文件。 站点导航脚本、Chartist 运行时与交互模块均使用共享文件；交互模块按内容寻址。内置预览服务为内容寻址的共享资源设置长期缓存，普通文件使用 ETag 重新验证。其他静态服务器需自行配置相应缓存头。`--site-root` 决定输出位置，不与 OUTPUT、`--output` 或 `--standalone` 混用。站点图片默认复制引用，可显式 `--images embed`。

站点模式下，指向支持高亮的本地代码文件的普通链接会自动编译目标，并指向 `完整文件名/index.html`；query 和 fragment 保留。重复引用按规范化路径复用，缺失、越界或编译失败会在引用位置报错。代码内容只按文本高亮。单独编译与链接触发的编译共用文件锁；standalone 模式保留原有链接行为。


## 图片和路径

相对路径基于输入 Markdown 所在目录解释，支持 `../`、URL 编码、查询参数和片段。默认 root 为当前工作目录，`--root` 可覆盖；解析符号链接后检查边界。站点模式中以 `/` 开头的路径基于笔记库 root 解释。

standalone 内嵌本地图片，普通本地链接仍指向源文件。站点模式复制附件并生成页面相对链接，`.md` 和 `.markdown` 链接映射为对应的 `文件名/index.html`，其他笔记须另行编译。允许普通链接目标尚不存在，悬空符号链接或越过 root 的路径报错。

## Chartist 图表

也可直接编译外部图表源码为 SVG：

```sh
cmrak svg charts/flow.mmd
cmrak svg charts/sales.js charts/sales.svg --root .
```

输入只接受 `.mmd` 和 `.js`；省略输出路径时使用输入文件名并改为 `.svg`。JS 模块沿用 Chartist 图表的导入白名单及同步渲染限制，生成的 SVG 内嵌项目主题样式。

在 Markdown 中使用完整标签，路径相对于当前文件：

```html
<chartist src="chart.js" width="75%" caption="数据变化"></chartist>
```

`width` 和 `caption` 可省略。默认图表容器占满正文宽度；指定宽度时不会超出正文容器。`chart.js` 同步生成 SVG：

```js
import { lineChartConfig } from 'chartist';
import { toScene, renderToString } from 'chartist/server';

export default function render(container) {
  const config = lineChartConfig({ labels: ['A', 'B', 'C'], series: [[1, 3, 2]] },
    { viewBox: { width: 600, height: 300 } });
  container.innerHTML = renderToString(toScene(config, { width: 600, height: 300 }));
}
```

只支持导入 `chartist`、`chartist/server` 和 `@chartist-js/essentials`；路径须位于 `--root` 内。默认输出静态 SVG，可选导出 `hydrate(container)` 为浏览器添加交互。更多示例见 `examples/viz/`；旧版 D3 示例尚未迁移。

Chartist 默认样式位于 `chartist.css`：图表使用正文衬线字体、紧凑线宽和柔和配色。可通过 `@chartist-js/essentials` 的 `axisTitle`、`valueLabels` 等图层添加坐标轴标题和数值标签；默认系列采用 Paul Tol Muted 配色。

需要指定系列颜色时，可使用 Chartist 数据中的 `className`，例如 `series: [{ className: 'tol-muted-rose', data: [1, 3, 2] }]`。可用类名为 `tol-muted-` 加 `indigo`、`cyan`、`teal`、`green`、`olive`、`sand`、`rose`、`wine`、`purple` 或 `grey`。具名颜色也适用于趋势线和目标线（目标线使用其 `className` 选项）；浅灰适合辅助数据。未指定时继续使用默认系列顺序。

独立编译使用 pulldown-cmark 事件流，不依赖 Comrak。标题仍生成原生 `<details>` 分节和兼容锚点；脚注在编译时生成为段落或表格后的原生 `<details>`，关闭 JavaScript 也可展开；启用脚本时隐藏脚注折叠标题和收起面板，仅点击正文上标开关段后脚注，沿用原有虚线边框样式，同段重复引用共用面板，不同段落分别生成，循环脚注保留为引用文字。数学推荐使用 `$…$` 和 `$$…$$`，不再解析 `\(…\)`、`\[…\]` 数学定界符。
