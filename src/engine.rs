use rquickjs::{CatchResultExt, Context, Function, Module, Object, Runtime};
use std::cell::{Cell, OnceCell};
use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};

type FragmentKey = (String, String, String, usize);
type FragmentCache = RenderCache<FragmentKey, Option<String>>;
static SHARED_FRAGMENTS: OnceLock<Mutex<FragmentCache>> = OnceLock::new();
type VizCache = RenderCache<(String, usize), (String, bool)>;
static SHARED_VIZ: OnceLock<Mutex<VizCache>> = OnceLock::new();

// One process-wide cache per result type. Evict the least recently used entry
// until both the byte budget and entry budget are satisfied.
struct RenderCache<K, V> {
    entries: VecDeque<(K, V, usize)>,
    bytes: usize,
}
impl<K: PartialEq, V> RenderCache<K, V> {
    fn new() -> Self {
        Self {
            entries: VecDeque::new(),
            bytes: 0,
        }
    }
    fn get(&mut self, key: &K) -> Option<&V> {
        let position = self.entries.iter().position(|(k, _, _)| k == key)?;
        let entry = self.entries.remove(position)?;
        self.entries.push_back(entry);
        self.entries.back().map(|(_, value, _)| value)
    }
    fn insert(&mut self, key: K, value: V, size: usize) {
        const LIMIT: usize = 8 * 1024 * 1024;
        if size > LIMIT {
            return;
        }
        if let Some(position) = self.entries.iter().position(|(k, _, _)| k == &key) {
            self.bytes -= self.entries.remove(position).unwrap().2;
        }
        while self.bytes + size > LIMIT || self.entries.len() >= 256 {
            self.bytes -= self.entries.pop_front().unwrap().2;
        }
        self.bytes += size;
        self.entries.push_back((key, value, size));
    }
}

#[derive(Default)]
pub struct Engine(OnceCell<Result<Renderer, String>>, Cell<u64>);

struct Renderer {
    context: Context,
    _runtime: Runtime,
}

impl Renderer {
    fn new() -> Result<Self, String> {
        let runtime = Runtime::new().map_err(|e| e.to_string())?;
        runtime.set_memory_limit(256 * 1024 * 1024);
        runtime.set_max_stack_size(4 * 1024 * 1024);
        let context = Context::full(&runtime).map_err(|e| e.to_string())?;
        context.with(|ctx| {
            ctx.eval::<(), _>(include_str!("../generated/intl.js"))
                .and_then(|_| ctx.eval::<(), _>(include_str!("../generated/render.js")))
                .catch(&ctx)
                .map_err(|e| format!("cannot initialize embedded renderers: {e}"))
        })?;
        context
            .with(|ctx| {
                Module::declare(
                    ctx.clone(),
                    "chartist",
                    include_str!("../generated/chartist-module.js"),
                )?
                .eval()?
                .1
                .finish::<()>()
            })
            .map_err(|e| format!("cannot initialize Chartist module: {e}"))?;
        context
            .with(|ctx| {
                Module::declare(
                    ctx.clone(),
                    "chartist/server",
                    include_str!("../generated/chartist-server-module.js"),
                )?
                .eval()?
                .1
                .finish::<()>()
            })
            .map_err(|e| format!("cannot initialize Chartist server module: {e}"))?;
        context
            .with(|ctx| {
                Module::declare(
                    ctx.clone(),
                    "@chartist-js/essentials",
                    include_str!("../generated/chartist-essentials-module.js"),
                )?
                .eval()?
                .1
                .finish::<()>()
            })
            .map_err(|e| format!("cannot initialize Chartist essentials module: {e}"))?;
        Ok(Self {
            context,
            _runtime: runtime,
        })
    }

    fn render(
        &self,
        kind: &str,
        source: &str,
        language: &str,
        index: usize,
    ) -> Result<Option<String>, String> {
        let (error, html) = self.context.with(|ctx| {
            let result = (|| -> rquickjs::Result<_> {
                let function: Function = ctx.globals().get("renderFragment")?;
                let result: Object = function.call((kind, source, language, index as u32))?;
                Ok((
                    result.get::<_, Option<String>>("error")?,
                    result.get::<_, Option<String>>("html")?,
                ))
            })();
            result
                .catch(&ctx)
                .map_err(|e| format!("embedded JavaScript execution failed: {e}"))
        })?;
        error.map_or(Ok(html), Err)
    }
}

impl Engine {
    pub fn render_chart_svg(&self, source: &str) -> Result<String, String> {
        let (markup, _) = self.viz(source, 0)?;
        let start = markup
            .find("<svg")
            .ok_or("Chartist did not render an SVG")?;
        let end = markup
            .rfind("</svg>")
            .ok_or("Chartist returned invalid SVG")?
            + "</svg>".len();
        let svg = &markup[start..end];
        let styles = format!(
            "{}\n{}",
            include_str!("../generated/chartist.css"),
            include_str!("../chartist.css")
        );
        let insert = svg.find('>').ok_or("Chartist returned invalid SVG")? + 1;
        Ok(format!(
            "{}<style>{}</style>{}",
            &svg[..insert],
            styles.replace("</style", "<\\/style"),
            &svg[insert..]
        ))
    }

    pub fn viz(&self, source: &str, index: usize) -> Result<(String, bool), String> {
        let key = (source.to_owned(), index);
        let shared = SHARED_VIZ.get_or_init(|| Mutex::new(RenderCache::new()));
        if let Some(value) = shared.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
            return Ok(value.clone());
        }
        let renderer = self
            .0
            .get_or_init(Renderer::new)
            .as_ref()
            .map_err(Clone::clone)?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        renderer
            ._runtime
            .set_interrupt_handler(Some(Box::new(move || {
                std::time::Instant::now() >= deadline
            })));
        let serial = self.1.get();
        self.1.set(serial.wrapping_add(1));
        let result = renderer.context.with(|ctx| {
            let result = (|| -> rquickjs::Result<(String, bool)> {
                let prepare: Function = ctx.globals().get("prepareViz")?;
                let container: Object = prepare.call((index as u32,))?;
                let (module, promise) =
                    Module::declare(ctx.clone(), format!("viz-module-{serial}"), source)?.eval()?;
                promise.finish::<()>()?;
                let render: Function = module.get("default")?;
                let hydrate: Option<Function> = module.get("hydrate")?;
                let run: Function = ctx.globals().get("runViz")?;
                Ok((run.call((render, container))?, hydrate.is_some()))
            })();
            result
                .catch(&ctx)
                .map_err(|e| format!("viz rendering failed: {e}"))
        });
        renderer._runtime.set_interrupt_handler(None);
        if let Ok(value) = &result {
            let mut shared = shared.lock().unwrap_or_else(|e| e.into_inner());
            shared.insert(key, value.clone(), source.len() + value.0.len());
        }
        result
    }

    pub fn render(
        &self,
        kind: &str,
        source: &str,
        language: &str,
        index: usize,
    ) -> Result<Option<String>, String> {
        let key = (
            kind.to_owned(),
            source.to_owned(),
            language.to_owned(),
            if kind == "diagram" { index } else { 0 },
        );
        let shared = SHARED_FRAGMENTS.get_or_init(|| Mutex::new(RenderCache::new()));
        if let Some(value) = shared.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
            return Ok(value.clone());
        }
        let value = self
            .0
            .get_or_init(Renderer::new)
            .as_ref()
            .map_err(Clone::clone)?
            .render(kind, source, language, index)?;
        // Bound shared memory and avoid caching unusually large documents.
        {
            let mut shared = shared.lock().unwrap_or_else(|e| e.into_inner());
            shared.insert(
                (
                    kind.to_owned(),
                    source.to_owned(),
                    language.to_owned(),
                    if kind == "diagram" { index } else { 0 },
                ),
                value.clone(),
                source.len() + language.len() + kind.len() + value.as_ref().map_or(0, String::len),
            );
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn intl_number_format_handles_grouping_and_rounding() {
        let renderer = Renderer::new().unwrap();
        renderer.context.with(|ctx| {
            let result: String = ctx.eval("new Intl.NumberFormat('en-US', {useGrouping: true, maximumFractionDigits: 2}).format(1234.567)").unwrap();
            assert_eq!(result, "1,234.57");
        });
    }
    #[test]
    fn shared_cache_reuses_fragments_across_engines() {
        let first = Engine::default();
        let second = Engine::default();
        let source = "\\operatorname{sharedcachetest}_{92731}";
        let expected = first.render("math", source, "", 1).unwrap();
        assert_eq!(second.render("math", source, "", 99).unwrap(), expected);
        assert!(second.0.get().is_none());
    }
    #[test]
    fn renderer_is_initialized_on_demand() {
        let engine = Engine::default();
        let temp = tempfile::tempdir().unwrap();
        let attachments = crate::document::Attachments::new(
            &temp.path().join("a.md"),
            &temp.path().join("a.html"),
            temp.path().to_owned(),
            crate::cli::Images::Reference,
            false,
        )
        .unwrap();
        let html =
            crate::document::render("# Hello\n\n```\n<plain>\n```", &engine, &attachments).unwrap();
        assert!(html.contains("&lt;plain&gt;"));
        assert!(engine.0.get().is_none());
        engine
            .render("math", "unique_{renderer_initialization_test}", "", 0)
            .unwrap();
        assert!(matches!(engine.0.get(), Some(Ok(_))));
    }

    #[test]
    fn chartist_examples_render() {
        let engine = Engine::default();
        assert!(
            engine
                .viz(include_str!("../examples/viz/bars.js"), 0)
                .unwrap()
                .0
                .contains("ct-axis-title")
        );
        for source in [
            include_str!("../examples/viz/bars.js"),
            include_str!("../examples/viz/line.js"),
            include_str!("../examples/viz/pie.js"),
        ] {
            assert!(engine.viz(source, 0).unwrap().0.contains("<svg"));
        }
    }

    #[test]
    fn chartist_interactive_example_preserves_hydration() {
        let engine = Engine::default();
        let (svg, interactive) = engine
            .viz(include_str!("../examples/viz/interactive-bars.js"), 1)
            .unwrap();
        assert!(svg.contains("ct-bar"));
        assert!(interactive);
    }

    #[test]
    fn chartist_renders_svg_in_quickjs() {
        let engine = Engine::default();
        let source = r#"import { barChartConfig } from 'chartist';
import { toScene, renderToString } from 'chartist/server';
export default c => { c.innerHTML = renderToString(toScene(barChartConfig({labels:['A','B'],series:[[1,2]]}),{width:500,height:300})); };"#;
        let (html, interactive) = engine.viz(source, 1).unwrap();
        assert!(html.contains("<svg"));
        assert!(html.contains("ct-bar"));
        assert!(!interactive);
    }

    #[test]
    fn real_renderers_work_without_dom_or_node() {
        let engine = Engine::default();
        assert!(
            engine
                .render("math", "x^2", "", 0)
                .unwrap()
                .unwrap()
                .contains("katex")
        );
        assert!(
            engine
                .render("code", "const x = 1;", "js", 0)
                .unwrap()
                .unwrap()
                .contains("hljs-keyword")
        );
        let svg = engine
            .render(
                "diagram",
                "flowchart LR\n accTitle: Example\n A[Start] --> B[End]",
                "",
                1,
            )
            .unwrap()
            .unwrap();
        assert!(svg.contains("<svg"));
        assert!(svg.contains("diagram-1"));
        assert!(engine.render("math", "\\unknowncommand", "", 0).is_err());
        assert!(engine.render("diagram", "not a diagram", "", 0).is_err());
        assert_eq!(
            engine.render("code", "hi", "unknown-language", 0).unwrap(),
            None
        );
    }
}
