/* Browser regression test. Playwright is a test-only tool, never shipped in the app. */
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const http = require('node:http');
const { chromium } = require('playwright');
const root = path.resolve(__dirname, '../desktop/overlay/ui');
const server = http.createServer((request, response) => {
  const name = new URL(request.url, 'http://localhost').pathname.slice(1) || 'index.html';
  if (!['index.html', 'style.css', 'overlay.js', 'control.html', 'control.css', 'control.js'].includes(name)) { response.writeHead(404).end(); return; }
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
      const desktop={code:'07'.repeat(32),address:'192.168.1.42',beacon:false,paired_age:null,telemetry_age:null,ready:true};
      window.__TAURI__ = {
        event: { listen: async (name, handler) => { listeners.set(name, handler); return () => listeners.delete(name); } },
        core: {
          convertFileSrc: path => { window.previewPath=path; return 'data:image/svg+xml,'+encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" width="48" height="32"><rect width="48" height="32" fill="cyan"/></svg>'); },
          invoke: async (name, payload) => {
            window.calls.push({ name, payload, time: performance.now() });
            if (name==='desktop_status')return {...desktop};
            if (name==='set_beacon')desktop.beacon=payload.enabled;
          }
        }
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
    await page.evaluate(() => emitTest('flick://asset', { id: 43, label: 'photo.png', angle: Math.PI, preview:'/inbox/capture-43-photo.png', manual:true }));
    await page.waitForFunction(() => document.querySelector('#asset-preview').naturalWidth > 0);
    assert.equal(await page.evaluate(() => window.previewPath), '/inbox/capture-43-photo.png');
    assert.ok((await page.locator('#asset').getAttribute('class')).includes('has-preview'));
    await page.waitForTimeout(950);
    await page.evaluate(() => emitTest('flick://status',{id:43,ok:true,message:'Saved in inbox'}));
    assert.equal(await page.locator('#channel-state').textContent(),'RECEIVED');
    assert.equal(await page.locator('#asset-state').textContent(),'Saved in desktop inbox');
    await page.goto(`http://127.0.0.1:${server.address().port}/control.html`);
    await page.waitForFunction(() => document.querySelector('#address').value === '192.168.1.42');
    assert.equal(await page.locator('#code').inputValue(), '07'.repeat(32));
    assert.equal(await page.locator('#code').getAttribute('readonly'), '');
    await page.evaluate(() => emitTest('flick://telemetry',{phase:'approach',velocity:.72,armed:false}));
    assert.ok((await page.locator('#gesture').textContent()).includes('Approach detected'));
    assert.ok((await page.locator('#velocity').textContent()).includes('0.720'));
    await page.evaluate(() => emitTest('flick://telemetry',{phase:'approach',velocity:0,locked:false,armed:true,motion:true}));
    assert.ok((await page.locator('#gesture').textContent()).includes('Push detected'));
    assert.ok((await page.locator('#velocity').textContent()).includes('no 20 kHz lock'));
    await page.locator('#beacon').click();
    await page.waitForFunction(() => document.querySelector('#beacon').textContent === 'Stop beacon');
    assert.ok(await page.evaluate(() => calls.some(c => c.name === 'set_beacon' && c.payload.enabled === true)));
    await page.locator('#beacon').click();
    await page.waitForFunction(() => document.querySelector('#beacon').textContent === 'Start beacon');
    await page.evaluate(() => emitTest('flick://asset', {id:44,label:'phone-photo.png',preview:'/inbox/capture-44-phone-photo.png'}));
    await page.waitForFunction(() => document.querySelector('#received-preview').naturalWidth > 0);
    assert.equal(await page.locator('#received-name').textContent(),'phone-photo.png');
    assert.equal(await page.locator('#received').getAttribute('hidden'),null);
    await page.locator('#open-inbox').click();
    assert.ok(await page.evaluate(() => calls.some(c=>c.name==='open_inbox')));
    assert.deepEqual(errors, []);
    console.log('PASS browser: overlay shader, safe labels, real image element, visible desktop pairing/status and beacon controls');
  } finally { if (browser) await browser.close(); await new Promise(resolve => server.close(resolve)); }
})().catch(error => { console.error(error); process.exitCode = 1; });
