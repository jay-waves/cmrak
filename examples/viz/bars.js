import { barChartConfig } from 'chartist';
import { toScene, renderToString } from 'chartist/server';
import { axisTitle } from '@chartist-js/essentials';

export default function render(container) {
  const data = { labels: ['春', '夏', '秋', '冬'], series: [{ className: 'tol-muted-rose', data: [34, 53, 42, 28] }] };
  const config = barChartConfig(data, { viewBox: { width: 600, height: 300 }, layers: { above: [axisTitle({ axis: 'x', text: '季度' })] } });
  container.innerHTML = renderToString(toScene(config, { width: 600, height: 300 }));
  container.querySelector('svg').setAttribute('aria-label', '四季数值柱状图');
}
