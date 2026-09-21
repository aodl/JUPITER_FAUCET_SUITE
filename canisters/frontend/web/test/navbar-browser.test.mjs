import test from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { readFile, stat } from 'node:fs/promises';
import { extname, resolve } from 'node:path';
import { execFileSync } from 'node:child_process';
import { chromium } from '@playwright/test';
import { renderFrontendAsset } from '../../../../tools/scripts/frontend-render.mjs';

const publicRoot = resolve(import.meta.dirname, '../../public');
const repoRoot = resolve(publicRoot, '../../..');
// Never run the browser fixture with the production-network bundle. The local
// build is deterministic and all non-asset requests are blocked below.
execFileSync('npm', ['run', 'build:frontend'], {
  cwd: repoRoot,
  env: { ...process.env, JUPITER_FRONTEND_NETWORK: 'local', CANISTER_ID_JUPITER_HISTORIAN: 'aaaaa-aa' },
  stdio: 'pipe',
});
const manifest = JSON.parse(await readFile(resolve(publicRoot, 'generated/frontend-bundle.json'), 'utf8'));
assert.match(manifest.bundlePath, /^generated\/app\.[a-f0-9]{12}\.js$/);
assert.ok((await stat(resolve(publicRoot, manifest.bundlePath))).isFile());
assert.ok((await stat(resolve(publicRoot, 'generated/chunks'))).isDirectory());
const contentTypes = new Map([
  ['.css', 'text/css'],
  ['.html', 'text/html'],
  ['.js', 'text/javascript'],
  ['.svg', 'image/svg+xml'],
]);
// The fixture has no local replica. Its fixed anonymous local agent may probe
// status and this one configured canister; neither is an application asset.
const expectedLocalProbe = (pathname) => pathname === '/api/v2/status'
  || /^\/api\/v3\/canister\/aaaaa-aa\/(query|read_state)$/.test(pathname);

async function serveFrontend() {
  const server = createServer(async (request, response) => {
    try {
      const pathname = new URL(request.url, 'http://localhost').pathname;
      const relative = pathname === '/' ? 'index.html' : pathname.slice(1);
      const path = resolve(publicRoot, relative);
      assert.ok(path.startsWith(`${publicRoot}/`) || path === resolve(publicRoot, 'index.html'));
      if (process.env.JUPITER_TEST_MISSING_APP_BUNDLE === '1'
        && path === resolve(publicRoot, manifest.bundlePath)) throw new Error('bundle deliberately omitted');
      assert.ok((await stat(path)).isFile());
      response.writeHead(200, { 'content-type': contentTypes.get(extname(path)) || 'application/octet-stream' });
      const omitNavbarCss = process.env.JUPITER_TEST_OMIT_NAVBAR_CSS === '1'
        && path === resolve(publicRoot, 'navbar.css');
      const breakBundle = process.env.JUPITER_TEST_BREAK_APP_BUNDLE === '1'
        && path === resolve(publicRoot, manifest.bundlePath);
      const bytes = await readFile(path);
      const body = ['.html', '.css'].includes(extname(path))
        ? renderFrontendAsset(bytes.toString('utf8'), 'frontend-browser-fixture', manifest.bundlePath)
        : bytes;
      response.end(omitNavbarCss ? '' : breakBundle ? 'export const broken = ;' : body);
    } catch {
      response.writeHead(404);
      response.end('not found');
    }
  });
  await new Promise((resolveListen) => server.listen(0, '127.0.0.1', resolveListen));
  const { port } = server.address();
  return { server, url: `http://127.0.0.1:${port}/` };
}

async function withBrowser(run) {
  const { server, url } = await serveFrontend();
  let browser;
  let primaryError;
  const checks = [];
  try {
    browser = await chromium.launch({ headless: true });
    await run({ openPage: (viewport) => openCheckedPage(browser, url, viewport, checks) });
    for (const check of checks) check();
  } catch (error) {
    primaryError = error;
    for (const check of checks) {
      try { check(); } catch (failure) {
        primaryError = new AggregateError([primaryError, failure], `browser scenario failed: ${error.message}; ${failure.message}`);
      }
    }
  }
  const cleanupErrors = [];
  try { await browser?.close(); } catch (error) { cleanupErrors.push(error); }
  try {
    await new Promise((resolveClose, rejectClose) => server.close((error) => error ? rejectClose(error) : resolveClose()));
  } catch (error) { cleanupErrors.push(error); }
  if (primaryError && cleanupErrors.length) throw new AggregateError([primaryError, ...cleanupErrors], `browser scenario failed: ${primaryError.message}; cleanup also failed`);
  if (primaryError) throw primaryError;
  if (cleanupErrors.length) throw new AggregateError(cleanupErrors, 'browser cleanup failed');
}

async function openCheckedPage(browser, url, viewport, checks) {
  const page = await browser.newPage({ viewport });
  const failures = [];
  const checkFailures = () => assert.deepEqual(failures, [], `application asset/load failure: ${failures.join(', ')}`);
  page.on('pageerror', (error) => failures.push(`pageerror: ${error.message}`));
  page.on('requestfailed', (request) => {
    const pathname = new URL(request.url()).pathname;
    if (expectedLocalProbe(pathname) && request.url().startsWith(url)) return;
    failures.push(`request failed: ${request.url()}: ${request.failure()?.errorText}`);
  });
  page.on('response', (response) => {
    const pathname = new URL(response.url()).pathname;
    if (expectedLocalProbe(pathname) && response.url().startsWith(url)) return;
    if (response.url().startsWith(url) && response.status() >= 400) failures.push(`asset ${response.status()}: ${response.url()}`);
  });
  await page.route('**/*', (route) => {
    if (route.request().url().startsWith(url)) return route.continue();
    failures.push(`unexpected external request: ${route.request().url()}`);
    return route.abort('blockedbyclient');
  });
  if (process.env.JUPITER_TEST_DISABLE_SIMULATOR_INPUT_EVENTS === '1') {
    await page.addInitScript(() => {
      const original = EventTarget.prototype.addEventListener;
      EventTarget.prototype.addEventListener = function (type, listener, options) {
        if (this?.id === 'commitment-simulator-form' && (type === 'input' || type === 'change')) return;
        return original.call(this, type, listener, options);
      };
    });
  }
  await page.goto(url, { waitUntil: 'load' });
  try {
    // The application module binds the simulator and renders an initial result.
    // Its non-placeholder result is an observable readiness signal; HTML load
    // alone does not prove the module or its dynamic imports have executed.
    await page.waitForFunction(() => {
      const value = document.querySelector('#simulator-required-commitment')?.textContent?.trim();
      return value && value !== '—';
    }, null, { timeout: 5000 });
  } catch (error) {
    checkFailures();
    throw error;
  }
  checkFailures();
  assert.equal(await page.locator('script[type="module"]').getAttribute('src'), `/${manifest.bundlePath}`);
  checks.push(checkFailures);
  return { page, checkFailures };
}

for (const width of [1440, 1024, 861, 720]) {
  test(`shipped navbar CSS aligns disclosure menus and preserves hit targets at ${width}px`, async () => {
    await withBrowser(async ({ openPage }) => {
      const { page, checkFailures } = await openPage({ width, height: 760 });

      const actions = page.locator('[data-nav-group="actions"] .nav-disclosure-toggle');
      const actionsMenu = page.locator('#actions-menu');
      const metrics = page.locator('[data-nav-group="metrics"] .nav-disclosure-toggle');
      const metricsMenu = page.locator('#metrics-menu');
      await actions.click();
      checkFailures();
      const [actionsBox, actionsMenuBox] = await Promise.all([actions.boundingBox(), actionsMenu.boundingBox()]);
      assert.ok(actionsBox && actionsMenuBox);
      assert.ok(Math.abs(actionsMenuBox.x - actionsBox.x) <= 2);
      assert.ok(actionsMenuBox.y >= actionsBox.y + actionsBox.height - 1);
      assert.ok(actionsMenuBox.x >= 0 && actionsMenuBox.x + actionsMenuBox.width <= width);
      const metricsHit = await page.evaluate(() => {
        const element = document.querySelector('[data-nav-group="metrics"] .nav-disclosure-toggle');
        const rect = element.getBoundingClientRect();
        return document.elementFromPoint(rect.left + rect.width / 2, rect.top + rect.height / 2) === element;
      });
      assert.equal(metricsHit, true);

      await metrics.click();
      checkFailures();
      const [metricsBox, metricsMenuBox] = await Promise.all([metrics.boundingBox(), metricsMenu.boundingBox()]);
      assert.ok(metricsBox && metricsMenuBox);
      assert.ok(Math.abs(metricsMenuBox.x + metricsMenuBox.width - metricsBox.x - metricsBox.width) <= 2);
      assert.ok(metricsMenuBox.y >= metricsBox.y + metricsBox.height - 1);
      assert.ok(metricsMenuBox.x >= 0 && metricsMenuBox.x + metricsMenuBox.width <= width);
      await page.keyboard.press('Escape');
      assert.equal(await metricsMenu.isVisible(), false);
      assert.equal(await metrics.getAttribute('aria-expanded'), 'false');
    });
  });
}

test('shipped navbar supports keyboard activation, focus restoration, panel dismissal and scrolling', async () => {
  await withBrowser(async ({ openPage }) => {
    const { page, checkFailures } = await openPage({ width: 720, height: 360 });
    const actions = page.locator('[data-nav-group="actions"] .nav-disclosure-toggle');
    await actions.focus();
    await page.keyboard.press('Enter');
    checkFailures();
    const simulator = page.locator('[data-nav-group="actions"] [data-panel="simulator"]');
    await simulator.focus();
    await page.keyboard.press('Enter');
    const backdrop = page.locator('#nav-panel-backdrop');
    assert.equal(await backdrop.evaluate((element) => element.classList.contains('is-open')), true);
    const panel = page.locator('#nav-panel-simulator');
    assert.equal(await panel.isVisible(), true);
    const scrollRegion = panel.locator('.nav-panel-scroll-region');
    const overflowY = await scrollRegion.evaluate((element) => getComputedStyle(element).overflowY);
    assert.ok(['auto', 'scroll'].includes(overflowY));
    const scroll = await scrollRegion.evaluate((element) => {
      const before = element.scrollTop;
      element.scrollTop = element.scrollHeight;
      return { before, after: element.scrollTop, height: element.scrollHeight, viewport: element.clientHeight };
    });
    assert.ok(scroll.height > scroll.viewport, `expected real overflow: ${JSON.stringify(scroll)}`);
    assert.ok(scroll.after > scroll.before, `expected changed scroll position: ${JSON.stringify(scroll)}`);
    await page.keyboard.press('Escape');
    assert.equal(await backdrop.evaluate((element) => element.classList.contains('is-open')), false);
    await page.evaluate(() => new Promise((resolveFrame) => requestAnimationFrame(resolveFrame)));
    assert.equal(await actions.evaluate((element) => document.activeElement === element), true);
    checkFailures();
  });
});

test('rendered simulator updates from user inputs and corner controls stay hittable over its backdrop', async () => {
  await withBrowser(async ({ openPage }) => {
    const { page, checkFailures } = await openPage({ width: 1024, height: 650 });
    await page.locator('.nav-item--simulator').click();
    const panel = page.locator('#nav-panel-simulator');
    assert.equal(await panel.isVisible(), true);
    const result = panel.locator('#simulator-required-commitment');
    const before = (await result.textContent()).trim();
    assert.notEqual(before, '—');
    await panel.locator('#simulator-icp-commitment').fill('100');
    await panel.locator('#simulator-daily-burn').fill('0.01');
    await panel.locator('#simulator-icp-price').fill('5');
    await panel.locator('#simulator-apy').fill('7');
    // Independently: 0.01 T/day * 365 / (5 T/ICP) / 0.07 APY,
    // rounded upward to the nearest e8s, is 10.42857143 ICP.
    await page.waitForFunction(() => document.querySelector('#simulator-required-commitment')?.textContent?.trim() === '10.42857143 ICP', null, { timeout: 3000 }).catch(() => {});
    assert.equal((await result.textContent()).trim(), '10.42857143 ICP');
    assert.notEqual(before, '10.42857143 ICP');
    checkFailures();
    await panel.locator('#simulator-daily-burn').fill('0.02');
    await page.waitForFunction(() => document.querySelector('#simulator-required-commitment')?.textContent?.trim() === '20.85714286 ICP', null, { timeout: 3000 }).catch(() => {});
    assert.equal((await result.textContent()).trim(), '20.85714286 ICP');
    checkFailures();
    await page.evaluate(() => window.scrollTo(0, document.documentElement.scrollHeight));
    await page.waitForFunction(() => getComputedStyle(document.querySelector('.github-corner')).visibility === 'visible');
    for (const [selector, xRatio, panelSelector] of [
      ['.github-corner', 0.85, '#nav-panel-source'],
      ['.parthenon-corner', 0.15, '#nav-panel-governance'],
    ]) {
      const hit = await page.locator(selector).evaluate((element) => {
        const box = element.getBoundingClientRect();
        const xRatio = element.classList.contains('github-corner') ? 0.85 : 0.15;
        const found = document.elementFromPoint(box.left + box.width * xRatio, box.top + box.height * 0.85);
        return { hittable: element.contains(found), element: found?.outerHTML.slice(0, 160), box: box.toJSON() };
      });
      assert.equal(hit.hittable, true, `${selector} should remain hittable: ${JSON.stringify(hit)}`);
      const box = await page.locator(selector).boundingBox();
      await page.mouse.click(box.x + box.width * xRatio, box.y + box.height * 0.85);
      assert.equal(await page.locator(panelSelector).isVisible(), true);
      checkFailures();
    }
  });
});

test('missing or broken rendered application bundle fails the load gate', async () => {
  for (const mode of ['JUPITER_TEST_MISSING_APP_BUNDLE', 'JUPITER_TEST_BREAK_APP_BUNDLE']) {
    const previous = process.env[mode];
    process.env[mode] = '1';
    try {
      await withBrowser(async ({ openPage }) => {
        await assert.rejects(openPage({ width: 720, height: 500 }),
          /application asset\/load failure/);
      });
    } finally {
      if (previous === undefined) delete process.env[mode];
      else process.env[mode] = previous;
    }
  }
});

test('a late application exception fails a geometrically sound navbar scenario', async () => {
  await assert.rejects(withBrowser(async ({ openPage }) => {
    const { page } = await openPage({ width: 720, height: 500 });
    const pageError = page.waitForEvent('pageerror');
    await page.evaluate(() => setTimeout(() => { throw new Error('late-app-fault-probe'); }, 0));
    await pageError;
    const actions = page.locator('[data-nav-group="actions"] .nav-disclosure-toggle');
    await actions.click();
    assert.equal(await page.locator('#actions-menu').isVisible(), true);
  }), /late-app-fault-probe/);
});
