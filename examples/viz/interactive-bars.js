import { barChartConfig } from 'chartist';
import { toScene, renderToString } from 'chartist/server';

export default function render(container) {
  const data = { labels: ['春', '夏', '秋', '冬'], series: [[34, 53, 42, 28]] };
  const config = barChartConfig(data, { viewBox: { width: 600, height: 300 } });
  container.innerHTML = renderToString(toScene(config, { width: 600, height: 300 }));
  container.querySelector('svg').setAttribute('aria-label', '四季数值柱状图');
}

export function hydrate(container) {
  for (const bar of container.querySelectorAll('.ct-bar')) {
    bar.addEventListener('mouseenter', () => bar.setAttribute('opacity', '0.6'));
    bar.addEventListener('mouseleave', () => bar.removeAttribute('opacity'));
  }
}
