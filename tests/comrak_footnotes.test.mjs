import test from 'node:test';
import assert from 'node:assert/strict';
import { parseHTML } from 'linkedom';
import { prepareFootnotes } from '../footnotes.mjs';

function fixture() {
  const { document, Event } = parseHTML(`<html><body><main id="content">
    <p><a href="#F1" data-note="F1">[1]</a><a href="#F1" data-note="F1">[1]</a>
    <a href="#F2" data-note="F2">[2]</a></p>
    <details class="inline-footnote" id="F1"><summary>[1]</summary>
      <a href="#F3" data-note="F3">[3]</a>
      <details class="inline-footnote" id="F3"><summary>[3]</summary>Nested</details>
    </details>
    <details class="inline-footnote" id="F2"><summary>[2]</summary>Other</details>
  </main></body></html>`);
  const root = document.getElementById('content');
  prepareFootnotes(root);
  return {
    root,
    note: id => document.getElementById(id),
    click: id => root.querySelector(`[data-note="${id}"]`).dispatchEvent(new Event('click', { bubbles: true, cancelable: true })),
  };
}

test('repeated references synchronize and unrelated panels close', () => {
  const { root, note, click } = fixture();
  click('F1');
  assert.equal(note('F1').open, true);
  assert.deepEqual([...root.querySelectorAll('[data-note="F1"]')].map(ref => ref.getAttribute('aria-expanded')), ['true', 'true']);
  click('F2');
  assert.equal(note('F1').open, false);
  assert.equal(note('F2').open, true);
  click('F2');
  assert.equal(note('F2').open, false);
});

test('nested panels keep their ancestors open and close with their branch', () => {
  const { note, click } = fixture();
  click('F1');
  click('F3');
  assert.equal(note('F1').open, true);
  assert.equal(note('F3').open, true);
  click('F2');
  assert.equal(note('F1').open, false);
  assert.equal(note('F3').open, false);
  assert.equal(note('F2').open, true);
});

test('opening a nested reference synchronizes its previously closed ancestors', () => {
  const { root, note, click } = fixture();
  click('F3');
  assert.equal(note('F1').open, true);
  assert.equal(note('F3').open, true);
  for (const ref of root.querySelectorAll('[data-note="F1"]')) {
    assert.equal(ref.getAttribute('aria-expanded'), 'true');
  }
});
