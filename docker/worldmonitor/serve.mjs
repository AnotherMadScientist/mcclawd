// serve.mjs — Zero-change container wrapper for WorldMonitor
// Sits OUTSIDE app/ so `git pull` inside app/ always works cleanly.
//
// - Delegates /api/* to WorldMonitor's local-api-server (sidecar)
// - Serves built frontend (dist/) for everything else
// - Binds 0.0.0.0 for Docker networking

import { createLocalApiServer } from './local-api-server.mjs';
import { createReadStream, existsSync, statSync, readFileSync } from 'node:fs';
import { join, extname } from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = fileURLToPath(new URL('.', import.meta.url));
const DIST_DIR = join(__dirname, 'dist');
const PORT = parseInt(process.env.LOCAL_API_PORT || '3000', 10);

// ── Container customization: inject scripts/css into index.html ───────
// Read once at startup, inject our container files before </head>.
// The originals in dist/ stay untouched.
const CONTAINER_FILES = {
  '/container-defaults.js': { path: join(__dirname, 'container-defaults.js'), mime: 'application/javascript; charset=utf-8' },
  '/container-overlays.js': { path: join(__dirname, 'container-overlays.js'), mime: 'application/javascript; charset=utf-8' },
  '/container-overlays.css': { path: join(__dirname, 'container-overlays.css'), mime: 'text/css; charset=utf-8' },
};

const rawIndex = join(DIST_DIR, 'index.html');
const injectedHtml = existsSync(rawIndex)
  ? readFileSync(rawIndex, 'utf-8').replace(
      '</head>',
      `    <link rel="stylesheet" href="/container-overlays.css">\n` +
      `    <script src="/container-defaults.js"></script>\n` +
      `    <script defer src="/container-overlays.js"></script>\n` +
      `  </head>`
    )
  : null;

const MIME = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'application/javascript; charset=utf-8',
  '.mjs': 'application/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg',
  '.gif': 'image/gif',
  '.svg': 'image/svg+xml',
  '.ico': 'image/x-icon',
  '.woff': 'font/woff',
  '.woff2': 'font/woff2',
  '.ttf': 'font/ttf',
  '.wasm': 'application/wasm',
  '.webp': 'image/webp',
  '.avif': 'image/avif',
  '.onnx': 'application/octet-stream',
};

function serveStatic(req, res) {
  const url = new URL(req.url || '/', `http://localhost:${PORT}`);

  // Serve container customization files from project root (not dist/)
  const containerFile = CONTAINER_FILES[url.pathname];
  if (containerFile && existsSync(containerFile.path)) {
    res.writeHead(200, {
      'content-type': containerFile.mime,
      'cache-control': 'no-cache',
    });
    createReadStream(containerFile.path).pipe(res);
    return;
  }

  let filePath = join(DIST_DIR, url.pathname);

  // Prevent directory traversal
  if (!filePath.startsWith(DIST_DIR)) {
    res.writeHead(403, { 'content-type': 'text/plain' });
    res.end('Forbidden');
    return;
  }

  // Intercept index.html — always serve the injected version
  if (url.pathname === '/' || url.pathname === '/index.html') {
    if (injectedHtml) {
      res.writeHead(200, {
        'content-type': 'text/html; charset=utf-8',
        'cache-control': 'no-cache',
      });
      res.end(injectedHtml);
    } else {
      res.writeHead(404, { 'content-type': 'text/plain' });
      res.end('Not found (no dist/index.html — was the app built?)');
    }
    return;
  }

  // Serve exact file if it exists
  if (existsSync(filePath) && statSync(filePath).isFile()) {
    const ext = extname(filePath);
    const mime = MIME[ext] || 'application/octet-stream';
    const headers = { 'content-type': mime };

    // Cache hashed assets (Vite puts them in /assets/)
    if (url.pathname.includes('/assets/')) {
      headers['cache-control'] = 'public, max-age=31536000, immutable';
    }

    res.writeHead(200, headers);
    createReadStream(filePath).pipe(res);
    return;
  }

  // SPA fallback: serve injected index.html for all non-file routes
  if (injectedHtml) {
    res.writeHead(200, {
      'content-type': 'text/html; charset=utf-8',
      'cache-control': 'no-cache',
    });
    res.end(injectedHtml);
  } else {
    res.writeHead(404, { 'content-type': 'text/plain' });
    res.end('Not found (no dist/index.html — was the app built?)');
  }
}

// Boot WorldMonitor's API server (handles /api/* routes)
const app = await createLocalApiServer({ port: PORT });

// Intercept requests: splice static-file handler before the sidecar's 404
const originalHandler = app.server.listeners('request')[0];
app.server.removeAllListeners('request');
app.server.on('request', (req, res) => {
  if (req.url?.startsWith('/api/')) {
    originalHandler(req, res);
  } else {
    serveStatic(req, res);
  }
});

// Listen on 0.0.0.0 (not 127.0.0.1) so Docker port mapping works
app.server.listen(PORT, '0.0.0.0', () => {
  console.log(`[worldmonitor] http://0.0.0.0:${PORT}`);
  console.log(`[worldmonitor] API routes: ${app.routes.length} | dist: ${DIST_DIR}`);
  console.log(`[worldmonitor] cloud fallback: ${app.context.cloudFallback}`);
});
