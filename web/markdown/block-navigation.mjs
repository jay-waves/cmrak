// Shared by the reader and exported HTML. Lines are one-based block starts.
export function blockForLine(root, line) {
  if (!Number.isSafeInteger(line) || line < 1) return null;
  let first = null, previous = null;
  for (const node of root.querySelectorAll('[data-line]')) {
    const start = Number(node.dataset.line);
    if (!Number.isSafeInteger(start) || start < 1) continue;
    if (!first || start < Number(first.dataset.line)) first = node;
    if (start <= line && (!previous || start > Number(previous.dataset.line))) previous = node;
  }
  return previous || first;
}

export function fragmentTarget(root, hash) {
  let id;
  try { id = decodeURIComponent(hash.replace(/^#/, '')); } catch { return null; }
  if (/^L[1-9]\d*$/.test(id)) return blockForLine(root, Number(id.slice(1)));
  return [...root.querySelectorAll('[id]')].find(node => node.id === id) || null;
}

export function revealBlock(target, root = target.ownerDocument) {
  for (let node = target; node; node = node.parentElement) {
    if (node.classList.contains('inline-footnote')) {
      node.hidden = false;
      for (const ref of root.querySelectorAll('[data-footnote-ref]')) {
        if (ref.getAttribute('data-footnote-ref') === node.id) ref.setAttribute('aria-expanded', 'true');
      }
    }
    if (node.classList.contains('heading-section')) node.classList.remove('is-collapsed');
    if (node.tagName === 'DETAILS') node.open = true;
  }
}
