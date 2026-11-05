import * as chartist from 'chartist';
import * as essentials from '@chartist-js/essentials';
import * as chartistServer from 'chartist/server';
import { parseHTML } from 'linkedom/worker';
import katex from 'katex';
import { highlightSource, rawLanguage } from './web/raw/raw-highlight.mjs';
import { render } from 'pele';

// Only string APIs are used: no DOM, Node runtime, network or browser globals.
globalThis.renderFragment = (kind, source, language, index) => {
  try {
    let html;
    if (kind === 'math' || kind === 'display') {
      html = katex.renderToString(source.replace(/\u200b/g, ''), {
        displayMode: kind === 'display', throwOnError: true, trust: false,
      });
    } else if (kind === 'diagram') {
      html = render(source, {
        fontFamily: "Georgia, Source Han Serif, Noto Serif CJK SC, Libertinus Serif, New Computer Modern, serif",
        idPrefix: `diagram-${index}`, autoDirection: false, maxWidth: 900,
        // Diagram image attachments require their own asset traversal; do not
        // silently leave unmanaged images in an otherwise standalone document.
        images: false,
      }).svg;
    } else {
      const resolved = rawLanguage(language);
      html = resolved ? highlightSource(source, resolved) : null;
    }
    return { html };
  } catch (error) {
    const component = kind === 'diagram' ? 'Pele' : (kind === 'math' || kind === 'display') ? 'KaTeX' : 'highlight.js';
    const detail = error?.message || String(error);
    const name = error?.name && error.name !== 'Error' ? `${error.name}: ` : '';
    let line = error?.line, column = error?.column;
    if ((kind === 'math' || kind === 'display') && Number.isInteger(error?.position) && !source.includes('\u200b')) {
      const before = source.slice(0, error.position);
      line = before.split('\n').length;
      column = before.length - before.lastIndexOf('\n');
    }
    const peleCode = ['syntax', 'semantic', 'unsupported-diagram', 'limit'].includes(error?.code) ? error.code : null;
    const formatted = kind === 'diagram' ? detail.split(/\r?\n/).map((part, index) => index ? `  ${part}` : part).join('\n') : detail;
    const prefix = kind === 'diagram' && peleCode ? `Pele ${peleCode}: ` : `${component}: ${name}`;
    const location = Number.isInteger(line) && line > 0 && Number.isInteger(column) && column > 0
      ? `\u001e${JSON.stringify({ line, column, source })}` : '';
    return { error: `${prefix}${formatted}${location}` };
  }
};

globalThis.__vizChartist = chartist;
globalThis.__vizEssentials = essentials;
globalThis.__vizChartistServer = chartistServer;
globalThis.prepareViz = index => {
  const { document } = parseHTML('<!doctype html><html><body></body></html>');
  globalThis.document = document;
  const container = document.createElement('div');
  container.id = `viz-${index}`;
  document.body.append(container);
  return container;
};

globalThis.runViz = (render, container) => {
  const result = render(container);
  if (result && typeof result.then === 'function') throw new Error('viz default export must be synchronous');
  return container.innerHTML;
};
