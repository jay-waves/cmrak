import * as chartist from 'chartist';
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
    return { error: error.message || String(error) };
  }
};

globalThis.__vizChartist = chartist;
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
