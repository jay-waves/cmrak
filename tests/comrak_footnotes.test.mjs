import test from 'node:test';
import assert from 'node:assert/strict';
import { parseHTML } from 'linkedom';
import { prepareFootnotes } from '../footnotes.mjs';

test('exported footnotes use bracketed numbers and a single open panel', () => {
  const { document, Event } = parseHTML('<html><body><main id="content"><p>Note <sup><a href="#F1" data-footnote-ref>1</a></sup> again <a href="#F1" data-footnote-ref>1</a></p><p>Other <a href="#F27" data-footnote-ref>27</a></p><section class="footnotes"><ol><li id="F1"><p>First <strong>note</strong><a data-footnote-backref>↩</a></p></li><li id="F27"><p>Other note</p></li></ol></section></main></body></html>');
  const root = document.getElementById('content');
  prepareFootnotes(root);
  const refs = [...root.querySelectorAll('[data-footnote-ref]')];
  const panels = [...root.querySelectorAll('.inline-footnote')];
  assert.deepEqual(refs.map(ref => ref.textContent), ['[1]', '[1]', '[27]']);
  assert.equal(panels.length, 2);
  assert.equal(root.querySelector('.footnotes'), null);
  assert.equal(panels[0].querySelector('[data-footnote-backref]'), null);
  assert.equal(panels[0].querySelector('strong').textContent, 'note');
  refs[0].dispatchEvent(new Event('click'));
  assert.equal(panels[0].hidden, false);
  assert.equal(refs[1].getAttribute('aria-expanded'), 'true');
  refs[2].dispatchEvent(new Event('click'));
  assert.equal(panels[0].hidden, true);
  assert.equal(panels[1].hidden, false);
  refs[2].dispatchEvent(new Event('click'));
  assert.equal(panels[1].hidden, true);
});
