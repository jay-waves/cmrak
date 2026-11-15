import test from 'node:test';
import assert from 'node:assert/strict';
import { parseHTML } from 'linkedom';
import { preparePrint } from '../web/markdown/print.mjs';

test('printing deduplicates nested notes, preserves destinations, and restores screen state', () => {
  const { document } = parseHTML(`<html><body><main id="content">
    <details class="heading-section"><summary>Heading</summary>
      <p><a data-note="F1" href="#F1">[1]</a></p>
      <details class="inline-footnote" id="F1"><summary>[1]</summary><div class="note-content">First
        <a data-note="F2" href="#F2">[2]</a>
        <details class="inline-footnote" id="F2"><summary>[2]</summary><div class="note-content">Nested</div></details>
      </div></details>
      <p><a id="existing" data-note="F1-3" href="#F1-3">[1]</a></p>
      <details class="inline-footnote" id="F1-3" open><summary>[1]</summary><div class="note-content">First</div></details>
    </details></main></body></html>`);
  const root = document.getElementById('content');
  const original = root.innerHTML;
  const events = new Map();
  preparePrint(root, { addEventListener: (name, handler) => events.set(name, handler) });
  for (let attempt = 0; attempt < 2; attempt++) {
    events.get('beforeprint')();
    events.get('beforeprint')();
    assert.equal(root.querySelectorAll('.print-endnotes').length, 1);
    assert.equal(root.querySelectorAll('.print-endnote').length, 2);
    assert.equal(root.querySelectorAll('.inline-footnote').length, 0);
    assert.equal(root.querySelectorAll('details:not([open])').length, 0);
    for (const link of root.querySelectorAll('a[href]')) {
      assert.ok(document.getElementById(link.getAttribute('href').slice(1)));
    }
    assert.equal(document.getElementById('F1').parentElement, document.getElementById('F1-3').parentElement);
    events.get('afterprint')();
    events.get('afterprint')();
    assert.equal(root.innerHTML, original);
  }
});

test('documents without notes still expand and restore closed sections', () => {
  const { document } = parseHTML('<main><details><summary>Title</summary>Text</details></main>');
  const root = document.querySelector('main');
  const events = new Map();
  preparePrint(root, { addEventListener: (name, handler) => events.set(name, handler) });
  events.get('beforeprint')();
  assert.ok(root.querySelector('details').hasAttribute('open'));
  assert.equal(root.querySelector('.print-endnotes'), null);
  events.get('afterprint')();
  assert.equal(root.querySelector('details').hasAttribute('open'), false);
});
