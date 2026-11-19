import { mathjax } from '@mathjax/src/js/mathjax.js';
import { liteAdaptor } from '@mathjax/src/js/adaptors/liteAdaptor.js';
import { RegisterHTMLHandler } from '@mathjax/src/js/handlers/html.js';
import { TeX } from '@mathjax/src/js/input/tex.js';
import '@mathjax/src/js/input/tex/ams/AmsConfiguration.js';
import { SVG } from '@mathjax/src/js/output/svg.js';
import { SafeHandler } from '@mathjax/src/js/ui/safe/SafeHandler.js';
import { MathJaxTexFont } from '@mathjax/mathjax-tex-font/js/svg.js';

const adaptor = liteAdaptor();
let document;

function typeset(tex) {
  if (!document) {
    SafeHandler(RegisterHTMLHandler(adaptor));
    document = mathjax.document('', {
      InputJax: new TeX({ packages: ['base', 'ams'], formatError: (_, error) => { throw error; } }),
      OutputJax: new SVG({ fontData: MathJaxTexFont, fontCache: 'none' }),
    });
  }
  try {
    return adaptor.firstChild(document.convert(tex, { display: true }));
  } catch {
    return null;
  }
}

export function diagramMath() {
  const formulas = new Map();
  return (tex, size) => {
    const key = JSON.stringify([tex, size]);
    if (formulas.has(key)) return formulas.get(key);
    const node = typeset(tex);
    let markup = null;
    if (node) {
      // MathJax expresses the SVG viewBox dimensions in thousandths of an em.
      const [, , viewWidth, viewHeight] = adaptor.getAttribute(node, 'viewBox').split(' ').map(Number);
      const width = viewWidth * size / 1000;
      const height = viewHeight * size / 1000;
      adaptor.setAttribute(node, 'width', width);
      adaptor.setAttribute(node, 'height', height);
      adaptor.removeAttribute(node, 'style');
      markup = { svg: adaptor.outerHTML(node), width, height };
    }
    formulas.set(key, markup);
    return markup;
  };
}
