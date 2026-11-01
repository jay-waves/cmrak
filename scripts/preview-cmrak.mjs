import http from 'node:http';
import * as fs from 'node:fs/promises';
import { createHash } from 'node:crypto';
const path = process.argv[2];
const port = Number(process.argv[3] || 8765);
if (!path || !Number.isInteger(port) || port < 1 || port > 65535) {
  console.error('Usage: node scripts/preview-cmrak.mjs <HTML> [PORT]');
  process.exit(2);
}
let snapshot;
async function page() {
  const stat = await fs.stat(path, { bigint: true });
  const version = `${stat.ino}:${stat.mtimeNs}:${stat.size}`;
  if (!snapshot || snapshot.version !== version) {
    const body = await fs.readFile(path);
    snapshot = { version, body, etag: `"${createHash('sha256').update(body).digest('hex')}"` };
  }
  return snapshot;
}
http.createServer(async (req, res) => {
  if (!['GET', 'HEAD'].includes(req.method)) { res.writeHead(405, { Allow: 'GET, HEAD' }); res.end(); return; }
  if (!['/', '/index.html'].includes(new URL(req.url, 'http://localhost').pathname)) { res.writeHead(404); res.end(); return; }
  try {
    const { body, etag } = await page();
    const headers = { 'Content-Type': 'text/html; charset=utf-8', 'Cache-Control': 'private, max-age=0, must-revalidate', ETag: etag };
    const matches = req.headers['if-none-match']?.split(',').some(value => value.trim().replace(/^W\//, '') === etag || value.trim() === '*');
    if (matches) { res.writeHead(304, headers); res.end(); return; }
    res.writeHead(200, { ...headers, 'Content-Length': body.length });
    res.end(req.method === 'HEAD' ? undefined : body);
  } catch { res.writeHead(503, { 'Cache-Control': 'no-store' }); res.end('Preview is not available yet'); }
}).listen(port, '127.0.0.1', () => console.log(`Cached preview: http://127.0.0.1:${port}/`));
