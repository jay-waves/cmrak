import { lineChartConfig } from 'chartist';
import { toScene, renderToString } from 'chartist/server';

export default function render(container) {
  const data = { labels: ['一月', '二月', '三月', '四月', '五月'], series: [[22, 28, 33, 39, 46], [25, 27, 31, 35, 38]] };
  const config = lineChartConfig(data, { viewBox: { width: 600, height: 300 } });
  container.innerHTML = renderToString(toScene(config, { width: 600, height: 300 }));
  container.querySelector('svg').setAttribute('aria-label', '实验组和对照组趋势');
}
