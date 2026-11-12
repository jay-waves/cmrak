import { fragmentTarget, revealBlock } from './block-navigation.mjs';
import { prepareFootnotes } from './footnotes.mjs';
const root = document.getElementById('content');
prepareFootnotes(root);
let generation = 0;
function navigate() {
  const token = ++generation;
  if (!/^#(?:L[1-9]\d*|F[1-9]\d*(?:-[1-9]\d*)?)$/.test(location.hash)) return;
  const scroll = () => {
    if (token !== generation) return;
    const target = fragmentTarget(root, location.hash);
    if (target) { revealBlock(target); target.scrollIntoView({ block: 'start' }); }
  };
  scroll();
  Promise.all([document.fonts?.ready, new Promise(resolve => {
    if (document.readyState === 'complete') resolve();
    else window.addEventListener('load', resolve, { once: true });
  })]).then(scroll);
}
window.addEventListener('hashchange', navigate);
navigate();
