import { test } from 'node:test';
import assert from 'node:assert/strict';
import { prepareFootnotes } from '../web/markdown/footnotes.mjs';

function fixture() {
  const handlers = new Map();
  const panels = new Map();
  const links = [];
  const root = {
    classList: { add(name) { assert.equal(name, 'footnotes-interactive'); } },
    addEventListener(type, callback) { handlers.set(type, callback); },
    contains(node) { return links.includes(node); },
    querySelectorAll(selector) { return links.filter(link => selector.includes(`"${link.dataset.note}"`)); },
    ownerDocument: { getElementById(id) { return panels.get(id); } },
  };
  const panel = (id, parentElement = null) => {
    const node = { id, parentElement, tagName: 'DETAILS', open: false,
      matches(selector) { return selector === 'details.inline-footnote'; },
      contains(other) { for (; other; other = other.parentElement) if (other === this) return true; return false; },
    };
    panels.set(id, node); return node;
  };
  const link = note => {
    const node = { dataset: { note: note.id }, attributes: {},
      closest() { return this; }, setAttribute(key, value) { this.attributes[key] = value; },
    };
    links.push(node); return node;
  };
  prepareFootnotes(root);
  return { panel, link,
    click(target) { let prevented = false; handlers.get('click')({ target, preventDefault() { prevented = true; } }); assert.ok(prevented); },
    toggle(target) { handlers.get('toggle')({ target }); },
  };
}

test('delegated references toggle panels and synchronize duplicate references', () => {
  const f = fixture(); const a = f.panel('F1'); const b = f.panel('F2');
  const a1 = f.link(a); const a2 = f.link(a); const b1 = f.link(b);
  f.click(a1); assert.equal(a.open, true); assert.equal(a2.attributes['aria-expanded'], 'true');
  f.click(b1); assert.equal(a.open, false); assert.equal(b.open, true); assert.equal(a1.attributes['aria-expanded'], 'false');
  f.click(b1); assert.equal(b.open, false);
  a.open = true; f.toggle(a); assert.equal(a1.attributes['aria-expanded'], 'true');
  a.open = false; f.toggle(a); assert.equal(a1.attributes['aria-expanded'], 'false');
});

test('nested notes keep ancestors open despite toggle event ordering', () => {
  const f = fixture(); const parent = f.panel('F1'); const child = f.panel('F2', parent); const other = f.panel('F3');
  const ref = f.link(child); const outside = f.link(other);
  f.click(ref); assert.equal(parent.open, true); assert.equal(child.open, true);
  f.toggle(parent); f.toggle(child); assert.equal(child.open, true);
  f.click(outside); assert.equal(child.open, false); assert.equal(parent.open, false);
});
