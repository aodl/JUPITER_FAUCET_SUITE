import test from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { readFile, stat, mkdir } from 'node:fs/promises';
import { extname, resolve } from 'node:path';
import { execFileSync } from 'node:child_process';
import { chromium } from '@playwright/test';
import { buildSync } from 'esbuild';
import { renderFrontendAsset } from '../../../../tools/scripts/frontend-render.mjs';

const publicRoot = resolve(import.meta.dirname, '../../public');
const repoRoot = resolve(publicRoot, '../../..');
// Never run the browser fixture with the production-network bundle. The local
// build is deterministic and all non-asset requests are blocked below.
execFileSync('npm', ['run', 'build:frontend'], {
  cwd: repoRoot,
  env: { ...process.env, JUPITER_FRONTEND_NETWORK: 'local', CANISTER_ID_JUPITER_HISTORIAN: 'aaaaa-aa', CANISTER_ID_MOCK_ICRC_LEDGER: 'aaaaa-aa' },
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
const relayFixture = buildSync({
  entryPoints: [resolve(import.meta.dirname, 'support/relay-browser-fixture.mjs')],
  bundle: true, write: false, format: 'esm', platform: 'browser',
}).outputFiles[0].text;
// The fixture has no replica. Failed local status/query probes and certified
// reads for the displayed Source IDs are API unavailability, not missing assets.
// Keep other canister queries and all external requests forbidden.
const sourceCanisterIds = new Set([...(
  await readFile(resolve(publicRoot, 'index.html'), 'utf8')
).matchAll(/data-source-(?:module-hash|controllers)="([^"]+)"/g)].map((match) => match[1]));
const expectedLocalProbe = (pathname) => pathname === '/api/v2/status'
  || /^\/api\/v3\/canister\/aaaaa-aa\/(query|read_state)$/.test(pathname)
  || sourceCanisterIds.has(/^\/api\/v3\/canister\/([^/]+)\/read_state$/.exec(pathname)?.[1]);

test('browser fixture permits only configured probes and displayed certified paths', () => {
  assert.ok(sourceCanisterIds.size > 0);
  for (const id of sourceCanisterIds) {
    assert.equal(expectedLocalProbe(`/api/v3/canister/${id}/read_state`), true);
    assert.equal(expectedLocalProbe(`/api/v3/canister/${id}/query`), false);
  }
  assert.equal(expectedLocalProbe('/api/v3/canister/ryjl3-tyaaa-aaaaa-aaaba-cai/read_state'), false);
  assert.equal(expectedLocalProbe('/api/v3/canister/aaaaa-aa/call'), false);
  assert.equal(expectedLocalProbe('/generated/missing.js'), false);
});

async function serveFrontend() {
  const server = createServer(async (request, response) => {
    try {
      const pathname = new URL(request.url, 'http://localhost').pathname;
      if (pathname === '/__relay-fixture.js') {
        response.writeHead(200, { 'content-type': 'text/javascript' });
        response.end(relayFixture);
        return;
      }
      if (pathname === '/__relay-fixture.html') {
        const html = await readFile(resolve(publicRoot, 'index.html'), 'utf8');
        response.writeHead(200, { 'content-type': 'text/html' });
        response.end(renderFrontendAsset(html, 'relay-fixture', '__relay-fixture.js'));
        return;
      }
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
    await run({
      openPage: (viewport) => openCheckedPage(browser, url, viewport, checks),
      openRelay: (viewport) => openCheckedPage(browser, url, viewport, checks, true),
    });
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

async function openCheckedPage(browser, url, viewport, checks, relay = false) {
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
  await page.goto(relay ? `${url}__relay-fixture.html#relay-setup` : url, { waitUntil: 'load' });
  try {
    if (relay) {
      await page.waitForFunction(() => Boolean(window.relayFixture));
      await page.locator('#page-loading-overlay').waitFor({ state: 'detached', timeout: 10000 });
    }
    // The application module binds the simulator and renders an initial result.
    // Its non-placeholder result is an observable readiness signal; HTML load
    // alone does not prove the module or its dynamic imports have executed.
    else await page.waitForFunction(() => {
      const value = document.querySelector('#simulator-required-commitment')?.textContent?.trim();
      return value && value !== '—';
    }, null, { timeout: 5000 });
  } catch (error) {
    checkFailures();
    throw error;
  }
  checkFailures();
  assert.equal(await page.locator('script[type="module"]').getAttribute('src'), relay ? '/__relay-fixture.js' : `/${manifest.bundlePath}`);
  checks.push(checkFailures);
  return { page, checkFailures };
}

async function captureRelay(page, width, name, focusSelector = null) {
  if (!process.env.JUPITER_RELAY_SCREENSHOTS) return;
  if (focusSelector) await page.locator(focusSelector).scrollIntoViewIfNeeded();
  else await page.locator('#nav-panel-relay-setup .nav-panel-scroll-region').evaluate((node) => { node.scrollTop = 0; });
  await mkdir(process.env.JUPITER_RELAY_SCREENSHOTS, { recursive: true });
  await page.screenshot({ path: resolve(process.env.JUPITER_RELAY_SCREENSHOTS, `${width}-${name}.png`) });
}

async function configureCycles(page) {
  await page.locator('[data-relay-target-input]').fill('22255-zqaaa-aaaas-qf6uq-cai');
  await page.locator('#relay-setup-mode-all-cycles').check();
}

async function assertRelayFits(page, width) {
  const overflow = await page.locator('#nav-panel-relay-setup').evaluate((node) => ({
    scroll: node.scrollWidth, client: node.clientWidth,
  }));
  assert.ok(overflow.scroll <= overflow.client + 1, `${width}px overflow: ${JSON.stringify(overflow)}`);
  for (const button of await page.locator('#nav-panel-relay-setup button:visible').all()) {
    const box = await button.boundingBox();
    assert.ok(box.x >= 0 && box.x + box.width <= width + 1, `clipped button: ${await button.textContent()}`);
  }
}

for (const width of [390, 1440]) {
  test(`Relay journey preserves binary memos, explicit creation and outcome funding at ${width}px`, async () => {
    await withBrowser(async ({ openRelay }) => {
      const { page } = await openRelay({ width, height: width === 390 ? 844 : 1000 });
      assert.equal(await page.locator('#relay-setup-mode-routing').isChecked(), false);
      assert.equal(await page.locator('#relay-setup-mode-all-cycles').isChecked(), false);
      assert.equal(await page.locator('#relay-setup-submit').isDisabled(), true);
      assert.equal(await page.locator('#relay-setup-recipient-editor').isVisible(), false);
      await captureRelay(page, width, 'initial');
      await configureCycles(page);
      await captureRelay(page, width, 'cycles-ready');
      await page.locator('#relay-setup-mode-routing').check();
      await page.locator('[data-relay-recipient-input]').fill('bg4sm-wzk');
      const memo = page.locator('[data-relay-recipient-memo]');
      await memo.locator('summary').click();
      await page.locator('select[data-relay-recipient-memo-mode]').selectOption('Hex');
      await page.locator('[data-relay-recipient-memo-input]').fill('00ff80fe');
      await captureRelay(page, width, 'memo-expanded', '[data-relay-recipient-memo-input]');
      await page.locator('select[data-relay-recipient-memo-mode]').selectOption('Text');
      assert.equal(await page.locator('select[data-relay-recipient-memo-mode]').inputValue(), 'Hex');
      assert.equal(await page.locator('[data-relay-recipient-memo-input]').inputValue(), '00ff80fe');
      await memo.locator('summary').click();
      assert.match(await memo.locator('summary').textContent(), /Memo added.*4 bytes/);
      await captureRelay(page, width, 'memo-collapsed', '[data-relay-recipient-memo]');
      await page.locator('#relay-setup-mode-all-cycles').check();
      await page.locator('#relay-setup-submit').click();
      await page.waitForFunction(() => window.relayFixture.controller.state.checkedConfigurationFingerprint);
      assert.deepEqual(await page.evaluate(() => window.relayFixture.calls.args.at(-1).recipients), []);
      assert.deepEqual(await page.evaluate(() => window.relayFixture.calls.neurons), []);
      assert.equal(await page.locator('#relay-setup-form').isVisible(), false);
      assert.equal(await page.locator('#relay-setup-create').isDisabled(), true);
      assert.equal(await page.locator('#relay-setup-shortfall').textContent(), '2 ICP');
      await captureRelay(page, width, 'review-below');
      const fundingDetails = page.locator('#relay-setup-payment-details details').filter({ hasText: 'How setup funding is used' });
      await fundingDetails.locator('summary').click();
      assert.match(await fundingDetails.textContent(), /forwarded once the endowment threshold is met/);
      assert.equal(await fundingDetails.getByRole('link', { name: 'memo tracker' }).getAttribute('href'), '/#metric-tracker');
      await captureRelay(page, width, 'setup-funding-explanation', '#relay-setup-payment-details details[open]');
      await fundingDetails.locator('summary').click();
      assert.equal(await page.locator('.relay-setup-technical, #relay-setup-phase, #relay-setup-configuration-hash, #relay-setup-factory').count(), 0);
      await captureRelay(page, width, 'payment-account', '#copy-relay-setup-icrc-account');
      await page.locator('#relay-setup-edit').click();
      assert.equal(await page.locator('#relay-setup-payment-details').isVisible(), false);
      await page.locator('#relay-setup-mode-routing').check();
      assert.equal(await page.locator('[data-relay-recipient-memo-input]').inputValue(), '00ff80fe');
      await page.locator('#relay-setup-submit').click();
      await page.waitForFunction(() => window.relayFixture.controller.state.checkedConfigurationFingerprint);
      assert.match(await page.locator('#relay-setup-canonical-recipients').textContent(), /00ff80fe/);
      await page.evaluate(() => { window.relayFixture.setBalance('600000000'); window.relayFixture.poll(); });
      await page.waitForFunction(() => !document.querySelector('#relay-setup-create').disabled);
      assert.equal(await page.locator('#relay-setup-shortfall').textContent(), '0 ICP');
      assert.equal(await page.evaluate(() => window.relayFixture.calls.notifications), 0);
      await captureRelay(page, width, 'review-funded');
      await captureRelay(page, width, 'create-ready', '#relay-setup-create');
      await page.locator('#copy-relay-setup-icrc-account').click();
      assert.equal(await page.evaluate(() => window.relayFixture.calls.copied.at(-1)), await page.locator('#relay-setup-icrc-account').textContent());
      await page.evaluate(() => window.relayFixture.holdNotify());
      await page.locator('#relay-setup-create').click();
      await page.waitForFunction(() => window.relayFixture.calls.notifications === 1);
      assert.equal(await page.locator('#relay-setup-edit').isDisabled(), true);
      await page.locator('#relay-setup-edit').dispatchEvent('click');
      assert.equal(await page.locator('#relay-setup-form').isVisible(), false);
      await page.evaluate(() => window.relayFixture.releaseNotify());
      await page.locator('#relay-funding-section').waitFor({ state: 'visible' });
      assert.equal(await page.locator('#relay-funding-title').evaluate((n) => document.activeElement === n), true);
      assert.equal(await page.evaluate(() => window.relayFixture.calls.notifications), 1);
      assert.equal(await page.locator('#relay-setup-review').isVisible(), false);
      assert.equal(await page.locator('#relay-setup-payment-details').isVisible(), false);
      await captureRelay(page, width, 'active-overview');
      await page.locator('#relay-funding-add').click();
      assert.equal(await page.locator('.relay-funding-card:visible').count(), 1);
      assert.match(await page.locator('.relay-funding-card h4').textContent(), /Operating payment account/);
      await captureRelay(page, width, 'fund-operating', '.relay-funding-card');
      await page.locator('#relay-funding-future').check();
      assert.equal(await page.locator('.relay-funding-card:visible').count(), 0);
      const link = await page.locator('#relay-funding-memo-builder-link').getAttribute('href');
      assert.equal(new URLSearchParams(link.split('?')[1]).get('mode'), 'rawIcp');
      assert.equal(new URLSearchParams(link.split('?')[1]).get('canister'), 'br5f7-7uaaa-aaaaa-qaaca-cai');
      await captureRelay(page, width, 'fund-future', '#relay-funding-memo-builder-link');
      await page.locator('#relay-funding-staging-alternative summary').click();
      await page.locator('.relay-funding-card').waitFor({ state: 'visible' });
      assert.equal(await page.locator('.relay-funding-card:visible').count(), 1);
      assert.match(await page.locator('.relay-funding-card h4').textContent(), /Relay staging account/);
      await captureRelay(page, width, 'fund-staging', '.relay-funding-card');
      await page.locator('#relay-funding-split').check();
      for (const percentage of [10, 20, 30, 40, 50, 60, 70, 80, 90]) {
        await page.locator('#relay-funding-percentage').selectOption(String(percentage));
        assert.equal(await page.locator('.relay-funding-card:visible').count(), 1);
        await page.locator('.relay-funding-card > .relay-funding-value button').click();
        assert.equal(await page.evaluate(() => window.relayFixture.calls.copied.at(-1)), await page.evaluate((p) => window.relayFixture.controller.state.fundingDestinations.find((d) => d.subaccountNumber === p).icrcAccountText, percentage));
      }
      await page.locator('#relay-funding-percentage').selectOption('50');
      await captureRelay(page, width, 'fund-split-50', '#relay-funding-percentage');
      await page.locator('#relay-setup-another').click();
      assert.equal(await page.locator('[data-relay-target-input]').count(), 1);
      assert.equal(await page.locator('[data-relay-target-input]').inputValue(), '');
      assert.equal(await page.locator('#relay-setup-mode-routing').isChecked(), false);
      assert.equal(await page.locator('#relay-setup-mode-all-cycles').isChecked(), false);
      assert.equal(await page.locator('[data-relay-recipient-input]').inputValue(), '');
      assert.equal(await page.locator('[data-relay-recipient-memo-input]').inputValue(), '');
      assert.equal(await page.locator('[data-relay-recipient-memo]').evaluate((n) => n.open), false);
      assert.equal(await page.locator('#relay-setup-submit').isDisabled(), true);
      assert.equal(await page.locator('#relay-setup-payment-details').isVisible(), false);
      assert.equal(await page.locator('#relay-funding-options').isVisible(), false);
      assert.equal(await page.locator('[data-relay-target-input]').evaluate((n) => n === document.activeElement), true);
      await assertRelayFits(page, width);
    });
  });

  test(`Relay progress and recovery are distinct and do not expose payment at ${width}px`, async () => {
    await withBrowser(async ({ openRelay }) => {
      const { page } = await openRelay({ width, height: width === 390 ? 844 : 1000 });
      await configureCycles(page);
      for (const [kind, phase, label] of [
        ['InProgress', 'CreateDispatched', 'in-progress'],
        ['InProgress', 'CmcTransferAccepted', 'resume-creation'],
        ['InProgress', 'RelayFunded', 'resume-finalization'],
        ['ManualRecoveryRequired', 'CreateDispatched', 'manual-recovery'],
      ]) {
        await page.evaluate(([k, p]) => window.relayFixture.setState(k, p), [kind, phase]);
        if (await page.locator('#relay-setup-form').isHidden()) await page.locator('#relay-setup-edit').click();
        await page.locator('#relay-setup-submit').click();
        await page.waitForFunction(() => window.relayFixture.controller.state.checkedConfigurationFingerprint);
        assert.equal(await page.locator('#relay-setup-payment-details').isVisible(), false);
        assert.equal(await page.locator('#relay-funding-section').isVisible(), false);
        const resumable = label.startsWith('resume-');
        assert.equal(await page.locator('#relay-setup-create').isVisible(), resumable);
        if (resumable) assert.match(await page.locator('#relay-setup-create').textContent(), /Resume/);
        await captureRelay(page, width, label);
        await assertRelayFits(page, width);
      }
      assert.equal(await page.evaluate(() => window.relayFixture.calls.notifications), 0);
      await page.locator('#relay-setup-edit').click();
      await page.evaluate(() => {
        window.relayFixture.setState('NotFunded');
        window.relayFixture.setBalance('600000000');
        window.relayFixture.setNotify({ FailedPreSpend: { message: 'Target probe failed before spending.' } });
      });
      await page.locator('#relay-setup-submit').click();
      await page.waitForFunction(() => !document.querySelector('#relay-setup-create').disabled);
      await page.locator('#relay-setup-create').click();
      await page.waitForFunction(() => document.querySelector('#relay-setup-status').textContent === 'Failed before spend');
      assert.equal(await page.locator('#relay-setup-payment-details').isVisible(), true);
      await captureRelay(page, width, 'failed-before-spend');
      await page.evaluate(() => window.relayFixture.setNotify({ Busy: null }));
      await page.locator('#relay-setup-create').click();
      await page.waitForFunction(() => document.querySelector('#relay-setup-status').textContent === 'Busy');
      await captureRelay(page, width, 'busy');
      assert.equal(await page.evaluate(() => window.relayFixture.calls.notifications), 2);
    });
  });
}

for (const width of [390, 1440]) {
  test(`Relay styling follows the memo builder without changing tracker controls at ${width}px`, async () => {
    await withBrowser(async ({ openPage }) => {
      const { page } = await openPage({ width, height: width === 390 ? 844 : 1000 });
      const signature = async (field) => {
        await field.focus();
        return field.evaluate((node) => {
          const style = getComputedStyle(node);
          return Object.fromEntries(['borderRadius', 'borderTopWidth', 'borderTopColor', 'backgroundColor',
            'paddingTop', 'paddingRight', 'paddingBottom', 'paddingLeft', 'fontFamily', 'fontSize', 'lineHeight']
            .map((property) => [property, style[property]]));
        });
      };
      await page.evaluate(() => { window.location.hash = '#memo-builder'; });
      const memo = page.locator('#memo-builder-canister');
      await memo.waitFor({ state: 'visible' });
      const expected = await signature(memo);
      await captureRelay(page, width, 'memo-builder-fields', '#memo-builder-canister');
      await page.evaluate(() => { window.location.hash = '#metric-tracker'; });
      const tracker = page.locator('#tracker-principal-input');
      await tracker.waitFor({ state: 'visible' });
      const trackerStyle = await signature(tracker);
      assert.equal(trackerStyle.borderRadius, '999px');
      assert.equal(trackerStyle.fontSize, '14px');
      assert.equal(trackerStyle.paddingLeft, '13px');
      assert.notDeepEqual(trackerStyle, expected);
      assert.equal(await tracker.evaluate((node) => getComputedStyle(node).outlineStyle), 'none');
      await page.evaluate(() => { window.location.hash = '#simulator'; });
      const simulator = page.locator('#simulator-icp-commitment');
      await simulator.waitFor({ state: 'visible' });
      assert.equal((await signature(simulator)).borderRadius, '999px');
      assert.equal((await signature(simulator)).fontFamily, 'Arial');
      assert.equal((await signature(simulator)).fontSize, '13px');
      assert.equal((await signature(simulator)).paddingLeft, '10px');
      await page.evaluate(() => { window.location.hash = '#metric-tracker'; });
      await captureRelay(page, width, 'tracker-field', '#tracker-principal-input');
      await page.evaluate(() => { window.location.hash = '#relay-setup'; });
      const target = page.locator('[data-relay-target-input]').first();
      await target.waitFor({ state: 'visible' });
      assert.deepEqual(await signature(target), expected);
      await target.fill('22255-zqaaa-aaaas-qf6uq-cai');
      await page.locator('#relay-setup-add-target').click();
      assert.deepEqual(await signature(page.locator('[data-relay-target-input]').nth(1)), expected);
      await page.locator('[data-relay-target-input]').nth(1).fill('qaa6y-5yaaa-aaaaa-aaafa-cai');
      await captureRelay(page, width, 'relay-fields', '#relay-setup-target-1');
      await page.locator('#relay-setup-mode-routing').check();
      const recipient = page.locator('[data-relay-recipient-input]').first();
      await recipient.fill('bg4sm-wzk');
      assert.deepEqual(await signature(recipient), expected);
      await page.locator('[data-relay-recipient-memo] summary').first().click();
      assert.deepEqual(await signature(page.locator('[data-relay-recipient-memo-input]').first()), expected);
      await captureRelay(page, width, 'relay-memo-field', '[data-relay-recipient-memo-input]');
      await assertRelayFits(page, width);
    });
  });

  test(`native dropdown options remain readable without hovering at ${width}px`, async () => {
    await withBrowser(async ({ openRelay }) => {
      const { page } = await openRelay({ width, height: width === 390 ? 844 : 1000 });
      await page.locator('[data-relay-target-input]').fill('22255-zqaaa-aaaas-qf6uq-cai');
      await page.locator('#relay-setup-mode-routing').check();
      await page.locator('[data-relay-recipient-input]').fill('bg4sm-wzk');
      await page.locator('[data-relay-recipient-memo] summary').click();
      const inspect = async (selector, name) => {
        const select = page.locator(selector);
        const theme = await select.evaluate((node) => ({
          scheme: getComputedStyle(node).colorScheme,
          options: [...node.options].map((option) => ({
            color: getComputedStyle(option).color,
            background: getComputedStyle(option).backgroundColor,
          })),
        }));
        assert.match(theme.scheme, /dark/);
        const luminance = (value) => {
          const rgb = value.match(/[\d.]+/g).map(Number);
          assert.equal(rgb[3] ?? 1, 1, 'option colors must be opaque');
          return rgb.slice(0, 3).map((channel) => {
            const s = channel / 255;
            return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
          }).reduce((sum, channel, i) => sum + channel * [0.2126, 0.7152, 0.0722][i], 0);
        };
        for (const option of theme.options) {
          const a = luminance(option.color), b = luminance(option.background);
          assert.ok((Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05) >= 4.5, `${name}: insufficient option contrast`);
        }
        await select.scrollIntoViewIfNeeded();
        await select.click();
        if (process.env.JUPITER_RELAY_SCREENSHOTS) {
          await page.screenshot({ path: resolve(process.env.JUPITER_RELAY_SCREENSHOTS, `${width}-${name}-open.png`) });
        }
        await page.keyboard.press('Escape');
      };
      await inspect('select[data-relay-recipient-type]', 'recipient-dropdown');
      await inspect('select[data-relay-recipient-memo-mode]', 'memo-format-dropdown');
      await page.locator('#relay-setup-submit').click();
      await page.waitForFunction(() => window.relayFixture.controller.state.checkedConfigurationFingerprint);
      await page.evaluate(async () => {
        window.relayFixture.setState('Active');
        await window.relayFixture.controller.refresh();
      });
      await page.locator('#relay-funding-add').click();
      await page.locator('#relay-funding-split').check();
      await inspect('#relay-funding-percentage', 'funding-split-dropdown');
      assert.equal(await page.evaluate(() => window.relayFixture.calls.notifications), 0);
    });
  });
}

test('Relay late response cannot restore an edited payment account or regress Active', async () => {
  await withBrowser(async ({ openRelay }) => {
    const { page } = await openRelay({ width: 390, height: 844 });
    await configureCycles(page);
    await page.evaluate(() => window.relayFixture.holdQuery());
    await page.locator('#relay-setup-submit').click();
    await page.waitForFunction(() => window.relayFixture.calls.views === 1);
    await page.locator('#relay-setup-edit').click();
    await page.locator('[data-relay-target-input]').fill('qaa6y-5yaaa-aaaaa-aaafa-cai');
    await page.evaluate(() => window.relayFixture.setState('Active'));
    await page.locator('#relay-setup-submit').click();
    await page.locator('#relay-funding-section').waitFor({ state: 'visible' });
    await page.evaluate(() => window.relayFixture.releaseQuery());
    assert.equal(await page.locator('#relay-setup-payment-details').isVisible(), false);
    assert.equal(await page.locator('#relay-funding-section').isVisible(), true);
    assert.deepEqual(await page.evaluate(() => window.relayFixture.controller.state.view.canonical_target_canister_ids.map((p) => p.toText())), ['qaa6y-5yaaa-aaaaa-aaafa-cai']);
  });
});

test('Relay error focus reveals invalid memos and explicit removal preserves no-memo semantics', async () => {
  await withBrowser(async ({ openRelay }) => {
    const { page } = await openRelay({ width: 390, height: 844 });
    await configureCycles(page);
    await page.locator('#relay-setup-add-target').click();
    await page.locator('[data-relay-target-input]').nth(1).fill('22255-zqaaa-aaaas-qf6uq-cai');
    assert.equal(await page.locator('#relay-setup-submit').isDisabled(), true);
    await page.locator('#relay-setup-error-focus').click();
    assert.equal(await page.locator('[data-relay-target-input]').first().evaluate((n) => n === document.activeElement), true);
    await page.locator('[data-relay-target-remove]').nth(1).click();
    await page.locator('#relay-setup-mode-routing').check();
    await page.locator('[data-relay-recipient-input]').fill('bg4sm-wzk');
    const memo = page.locator('[data-relay-recipient-memo]');
    await memo.locator('summary').click();
    await page.locator('select[data-relay-recipient-memo-mode]').selectOption('Hex');
    await page.locator('[data-relay-recipient-memo-input]').fill('ff'.repeat(32));
    assert.equal(await page.locator('#relay-setup-submit').isEnabled(), true);
    await page.locator('[data-relay-recipient-memo-input]').fill('ff'.repeat(33));
    assert.equal(await page.locator('#relay-setup-submit').isDisabled(), true);
    await memo.locator('summary').click();
    await page.locator('#relay-setup-form').evaluate((form) => form.requestSubmit());
    assert.equal(await memo.evaluate((node) => node.open), true);
    assert.equal(await page.locator('[data-relay-recipient-memo-input]').evaluate((n) => n === document.activeElement), true);
    assert.equal(await page.evaluate(() => window.relayFixture.calls.views), 0);
    await page.locator('[data-relay-recipient-memo-remove]').click();
    assert.equal(await page.locator('[data-relay-recipient-memo-input]').inputValue(), '');
    await page.locator('#relay-setup-submit').click();
    await page.waitForFunction(() => window.relayFixture.controller.state.checkedConfigurationFingerprint);
    assert.deepEqual(await page.evaluate(() => Array.from(window.relayFixture.calls.args.at(-1).recipients[0].Principal.memo)), []);
  });
});

for (const width of [320, 768]) {
  test(`Relay configuration and funding fit ${width}px and support keyboard review`, async () => {
    await withBrowser(async ({ openRelay }) => {
      const { page } = await openRelay({ width, height: 1000 });
      await configureCycles(page);
      await assertRelayFits(page, width);
      await page.locator('#relay-setup-submit').focus();
      await page.keyboard.press('Enter');
      await page.waitForFunction(() => window.relayFixture.controller.state.checkedConfigurationFingerprint);
      assert.equal(await page.locator('#relay-setup-review-heading').evaluate((n) => document.activeElement === n), true);
      await page.locator('#relay-setup-edit').click();
      await page.locator('#relay-setup-mode-routing').check();
      await page.locator('[data-relay-recipient-input]').fill('bg4sm-wzk');
      const size = await page.locator('#relay-setup-recipient-editor').evaluate((node) => ({ width: node.clientWidth, scroll: node.scrollWidth }));
      assert.ok(size.scroll <= size.width + 1, `recipient editor overflow at ${width}px: ${JSON.stringify(size)}`);
      await assertRelayFits(page, width);
    });
  });
}

for (const width of [390, 1440]) {
  test(`Relay all-cycles Review action is visible and clickable at ${width}px`, async () => {
    await withBrowser(async ({ openPage }) => {
      const { page, checkFailures } = await openPage({ width, height: 844 });
      await page.evaluate(() => { window.location.hash = '#relay-setup'; });
      await page.locator('[data-relay-target-input]').fill('22255-zqaaa-aaaas-qf6uq-cai');
      await page.locator('#relay-setup-mode-all-cycles').check();
      const review = page.locator('#relay-setup-submit');
      assert.equal(await review.isEnabled(), true);
      assert.equal(await review.isVisible(), true);
      // Observe a real form submission without dispatching a backend request.
      await page.evaluate(() => {
        document.querySelector('#relay-setup-form').addEventListener('submit', (event) => {
          event.preventDefault();
          event.stopImmediatePropagation();
          window.relayReviewClicked = true;
        }, { capture: true });
      });
      await review.click();
      assert.equal(await page.evaluate(() => window.relayReviewClicked), true);
      assert.equal(await page.locator('#relay-setup-recipient-editor').isVisible(), false);
      checkFailures();
    });
  });
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
      if (panelSelector === '#nav-panel-source') {
        await page.waitForFunction(() => {
          const fields = [...document.querySelectorAll('[data-source-module-hash], [data-source-controllers]')];
          return fields.length > 0 && fields.every((field) => field.textContent === 'Unavailable');
        });
      }
      checkFailures();
    }
  });
});

test('same-page memo-builder navigation clears an omitted destination subaccount', async () => {
  await withBrowser(async ({ openPage }) => {
    const { page, checkFailures } = await openPage({ width: 1024, height: 650 });
    const owner = '22255-zqaaa-aaaas-qf6uq-cai';
    const memoOwner = '22255zqaaaaaaasqf6uqcai';
    await page.evaluate((hash) => { window.location.hash = hash; }, `#memo-builder?canister=${owner}&subaccount=50`);
    await page.waitForFunction((output) => document.querySelector('#memo-builder-output')?.value === output,
      `${memoOwner}:50.`);
    await page.evaluate((hash) => { window.location.hash = hash; }, `#memo-builder?canister=${owner}`);
    await page.waitForFunction((output) => document.querySelector('#memo-builder-output')?.value === output,
      `${memoOwner}.`);
    assert.equal(await page.locator('#memo-builder-raw-subaccount').inputValue(), '');
    assert.equal(await page.locator('#memo-builder-raw-subaccount').isEditable(), true);
    assert.equal(await page.locator('#memo-builder-copy').isEnabled(), true);
    checkFailures();
  });
});

test('shipped tracker bundle preserves invalid hash validation after obsolete raw load settles', async () => {
  await withBrowser(async ({ openPage }) => {
    const { page, checkFailures } = await openPage({ width: 1024, height: 650 });
    const owner = 'gvey7-gyaaa-aaaar-qb4fq-cai';
    const validMemo = `${owner}:50.d`;
    const invalidMemo = `${owner}:0`;
    const expected = 'Raw ICP subaccount memos require a full stop after the number. Use owner:number. for an empty outgoing memo.';
    const origin = new URL(page.url()).origin;
    const routePattern = '**/api/v3/canister/aaaaa-aa/**';
    const methodMarker = Buffer.from('get_raw_icp_commitment_history', 'utf8');
    let releaseRequest;
    let markRequestEntered;
    let enteredRequests = 0;
    let completedHandlers = 0;
    const gate = new Promise((resolveGate) => { releaseRequest = resolveGate; });
    const requestEntered = new Promise((resolveEntered) => { markRequestEntered = resolveEntered; });
    const handlerTasks = [];
    const withDeadline = async (promise, label) => {
      let timer;
      try {
        return await Promise.race([
          promise,
          new Promise((_, reject) => {
            timer = setTimeout(() => reject(new Error(`Timed out waiting for ${label}`)), 30_000);
          }),
        ]);
      } finally {
        clearTimeout(timer);
      }
    };
    const handler = (route) => {
      const request = route.request();
      const url = new URL(request.url());
      // This fixed test's suffix is "d", so the method marker cannot come from its memo.
      const isRawHistory = url.origin === origin
        && url.pathname === '/api/v3/canister/aaaaa-aa/query'
        && request.method() === 'POST'
        && request.postDataBuffer()?.includes(methodMarker);
      if (!isRawHistory) return route.fallback();
      const task = (async () => {
        enteredRequests += 1;
        markRequestEntered();
        await gate;
        await route.abort('failed');
        completedHandlers += 1;
      })();
      handlerTasks.push(task);
      return task;
    };

    await page.route(routePattern, handler);
    try {
      await page.evaluate(() => { window.location.hash = '#metric-tracker'; });
      await page.waitForFunction(() => (
        document.querySelector('#nav-panel-metric-tracker')?.classList.contains('nav-panel-section--active')
        && document.querySelector('#tracker-form')?.dataset.bound === 'true'
      ), null, { timeout: 30_000 });

      // Observe the real loader's aggregate promise. Do not replace its result,
      // its input promises, the loader, the controller or any application asset.
      await page.evaluate((memo) => {
        if (window.__jupiterRawLoadObserver) throw new Error('Raw-load observer already installed');
        const originalAllSettled = Promise.allSettled;
        const channels = new Set();
        let disposed = false;
        const probe = {
          joins: 0,
          settled: false,
          continuationsDrained: false,
          outcomeStatuses: [],
          error: null,
        };
        function observeAllSettled(values) {
          const aggregate = Reflect.apply(originalAllSettled, this, [values]);
          // tracker-history.js has one five-input aggregate: the raw-canister loader.
          // Other application loaders use three, four or six inputs. Ambiguity fails.
          if (!disposed && Array.isArray(values) && values.length === 5
            && document.querySelector('#tracker-principal-input')?.value === memo) {
            probe.joins += 1;
            if (probe.joins !== 1) probe.error = 'More than one matching raw-load aggregate was observed';
            void aggregate.then((outcomes) => {
              if (disposed) return;
              probe.settled = true;
              probe.outcomeStatuses = outcomes.map((outcome) => outcome.status);
              // The loader returns synchronously after this join. The awaiting
              // controller also finishes without another await. A task boundary
              // therefore follows both promise continuations, without a sleep.
              const channel = new MessageChannel();
              channels.add(channel);
              channel.port1.onmessage = () => {
                if (!disposed) probe.continuationsDrained = true;
                channel.port1.close();
                channel.port2.close();
                channels.delete(channel);
              };
              channel.port2.postMessage(null);
            }).catch((error) => {
              if (!disposed) probe.error = String(error?.message || error);
            });
          }
          return aggregate;
        }
        Promise.allSettled = observeAllSettled;
        window.__jupiterRawLoadObserver = {
          probe,
          restore() {
            disposed = true;
            Promise.allSettled = originalAllSettled;
            for (const channel of channels) {
              channel.port1.close();
              channel.port2.close();
            }
            channels.clear();
            delete window.__jupiterRawLoadObserver;
          },
        };
      }, validMemo);

      await page.locator('#tracker-principal-input').fill(validMemo);
      await page.locator('#tracker-submit').click();
      await withDeadline(requestEntered, 'the raw-history request to enter its gate');
      await page.waitForFunction(() => {
        const probe = window.__jupiterRawLoadObserver?.probe;
        return probe?.error || probe?.joins > 0;
      }, null, { timeout: 30_000 });
      const before = await page.evaluate(() => window.__jupiterRawLoadObserver.probe);
      assert.equal(before.error, null);
      assert.equal(before.joins, 1);
      assert.equal(before.settled, false);
      assert.ok(enteredRequests >= 1);
      assert.equal(completedHandlers, 0);
      assert.match(await page.locator('#tracker-result').textContent(), /Subaccount 50/);

      await page.evaluate((hash) => { window.location.hash = hash; },
        `#metric-tracker?memo=${encodeURIComponent(invalidMemo)}&range=month`);
      await page.waitForFunction((message) => document.querySelector('#tracker-status')?.textContent === message,
        expected, { timeout: 30_000 });
      assert.equal(await page.locator('#tracker-principal-input').inputValue(), invalidMemo);
      assert.equal(await page.locator('#tracker-principal-input').isEnabled(), true);
      assert.equal(await page.locator('#tracker-submit').isEnabled(), true);
      assert.doesNotMatch(await page.locator('#tracker-result').textContent(), /Subaccount 50/);

      releaseRequest();
      await page.waitForFunction(() => {
        const probe = window.__jupiterRawLoadObserver?.probe;
        return probe?.error || probe?.continuationsDrained;
      }, null, { timeout: 30_000 });
      const after = await page.evaluate(() => window.__jupiterRawLoadObserver.probe);
      assert.equal(after.error, null);
      assert.equal(after.joins, 1);
      assert.equal(after.settled, true);
      assert.equal(after.continuationsDrained, true);
      assert.equal(after.outcomeStatuses.length, 5);
      assert.equal(after.outcomeStatuses[2], 'rejected'); // Gated raw-history query failed.
      assert.equal(after.outcomeStatuses[4], 'fulfilled'); // Short "d" suffix has no candidate lookup.
      assert.ok(completedHandlers >= 1);
      assert.equal(await page.locator('#tracker-status').textContent(), expected);
      assert.equal(await page.locator('#tracker-principal-input').inputValue(), invalidMemo);
      assert.equal(await page.locator('#tracker-principal-input').isEnabled(), true);
      assert.equal(await page.locator('#tracker-submit').isEnabled(), true);
      assert.doesNotMatch(await page.locator('#tracker-result').textContent(), /Subaccount 50/);
      checkFailures();
    } finally {
      releaseRequest();
      // Remove only this test's observer and route, not the harness's safety checks.
      try {
        await page.evaluate(() => { window.__jupiterRawLoadObserver?.restore(); });
      } finally {
        await page.unroute(routePattern, handler);
        const outcomes = await withDeadline(Promise.allSettled(handlerTasks), 'controlled route cleanup');
        const errors = outcomes.filter((outcome) => outcome.status === 'rejected').map((outcome) => outcome.reason);
        if (errors.length) throw new AggregateError(errors, 'Controlled raw-history route cleanup failed');
      }
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

test('shipped dashboard keeps feed pagination across resize without registered-pane requests', async () => {
  await withBrowser(async ({ openPage }) => {
    const { page, checkFailures } = await openPage({ width: 1024, height: 650 });
    let removedRequests = 0;
    page.on('request', (request) => {
      if (request.postDataBuffer()?.includes(Buffer.from('list_memo_registered_canister_summaries'))) removedRequests += 1;
    });
    await page.reload({ waitUntil: 'load' });
    await page.waitForFunction(() => window.__JUPITER_LANDING_DATA__, null, { timeout: 30_000 });
    await page.evaluate(() => {
      window.__JUPITER_LANDING_DATA__ = {
        recent: { items: Array.from({ length: 20 }, (_, i) => ({
          canister_id: ['jufzc-caaaa-aaaar-qb5da-cai'], tx_id: BigInt(i),
          timestamp_nanos: [1_700_000_000_000_000_000n], amount_e8s: 100_000_000n,
        })) }, errors: {},
      };
      window.location.hash = '#metric-commitments';
    });
    await page.setViewportSize({ width: 1024, height: 900 });
    await page.waitForFunction(() => document.querySelector('#commitments-page-info')?.textContent === 'Page 1 of 3');
    await page.locator('#commitments-next-page').click();
    assert.equal(await page.locator('#commitments-page-info').textContent(), 'Page 2 of 3');
    await page.setViewportSize({ width: 1024, height: 600 });
    await page.waitForFunction(() => document.querySelector('#commitments-page-info')?.textContent === 'Page 2 of 4');
    await page.locator('#commitments-next-page').click();
    assert.equal(await page.locator('#commitments-page-info').textContent(), 'Page 3 of 4');
    assert.equal(await page.locator('#registered-pane-body, #registered-prev-page, #registered-next-page, #registered-pane-subtitle').count(), 0);
    assert.equal(removedRequests, 0);
    checkFailures();
  });
});
