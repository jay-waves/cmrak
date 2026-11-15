// Build a temporary print tree so fragment links retain real PDF destinations.
export function preparePrint(root, view = root.ownerDocument.defaultView) {
  const document = root.ownerDocument;
  let restore;
  view.addEventListener('beforeprint', () => {
    if (restore) return;
    const undo = [];
    restore = () => { for (const action of undo.reverse()) action(); restore = null; };
    const panels = [...root.querySelectorAll('details.inline-footnote')];
    if (panels.length) {
      const section = document.createElement('section');
      section.className = 'print-endnotes';
      const heading = document.createElement('h2');
      heading.textContent = 'Footnotes';
      section.append(heading);
      const notes = new Map();
      for (const panel of panels) {
        const number = Number(/^F(\d+)/.exec(panel.id)[1]);
        let note = notes.get(number);
        if (!note) {
          note = document.createElement('div');
          note.className = 'print-endnote';
          const label = document.createElement('strong');
          label.textContent = `[${number}] `;
          const content = panel.querySelector('.note-content').cloneNode(true);
          for (const nested of content.querySelectorAll('details.inline-footnote')) nested.remove();
          note.append(label, content);
          notes.set(number, note);
        }
        const anchor = document.createElement('span');
        anchor.id = panel.id;
        note.prepend(anchor);
      }
      // Remove children first; reversing the undo list restores parents first.
      for (const panel of panels.reverse()) {
        const placeholder = document.createComment('print footnote');
        panel.replaceWith(placeholder);
        undo.push(() => placeholder.replaceWith(panel));
      }
      for (const [, note] of [...notes].sort(([a], [b]) => a - b)) section.append(note);
      root.append(section);
      undo.push(() => section.remove());
      let reference = 0;
      for (const ref of root.querySelectorAll('a[data-note]')) {
        const note = notes.get(Number(/^F(\d+)/.exec(ref.dataset.note)?.[1]));
        if (!note) continue;
        const oldId = ref.getAttribute('id');
        let id;
        do { id = `cmrak-print-ref-${++reference}`; } while (document.getElementById(id));
        ref.id = id;
        undo.push(() => oldId === null ? ref.removeAttribute('id') : ref.setAttribute('id', oldId));
        const back = document.createElement('a');
        back.href = `#${id}`;
        back.textContent = ' ↩';
        back.setAttribute('aria-label', 'Return to reference');
        note.append(back);
      }
    }
    for (const details of root.querySelectorAll('details:not([open])')) {
      details.setAttribute('open', '');
      undo.push(() => details.removeAttribute('open'));
    }
  });
  view.addEventListener('afterprint', () => restore?.());
}
