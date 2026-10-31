import { pieChartConfig } from 'chartist';
import { toScene, renderToString } from 'chartist/server';

export default function render(container) {
  const data = { labels: ['开发', '设计', '测试'], series: [45, 30, 25] };
  const config = pieChartConfig(data, { viewBox: { width: 600, height: 300 } });
  container.innerHTML = renderToString(toScene(config, { width: 600, height: 300 }));
  container.querySelector('svg').setAttribute('aria-label', '资源分配比例');
}
