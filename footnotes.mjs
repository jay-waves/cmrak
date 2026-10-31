// Keep Comrak's endnotes as the no-script fallback.
export function prepareFootnotes(root) {
  const endnotes = root.querySelector('section.footnotes');
  if (!endnotes) return;
  const notes = new Map([...endnotes.querySelectorAll('li[id]')].map(note => [note.id, note]));
  const groups = new WeakMap();
  const tails = new WeakMap();
  const converted = new Set();
  let active;
  let index = 0;
  for (const ref of root.querySelectorAll('a[data-footnote-ref]')) {
    const source = notes.get(ref.getAttribute('href')?.slice(1));
    if (!source) continue;
    const container = ref.closest('table') || ref.closest('p, h1, h2, h3, h4, h5, h6, li, blockquote');
    if (!container) continue;
    let panels = groups.get(container);
    if (!panels) groups.set(container, panels = new Map());
    let panel = panels.get(source.id);
    if (!panel) {
      const note = root.ownerDocument.createElement('aside');
      note.className = 'inline-footnote';
      note.id = `inline-footnote-${++index}`;
      note.setAttribute('role', 'doc-footnote');
      note.hidden = true;
      const content = root.ownerDocument.createElement('div');
      content.className = 'note-content';
      content.innerHTML = source.innerHTML;
      content.querySelectorAll('a[data-footnote-backref], .footnote-backref').forEach(link => link.remove());
      note.append(content);
      (tails.get(container) || container).after(note);
      tails.set(container, note);
      panels.set(source.id, panel = { note, refs: [] });
      converted.add(source.id);
    }
    const number = Number(ref.textContent);
    if (number > 0) ref.textContent = `[${number}]`;
    ref.setAttribute('href', `#${panel.note.id}`);
    ref.setAttribute('data-footnote-ref', panel.note.id);
    ref.setAttribute('aria-controls', panel.note.id);
    ref.setAttribute('aria-expanded', 'false');
    panel.refs.push(ref);
    ref.addEventListener('click', event => {
      event.preventDefault();
      const opening = panel.note.hidden;
      if (active) {
        active.note.hidden = true;
        active.refs.forEach(link => link.setAttribute('aria-expanded', 'false'));
      }
      active = opening ? panel : null;
      if (active) {
        active.note.hidden = false;
        active.refs.forEach(link => link.setAttribute('aria-expanded', 'true'));
      }
    });
  }
  converted.forEach(id => notes.get(id).remove());
  if (!endnotes.querySelector('li[id]')) endnotes.remove();
}
