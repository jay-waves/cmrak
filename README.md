# comrakk

独立 Markdown → HTML 编译器。Rust / Comrak 解析 Markdown，内嵌 QuickJS 在编译时执行 KaTeX、highlight.js 和 Pele。默认输出不包含渲染脚本；交互式 `<chartist>` 可显式保留初始化模块。运行不需要启动后端或安装 Node.js。

## 构建

在仓库根目录安装现有 Node 依赖，然后执行：

```sh
npm ci
npm run package:comrak
```

产物为 `release/comrakk-<platform>-<arch>/comrakk.exe`（Windows）或 `comrakk`（其他平台）。Cargo 包名为 `lib-comrak`，可执行文件名为 `comrakk`。此包不依赖 lib.ts 的 HTTP 服务、Typst 或媒体处理模块。

单独构建和测试：

```sh
npm run build:comrak-assets
cargo build --release --locked -p lib-comrak
cargo test --release --locked -p lib-comrak
```

修改前端渲染代码、主题或 npm 依赖后，重新运行资源构建脚本。生成资源不提交 Git。构建需要 Node 和 C 编译器（QuickJS）；运行 exe 不需要这些环境。

## GitHub Actions

可以在其他 workflow 中引用此仓库的 composite action，下载并安装 `comrakk`，然后在后续步骤直接使用：

```yaml
steps:
  - uses: actions/checkout@v7
  - uses: <owner>/comrak2@<ref>
  - run: comrakk compile note.md output.html --root .
```

将 `<owner>/comrak2@<ref>` 替换为实际仓库和分支、tag 或提交 SHA。Action 从 `0.5.0` release 下载预编译可执行文件，需要 GitHub CLI（GitHub 托管 runner 已预装），运行时不需要 Node.js 或 Rust。支持 Linux 和 Windows x64。该 release 随每次分支 push 更新，下载的版本不随 action 引用的提交固定。

每次分支 push 会构建两平台并更新 `0.5.0` tag 和 release；工作流也支持手动触发。仓库 Actions 需要内容写入权限，tag 保护规则必须允许更新。

## 使用

```sh
comrakk compile note.md
comrakk compile note.md output.html
comrakk compile notes/topic/note.md output.html --root .
comrakk compile note.md -o output.html
comrakk compile note.md --site-root _site
comrakk compile note.md output.html --standalone
```


图表的编译结果按脚本内容复用；请让绘图输出仅由脚本和内置数据决定，避免依赖随机数或时间。修改脚本会使缓存失效。


原生站点预览无需 Node.js：

```sh
comrakk serve note.md --root . --fragment section
comrakk stop
comrakk serve --site-root _site --port 8765
```

`serve INPUT` 将文档编译到系统临时目录 `lib-comrak-preview`，输出预览 URL，并启动或复用 `127.0.0.1:8765` 上的后台服务。同一项目的多个进程共享站点和静态资源；相同文件的编译通过文件锁串行化。页面在生成的 HTML 内容变化时自动刷新，服务保留在后台直到执行 `stop`。端口已被其他服务占用时会报错。`serve` 前台托管 `--site-root` 指定的已有目录（`--dir` 也可使用），按 Ctrl+C 退出；不要与共享预览服务同时占用同一端口。

`serve INPUT` 负责选择站点输出路径，不接受输出路径、`--site-root` 或 `--standalone`。每次由编辑器主动调用 `serve INPUT` 重新编译，页面随后自动刷新。可选的 `--fragment` 接受 URL fragment（可带 `#`）；编译成功后，打开对应页面的浏览器会通过 SSE 更新 fragment 并刷新。编译失败不会发送通知。

图表 ID 根据声明及相同声明的出现次序生成，不随普通正文的增删而改变；JS 或 Mermaid 文件内容变化会重新渲染图表。多个完全相同的声明依靠出现次序区分，插入或删除其中一个时，其后的相同声明可能获得不同 ID。

也允许省略 `compile`。默认输出为输入文件同名 `.html`，本地图片内嵌。`--help` 列出所有选项。成功退出码为 0，编译失败为 1，参数错误为 1。支持 `--option=value` 和 `--` 后的文件名。

生成的 HTML 与主应用共用块级定位：`output.html#L42` 跳到起始行不超过第 42 行的最近块顶部，空行归入前一块；首块之前及文末之后分别定位首块、末块。每块只标记一个从 1 开始的 `data-line`，标题原有的 `#标题` 锚点保留。HTML 附带小型定位脚本，字体和图片加载后会校正定位；关闭 JavaScript 时仍可使用精确标题锚点。

诊断格式为 `path:line:column: error: message`，行列从 1 开始；数学或图表渲染错误、附件错误定位到对应 Markdown 节点。诊断写 stderr。编译失败不会覆盖已有 HTML。

## 输出模式

默认 standalone：内嵌基础 CSS、本地图片，以及当前页面需要的公式字体、图表资源。无公式页面不附带 KaTeX 样式和字体。远程图片保留 URL，由浏览器加载。

显式传入 `--site-root _site` 启用站点模式：

```sh
comrakk compile ai/chatgpt/note.md --site-root _site
```

笔记库根目录默认当前工作目录，可通过 `--root DIR` 指定。示例输出 `_site/ai/chatgpt/note/index.html`，本地附件按笔记库目录结构复制，页面生成相对 URL。`/assets/example.webp` 表示笔记库根目录下的资源，生成 `../../../assets/example.webp`。Markdown 链接改为对应的 `目录/index.html` 链接，但不会递归编译其他笔记。

共享资源位于 `_site/_shared/lib-comrak/<内容版本>/`；内容一致时复用，缺失或损坏时修复，不删除其他文件。`--site-root` 决定输出位置，不与 OUTPUT、`--output` 或 `--standalone` 混用。站点图片默认复制引用，可显式 `--images embed`。


## 图片和路径

相对路径基于输入 Markdown 所在目录解释，支持 `../`、URL 编码、查询参数和片段。默认 root 为当前工作目录，`--root` 可覆盖；解析符号链接后检查边界。站点模式中以 `/` 开头的路径基于笔记库 root 解释。

standalone 内嵌本地图片，普通本地链接仍指向源文件。站点模式复制附件并生成页面相对链接，`.md` 和 `.markdown` 链接映射为对应的 `目录/index.html`，其他笔记须另行编译。允许普通链接目标尚不存在，悬空符号链接或越过 root 的路径报错。

## Chartist 图表

在 Markdown 中使用完整标签，路径相对于当前文件：

```html
<chartist src="chart.js" width="75%" caption="数据变化"></chartist>
```

`width` 和 `caption` 可省略。`chart.js` 同步生成 SVG：

```js
import { lineChartConfig } from 'chartist';
import { toScene, renderToString } from 'chartist/server';

export default function render(container) {
  const config = lineChartConfig({ labels: ['A', 'B', 'C'], series: [[1, 3, 2]] },
    { viewBox: { width: 600, height: 300 } });
  container.innerHTML = renderToString(toScene(config, { width: 600, height: 300 }));
}
```

只支持导入 `chartist` 和 `chartist/server`；路径须位于 `--root` 内。默认输出静态 SVG，可选导出 `hydrate(container)` 为浏览器添加交互。更多示例见 `examples/viz/`；旧版 D3 示例尚未迁移。

Chartist 默认样式位于 `chartist.css`：图表使用正文衬线字体、紧凑线宽和柔和配色。Chartist 不自动生成坐标轴标题；自定义标题可使用 `ct-axis-title-x`（横排）和 `ct-axis-title-y`（竖排），刻度文字保持横排。
