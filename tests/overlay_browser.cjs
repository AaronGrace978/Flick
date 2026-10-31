/* Browser regression test. Playwright is a test-only tool, never shipped in the app. */
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const http = require('node:http');
const { chromium } = require('playwright');
const root = path.resolve(__dirname, '../desktop/overlay/ui');
const server = http.createServer((request, response) => {
  const name = new URL(request.url, 'http://localhost').pathname.slice(1) || 'index.html';
  if (!['index.html', 'style.css', 'overlay.js'].includes(name)) { response.writeHead(404).end(); return; }
  response.setHeader('Content-Type', name.endsWith('.js') ? 'application/javascript' : name.endsWith('.css') ? 'text/css' : 'text/html');
  response.end(fs.readFileSync(path.join(root, name)));
});
(async () => {
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  let browser;
  try {
    browser = await chromium.launch({ executablePath: process.env.CHROMIUM || '/usr/bin/chromium', headless: true,
      args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader'] });
    const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.addInitScript(() => {
      const listeners = new Map(); window.calls = [];
      window.__TAURI__ = {
        event: { listen: async (name, handler) => { listeners.set(name, handler); return () => listeners.delete(name); } },
        core: { invoke: async (name, payload) => { window.calls.push({ name, payload, time: performance.now() }); } }
      };
      window.emitTest = (name, payload) => listeners.get(name)?.({ payload });
    });
    await page.goto(`http://127.0.0.1:${server.address().port}`);
    await page.waitForFunction(() => window.calls.some(c => c.name === 'frontend_ready'));
    async function alpha() {
      return page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => {
        const canvas = document.querySelector('#warp'), gl = canvas.getContext('webgl');
        if (!gl) throw Error('WebGL unavailable');
        const pixels = new Uint8Array(canvas.width * canvas.height * 4);
        gl.readPixels(0, 0, canvas.width, canvas.height, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
        let sum = 0; for (let i = 3; i < pixels.length; i += 4) sum += pixels[i];
        resolve(sum);
      })));
    }
    assert.equal(await alpha(), 0, 'Idle canvas must be fully transparent');
    assert.equal(await page.evaluate(() => getComputedStyle(document.body).pointerEvents), 'none');
    await page.evaluate(() => emitTest('flick://telemetry', { distance: .15, velocity: 1, angle: Math.PI, phase: 'approach' }));
    await page.waitForTimeout(150);
    assert.ok(await alpha() > 0, 'Approach shader must render a visible edge');
    const label = '<img src=x onerror=alert(1)> .txt';
    await page.evaluate(label => emitTest('flick://asset', { id: 42, label, angle: Math.PI }), label);
    await page.waitForTimeout(300);
    assert.equal(await page.locator('#asset-name').textContent(), label);
    assert.equal(await page.locator('#asset-name img').count(), 0, 'Filename must never be HTML');
    assert.equal(await page.evaluate(() => calls.filter(c => c.name === 'complete_handoff').length), 0, 'Paste must wait for animation');
    await page.waitForTimeout(650);
    const calls = await page.evaluate(() => window.calls.filter(c => c.name === 'complete_handoff'));
    assert.equal(calls.length, 1); assert.equal(calls[0].payload.id, 42);
    await page.evaluate(() => emitTest('flick://status', { id: 42, ok: true, message: 'Paste sent' }));
    assert.equal(await page.locator('#asset').getAttribute('class'), 'delivered');
    assert.deepEqual(errors, []);
    console.log('PASS browser: WebGL, idle transparency, click-through CSS, edge rendering, safe labels, single post-animation handoff');
  } finally { if (browser) await browser.close(); await new Promise(resolve => server.close(resolve)); }
})().catch(error => { console.error(error); process.exitCode = 1; });
