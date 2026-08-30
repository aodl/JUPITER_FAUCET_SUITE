import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';

const __dirname = dirname(fileURLToPath(import.meta.url));
const indexHtml = readFileSync(resolve(__dirname, '../../public/index.html'), 'utf8');
const indexCss = readFileSync(resolve(__dirname, '../../public/index.css'), 'utf8');
const notFoundHtml = readFileSync(resolve(__dirname, '../../public/404.html'), 'utf8');
const notFoundCss = readFileSync(resolve(__dirname, '../../public/404.css'), 'utf8');
const noscriptCss = readFileSync(resolve(__dirname, '../../public/noscript.css'), 'utf8');
const immutableLabelSvg = readFileSync(resolve(__dirname, '../../public/immutable-label/immutable-label.svg'), 'utf8');
const loadingOverlayJs = readFileSync(resolve(__dirname, '../../public/loading-overlay.js'), 'utf8');
const metricsCss = readFileSync(resolve(__dirname, '../../public/metrics.css'), 'utf8');
const bootstrapJs = readFileSync(resolve(__dirname, '../src/app/bootstrap.js'), 'utf8');
const advancedMemoControllerJs = readFileSync(resolve(__dirname, '../src/app/advanced-memo-controller.js'), 'utf8');
const configJs = readFileSync(resolve(__dirname, '../src/app/config.js'), 'utf8');
const countDisplaysJs = readFileSync(resolve(__dirname, '../src/app/count-displays.js'), 'utf8');
const hashRoutesJs = readFileSync(resolve(__dirname, '../src/app/hash-routes.js'), 'utf8');
const dashboardTablesControllerJs = readFileSync(resolve(__dirname, '../src/app/dashboard-tables-controller.js'), 'utf8');
const responsiveTablesJs = readFileSync(resolve(__dirname, '../src/app/responsive-tables.js'), 'utf8');
const simulatorControllerJs = readFileSync(resolve(__dirname, '../src/app/simulator-controller.js'), 'utf8');
const sourcePaneControllerJs = readFileSync(resolve(__dirname, '../src/app/source-pane-controller.js'), 'utf8');
const stakePaneControllerJs = readFileSync(resolve(__dirname, '../src/app/stake-pane-controller.js'), 'utf8');
const trackerControllerJs = readFileSync(resolve(__dirname, '../src/app/tracker-controller.js'), 'utf8');
const viewFormattersJs = readFileSync(resolve(__dirname, '../src/app/view-formatters.js'), 'utf8');
const mainJs = [
  bootstrapJs,
  advancedMemoControllerJs,
  configJs,
  countDisplaysJs,
  hashRoutesJs,
  dashboardTablesControllerJs,
  responsiveTablesJs,
  simulatorControllerJs,
  sourcePaneControllerJs,
  stakePaneControllerJs,
  trackerControllerJs,
  viewFormattersJs,
].join('\n');
const trackerCyclesJs = readFileSync(resolve(__dirname, '../src/tracker-cycles.js'), 'utf8');
const nnsGovernanceDidJs = readFileSync(resolve(__dirname, '../declarations/nns_governance/nns_governance.did.js'), 'utf8');
const navbarJs = readFileSync(resolve(__dirname, '../../public/navbar.js'), 'utf8');
const navbarCss = readFileSync(resolve(__dirname, '../../public/navbar.css'), 'utf8');

function sectionMarkup(panelId) {
  const start = indexHtml.indexOf(`id="nav-panel-${panelId}"`);
  assert.notEqual(start, -1, `missing panel ${panelId}`);
  const articleStart = indexHtml.lastIndexOf('<article', start);
  const articleEnd = indexHtml.indexOf('</article>', start);
  assert.notEqual(articleStart, -1, `missing article start for ${panelId}`);
  assert.notEqual(articleEnd, -1, `missing article end for ${panelId}`);
  return indexHtml.slice(articleStart, articleEnd + '</article>'.length);
}

function indexOfInput(simulator, id) {
  const index = simulator.indexOf(`id="${id}"`);
  assert.ok(index >= 0, `missing ${id}`);
  return index;
}

function elementById(markup, id) {
  const index = markup.indexOf(`id="${id}"`);
  assert.ok(index >= 0, `missing element ${id}`);
  const tagStart = markup.lastIndexOf('<', index);
  const tagEnd = markup.indexOf('>', index);
  assert.ok(tagStart >= 0 && tagEnd > tagStart, `malformed element ${id}`);
  return markup.slice(tagStart, tagEnd + 1);
}

function attrValue(tag, name) {
  const match = tag.match(new RegExp(`${name}="([^"]*)"`));
  return match ? match[1] : null;
}

test('top navbar exposes Simulator and Domains and no longer exposes Partners', () => {
  assert.match(
    indexHtml,
    /<div class="nav-links">\s*<a href="\/#intro" class="nav-brand" aria-label="Jupiter Faucet intro">\s*<img\s+class="nav-logo"\s+src="\/jupiter_faucet_token_logo\.svg\?v=__ASSET_VERSION__"\s+alt=""\s+width="32"\s+height="32"\s+decoding="async"\s+\/>\s*<\/a>\s*<a href="#about" class="nav-item" data-panel="about">About<\/a>/
  );
  assert.match(navbarCss, /\.nav-brand \{[\s\S]*display: inline-flex;[\s\S]*text-decoration: none;/);
  assert.match(navbarCss, /\.nav-logo \{[\s\S]*width: 32px;[\s\S]*height: 32px;[\s\S]*flex: 0 0 32px;/);
  assert.doesNotMatch(indexHtml, /<span class="nav-brand-text">JUPITER FAUCET<\/span>/);
  assert.match(indexHtml, /<a href="#simulator" class="nav-item nav-item--simulator" data-panel="simulator">Simulator<\/a>/);
  assert.doesNotMatch(indexHtml, /<a href="#relay-setup" class="nav-item" data-panel="relay-setup">Relay setup<\/a>/);
  assert.match(indexHtml, /<a href="#domains" class="nav-item nav-item--domains" data-panel="domains">Domains<\/a>/);
  assert.match(navbarCss, /@media \(max-width: 860px\) \{[\s\S]*\.nav-item--simulator \{[\s\S]*display: none;[\s\S]*\}/);
  assert.match(navbarCss, /@media \(max-width: 720px\) \{[\s\S]*\.nav-item--domains \{[\s\S]*display: none;[\s\S]*\}/);
  assert.doesNotMatch(indexHtml, /data-panel="partners"/i);
  assert.doesNotMatch(indexHtml, />Partners<\/a>/i);
});

test('bottom corner controls remain interactive above an open pane backdrop', () => {
  assert.match(
    indexCss,
    /\.github-corner,\s*\.parthenon-corner \{[^}]*z-index: 20000;/,
  );
  assert.match(navbarCss, /\.navbar \{[^}]*z-index: 20000;/);
  assert.match(navbarCss, /\.nav-panel-backdrop \{[^}]*z-index: 19000;/);
});

test('hero How link opens the maturity and rewards page', () => {
  assert.match(indexHtml, /<a href="#how-it-works:1"[^>]*data-panel="how-it-works"[^>]*>How\?<\/a>/);
});

test('static frontend markup does not depend on inline CSS', () => {
  for (const [label, body] of [
    ['index.html', indexHtml],
    ['404.html', notFoundHtml],
    ['immutable-label.svg', immutableLabelSvg],
  ]) {
    assert.doesNotMatch(body, /<style\b/i, `${label} should not embed style blocks`);
    assert.doesNotMatch(body, /\sstyle=/i, `${label} should not use style attributes`);
  }
});

test('first-load overlay uses the token logo and rotating cycle phrases', () => {
  const overlayTag = elementById(indexHtml, 'page-loading-overlay');
  const titleTag = elementById(indexHtml, 'page-loading-title');

  assert.match(overlayTag, /class="page-loading-overlay"/);
  assert.equal(attrValue(overlayTag, 'aria-label'), 'Jupiter Faucet is loading');
  assert.equal(attrValue(overlayTag, 'role'), null);
  assert.equal(attrValue(overlayTag, 'aria-live'), null);
  assert.match(indexHtml, /<script src="\/loading-overlay\.js\?v=__ASSET_VERSION__" defer><\/script>/);
  assert.doesNotMatch(indexHtml, /<script>\s*\(\(\) => \{[\s\S]*page-loading-overlay/);
  assert.match(indexHtml, /class="page-loading-pane"/);
  assert.match(indexHtml, /<link rel="preload" as="image" href="\/jupiter_faucet_token_logo\.svg\?v=__ASSET_VERSION__" type="image\/svg\+xml" fetchpriority="high">/);
  assert.match(indexHtml, /<link rel="preload" as="image" href="\/background-orbit\/background-orbit-firstpaint\.jpg\?v=__ASSET_VERSION__" type="image\/jpeg" media="\(min-width: 1025px\)" fetchpriority="high">/);
  assert.match(indexHtml, /<link rel="preload" as="image" href="\/background-orbit\/background-orbit-mobile-firstpaint\.jpg\?v=__ASSET_VERSION__" type="image\/jpeg" media="\(max-width: 1024px\)" fetchpriority="high">/);
  assert.match(indexHtml, /class="page-loading-logo" src="\/jupiter_faucet_token_logo\.svg\?v=__ASSET_VERSION__"/);
  assert.match(indexHtml, /class="page-loading-logo"[^>]*decoding="sync"[^>]*fetchpriority="high"/);
  assert.match(indexHtml, /<link rel="stylesheet" href="\/index\.css\?v=__ASSET_VERSION__" \/>/);
  assert.match(indexHtml, /<noscript>\s*<link rel="stylesheet" href="\/noscript\.css\?v=__ASSET_VERSION__" \/>\s*<\/noscript>/);
  assert.match(indexCss, /\.page-loading-overlay \{[^}]*display: grid;/);
  assert.doesNotMatch(indexCss, /\.page-loading-overlay \{[^}]*display: none;/);
  assert.match(indexCss, /\.page-loading-overlay\.is-active \{[^}]*display: grid;/);
  assert.match(indexCss, /body \{[\s\S]*background-image:\s*var\(--orbit-background-firstpaint\),\s*var\(--orbit-background-lqip\);/);
  assert.match(indexCss, /body\.background-orbit-enhanced \{[\s\S]*background-image:\s*var\(--orbit-background-full\),\s*var\(--orbit-background-firstpaint\),\s*var\(--orbit-background-lqip\);/);
  assert.match(indexCss, /@media \(max-width: 1024px\) \{[\s\S]*--orbit-background-full: url\("background-orbit\/background-orbit-mobile\.jpg\?v=__ASSET_VERSION__"\);[\s\S]*--orbit-background-firstpaint: url\("background-orbit\/background-orbit-mobile-firstpaint\.jpg\?v=__ASSET_VERSION__"\);/);
  assert.doesNotMatch(indexCss, /@media \(max-width: 1024px\) \{[\s\S]*background-image:\s*url\("background-orbit\/background-orbit-mobile\.jpg/);
  assert.match(indexHtml, /id="visor"[^>]*fetchpriority="low"/);
  assert.match(indexHtml, /id="visor_glow"[^>]*fetchpriority="low"/);
  assert.match(noscriptCss, /#page-loading-overlay \{[\s\S]*display: none !important;[\s\S]*\}/);
  assert.equal(attrValue(titleTag, 'aria-hidden'), 'true');
  assert.match(indexHtml, /class="page-loading-title" id="page-loading-title" aria-hidden="true">Infinite Cycles Begin Here<\/p>/);
  assert.match(indexHtml, /class="page-loading-status" role="status" aria-live="polite">Loading<span aria-hidden="true">/);
  assert.match(indexCss, /@media \(prefers-reduced-motion: reduce\) \{[\s\S]*\.page-loading-overlay,[\s\S]*\.page-loading-overlay::before,[\s\S]*\.page-loading-pane,[\s\S]*\.page-loading-pane::before \{[\s\S]*transition: none;/);
  assert.match(indexCss, /@media \(prefers-reduced-motion: reduce\) \{[\s\S]*\.page-loading-title\.is-swiping-out,[\s\S]*\.page-loading-title\.is-swiping-in \{[\s\S]*animation: none;/);
  assert.match(indexCss, /@media \(prefers-reduced-motion: reduce\) \{[\s\S]*\.page-loading-dot \{[\s\S]*animation: none;[\s\S]*opacity: 1;/);
  assert.match(indexCss, /--loader-progress/);
  assert.match(indexCss, /conic-gradient\(/);
  assert.match(loadingOverlayJs, /overlay\.classList\.add\("is-active"\);/);
  assert.match(loadingOverlayJs, /Cycles keep canisters alive/);
  assert.match(loadingOverlayJs, /Keep every canister fueled/);
  assert.match(loadingOverlayJs, /Autonomous top-ups for unstoppable software/);
  assert.doesNotMatch(loadingOverlayJs, /They die from lack of cycles/);
  assert.match(loadingOverlayJs, /Math\.floor\(Math\.random\(\) \* phrases\.length\)/);
  assert.match(loadingOverlayJs, /title\.textContent = phrases\[phraseIndex\];/);
  assert.match(loadingOverlayJs, /title\.classList\.add\("is-swiping-out"\)/);
  assert.match(loadingOverlayJs, /title\.classList\.add\("is-swiping-in"\)/);
  assert.match(loadingOverlayJs, /\}, 2100\);/);
  assert.match(loadingOverlayJs, /const minVisibleMs = 2000;/);
  assert.match(loadingOverlayJs, /const maxVisibleMs = 60000;/);
  assert.match(loadingOverlayJs, /const maxVisibleTimer = window\.setTimeout\(finish, maxVisibleMs\);/);
  assert.match(loadingOverlayJs, /window\.requestAnimationFrame\(animateProgress\)/);
  assert.match(loadingOverlayJs, /function enableFullBackgroundAfterOverlayPaint\(\)/);
  assert.match(loadingOverlayJs, /document\.body\.classList\.add\("background-orbit-enhanced"\)/);
  assert.match(loadingOverlayJs, /window\.requestAnimationFrame\(function \(\) \{\s*window\.requestAnimationFrame\(enable\);/);
  assert.match(loadingOverlayJs, /1 - Math\.exp\(-elapsedMs \/ 2600\)/);
  assert.match(loadingOverlayJs, /window\.cancelAnimationFrame\(animationFrame\)/);
  assert.match(loadingOverlayJs, /window\.addEventListener\("load", finish, \{ once: true \}\)/);
  assert.match(loadingOverlayJs, /overlay\.style\.setProperty\("--loader-progress", progress\.toFixed\(1\)\)/);
  assert.match(loadingOverlayJs, /overlay\.classList\.add\("is-fading"\)/);
  assert.match(loadingOverlayJs, /\}, 1500\);/);
});

test('not found page displays the Jupiter Faucet token logo', () => {
  assert.match(notFoundHtml, /class="not-found-logo" src="\/jupiter_faucet_token_logo\.svg\?v=__ASSET_VERSION__"/);
  assert.match(notFoundHtml, /<link rel="stylesheet" href="\/404\.css\?v=__ASSET_VERSION__" \/>/);
  assert.match(notFoundCss, /\.not-found-logo \{[\s\S]*width: clamp\(112px, 24vw, 172px\);[\s\S]*height: clamp\(112px, 24vw, 172px\);/);
  assert.match(notFoundHtml, /<h1>Not Found<\/h1>/);
});

test('orbit scene includes hoverable infographic callouts', () => {
  const orbitCss = readFileSync(resolve(__dirname, '../../public/background-orbit/background-orbit.css'), 'utf8');
  const orbitJs = readFileSync(resolve(__dirname, '../../public/background-orbit/background-orbit.js'), 'utf8');

  assert.match(indexHtml, /class="orbit-infographic"/);
  assert.match(indexHtml, /id="orbit-infographic-copy"/);
  assert.match(indexHtml, /id="orbit-infographic-line"/);
  assert.match(indexHtml, /id="orbit-infographic-marker"/);
  assert.match(indexHtml, /id="orbit-infographic-hotspots"/);
  assert.match(orbitCss, /\.orbit-infographic-copy/);
  assert.match(orbitCss, /#orbit-infographic-line/);
  assert.match(orbitCss, /#orbit-infographic-marker/);
  assert.match(orbitCss, /\.orbit-infographic-hotspot/);
  assert.match(orbitCss, /\.orbit-infographic-particle-hotspot/);
  assert.match(orbitCss, /#visor, #visor_glow, \.neon \{/);
  assert.match(orbitCss, /\.neon-top\{\s*top: 75dvw;\s*left: 54dvw;\s*\}/);
  assert.match(orbitCss, /\.neon-ups\{\s*top: 78dvw;\s*left: 57dvw;\s*\}/);
  assert.doesNotMatch(orbitCss, /\.neon-top\{[\s\S]*?padding-top:/);
  assert.doesNotMatch(orbitCss, /\.neon-ups\{[\s\S]*?padding-top:/);
  assert.match(orbitJs, /Automated disbursals keep flowing, minting new ICP from voting rewards, powering downstream smart contracts\./);
  assert.doesNotMatch(orbitJs, /statusSlot: "orbit-disbursement-status"/);
  assert.doesNotMatch(orbitJs, /__JUPITER_ORBIT_DISBURSEMENT_TEXT__/);
  assert.match(indexHtml, /class="orbit-disbursement-status" id="orbit-disbursement-status" hidden aria-live="polite"/);
  assert.doesNotMatch(orbitCss, /\.metric-rail:not\(\.metric-rail--visible\) ~ \.orbit-disbursement-status/);
  assert.match(orbitCss, /\.orbit-disbursement-status\[hidden\] \{[\s\S]*display: none;[\s\S]*\}/);
  assert.match(orbitCss, /body\.metrics-menu-open \.orbit-disbursement-status:not\(\[hidden\]\) \{[\s\S]*display: block;[\s\S]*\}/);
  assert.match(orbitCss, /\.orbit-disbursement-status \{[\s\S]*display: none;[\s\S]*top: 22dvw;[\s\S]*left: 6\.5dvw;[\s\S]*width: min\(23dvw, 16rem\);[\s\S]*color: rgba\(255, 255, 255, 0\.56\);[\s\S]*font-size: 11px;[\s\S]*line-height: 1\.35;[\s\S]*font-weight: 400;[\s\S]*\}/);
  assert.match(orbitCss, /\.orbit-disbursement-status::before \{[\s\S]*width: min\(8\.7dvw, 7\.35rem\);[\s\S]*\}/);
  assert.match(orbitCss, /\.orbit-disbursement-status::before \{[\s\S]*float: right;[\s\S]*shape-outside: polygon\(150% 0, 100% 100%, 0 100%\);[\s\S]*\}/);
  assert.doesNotMatch(orbitCss, /\.orbit-disbursement-status \{[^}]*background:/);
  assert.doesNotMatch(orbitCss, /\.orbit-disbursement-status \{[^}]*border:/);
  assert.doesNotMatch(orbitCss, /\.orbit-disbursement-status \{[^}]*padding:/);
  assert.doesNotMatch(orbitCss, /\.orbit-disbursement-status \{[^}]*text-shadow:/);
  assert.match(orbitCss, /\.orbit-disbursement-status \.orbit-infographic-copy-link \{[\s\S]*color: inherit;[\s\S]*\}/);
  assert.match(orbitCss, /\.orbit-disbursement-status \.orbit-infographic-copy-link:hover,[\s\S]*\.orbit-disbursement-status \.orbit-infographic-copy-link:focus \{[\s\S]*color: #fff;[\s\S]*\}/);
  assert.match(orbitJs, /Disbursals are orchestrated by immutable \(unmodifiable\) smart contracts\."/);
  assert.doesNotMatch(orbitJs, /aka 'blackholed'/);
  assert.doesNotMatch(orbitJs, /COMING SOON/);
  assert.match(orbitJs, /ctaLabel: "MORE INFO"/);
  assert.match(orbitJs, /ctaPanel: "governance"/);
  assert.match(orbitJs, /copy\.appendChild\(document\.createElement\("br"\)\);[\s\S]*link\.textContent = item\.ctaLabel;/);
  assert.match(orbitJs, /link\.dataset\.panel = item\.ctaPanel/);
  assert.match(orbitCss, /\.orbit-infographic-copy-link \{[\s\S]*border: 1px solid currentColor;[\s\S]*border-radius: 999px;[\s\S]*text-decoration: none;[\s\S]*\}/);
  assert.match(orbitCss, /\.orbit-infographic-copy-link:hover,[\s\S]*\.orbit-infographic-copy-link:focus-visible \{[\s\S]*transform: translateY\(-1px\);[\s\S]*\}/);
  assert.match(orbitCss, /\.orbit-infographic-copy\.is-visible \{[\s\S]*pointer-events: auto;[\s\S]*\}/);
  assert.match(orbitJs, /Disbursed ICP is automatically converted into cycles, forming a giant, unstoppable faucet\./);
  assert.match(orbitJs, /lineStartX: 370/);
  assert.match(orbitJs, /lineStartY: 725/);
  assert.match(orbitCss, /white-space: pre-line/);
  assert.match(orbitJs, /Cycles are permanently routed to canisters that were declared by Jupiter Faucet users, removing or reducing economic dependency on developers, and the risk of service disruption\/deletion\./);
  assert.match(orbitJs, /animatedSvgPosition/);
  assert.match(orbitJs, /TOUCH_ACTIVE_MS = 5000/);
  assert.match(orbitJs, /clearClickActivation/);
  assert.match(orbitJs, /orbit-infographic-swirl-1/);
  assert.match(orbitJs, /addEventListener\("click"/);
  assert.match(orbitJs, /PARTICLE_DURATIONS_SECONDS = \[600, 400, 1200\]/);
  assert.match(orbitJs, /mouseenter/);
  assert.match(orbitJs, /textWidthVw/);
  assert.match(orbitJs, /fontSizeVw/);
  assert.match(orbitJs, /fontSizeMaxRem/);
  assert.match(orbitCss, /font-size: min\(1\.65rem, 1\.55dvw\)/);
  assert.doesNotMatch(orbitJs, /Copy config/);
  assert.doesNotMatch(orbitJs, /orbit-tuning/);
  assert.doesNotMatch(orbitCss, /orbit-tuning/);
  assert.doesNotMatch(orbitCss, /transform: scale\(0\.72\)/);
});

test('partners pane content has been removed', () => {
  assert.doesNotMatch(indexHtml, /id="nav-panel-partners"/);
  assert.doesNotMatch(indexHtml, /Life On Ledger/);
  assert.doesNotMatch(indexHtml, /WaterNeuron/);
});

test('transaction table pagination uses a responsive page size', () => {
  assert.match(mainJs, /const TABLE_MIN_PAGE_SIZE = 6;/);
  assert.match(mainJs, /const COMMITMENT_TABLE_PAGE_SIZE_ADJUSTMENT = -1;/);
  assert.match(mainJs, /function calculateResponsiveTablePageSize\(viewportHeight = window\.innerHeight\)/);
  assert.match(mainJs, /Math\.min\(TABLE_MAX_PAGE_SIZE, Math\.max\(TABLE_MIN_PAGE_SIZE, estimatedRows\)\)/);
  assert.match(mainJs, /const currentPageSizeForTable = \(kind\) => \{[\s\S]*kind === 'commitments'[\s\S]*kind === 'commitments-raw'[\s\S]*kind === 'commitments-neurons'[\s\S]*currentTablePageSize\(\) \+ adjustment/);
  assert.match(mainJs, /const pageSize = currentPageSizeForTable\(kind\);[\s\S]*state\.items\.slice\(start, start \+ pageSize\)/);
  assert.match(mainJs, /registeredPageSize: dashboardTablesController\.currentTablePageSize\(\)/);
  assert.match(mainJs, /window\.addEventListener\('resize'/);
  assert.doesNotMatch(mainJs, /const TABLE_PAGE_SIZE = 6;/);
});

test('About pane includes social links and projects slide', () => {
  const about = sectionMarkup('about');
  assert.match(about, /<strong>Jupiter Faucet<\/strong> is a perpetual cycles top-up protocol/);
  assert.match(about, /href="https:\/\/internetcomputer\.org\/"[^>]*>Internet Computer<\/a>/);
  assert.match(about, /designed for tamper-proof, "unstoppable" on-chain services/);
  assert.match(about, /href="https:\/\/learn\.internetcomputer\.org\/hc\/en-us\/articles\/34573913497108-Cycles"[^>]*>Internet Computer cycles guide<\/a>/);
  assert.match(about, /class="about-social-links"[^>]*aria-label="Jupiter Faucet social links"/);
  assert.match(about, /href="https:\/\/oc\.app\/community\/xfokc-3yaaa-aaaac-be5ia-cai\/channel\/3626918149"[^>]*>[\s\S]*Open Chat Community[\s\S]*Onchain Q&amp;A/);
  assert.match(about, /href="https:\/\/taggr\.link\/#\/realm\/JUPITER_FAUCET"[^>]*>[\s\S]*TAGGR Realm[\s\S]*Decentralized social network/);
  assert.match(about, /href="https:\/\/x\.com\/JupiterFaucet"[^>]*>[\s\S]*@JupiterFaucet/);
  assert.match(about, /src="\/social-icons\/openchat-favicon\.png"/);
  assert.match(about, /src="\/social-icons\/taggr-favicon\.ico"/);
  assert.match(about, /src="\/social-icons\/x-favicon\.png"/);
  assert.match(about, /one-off operation/);
  assert.match(about, /data-panel="how-it-works"[^>]*>How It Works<\/a>/);
  assert.match(about, /memo-builder-safety-notice[\s\S]*<strong>Due diligence:<\/strong>/);
  assert.match(about, /The frontend is accessible via multiple <a href="#domains" data-panel="domains" class="pane-external-link">domains<\/a> controlled by independent parties\./);
  assert.match(about, /The core components will be blackholed/);
  assert.match(about, /NNS-managed platform dependency prevents disbursals\s*or other core functionality/);
  assert.match(about, /blackhole themself again once service resumes/);
  assert.match(about, /data-panel="source"[^>]*>open source<\/a>/);
  assert.match(about, /data-panel="governance"[^>]*>decentralize control<\/a>/);
  assert.doesNotMatch(about, /fixed\s*corner links at the bottom of the page/);
  assert.doesNotMatch(about, /Status:/);
  assert.doesNotMatch(about, /planned launch within/);
  assert.match(about, /class="nav-panel-page is-active" data-page="0"/);
  assert.match(about, /class="nav-panel-page" data-page="1"[\s\S]*<h2 class="about-projects-title">Featured Projects Powered by Jupiter Faucet<\/h2>[\s\S]*More coming soon!/);
  assert.match(about, /data-page-target="0"[^>]*>social channels<\/a>[\s\S]*class="about-project-grid"/);
  assert.match(about, /class="about-project-preview" href="\/"/);
  assert.match(about, /src="\/og\/preview-20260520\.jpg"[\s\S]*<strong>Jupiter Faucet<\/strong>/);
  assert.match(about, /Yes, Jupiter Faucet powers itself!/);
  assert.match(about, /<p class="about-project-canisters-title">Canisters<\/p>/);
  assert.match(about, /data-tracker-principal="uccpi-cqaaa-aaaar-qby3q-cai"[\s\S]*>Disburser<\/a>/);
  assert.match(about, /data-tracker-principal="acjuz-liaaa-aaaar-qb4qq-cai"[\s\S]*>Faucet<\/a>/);
  assert.match(about, /data-tracker-principal="j5gs6-uiaaa-aaaar-qb5cq-cai"[\s\S]*>Historian<\/a>/);
  assert.match(about, /data-tracker-principal="afisn-gqaaa-aaaar-qb4qa-cai"[\s\S]*>Lifeline<\/a>/);
  assert.match(about, /data-tracker-principal="alk7f-5aaaa-aaaar-qb4ra-cai"[\s\S]*>SNS Rewards<\/a>/);
  assert.match(about, /data-tracker-principal="u2qkp-aqaaa-aaaar-qb7ea-cai"[\s\S]*>Relay<\/a>/);
  assert.match(about, /data-tracker-principal="jufzc-caaaa-aaaar-qb5da-cai"[\s\S]*>Frontend<\/a>/);
  assert.match(about, /class="about-project-card about-project-card--soon"[\s\S]*More coming soon!/);
  assert.match(about, /class="about-projects-see-all"[\s\S]*href="#metric-commitments"[^>]*data-panel="metric-commitments"[^>]*>See All<\/a>/);
  assert.match(about, /href="https:\/\/github\.com\/aodl\/JUPITER_FAUCET_SUITE\/pulls"[^>]*>raise a pull request<\/a>/);
  assert.match(about, /<div class="nav-panel-dots" role="tablist" aria-label="About pages">/);
  assert.match(about, /data-page="1" aria-label="Featured Projects Powered by Jupiter Faucet"/);
  assert.match(metricsCss, /\.about-projects-slide \{[\s\S]*overflow: visible;[\s\S]*\}/);
  assert.doesNotMatch(metricsCss, /\.about-projects-slide \{[\s\S]*min-height: 100%;[\s\S]*\}/);
  assert.match(metricsCss, /\.about-projects-title \{[\s\S]*font-size: 20px;[\s\S]*letter-spacing: 0\.08em;[\s\S]*text-transform: uppercase;[\s\S]*\}/);
  assert.match(metricsCss, /\.about-project-grid \{[\s\S]*grid-template-columns: repeat\(2, minmax\(0, calc\(\(100% - 12px\) \/ 2\)\)\);[\s\S]*\}/);
  assert.match(metricsCss, /\.about-project-card \{[\s\S]*container-type: inline-size;[\s\S]*\}/);
  assert.doesNotMatch(metricsCss, /\.about-project-card \{[\s\S]*aspect-ratio: 1 \/ 1;[\s\S]*\}/);
  assert.match(metricsCss, /\.about-project-preview img \{[\s\S]*height: clamp\(128px, 38cqw, 160px\);[\s\S]*object-fit: cover;[\s\S]*\}/);
});

test('Domains pane explains frontend origins and domain listing workflow', () => {
  const domains = sectionMarkup('domains');
  assert.match(domains, /<h2 class="nav-panel-title">Domains<\/h2>/);
  assert.match(domains, /Jupiter Faucet runs on the\s*<a href="https:\/\/internetcomputer\.org\/"[^>]*>Internet Computer<\/a>/);
  assert.doesNotMatch(domains, /Jupiter Faucet's core canisters run/);
  assert.match(domains, /DNS still depends on ordinary domain registrars and\s*gateway routing today/);
  assert.match(domains, /Work <a href="https:\/\/dashboard\.internetcomputer\.org\/proposal\/35639"[^>]*>toward decentralized DNS<\/a>\s*is ongoing/);
  assert.match(domains, /independent\s*parties can publish and manage domains/);
  assert.match(domains, /href="https:\/\/jupiter-faucet\.com\/"[^>]*>jupiter-faucet\.com<\/a>/);
  assert.match(domains, /Currently managed by <a href="https:\/\/dashboard\.internetcomputer\.org\/neuron\/16459595263909468577"[^>]*>LORIMER ♾️ 🐶<\/a> until decentralized DNS is available/);
  assert.match(domains, /href="https:\/\/jufzc-caaaa-aaaar-qb5da-cai\.icp0\.io\/"[^>]*>jufzc-caaaa-aaaar-qb5da-cai\.icp0\.io<\/a>/);
  assert.match(domains, /Managed by <a href="https:\/\/dashboard\.internetcomputer\.org\/neuron\/27"[^>]*>DFINITY<\/a> through the default ICP HTTP gateway canister URL/);
  assert.match(domains, /href="https:\/\/docs\.internetcomputer\.org\/guides\/frontends\/custom-domains\/"[^>]*>ICP custom domains documentation<\/a>/);
  assert.match(domains, /href="https:\/\/github\.com\/aodl\/JUPITER_FAUCET_SUITE\/pulls"[^>]*>pull request<\/a>/);
  assert.match(domains, /href="#about" data-panel="about"[^>]*>social channels<\/a>/);
  assert.match(navbarCss, /\.domain-pane-list \{[\s\S]*display: grid;[\s\S]*\}/);
  assert.match(navbarCss, /\.domain-pane-entry \{[\s\S]*border: 1px solid rgba\(255, 255, 255, 0\.14\);[\s\S]*\}/);
});

test('Source and Governance panes expose subnet context', () => {
  const source = sectionMarkup('source');
  const governance = sectionMarkup('governance');
  assert.match(source, /source-pane-subnet-link pane-external-link[^>]*>Subnet pzp6e<\/a>/);
  assert.match(source, /network\/subnets\/pzp6e-ekpqk-3c5x7-2h6so-njoeq-mt45d-h3h6c-q3mxf-vpeq5-fk5o7-yae/);
  assert.match(source, /data-source-memory="uccpi-cqaaa-aaaar-qby3q-cai"/);
  assert.match(indexHtml, /jupiter_relay/);
  assert.doesNotMatch(indexHtml, /Display name: Jupiter Relay/);
  assert.match(indexHtml, /data-source-module-hash="u2qkp-aqaaa-aaaar-qb7ea-cai"/);
  assert.match(indexHtml, /JUPITER_FAUCET_SUITE\/tree\/master\/canisters\/relay/);
  assert.doesNotMatch(indexHtml, /canisters\/relay\/jupiter_relay\.did/);
  assert.doesNotMatch(indexHtml, /canisters\/relay\/jupiter_relay_debug\.did/);
  assert.doesNotMatch(indexHtml, /canisters\/relay\/README\.md/);
  assert.doesNotMatch(indexHtml, /canisters\/relay\/mainnet-install-args\.did/);
  assert.match(indexHtml, /Converts ICP to cycles and distributes to Jupiter Faucet Suite canisters proportionally based on consumption rates\./);
  assert.match(navbarCss, /\.source-pane-canister \{[\s\S]*position: relative;[\s\S]*\}/);
  assert.match(navbarCss, /\.source-pane-subnet-link \{[\s\S]*position: absolute;[\s\S]*right: 16px;[\s\S]*\}/);
  assert.match(governance, /All Jupiter Faucet suite canisters reside on either the/);
  assert.match(governance, /network\/subnets\/pzp6e-ekpqk-3c5x7-2h6so-njoeq-mt45d-h3h6c-q3mxf-vpeq5-fk5o7-yae[^>]*>Fiduciary<\/a>/);
  assert.match(governance, /network\/subnets\/x33ed-h457x-bsgyx-oqxqf-6pzwv-wkhzr-rm2j3-npodi-purzm-n66cg-gae[^>]*>SNS subnet<\/a>/);
  assert.match(governance, /both composed of over 30 nodes/);
  assert.match(governance, /moving toward SNS DAO control/);
  assert.match(governance, /data-panel="source"[^>]*>open source<\/a>/);
  assert.match(governance, /data-panel="source"[^>]*>reproducible builds<\/a>/);
  assert.match(governance, /data-panel="source"[^>]*>Source Code<\/a>\s*pane/);
  assert.match(governance, /memo-builder-safety-notice[\s\S]*<strong>Blackholing:<\/strong>/);
  assert.match(governance, /core value-moving canisters/);
  assert.match(governance, /underlying Internet Computer system API\s*\n\s*\(<a href="https:\/\/nns\.ic0\.app\/"[^>]*>NNS-managed code<\/a>\)/);
  assert.match(governance, /lifeline canister, which is controlled by the SNS DAO/);
  assert.match(governance, /at least six months/);
  assert.match(governance, /built-in trigger that causes both canisters to blackhole themselves/);
});

test('How it works copy is concise and links tracker, simulator, and rewards references', () => {
  const howItWorks = sectionMarkup('how-it-works');
  assert.match(howItWorks, /set your <strong>declared canister ID<\/strong> as the transaction\s*<strong>memo<\/strong> \(see example below\)/);
  assert.doesNotMatch(howItWorks, /plain ASCII text \(see <i>Ctrl \+ K 'memo'<\/i> tip below\)/);
  assert.doesNotMatch(howItWorks, /The 1 ICP minimum is intentional/);
  assert.doesNotMatch(howItWorks, /target canister ID/);
  assert.match(howItWorks, /how-it-works-guide-card is-optional/);
  assert.match(howItWorks, /href="https:\/\/nns\.ic0\.app\/address-book"[^>]*>[\s\S]*how-it-works-edit-address\.png/);
  assert.match(howItWorks, /with a nickname to make future commitments easier/);
  assert.match(howItWorks, /<strong>ICRC staking account<\/strong>[\s\S]*id="copy-how-staking-account"[^>]*>Copy<\/button>/);
  assert.match(howItWorks, /<strong>Alternative account identifier<\/strong>[\s\S]*id="copy-how-staking-account-identifier"[^>]*>Copy<\/button>/);
  assert.match(howItWorks, /id="how-staking-account-identifier-link"[^>]*dashboard\.internetcomputer\.org\/account\/22594ba982e201a96a8e3e51105ac412221a30f231ec74bb320322deccb5061d/);
  assert.doesNotMatch(howItWorks, /<strong>Staking account<\/strong>/);
  assert.match(indexCss, /\.how-it-works-guide-card\.is-send-step \{[\s\S]*grid-row: 1 \/ span 2;[\s\S]*\}/);
  assert.match(howItWorks, /set the transaction memo to your declared canister ID/);
  assert.doesNotMatch(howItWorks, /Transfer ICP to the long-form ICRC-1 staking account address displayed above/);
  assert.doesNotMatch(howItWorks, /While stake commitments can be made today/);
  assert.match(howItWorks, /data-panel="metric-tracker"[^>]*>memo tracker<\/a>/);
  assert.match(howItWorks, /data-panel="simulator"[^>]*>simulator<\/a>/);
  assert.match(howItWorks, /newly minted <strong>IO<\/strong> \(a liquid staking protocol that will be launched alongside Jupiter Faucet\)/);
  assert.match(howItWorks, /<strong>0%–19%<\/strong> distributed to <strong>SNS jUP stakers<\/strong>/);
  assert.match(howItWorks, /<strong>0%–1%<\/strong> restaked into/);
  assert.match(howItWorks, /D-QUORUM is a special known neuron owned by the NNS Governance canister itself/);
  assert.match(howItWorks, /dashboard\.internetcomputer\.org\/neuron\/2947465672511369[^>]*>\s*αlpha-vote<\/a\s*>/);
  assert.match(howItWorks, /follows D-QUORUM indirectly through[\s\S]*to maximise voting rewards/);
  assert.match(howItWorks, /helps incentivise elected NNS governance reviewers who perform decentralized due diligence on proposals/);
  assert.match(howItWorks, /partnership is foundational to Jupiter Faucet because truly unstoppable canisters depend on a secure and decentralized network/);
  assert.match(howItWorks, /memo-builder-safety-notice[\s\S]*<strong>Rewards:<\/strong>[\s\S]*jUP SNS tokens will be minted/);
  assert.match(howItWorks, /jUP SNS tokens will be minted[\s\S]*While the Jupiter Faucet SNS rewards components are still being finalized/);
  assert.match(howItWorks, /data-panel="metric-commitments"[^>]*>committed ICP<\/a>/);
  assert.match(howItWorks, /dashboard\.internetcomputer\.org\/account\/22594ba982e201a96a8e3e51105ac412221a30f231ec74bb320322deccb5061d[^>]*>staking account<\/a>/);
  assert.match(howItWorks, /dashboard\.internetcomputer\.org\/neuron\/11614578985374291210[^>]*>neuron<\/a>/);
  assert.match(howItWorks, /data-page-target="0"[^>]*>rules described<\/a>/);
  assert.match(howItWorks, /Contributions must meet the requirements in order to be counted/);
  assert.match(howItWorks, /transactions of at least 1 ICP featuring a memo that declares a canister ID or neuron ID/);
  assert.match(howItWorks, /dashboard\.internetcomputer\.org\/account\/4d6afc06456fc7d5e5d6c9096a12ca60182a9fdb4ee50c4ff2feb2112c86222f[^>]*>rewards account<\/a>/);
  assert.match(howItWorks, /data-panel="governance"[^>]*>Governance<\/a>/);
  assert.match(navbarJs, /data-page-target/);
  assert.match(howItWorks, /<strong>Developer tip:<\/strong> If you need many small ICP transfers to build up to\s*the minimum 1 ICP threshold/);
  assert.match(howItWorks, /href="\/#how-it-works:3"[^>]*data-panel="how-it-works"[^>]*>Jupiter Relay<\/a>/);
  assert.doesNotMatch(howItWorks, /See <a href="#how-it-works:2"[^>]*>Advanced Usage<\/a>/);
});

test('cycles help opens a dedicated observability slide with actionable route guidance', () => {
  const howItWorks = sectionMarkup('how-it-works');
  assert.match(
    bootstrapJs,
    /href="#metric-tracker" data-tooltip-navigation>Memo Tracker<\/a>/
  );
  assert.match(
    bootstrapJs,
    /class="pane-page-button pane-fixed-tooltip-more-info" href="#how-it-works:4" data-tooltip-navigation>More info<\/a>/
  );
  assert.match(
    bootstrapJs,
    /Cycles observability first uses protocol-native direct canister status/
  );
  assert.doesNotMatch(bootstrapJs, /For ordinary canisters, cycles observability/);
  assert.match(
    bootstrapJs,
    /<div class="pane-fixed-tooltip-actions">\s*<button class="pane-fixed-tooltip-close"/
  );
  assert.match(
    bootstrapJs,
    /const moreInfoLink = textNode\?\.querySelector\('\.pane-fixed-tooltip-more-info'\);\s*if \(moreInfoLink && actionsNode\) actionsNode\.prepend\(moreInfoLink\)/
  );
  assert.match(
    bootstrapJs,
    /event\.target\.closest\('\[data-tooltip-navigation\]'\)[\s\S]*popover\.hidden = true/
  );
  assert.match(howItWorks, /data-page="4"[\s\S]*<h3 class="pane-section-title">Cycles Observability<\/h3>/);
  assert.match(howItWorks, /Registering a canister through a qualifying memo commitment tells the\s*<a href="#metric-tracker"[^>]*>Memo Tracker<\/a>\s*what to track/);
  assert.match(howItWorks, /e3mmv-5qaaa-aaaah-aadma-cai[^>]*>[\s\S]*13-node blackhole<\/a>/);
  assert.match(howItWorks, /77deu-baaaa-aaaar-qb6za-cai[^>]*>[\s\S]*Fiduciary blackhole<\/a>/);
  assert.match(howItWorks, /Both are immutable, run the same independently reproducible\s*blackhole Wasm/);
  assert.match(howItWorks, /trust-in-canisters\/#black-holed-canisters[^>]*>documented by DFINITY<\/a>/);
  assert.match(howItWorks, /introduces no trusted operator and does not remove existing controllers/);
  assert.match(howItWorks, /<strong>Direct canister status:<\/strong>/);
  assert.match(howItWorks, /<code>canister_status<\/code>/);
  assert.match(howItWorks, /<strong>SNS fallback:<\/strong>/);
  assert.doesNotMatch(howItWorks, /Direct self balance/);
  assert.doesNotMatch(howItWorks, /Cached positive route/);
  assert.doesNotMatch(howItWorks, /Adding a controller changes the canister's security/);
  assert.match(howItWorks, /<strong>TL;DR:<\/strong>/);
  assert.match(howItWorks, /Direct status is tried before every cached, blackhole, or SNS proxy route/);
  assert.match(howItWorks, /new self-service Relay can rely on direct status\s*only when the target sets it to <code>public<\/code>/);
  assert.match(howItWorks, /href="#how-it-works" data-page-target="0"[^>]*>Jupiter Faucet<\/a>/);
  assert.match(howItWorks, /href="#how-it-works:3" data-page-target="3"[^>]*>Jupiter Relay<\/a>/);
  assert.match(howItWorks, /Jupiter Relay<\/a>\s*target canisters must expose their cycles so the Relay can calculate how much each needs/);
  const memoTrackerReferences = howItWorks.match(/Memo Tracker/g) || [];
  const memoTrackerLinks = howItWorks.match(/href="#metric-tracker"[^>]*>Memo Tracker<\/a>/g) || [];
  assert.equal(memoTrackerReferences.length, memoTrackerLinks.length);
  assert.match(howItWorks, /data-page="4" aria-label="Cycles Observability" aria-selected="false"/);
  assert.match(metricsCss, /\.pane-fixed-tooltip-more-info \{[\s\S]*display: inline-flex;[\s\S]*text-decoration: none;/);
  assert.match(metricsCss, /\.pane-fixed-tooltip-actions \{[\s\S]*display: flex;[\s\S]*align-items: center;[\s\S]*gap: 10px;/);
  assert.match(metricsCss, /\.pane-fixed-tooltip-close \{[\s\S]*margin-left: auto;/);
});

test('How it works pane includes advanced usage memo builder without restoring simulator controls', () => {
  const howItWorks = sectionMarkup('how-it-works');
  const memoBuilder = sectionMarkup('memo-builder');
  assert.doesNotMatch(howItWorks, /commitment-simulator-form/);
  assert.doesNotMatch(howItWorks, /Commitment simulator/);
  assert.match(howItWorks, /data-page="0"/);
  assert.match(howItWorks, /data-page="1"/);
  assert.match(howItWorks, /data-page="2"/);
  assert.match(howItWorks, /data-page="3"/);
  assert.match(howItWorks, /data-page="4"/);
  assert.match(howItWorks, /data-page="1"[\s\S]*Base maturity[\s\S]*data-page="2"[\s\S]*Advanced Usage[\s\S]*data-page="3"[\s\S]*Relay Canisters/);
  assert.match(howItWorks, /Advanced Usage/);
  assert.match(howItWorks, /three memo-directed flows/);
  assert.match(howItWorks, /plain declared canister ID/);
  assert.match(howItWorks, /To target a public neuron instead, use the neuron ID in the memo/);
  assert.match(howItWorks, /Declared neurons must be\s*'public'\s*in order for Jupiter Faucet to derive their staking accounts/);
  assert.match(howItWorks, /href="#memo-builder"[^>]*data-panel="memo-builder"[^>]*>memo builder<\/a>/);
  assert.match(howItWorks, /<strong>Developer tip:<\/strong> You can adjust the <code>canister<\/code>\/<code>neuron<\/code>,\s*<code>title<\/code>, and <code>label<\/code> parameters/);
  assert.match(howItWorks, /customize the memo helper\s*form for a smoother user experience/);
  assert.doesNotMatch(howItWorks, /<a class="memo-builder-tip-url/);
  assert.match(howItWorks, /id="memo-builder-tip-url"[^>]*data-copy-value="#memo-builder\?canister=\{protocol canister ID\}&amp;title=\{custom title\}&amp;label=\{custom label\}"/);
  assert.match(howItWorks, /memo-builder-tip-url mono[\s\S]*#memo-builder\?canister=/);
  assert.match(howItWorks, /id="copy-memo-builder-tip-url"[^>]*>Copy URL<\/button>/);
  assert.match(howItWorks, /memo-builder-placeholder[^>]*>\{protocol canister ID\}<\/span>&amp;title=/);
  assert.match(howItWorks, /memo-builder-placeholder[^>]*>\{custom title\}<\/span>&amp;label=/);
  assert.match(howItWorks, /memo-builder-placeholder[^>]*>\{custom label\}<\/span>/);
  assert.match(howItWorks, /<strong>e\.g\.<\/strong>/);
  assert.match(howItWorks, /href="\/#memo-builder\?canister=r5m5y-diaaa-aaaaa-qanaa-cai&amp;title=mAIner%20ID&amp;label=mAIner%20ID%20Prefix%20\(xxxxx-xx\)"[^>]*data-panel="memo-builder"[^>]*>mAIner ID Memo Builder<\/a>/);
  assert.match(howItWorks, /tops up the protocol's GameState canister with raw ICP; GameState then routes a\s*portion as cycles to the declared mAIner/);
  assert.match(howItWorks, /href="\/#memo-builder\?neuron=10292412127977304661&amp;title=IO%20Perpetual%20Stake&amp;label=Optional%20Donor%20Name"[^>]*data-panel="memo-builder"[^>]*>IO Perpetual Stake Memo Builder<\/a>/);
  assert.match(howItWorks, /perpetually pay into the IO neuron's staking account/);
  assert.match(howItWorks, /In this particular\s+case a <code>'\.'<\/code> suffix is superfluous/);
  assert.doesNotMatch(howItWorks, /<strong>Extra tip:<\/strong>/);
  assert.match(howItWorks, /A relay canister provides one stable destination for ICP transfers/);
  assert.match(howItWorks, /<strong>Target requirement:<\/strong> Every Relay target canister must make its cycles\s*balance publicly observable/);
  assert.match(howItWorks, /href="#how-it-works:4" data-page-target="4"[^>]*>Cycles Observability<\/a>\s*on the next slide/);
  assert.match(howItWorks, /steadily growing cycles balance for configured target canisters/);
  assert.match(howItWorks, /topping up 1% more\s*cycles than needed to restore the previously sampled balance/);
  assert.match(howItWorks, /<strong>Example:<\/strong> The full suite of Jupiter Faucet protocol canisters are managed by a single/);
  assert.match(howItWorks, /href="\/#metric-tracker\?memo=u2qkp-aqaaa-aaaar-qb7ea-cai\.&amp;range=month"[^>]*data-panel="metric-tracker"[^>]*>relay canister<\/a>/);
  assert.match(howItWorks, /href="\/#memo-builder\?canister=u2qkp-aqaaa-aaaar-qb7ea-cai&amp;title=Relay%20Canister&amp;label=Optional%20Donor%20Name"[^>]*data-panel="memo-builder"[^>]*>Relay Canister Memo Builder<\/a>/);
  assert.match(howItWorks, /raw ICP top-ups described in the\s*<a href="\/#how-it-works:2"[^>]*data-panel="how-it-works"[^>]*>Advanced Usage<\/a>/);
  assert.match(howItWorks, /This ensures Jupiter Faucet perpetually transfers raw ICP rather than\s*directly topping up the relay canister with cycles/);
  assert.match(howItWorks, /routes 50% of any surplus ICP back into\s*the Jupiter Faucet neuron/);
  assert.match(howItWorks, /forever increasing the rate of future\s*maturity-minted ICP back into the relay/);
  assert.match(howItWorks, /remaining surplus ICP\s*into the IO neuron's\s*staking account \(described below\)/);
  assert.match(howItWorks, /<strong>Example:<\/strong>[\s\S]*This flow depends on raw ICP top-ups[\s\S]*Anyone can deploy their own relay canister/);
  assert.match(howItWorks, /choosing either no surplus recipients for all-cycles mode or one to five immutable typed recipients/);
  assert.match(howItWorks, /Each recipient may be a Principal/);
  assert.match(howItWorks, /public NNS neuron ID/);
  assert.match(howItWorks, /No IO recipient\s*is added automatically to a self-service Relay/);
  assert.doesNotMatch(howItWorks, /surplus ICP will automatically be routed to the IO neuron/);
  assert.match(howItWorks, /href="\/#relay-setup"[^>]*data-panel="relay-setup"[^>]*>Relay Setup<\/a>/);
  assert.match(howItWorks, /href="\/#source"[^>]*data-panel="source"[^>]*>source code<\/a>/);
  assert.match(howItWorks, /A relay canister also accepts ICP transfers into its\s*<code>'1'<\/code> subaccount/);
  assert.match(howItWorks, /<code>u2qkp-aqaaa-aaaar-qb7ea-cai\.1<\/code>/);
  assert.match(howItWorks, /href="\/#how-it-works"[^>]*data-panel="how-it-works"[^>]*>1 ICP minimum Jupiter Faucet threshold<\/a>/);
  assert.doesNotMatch(howItWorks, /custom identifier label/);
  assert.match(metricsCss, /\.memo-builder-tip-url \{[\s\S]*overflow-wrap: anywhere;[\s\S]*\}/);
  assert.match(metricsCss, /\.memo-builder-tip-copy-row \{[\s\S]*display: flex;[\s\S]*\}/);
  assert.match(metricsCss, /\.memo-builder-example-list \{[\s\S]*border-left: 2px solid rgba\(255, 255, 255, 0\.16\);[\s\S]*\}/);
  assert.match(metricsCss, /\.memo-builder-placeholder \{[\s\S]*color: rgba\(255, 226, 168, 0\.78\);[\s\S]*\}/);
  assert.doesNotMatch(howItWorks, /memo helper utility coming soon/);
  assert.doesNotMatch(howItWorks, /This enables more specialized designs/);
  assert.match(howItWorks, /32 characters/);
  assert.match(memoBuilder, /Memo Builder/);
  assert.match(memoBuilder, /id="memo-builder-title"[^>]*>Memo Builder<\/h3>/);
  assert.match(memoBuilder, /id="memo-builder-prefill-note"[^>]*hidden/);
  assert.match(memoBuilder, /This memo helper simplifies constructing a memo from your chosen ID/);
  assert.match(memoBuilder, /protocol\s+canister that facilitates a specialized top-up flow/);
  assert.match(memoBuilder, /id="memo-builder-safety-notice"[^>]*hidden/);
  assert.match(memoBuilder, /<strong>Health &amp; Safety Notice:<\/strong>/);
  assert.match(memoBuilder, /identified\s+the controller of the <span id="memo-builder-safety-target-kind">protocol canister<\/span>/);
  assert.match(memoBuilder, /id="memo-builder-canister-dashboard-link"[^>]*href="#"/);
  assert.match(memoBuilder, /DAO or reputable pre-DAO dev team who prescribe this\s*<span id="memo-builder-safety-prescription-kind">canister<\/span>/);
  assert.match(memoBuilder, /Jupiter Faucet\s+is not responsible for lost funds resulting from user indiscretion/);
  assert.match(memoBuilder, /id="memo-builder-url-context"/);
  assert.match(memoBuilder, /id="memo-builder-mode-fieldset"/);
  assert.doesNotMatch(memoBuilder, /value="rawIcp" checked/);
  assert.match(memoBuilder, /value="cycles" checked/);
  assert.match(memoBuilder, /Cycles top-up canister/);
  assert.match(memoBuilder, /value="rawIcp"/);
  assert.match(memoBuilder, /Canister default account/);
  assert.match(memoBuilder, /Public neuron staking account/);
  assert.match(memoBuilder, /Canister default account[\s\S]*Public neuron staking account[\s\S]*Cycles top-up canister/);
  assert.doesNotMatch(memoBuilder, />Neuron staking account<\/span>/);
  assert.match(memoBuilder, /id="memo-builder-canister"/);
  assert.match(memoBuilder, /id="memo-builder-canister"[^>]*pattern="\[A-Za-z2-7-\]\*"/);
  assert.match(memoBuilder, /id="memo-builder-neuron-fields" hidden/);
  assert.match(memoBuilder, /Declared public neuron ID/);
  assert.doesNotMatch(memoBuilder, />Declared neuron ID<\/label>/);
  assert.match(memoBuilder, /id="memo-builder-neuron"/);
  assert.match(memoBuilder, /id="memo-builder-neuron"[^>]*pattern="\[0-9\]\*"/);
  assert.match(memoBuilder, /id="memo-builder-optional-fields" hidden/);
  assert.match(memoBuilder, /id="memo-builder-optional-label"[^>]*>Optional outgoing transfer memo<\/label>/);
  assert.match(memoBuilder, /id="memo-builder-optional"[^>]*contenteditable="plaintext-only"/);
  assert.match(memoBuilder, /id="memo-builder-optional"[^>]*role="textbox"/);
  assert.match(memoBuilder, /id="memo-builder-optional"[^>]*aria-labelledby="memo-builder-optional-label"/);
  assert.doesNotMatch(memoBuilder, /memo-builder-input-wrap/);
  assert.doesNotMatch(memoBuilder, /memo-builder-input-overlay/);
  assert.doesNotMatch(memoBuilder, /id="memo-builder-preview"/);
  assert.doesNotMatch(memoBuilder, /Used text:/);
  assert.doesNotMatch(memoBuilder, /Truncated text:/);
  assert.doesNotMatch(memoBuilder, /id="memo-builder-status"/);
  assert.doesNotMatch(memoBuilder, /id="memo-builder-availability"/);
  assert.doesNotMatch(memoBuilder, /ASCII bytes used/);
  assert.doesNotMatch(memoBuilder, /bytes available for optional memo text/);
  assert.doesNotMatch(memoBuilder, /id="memo-builder-remove-hyphens"/);
  assert.doesNotMatch(memoBuilder, /id="memo-builder-remove-optional-hyphens"/);
  assert.doesNotMatch(memoBuilder, />Remove hyphens<\/button>/);
  assert.match(memoBuilder, /id="memo-builder-output"[^>]*readonly/);
  assert.match(memoBuilder, /id="memo-builder-copy"[^>]*>Copy memo<\/button>/);
  assert.match(memoBuilder, /Use the generated memo as described in the/);
  assert.match(memoBuilder, /href="#how-it-works"[^>]*data-panel="how-it-works"[^>]*>basic instructions<\/a>/);
  assert.match(memoBuilder, /\(in place of the "declared canister ID"\) to make your ICP commitment and initiate\s+perpetual top-ups/);
  assert.match(memoBuilder, /Use the generated memo[\s\S]*For more information about this memo builder see/);
  assert.match(memoBuilder, /For more information about this memo builder see[\s\S]*href="#how-it-works:2"[^>]*data-panel="how-it-works"[^>]*>Advanced Usage<\/a>/);
  assert.doesNotMatch(howItWorks, /Use the copied memo/);
  assert.doesNotMatch(howItWorks, /target="_blank"[^>]*>Advanced Usage<\/a>/);
  assert.doesNotMatch(howItWorks, /target="_blank"[^>]*>basic instructions<\/a>/);
  assert.match(navbarJs, /route\.match\(\/\^\(\[\^:\]\+\):\(\\d\+\)\$\/\)/);
  assert.match(navbarJs, /panelHashFor\(key, clamped\)/);
  assert.match(mainJs, /params\.get\('canister'\)/);
  assert.match(mainJs, /params\.get\('neuron'\)/);
  assert.match(mainJs, /params\.get\('mode'\)/);
  assert.match(mainJs, /params\.get\('title'\)/);
  assert.match(mainJs, /params\.get\('label'\)/);
  assert.match(mainJs, /\(params\.get\('title'\) \|\| ''\)\.slice\(0, 30\)/);
  assert.match(mainJs, /\(params\.get\('label'\) \|\| ''\)\.slice\(0, 30\)/);
  assert.doesNotMatch(mainJs, /params\.get\('Label'\)/);
  assert.doesNotMatch(mainJs, /params\.get\('optionalLabel'\)/);
  assert.doesNotMatch(mainJs, /params\.get\('memoLabel'\)/);
  assert.doesNotMatch(mainJs, /params\.get\('memo_label'\)/);
  assert.match(mainJs, /builderTitle\.textContent = title \? `\$\{title\} Memo Builder` : defaultBuilderTitle/);
  assert.match(mainJs, /const hasPrefillTarget = canister !== null \|\| neuron !== null/);
  assert.doesNotMatch(mainJs, /urlPrefillWasActive/);
  assert.doesNotMatch(mainJs, /if \(!hasPrefillTarget[\s\S]*setOptionalMemoText\(''\)/);
  assert.match(mainJs, /let lastAppliedPrefillFragment = ''/);
  assert.match(mainJs, /shouldApplyAdvancedMemoUrlTargetValue\(currentFragment, lastAppliedPrefillFragment\)/);
  assert.doesNotMatch(mainJs, /shouldApplyTargetValue = currentFragment !== lastAppliedPrefillFragment \|\| hasCustomLabel/);
  assert.match(mainJs, /optionalLabel\.textContent = label \|\| \(hasPrefillTarget \? 'Identifier' : defaultOptionalLabel\)/);
  assert.doesNotMatch(mainJs, /canisterLabel\.textContent/);
  assert.doesNotMatch(mainJs, /neuronLabel\.textContent/);
  assert.match(mainJs, /advancedMemoUrlPrefillState\(\{ canister, neuron, requestedMode \}\)/);
  assert.match(mainJs, /targetType: urlTargetType, displayTarget: urlDisplayTarget/);
  assert.doesNotMatch(mainJs, /target: urlTarget/);
  assert.match(mainJs, /prefillNote\.hidden = !hasPrefillTarget/);
  assert.match(mainJs, /prefillNote\.textContent = urlTargetType === 'neuron' \? PREFILL_NEURON_NOTE : PREFILL_CANISTER_NOTE/);
  assert.match(mainJs, /This memo helper simplifies constructing a memo from your chosen ID and a public protocol neuron/);
  assert.match(mainJs, /safetyNotice\.hidden = !urlDisplayTarget/);
  assert.match(mainJs, /safetyTargetKind\.textContent = urlTargetType === 'neuron' \? 'protocol neuron' : 'protocol canister'/);
  assert.match(mainJs, /safetyPrescriptionKind\.textContent = urlTargetType === 'neuron' \? 'neuron' : 'canister'/);
  assert.match(mainJs, /urlContext\.textContent = urlDisplayTarget/);
  assert.match(mainJs, /const suppliedContext = \[`\$\{urlTargetType\} ID`\]/);
  assert.match(mainJs, /if \(title\) suppliedContext\.push\(`title '\$\{title\}'`\)/);
  assert.match(mainJs, /if \(label\) suppliedContext\.push\(`memo label '\$\{label\}'`\)/);
  assert.match(mainJs, /const suppliedContextText = suppliedContext\.length > 2/);
  assert.match(mainJs, /suppliedContext\.slice\(0, -1\)\.join\(', '\)/);
  assert.match(mainJs, /suppliedContext\.join\(' and '\)/);
  assert.match(mainJs, /The \$\{suppliedContextText\} \$\{suppliedContext\.length === 1 \? 'was' : 'were'\} supplied in the URL/);
  assert.doesNotMatch(mainJs, /optional memo label/);
  assert.match(mainJs, /canisterDashboardLink\.textContent = urlDisplayTarget/);
  assert.match(mainJs, /dashboard\.internetcomputer\.org\/\$\{urlTargetType\}\/\$\{encodeURIComponent\(urlDisplayTarget\)\}/);
  assert.doesNotMatch(mainJs, /requestedMode !== 'cycles' && hasCustomLabel/);
  assert.match(mainJs, /urlTargetMode = urlPrefill\.mode/);
  assert.match(mainJs, /urlLocksTarget = true/);
  assert.match(mainJs, /canisterInput && shouldApplyTargetValue/);
  assert.match(mainJs, /neuronInput && shouldApplyTargetValue/);
  assert.match(mainJs, /hasPrefillTarget && shouldApplyTargetValue/);
  assert.match(mainJs, /lastAppliedPrefillFragment = currentFragment/);
  assert.match(mainJs, /tipUrlCopyButton\?\.addEventListener\('click'/);
  assert.match(mainJs, /copyTextToClipboard\(value\)/);
  assert.match(mainJs, /const url = currentPageUrlForFragment\(value\);/);
  assert.match(mainJs, /copyTextToClipboard\(url\)/);
  assert.match(mainJs, /window\.addEventListener\('popstate', render\)/);
  assert.match(mainJs, /document\.addEventListener\('navpanel:open', render\)/);
  assert.match(mainJs, /document\.addEventListener\('navpanel:pagechange', render\)/);
  assert.match(mainJs, /if \(sanitizedCanister\) \{[\s\S]*\} else if \(sanitizedNeuron\) \{/);
  assert.match(mainJs, /\|\| 'cycles'/);
  assert.match(mainJs, /sanitizeCanisterPrincipalText/);
  assert.match(mainJs, /sanitizeNeuronIdText/);
  assert.doesNotMatch(mainJs, /removeHyphensButton/);
  assert.doesNotMatch(mainJs, /removeOptionalHyphensButton/);
  assert.doesNotMatch(mainJs, /replaceAll\('-', ''\)/);
  assert.match(mainJs, /clearHiddenModeInputs/);
  assert.match(mainJs, /const optionalMemoText = \(\) => optionalInput\?\.textContent \|\| ''/);
  assert.match(mainJs, /mode === 'cycles' && optionalMemoText\(\)/);
  assert.match(mainJs, /modeFieldset\.hidden = Boolean\(urlTargetMode\)/);
  assert.match(mainJs, /canisterFields\.hidden = urlLocksTarget \|\| mode === 'neuron'/);
  assert.match(mainJs, /neuronFields\.hidden = urlLocksTarget \|\| mode !== 'neuron'/);
  assert.match(mainJs, /optionalFields\.hidden = !hasOptionalMemoField/);
  assert.match(mainJs, /const renderOptionalMemoText = \(result, caretOffset = null\)/);
  assert.match(mainJs, /muted\.className = 'memo-builder-muted'/);
  assert.match(mainJs, /preserveOptionalCaret: Boolean/);
  assert.match(mainJs, /optionalInput\?\.addEventListener\('keydown'/);
  assert.match(mainJs, /optionalInput\?\.addEventListener\('paste'/);
  assert.doesNotMatch(mainJs, /optionalOverlay/);
  assert.doesNotMatch(mainJs, /optionalKept/);
  assert.doesNotMatch(mainJs, /optionalMuted/);
  assert.doesNotMatch(mainJs, /memo-builder-input--muted-overlay/);
  assert.doesNotMatch(mainJs, /syncOptionalOverlayScroll/);
  assert.doesNotMatch(mainJs, /memo-builder-preview/);
  assert.doesNotMatch(mainJs, /memo-builder-status/);
  assert.doesNotMatch(mainJs, /memo-builder-availability/);
  assert.doesNotMatch(mainJs, /ASCII bytes used/);
  assert.doesNotMatch(mainJs, /bytes available for optional memo text/);
  assert.doesNotMatch(howItWorks, /More information coming soon/);
});

test('simulator pane keeps controls outside the scroll region and places intro directly above charts', () => {
  const simulator = sectionMarkup('simulator');
  const headerIndex = simulator.indexOf('simulator-pane-header');
  const formIndex = simulator.indexOf('commitment-simulator-form');
  const scrollIndex = simulator.indexOf('simulator-scroll-region');
  const introIndex = simulator.indexOf('simulator-intro');
  const chartIndex = simulator.indexOf('simulator-chart-wrapper');
  const statusIndex = simulator.indexOf('simulator-assumption-note');
  const summaryIndex = simulator.indexOf('simulator-summary-grid');

  assert.ok(headerIndex >= 0, 'missing simulator header');
  assert.ok(formIndex > headerIndex, 'form should be inside the header');
  assert.ok(scrollIndex > formIndex, 'scroll region should start after the form');
  assert.ok(introIndex > scrollIndex, 'intro should be inside the scroll region');
  assert.ok(chartIndex > introIndex, 'intro should appear directly above the charts');
  assert.ok(statusIndex > chartIndex, 'assumption text should appear below the charts');
  assert.ok(summaryIndex > chartIndex, 'stats grid should appear below the charts');
  assert.match(simulator, /declared canister/);
  assert.doesNotMatch(simulator, /elected canister/);
});

test('simulator inputs are ordered by user control priority and use compact numeric controls', () => {
  const simulator = sectionMarkup('simulator');
  const commitmentIndex = indexOfInput(simulator, 'simulator-icp-commitment');
  const burnIndex = indexOfInput(simulator, 'simulator-daily-burn');
  const priceIndex = indexOfInput(simulator, 'simulator-icp-price');
  const apyIndex = indexOfInput(simulator, 'simulator-apy');

  assert.ok(commitmentIndex < burnIndex, 'ICP commitment should be first');
  assert.ok(burnIndex < priceIndex, 'daily burn should be second');
  assert.ok(priceIndex < apyIndex, 'APY should follow price');

  const commitment = elementById(simulator, 'simulator-icp-commitment');
  const burn = elementById(simulator, 'simulator-daily-burn');
  const price = elementById(simulator, 'simulator-icp-price');
  const apy = elementById(simulator, 'simulator-apy');
  assert.equal(attrValue(commitment, 'min'), '1');
  assert.equal(attrValue(commitment, 'step'), '0.1');
  assert.equal(attrValue(commitment, 'value'), null);
  assert.equal(attrValue(burn, 'step'), '0.0001');
  assert.equal(attrValue(burn, 'value'), '0.0001');
  assert.equal(attrValue(price, 'step'), '0.1');
  assert.equal(attrValue(price, 'value'), null);
  assert.equal(attrValue(price, 'placeholder'), 'Loading');
  assert.equal(attrValue(apy, 'step'), '0.1');
  assert.equal(attrValue(apy, 'value'), '7.0');
  assert.match(simulator, /Daily burn \(T cycles\)/);
  assert.match(simulator, /APY \(%\)/);
  assert.doesNotMatch(simulator, /id="simulator-icp-commitment"[^>]*value="100\.0"/);
});

test('simulator input binding sanitizes invalid values without blocking native controls', () => {
  assert.match(mainJs, /SIMULATOR_INPUT_CONSTRAINTS/);
  assert.match(mainJs, /'simulator-icp-commitment': \{ min: 1, fractionDigits: 1 \}/);
  assert.match(mainJs, /'simulator-daily-burn': \{ min: 0, fractionDigits: 4 \}/);
  assert.match(mainJs, /'simulator-icp-price': \{ min: 0\.1, fractionDigits: 1 \}/);
  assert.match(mainJs, /'simulator-apy': \{ min: 0, fractionDigits: 1 \}/);
  assert.doesNotMatch(mainJs, /beforeinput/);
  assert.doesNotMatch(mainJs, /wouldAcceptSimulatorInput/);
  assert.match(mainJs, /sanitiseSimulatorInput/);
});

test('simulator no longer exposes a starting-buffer stat or copy', () => {
  const simulator = sectionMarkup('simulator');
  assert.doesNotMatch(simulator, /starting buffer/i);
  assert.doesNotMatch(simulator, /simulator-required-buffer/);
  assert.doesNotMatch(mainJs, /requiredStartingBufferCycles/);
  assert.doesNotMatch(mainJs, /one-year starting buffer/i);
});

test('simulator renders the cycles balance chart before a weekly top-ups headline and clarifies APY copy', () => {
  const balanceIndex = mainJs.indexOf('<h3>Projected cycles balance</h3>');
  const topupsIndex = mainJs.indexOf('Projected weekly top-ups:');
  assert.ok(balanceIndex >= 0, 'missing balance chart header');
  assert.ok(topupsIndex >= 0, 'missing top-ups headline');
  assert.ok(balanceIndex < topupsIndex, 'balance chart should render before top-ups headline');
  assert.match(mainJs, /Projection uses the configured APY\. Exact APY depends on numerous factors/);
  assert.match(mainJs, /dashboard\.internetcomputer\.org\/neuron\/\$\{neuronId\.toString\(\)\}/);
  assert.match(mainJs, /effective top-up APY discounts the current age-bonus component/);
  assert.match(mainJs, /first projected payout happens on day one/);
  assert.match(mainJs, /weekly-cadence one-year projection/);
  assert.match(mainJs, /formatCompactTrillionCycles\(weeklyTopupCycles\)/);
  assert.match(mainJs, /Per weekly CMC top-up, based on the configured APY/);
  assert.doesNotMatch(mainJs, /Projected weekly CMC top-up cycles over one year/);
  assert.doesNotMatch(mainJs, /amountKey: 'projectedTopupCycles'/);
  assert.match(mainJs, /Projected weekly cycles balance over one year/);
});


test('simulator and Jupiter Stake expose age-bonus discount information', () => {
  const simulator = sectionMarkup('simulator');
  const stake = sectionMarkup('metric-stake');

  assert.match(viewFormattersJs, /of total maturity diverted/);
  assert.match(viewFormattersJs, /age bonus relative to base maturity/);
  assert.match(simulator, /Age bonus diverted/);
  assert.match(simulator, /id="simulator-age-bonus"/);
  assert.match(simulator, /Effective top-up APY/);
  assert.match(simulator, /id="simulator-effective-apy"/);
  assert.match(simulator, /ICP\/XDR rate source/);
  assert.match(simulator, /id="simulator-icp-xdr-source"/);
  assert.match(stake, /Age bonus diverted/);
  assert.match(stake, /id="stake-neuron-age-bonus"/);
  assert.match(stake, /Current maturity/);
  assert.match(stake, /id="stake-neuron-maturity"/);
  assert.match(stake, /Maturity disbursal/);
  assert.match(stake, /id="stake-neuron-disbursement"/);
  assert.match(mainJs, /formatIcpE8s\(neuron\.maturity_e8s_equivalent\)/);
  assert.match(mainJs, /formatMaturityDisbursementStatus/);
  assert.match(mainJs, /formatMaturityDisbursementLandingText/);
  assert.match(mainJs, /updateLandingDisbursementStatus/);
  assert.match(mainJs, /void ensureNeuronDetailsLoaded\(data\);/);
  assert.match(mainJs, /link\.href = '#metric-stake'/);
  assert.match(mainJs, /link\.textContent = 'More info'/);
  assert.match(nnsGovernanceDidJs, /maturity_disbursements_in_progress/);
  assert.match(nnsGovernanceDidJs, /maturity_e8s_equivalent/);
  assert.match(mainJs, /calculateAgeBonusBasisPointsFromAgingSince/);
  assert.match(mainJs, /state\.ageBonusBasisPoints/);
});

test('Total Output and Total Rewards are pages of Jupiter Stake rather than metric rail buttons', () => {
  const stake = sectionMarkup('metric-stake');
  const railStart = indexHtml.indexOf('<div class="nav-popover metric-rail" id="metrics-menu"');
  assert.ok(railStart >= 0, 'missing metrics rail');
  const rail = indexHtml.slice(railStart, indexHtml.indexOf('</div>', railStart) + '</div>'.length);

  assert.match(rail, /id="landing-next-run"[\s\S]*Jupiter Stake/);
  assert.match(mainJs, /setText\('landing-next-run', subtitle\);/);
  assert.match(rail, /Jupiter Stake[\s\S]*Patron Commitments[\s\S]*Track Memos/);
  assert.doesNotMatch(rail, /Create Relay/);
  assert.doesNotMatch(rail, /Declared Canisters/);
  assert.doesNotMatch(rail, /Target Canisters/);
  assert.doesNotMatch(rail, />Commitments<\/span>/);
  assert.doesNotMatch(rail, /data-panel="metric-output"/);
  assert.doesNotMatch(rail, /data-panel="metric-rewards"/);
  assert.doesNotMatch(indexHtml, /id="nav-panel-metric-output"/);
  assert.doesNotMatch(indexHtml, /id="nav-panel-metric-rewards"/);
  assert.match(indexHtml, /id="nav-panel-metric-commitments"[\s\S]*Patron Commitments/);
  assert.doesNotMatch(indexHtml, /id="nav-panel-metric-registered"/);
  assert.match(stake, /data-page="1"[\s\S]*Total Output/);
  assert.match(stake, /data-page="2"[\s\S]*Total Rewards/);
  assert.match(stake, /data-page="3"[\s\S]*D-QUORUM Route/);
  assert.match(stake, /aria-label="D-QUORUM route"/);
  assert.match(navbarJs, /key === "metric-output"[\s\S]*key: "metric-stake", page: 1/);
  assert.match(navbarJs, /key === "metric-rewards"[\s\S]*key: "metric-stake", page: 2/);
  assert.match(navbarJs, /key === "metric-registered"[\s\S]*key: "metric-commitments", page: 0/);
});

test('Actions nav button exposes Plan Commit and Optimize pane links', () => {
  const actionsStart = indexHtml.indexOf('<div class="nav-popover action-rail"');
  assert.ok(actionsStart >= 0, 'missing actions rail');
  const actionsRail = indexHtml.slice(actionsStart, indexHtml.indexOf('</div>', actionsStart) + '</div>'.length);

  assert.match(indexHtml, /id="actions-menu-toggle"[\s\S]*aria-controls="actions-menu"[\s\S]*>Actions<\/button>/);
  assert.match(indexHtml, /id="metrics-menu-toggle"[\s\S]*aria-controls="metrics-menu"[\s\S]*>Metrics<\/button>/);
  assert.match(actionsRail, /href="#simulator"[^>]*data-panel="simulator"[\s\S]*>Plan<\/span>/);
  assert.match(actionsRail, /href="#memo-builder"[^>]*data-panel="memo-builder"[\s\S]*>Commit<\/span>/);
  assert.match(actionsRail, /href="#relay-setup"[^>]*data-panel="relay-setup"[\s\S]*>Optimize<\/span>/);
  assert.match(indexHtml, /<div class="nav-disclosure" data-nav-group="actions">\s*<button[\s\S]*id="actions-menu-toggle"[\s\S]*<\/button>\s*<div class="nav-popover action-rail" id="actions-menu"[^>]*hidden>/);
  assert.match(indexHtml, /<div class="nav-disclosure nav-disclosure--end" data-nav-group="metrics">\s*<button[\s\S]*id="metrics-menu-toggle"[\s\S]*<\/button>\s*<div class="nav-popover metric-rail" id="metrics-menu"[^>]*hidden>/);
  const navItemRule = navbarCss.match(/\.nav-item \{[^}]*\}/)?.[0] || '';
  assert.match(navbarCss, /\.nav-disclosure \{[\s\S]*position: relative;[\s\S]*display: inline-flex;[\s\S]*flex: 0 0 auto;[\s\S]*\}/);
  assert.doesNotMatch(navItemRule, /white-space: nowrap/);
  assert.match(navbarCss, /\.navbar-inner > \.nav-links > \.nav-item,[\s\S]*\.nav-disclosure-toggle \{[\s\S]*white-space: nowrap;[\s\S]*\}/);
  assert.match(metricsCss, /\.nav-popover \{[\s\S]*position: absolute;[\s\S]*top: calc\(100% \+ 8px\);[\s\S]*left: 0;[\s\S]*width: max-content;[\s\S]*min-width: 100%;[\s\S]*\}/);
  assert.match(metricsCss, /\.nav-disclosure--end \.nav-popover \{[\s\S]*left: auto;[\s\S]*right: 0;[\s\S]*\}/);
  assert.match(metricsCss, /\.nav-popover\[hidden\] \{[\s\S]*display: none;[\s\S]*\}/);
  assert.match(metricsCss, /\.action-rail \{[\s\S]*text-align: left;[\s\S]*\}/);
  assert.match(metricsCss, /\.action-rail-list \{[\s\S]*align-items: flex-start;[\s\S]*\}/);
  assert.match(metricsCss, /\.action-rail \.metric-rail-link \{[\s\S]*justify-content: flex-start;[\s\S]*text-align: left;[\s\S]*white-space: nowrap;[\s\S]*\}/);
  assert.match(metricsCss, /\.metric-rail-subtitle \{[\s\S]*max-width: none;[\s\S]*white-space: nowrap;[\s\S]*\}/);
  assert.match(metricsCss, /@media \(max-width: 720px\) \{[\s\S]*\.metric-rail-link \{[\s\S]*white-space: normal;[\s\S]*\}/);
  assert.match(metricsCss, /@media \(max-width: 720px\) \{[\s\S]*\.metric-rail-subtitle \{[\s\S]*max-width: min\(78vw, 340px\);[\s\S]*white-space: normal;[\s\S]*overflow-wrap: anywhere;[\s\S]*\}/);
  assert.match(navbarJs, /const CLOSED_NAV_STATE = Object\.freeze/);
  assert.match(navbarJs, /let navState = \{ \.\.\.CLOSED_NAV_STATE \};/);
  assert.match(navbarJs, /function renderNavState/);
  assert.match(navbarJs, /const actionsToggle = document\.getElementById\("actions-menu-toggle"\);/);
  assert.match(navbarJs, /function groupForPanelRoute\(key, page = 0, trigger = null\)/);
  assert.match(navbarJs, /\(key === "how-it-works" && page === 3\)/);
  assert.match(navbarJs, /document\.body\.classList\.toggle\("metrics-menu-open", metricsDisclosureVisible\);/);
  assert.match(navbarJs, /if \(navState\.openPanel\) clearPanelHash\(\);[\s\S]*openMenu: group/);
  assert.doesNotMatch(navbarJs, /positionMenuRails|getBoundingClientRect|let actionsMenuOpen|let metricsMenuOpen|activePanelKey|metric-rail--visible/);
});

test('Relay Setup uses accessible repeatable target and recipient fields', () => {
  const relaySetup = sectionMarkup('relay-setup');

  assert.equal(relaySetup.match(/<fieldset class="relay-setup-principal-fieldset">/g)?.length, 2);
  assert.match(relaySetup, /<legend class="tracker-label">Target canisters<\/legend>/);
  assert.doesNotMatch(relaySetup, /Target canisters \(1–20\)/);
  assert.match(relaySetup, /Enter one canister ID per field/);
  assert.match(relaySetup, /id="relay-setup-target-list"/);
  assert.match(relaySetup, /data-relay-target-input="true"/);
  assert.match(relaySetup, /id="relay-setup-add-target"[^>]*>Add another canister<\/button>/);
  assert.match(relaySetup, /data-relay-target-remove="true"[^>]*hidden>Remove<\/button>/);
  assert.match(relaySetup, /id="relay-setup-target-count-hint"[^>]*>1 target canister<\/span>/);
  assert.match(relaySetup, /<legend class="tracker-label">Surplus recipients<\/legend>/);
  assert.match(relaySetup, /<legend class="tracker-label">Surplus handling<\/legend>/);
  assert.match(relaySetup, /id="relay-setup-mode-routing"[^>]*type="radio"[^>]*checked/);
  assert.match(relaySetup, /id="relay-setup-mode-all-cycles"[^>]*type="radio"/);
  assert.match(relaySetup, /No surplus recipients — all-cycles mode/);
  const surplusModeStatusIndex = relaySetup.indexOf('id="relay-setup-surplus-mode-summary"');
  const recipientEditorIndex = relaySetup.indexOf('id="relay-setup-recipient-editor"');
  assert.ok(surplusModeStatusIndex >= 0 && surplusModeStatusIndex < recipientEditorIndex);
  assert.match(relaySetup, /id="relay-setup-surplus-mode-summary"[^>]*role="status"[^>]*aria-live="polite"/);
  assert.match(relaySetup, /data-relay-recipient-input="true"/);
  assert.match(relaySetup, /data-relay-recipient-type="true"/);
  assert.match(relaySetup, /<option value="Principal" selected>Principal<\/option>/);
  assert.match(relaySetup, /<option value="Neuron">Neuron ID<\/option>/);
  assert.match(relaySetup, /id="relay-setup-add-recipient"[^>]*>Add another recipient<\/button>/);
  assert.match(relaySetup, /data-relay-recipient-remove="true"[^>]*>Remove<\/button>/);
  assert.match(relaySetup, /data-relay-recipient-memo-mode="true"/);
  assert.match(relaySetup, /<option value="Text" selected>Text<\/option>/);
  assert.match(relaySetup, /<option value="Hex">Hexadecimal<\/option>/);
  assert.match(relaySetup, /aria-describedby="relay-setup-recipient-memo-count-1 relay-setup-recipient-memo-error-1 relay-setup-recipient-memo-notice-1"[^>]*data-relay-recipient-memo-input="true"/);
  assert.match(relaySetup, /data-relay-recipient-memo-count="true">0\/32 bytes<\/span>/);
  assert.match(relaySetup, /id="relay-setup-recipient-memo-notice-1"[^>]*role="status"[^>]*aria-live="polite"[^>]*data-relay-recipient-memo-notice="true"/);
  assert.match(relaySetup, /id="relay-setup-recipient-count-hint"[^>]*>1 surplus recipient<\/span>/);
  assert.match(relaySetup, /id="relay-setup-submit"[^>]*disabled[^>]*>Check Relay configuration<\/button>/);
  assert.match(relaySetup, /id="relay-setup-warning"[^>]*role="status"[^>]*aria-live="polite"/);
  assert.match(relaySetup, /Targets, recipient destinations, and exact memo bytes determine the setup address/);
  assert.match(relaySetup, /Changing a memo usually changes the funding account/);
  assert.match(relaySetup, /incorrectly selected Relay configuration are not automatically refundable/);
  assert.match(relaySetup, /id="relay-setup-recipient-count"/);
  assert.match(relaySetup, /id="relay-setup-surplus-mode"/);
  assert.match(relaySetup, /id="relay-setup-canonical-recipients"/);
  assert.match(relaySetup, /id="relay-setup-configuration-hash"/);
  assert.doesNotMatch(relaySetup, /Separate canister IDs with newlines, commas, or spaces/);
  assert.doesNotMatch(relaySetup, /<textarea/);
  assert.match(metricsCss, /\.relay-setup-principal-row--error \{[\s\S]*border-color:/);
  assert.match(metricsCss, /\.relay-setup-principal-input\[aria-invalid="true"\]/);
  assert.match(metricsCss, /\.relay-setup-submit \{[\s\S]*margin-left: auto;/);
});

test('Patron Commitments table omits redundant category column', () => {
  const commitments = sectionMarkup('metric-commitments');
  assert.match(commitments, /See <a href="#how-it-works"[^>]*data-panel="how-it-works"[^>]*>How It Works<\/a> for qualifying commitment rules[\s\S]*<h3 class="pane-section-title">Declared Canisters <span id="commitments-canister-count"><\/span><\/h3>[\s\S]*<th>Timestamp<\/th>[\s\S]*<th>Amount<\/th>[\s\S]*<th>Declared<\/th>/);
  assert.match(commitments, /See <a href="#how-it-works:2"[^>]*>Advanced Usage<\/a> for raw ICP commitment rules[\s\S]*<h3 class="pane-section-title">Declared Raw ICP Canisters <span id="commitments-raw-canister-count"><\/span><\/h3>[\s\S]*<th>Declared<\/th>/);
  assert.match(commitments, /See <a href="#how-it-works:2"[^>]*>Advanced Usage<\/a> for neuron commitment rules[\s\S]*<h3 class="pane-section-title">Declared Neurons <span id="commitments-neuron-count"><\/span><\/h3>[\s\S]*<th>Declared<\/th>/);
  assert.doesNotMatch(commitments, /<th>Memo<\/th>/);
  assert.match(commitments, /aria-label="Patron Commitment pages"[\s\S]*aria-label="Declared Neurons"/);
  assert.match(mainJs, /const countDisplays = dashboardCountDisplays\(data\?\.counts\);/);
  assert.match(countDisplaysJs, /tracked_canister_count/);
  assert.match(countDisplaysJs, /memo_registered_canister_count/);
  assert.match(mainJs, /setText\('commitments-pane-subtitle', subtitle\);/);
  assert.doesNotMatch(mainJs, /\$\{formatInteger\(registeredCount\)\} declared canisters\./);
  assert.match(mainJs, /setText\('commitments-canister-count', countDisplays\.declaredCanisterBadge\);/);
  assert.match(mainJs, /const optionalCountValue = \(value\) => \(Array\.isArray\(value\) \? value\[0\] : value\);/);
  assert.match(mainJs, /const rawIcpDeclaredCanisterCount = optionalCountValue\(data\?\.counts\?\.raw_icp_declared_canister_count\);/);
  assert.match(mainJs, /const declaredNeuronCount = optionalCountValue\(data\?\.counts\?\.declared_neuron_count\);/);
  assert.match(mainJs, /setText\('commitments-raw-canister-count', commitmentsRawCanisterCount\);/);
  assert.match(mainJs, /setText\('commitments-neuron-count', commitmentsNeuronCount\);/);
  assert.doesNotMatch(commitments, /private neurons cannot be refreshed by the faucet top-up process/);
  assert.doesNotMatch(commitments, /<th>Category<\/th>/);
  assert.match(commitments, /<td colspan="3" class="empty-cell">Loading…<\/td>/);
  assert.doesNotMatch(commitments, /<td colspan="4" class="empty-cell">Loading…<\/td>/);
  assert.doesNotMatch(mainJs, /formatCommitmentOutcome/);
  assert.doesNotMatch(mainJs, /commitmentOutcomeCategory/);
  assert.match(mainJs, /const renderCommitmentsPane = \(data\) => \{[\s\S]*rawMemo === undefined \|\| rawMemo === null[\s\S]*commitments-raw[\s\S]*rawIcpDeclaredMemo\(item\)[\s\S]*commitments-neurons[\s\S]*neuronDeclaredMemo\(item\)/);
  assert.match(mainJs, /'commitments-raw', renderCommitmentsPane/);
  assert.match(mainJs, /'commitments-neurons', renderCommitmentsPane/);
});

test('Tracker results render chart controls and graphs before explanatory text', () => {
  const rangeControlsIndex = mainJs.indexOf('${renderTrackerRangeControls()}');
  const chartWrapperIndex = mainJs.indexOf('<div class="tracker-chart-wrapper" id="tracker-chart-wrapper"></div>');
  const logsIndex = mainJs.indexOf('${renderTrackerLogs(data)}');
  const cyclesProbeIssueIndex = mainJs.indexOf('${cyclesProbeIssueNote}');
  const summaryGridIndex = mainJs.indexOf('<dl class="pane-detail-grid tracker-summary-grid">');
  const showingNoteIndex = mainJs.indexOf('Showing ${escapeHtml(rangeLabel)} using');

  assert.ok(rangeControlsIndex >= 0, 'missing tracker range controls render');
  assert.ok(chartWrapperIndex > rangeControlsIndex, 'chart wrapper should render after range controls');
  assert.ok(cyclesProbeIssueIndex > chartWrapperIndex, 'cycles probe info should render below charts');
  assert.ok(logsIndex > cyclesProbeIssueIndex, 'logs should render below related cycle probe info');
  assert.ok(summaryGridIndex > logsIndex, 'summary text should render below logs');
  assert.ok(summaryGridIndex > chartWrapperIndex, 'summary text should render below charts');
  assert.ok(showingNoteIndex > summaryGridIndex, 'explanatory text should render below summary');
  assert.match(metricsCss, /\.tracker-chart-wrapper \{[\s\S]*margin: 16px 0 20px;/);
  assert.match(trackerControllerJs, /renderInitialTrackerLoadingState\(parsed\)/);
  assert.match(trackerControllerJs, /Tracker charts are loading\. Available data will appear as each source completes\./);
  assert.match(metricsCss, /\.tracker-chart-loading \{[\s\S]*min-height: 132px;[\s\S]*\}/);
  assert.match(metricsCss, /@media \(prefers-reduced-motion: reduce\) \{[\s\S]*\.tracker-chart-loading-bars i \{[\s\S]*animation: none;/);
});



test('paged nav panel content keeps a stable panel height while preserving overflow scrolling', () => {
  assert.match(navbarCss, /\.nav-panel \{[\s\S]*height: min\(720px, calc\(100dvh - 112px\)\);[\s\S]*overflow: hidden;[\s\S]*\}/);
  assert.match(navbarCss, /\.nav-panel-page\.is-active \{[\s\S]*display: flex;[\s\S]*flex: 1;[\s\S]*overflow: auto;[\s\S]*\}/);
  assert.match(navbarCss, /\.nav-panel-page\.is-active > \.nav-panel-scroll-region \{[\s\S]*padding-right: 0;[\s\S]*\}/);
  assert.doesNotMatch(navbarCss, /\.nav-panel-page\.is-active \{[\s\S]*max-height: 40vh;[\s\S]*\}/);
  assert.match(navbarCss, /\.nav-panel-scroll-region \{[\s\S]*overflow: auto;[\s\S]*\}/);
  assert.match(navbarCss, /\.nav-panel-dots \{[\s\S]*flex-shrink: 0;[\s\S]*margin-top: auto;[\s\S]*\}/);
  assert.doesNotMatch(navbarCss, /\.nav-panel-scroll-region \{[\s\S]*max-height: min\(46vh, calc\(100dvh - 220px\)\);[\s\S]*\}/);
  const commitments = sectionMarkup('metric-commitments');
  const scrollRegionEnd = commitments.indexOf('</div>\n          <div class="nav-panel-dots"');
  assert.ok(scrollRegionEnd > 0, 'commitment pane dots should sit outside the scroll region');
  assert.match(navbarJs, /pointerDownOnBackdrop = evt\.target === backdrop/);
  assert.match(navbarJs, /const shouldClose = evt\.target === backdrop && pointerDownOnBackdrop/);
});

test('maturity route pages clarify staging account routing and D-QUORUM account lookup', () => {
  assert.match(mainJs, /Jupiter neuron maturity is disbursed to the controlling canister's/);
  assert.match(mainJs, /dashboardAccountLink\(data\?\.status\?\.output_source_account/);
  assert.doesNotMatch(mainJs, /DQUORUM_STAKING_ACCOUNT_EXPLORER_ACCOUNT_HEX/);
  assert.match(mainJs, /dashboardAccountLink\(destinationAccount, destinationLabel\)/);
  assert.match(mainJs, /renderDquorumPane/);
  assert.match(mainJs, /dquorumStakingAccount/);
  assert.match(mainJs, /No D-QUORUM route transfers found in the recent index window/);
  assert.match(indexHtml, /a well-known ecosystem participant/);
});

test('simulator displays T-cycle values with four decimal places and uses weekly headline copy', () => {
  assert.match(mainJs, /const tenThousandths = \(absolute \* 10_000n\) \/ 1_000_000_000_000n;/);
  assert.match(mainJs, /padStart\(4, '0'\)/);
  assert.match(mainJs, /formatCompactTrillionCycles/);
  assert.match(mainJs, /Per weekly CMC top-up, based on the configured APY/);
  assert.match(mainJs, /Line samples the weekly cadence/);
});

test('metrics nav button closes an open pane before showing the metrics rail', () => {
  assert.match(navbarJs, /navState\.openMenu === "metrics" \|\| navState\.panelOwner === "metrics"/);
  assert.match(navbarJs, /setClosedState\(\);[\s\S]*return;[\s\S]*setMenuState\("metrics"\);/);
});

test('navbar brand link closes visible pane state before navigating to intro', () => {
  assert.match(indexHtml, /<a href="\/#intro" class="nav-brand" aria-label="Jupiter Faucet intro">/);
  assert.match(navbarJs, /const brandLink = document\.querySelector\("\.nav-brand"\);/);
  assert.match(navbarJs, /brandLink\?\.addEventListener\("click", \(\) => \{/);
  assert.match(navbarJs, /setClosedState\(\{ syncHash: false, restoreFocus: false \}\);/);
  assert.doesNotMatch(navbarJs, /brandLink\?\.addEventListener\("click", \(evt\) => \{[\s\S]*?evt\.preventDefault\(\);/);
});

test('pane fragment navigation participates in browser history', () => {
  assert.match(navbarJs, /const hrefTarget = panelTargetFromHash\(trigger\.getAttribute\("href"\)\);/);
  assert.match(navbarJs, /const page = hrefTarget\.key === key \? hrefTarget\.page : 0;/);
  assert.match(navbarJs, /const nextHash = hashOverride \|\| panelHashFor\(key, page\);/);
  assert.match(navbarJs, /history\.pushState\(null, "", nextHash\);/);
  assert.match(navbarJs, /setPanelState\(key, page, owner, \{ hashOverride: hrefTarget\.hash \}\);/);
  assert.match(navbarJs, /function panelHashFor\(key, pageIndex = 0\)/);
  assert.match(navbarJs, /return pageIndex > 0 \? `#\$\{key\}:\$\{pageIndex\}` : `#\$\{key\}`;/);
  assert.match(navbarJs, /document\.dispatchEvent\(new CustomEvent\("navpanel:pagechange"/);
  assert.match(navbarJs, /detail: \{[\s\S]*key: sectionEl\.getAttribute\("data-panel"\),[\s\S]*page: clamped/);
  assert.match(navbarJs, /window\.addEventListener\("popstate", \(\) => applyHash\(window\.location\.hash\)\);/);
  assert.match(navbarJs, /if \(!key\) \{[\s\S]*navState = \{ \.\.\.CLOSED_NAV_STATE \};/);
  assert.match(navbarJs, /function setClosedState\(\{ syncHash = true, restoreFocus = true \} = \{\}\)/);
  assert.match(navbarJs, /if \(syncHash\) \{[\s\S]*clearPanelHash\(\);[\s\S]*\}/);
  assert.doesNotMatch(navbarJs, /history\.replaceState\(null, "", `#\$\{key\}`\);/);
});

test('pane arrow-key navigation does not intercept text field caret movement', () => {
  assert.match(navbarJs, /function isTextEditingTarget\(target\)/);
  assert.match(navbarJs, /target\.closest\("input, textarea, select, \[contenteditable\]"\)/);
  assert.match(navbarJs, /target\.isContentEditable/);
  assert.match(navbarJs, /function handlePanelArrowKeydown\(evt\)/);
  assert.match(navbarJs, /if \(isTextEditingTarget\(evt\.target\) \|\| isTextEditingTarget\(document\.activeElement\)\) return;/);
  assert.match(navbarJs, /document\.addEventListener\("keydown", handlePanelArrowKeydown\);/);
});

test('pane arrow-key guard covers plaintext-only contenteditable fields', () => {
  assert.match(indexHtml, /id="memo-builder-optional"[^>]*contenteditable="plaintext-only"/);
  assert.match(navbarJs, /\[contenteditable\]/);
});

test('pane focus does not strip deep-link query parameters', () => {
  const focusStart = navbarJs.indexOf('backdrop.addEventListener("focusin"');
  assert.ok(focusStart >= 0, 'missing focusin listener');
  const focusBlock = navbarJs.slice(focusStart, navbarJs.indexOf('function handlePanelArrowKeydown', focusStart));
  assert.match(focusBlock, /navState\.panelPage = page;[\s\S]*renderNavState\(\);/);
  assert.doesNotMatch(focusBlock, /syncPanelHash: true/);
});

test('canister tracker defaults to last month history', () => {
  assert.match(mainJs, /const TRACKER_DEFAULT_RANGE = 'month'/);
  assert.match(mainJs, /const state = \{[\s\S]*?range: TRACKER_DEFAULT_RANGE/);
});

test('simulator header and scroll region have dedicated compact layout CSS', () => {
  assert.match(metricsCss, /\.simulator-pane-header\s*\{/);
  assert.match(metricsCss, /\.simulator-form--header\s*\{/);
  assert.match(metricsCss, /\.simulator-scroll-region\s*\{[\s\S]*flex: 1;[\s\S]*min-height: 0;[\s\S]*\}/);
  assert.doesNotMatch(metricsCss, /\.simulator-scroll-region\s*\{[\s\S]*max-height:/);
  assert.match(metricsCss, /display: flex;/);
  assert.match(metricsCss, /flex-wrap: wrap;/);
  assert.match(metricsCss, /#simulator-daily-burn \{\n  width: 112px;/);
  assert.match(metricsCss, /@media \(max-width: 560px\)/);
});

test('simulator prepopulates ICP/XDR price from historian XRC cache without overwriting user edits', () => {
  assert.match(mainJs, /const applyIcpXdrRateFromStatus = \(status\) =>/);
  assert.match(mainJs, /readOpt\(status\?\.icp_xdr_rate\)/);
  assert.match(mainJs, /formatIcpXdrRateInput/);
  assert.match(mainJs, /state\.icpPriceUserEdited/);
  assert.match(mainJs, /historian’s daily XRC cache/);
  assert.match(mainJs, /No cached XRC ICP\/XDR rate is available yet/);
  assert.match(mainJs, /formatIcpXdrRateSource/);
  assert.match(mainJs, /Historian XRC cache:/);
  assert.match(mainJs, /Fetched \$\{formatTimestampSeconds/);
  assert.match(mainJs, /formatIcpXdrRateSource\(snapshot, manualOverride = false\)/);
  assert.match(mainJs, /Manual override; \$\{cacheText\}/);
  assert.match(mainJs, /formatIcpXdrRateSource\(\s*state\.icpXdrRateSnapshot,\s*state\.icpPriceUserEdited,\s*\)/);
});


test('canister tracker links use shareable metric-tracker fragments', () => {
  assert.match(mainJs, /const TRACKER_HASH_PREFIX = '#metric-tracker-'/);
  assert.match(mainJs, /trackerHashForPrincipal/);
  assert.match(mainJs, /trackerStateFromHash/);
  assert.match(navbarJs, /key\.startsWith\("metric-tracker-"\)/);
  assert.match(indexHtml, /href="#metric-tracker-uccpi-cqaaa-aaaar-qby3q-cai"/);
});

test('simulator prefill links use shareable simulator fragments', () => {
  const simulator = sectionMarkup('simulator');
  assert.match(simulator, /id="simulator-copy-url"[^>]*type="button"[^>]*>Copy to URL<\/button>/);
  assert.match(mainJs, /const SIMULATOR_HASH_PREFIX = '#simulator-'/);
  assert.match(mainJs, /simulatorHashForPrefill/);
  assert.match(mainJs, /simulatorPrefillFromHash/);
  assert.match(mainJs, /hydrateFromLocationHash/);
  assert.match(mainJs, /simulatorShareHashFromInputs/);
  assert.match(mainJs, /simulatorShareUrlFromInputs/);
  assert.match(mainJs, /bindSimulatorShareUrlButton/);
  assert.match(mainJs, /history\.replaceState\(null, '', hash\)/);
  assert.match(mainJs, /copyTextToClipboard\(url\)/);
  assert.match(mainJs, /Copied to URL/);
  assert.match(mainJs, /new URLSearchParams/);
  assert.match(mainJs, /params\.set\('burn'/);
  assert.match(mainJs, /params\.set\('commitment'/);
  assert.match(mainJs, /params\.set\('price'/);
  assert.match(mainJs, /params\.set\('apy'/);
  assert.match(mainJs, /assumedIcpPrice: params\.get\('price'\)/);
  assert.match(mainJs, /annualApyPercent: params\.get\('apy'\)/);
  assert.match(mainJs, /SIMULATOR_INPUT_CONSTRAINTS\['simulator-icp-price'\]/);
  assert.match(mainJs, /SIMULATOR_INPUT_CONSTRAINTS\['simulator-apy'\]/);
  assert.match(mainJs, /state\.icpPriceUserEdited = true;/);
  assert.match(mainJs, /href="\$\{escapeHtml\(simulatorHashForPrefill/);
  assert.match(navbarJs, /key\.startsWith\("simulator-"\)/);
  assert.match(metricsCss, /\.simulator-copy-url\s*\{[\s\S]*height: 32px;[\s\S]*white-space: nowrap;/);
});

test('metric tracker hash deep links submit once on cold load and panel open', () => {
  assert.match(mainJs, /let lastHashSubmitMemo = ''/);
  assert.match(mainJs, /getTrackerController\(\)\.then\(\(controller\) => controller\.hydrateFromLocationHash\(\{ submit: true \}\)\)/);
  assert.match(mainJs, /const trackerSubmissionKey = \(/);
  assert.match(mainJs, /lastHashSubmitMemo = trackerSubmissionKey\(raw\)/);
  assert.match(mainJs, /const submissionKey = trackerSubmissionKey\(memoText\)/);
  assert.match(mainJs, /submit && lastHashSubmitMemo !== submissionKey/);
  assert.match(mainJs, /lastHashSubmitMemo = submissionKey/);
  assert.match(mainJs, /replaceLocationHash\(raw\);/);
  assert.match(mainJs, /event\?\.detail\?\.key === 'metric-tracker'[\s\S]*await getTrackerController\(\)[\s\S]*trackerController\.hydrateFromLocationHash\(\{ submit: true \}\)/);
});

test('canister tracker displays cycles as T cycles and estimates burn per day', () => {
  assert.match(mainJs, /function formatCycles\(value\) \{\n  return formatTrillionCycles\(value\);/);
  assert.match(mainJs, /const trackerCyclesChartPoints = \(data\) =>/);
  assert.match(mainJs, /trackerCyclesChartPoints = \(data\) => sortedCycleSamples\(data\)\.map/);
  assert.match(trackerCyclesJs, /function sortedLogCycleSamples\(data\)/);
  assert.match(trackerCyclesJs, /Cycles:\\s\*\(\[0-9\]\[0-9_,\]\*\)/);
  assert.match(mainJs, /const trackerCyclesPointLabel = \(point\) =>/);
  assert.match(mainJs, /formatTimestampNanos\(point\.timestampNanos\)/);
  assert.match(mainJs, /pointLabelBuilder: trackerCyclesPointLabel/);
  assert.match(mainJs, /xDomainBuckets: timelineBuckets/);
  assert.match(mainJs, /xTickBuckets: timelineBuckets/);
  assert.doesNotMatch(mainJs, /Line shows each loaded cycles probe/);
  assert.match(mainJs, /cyclesProbeIssueNote/);
  assert.match(mainJs, /cyclesStatus\.kind !== 'error'/);
  assert.match(mainJs, /cyclesStatus\.kind !== 'notAvailable'/);
  assert.match(mainJs, /Estimated observed cycles burned\/day/);
  assert.match(mainJs, /data-tooltip-id="tracker-burn-estimate-help"/);
  assert.match(mainJs, /Observation window:/);
  assert.doesNotMatch(mainJs, /Estimated observed cycles burn is calculated from loaded/);
  assert.match(mainJs, /renderCyclesProbeInfoNote/);
  assert.match(mainJs, /using canister log cycles/);
  assert.match(mainJs, /const estimatedObservedCyclesBurnedPerDay = estimateCyclesBurnedPerDay\(classifiedData\);/);
  assert.match(trackerCyclesJs, /estimateCyclesBurnedPerDay/);
  assert.match(mainJs, /formatTrillionCyclesPerDay/);
  assert.match(mainJs, /renderTrackerLogs\(data\)/);
  assert.match(mainJs, /data-simulator-prefill="true"/);
  assert.match(trackerControllerJs, /const TRACKER_REGISTRATION_URL = '#how-it-works';/);
  assert.match(trackerControllerJs, /href="\$\{TRACKER_REGISTRATION_URL\}" data-panel="how-it-works">How it works guide<\/a>/);
  assert.match(metricsCss, /\.tracker-log-details\s*\{/);
});

test('burn estimate help describes the approximation and CMC conversion assumptions', () => {
  assert.match(bootstrapJs, /'tracker-burn-estimate-help'/);
  assert.match(bootstrapJs, /Estimated, not a live burn-rate reading/);
  assert.match(bootstrapJs, /averages balance-derived cycle consumption across the observation window/);
  assert.match(bootstrapJs, /top-ups from masking consumption/);
  assert.match(bootstrapJs, /estimated minted cycles and added back/);
  assert.match(bootstrapJs, /does not track historical ICP\/XDR rates for this estimate/);
  assert.match(bootstrapJs, /Historian's latest cached rate/);
  assert.match(bootstrapJs, /rates during the window were reasonably close/);
  assert.match(bootstrapJs, /each observed CMC deposit successfully minted cycles/);
  assert.match(bootstrapJs, /falls back to observed downward balance changes/);
  assert.match(trackerControllerJs, /Distinct ICP-ledger transfers to the canister's CMC deposit account/);
  assert.match(metricsCss, /\.tracker-burn-estimate-value \{[\s\S]*flex-direction: column;[\s\S]*align-items: flex-start;/);
  assert.match(metricsCss, /\.tracker-burn-observation \{[\s\S]*font-size: 11px;[\s\S]*opacity: 0\.68;/);
});

test('simulator prepopulates commitment from calculated break-even minimum', () => {
  assert.match(mainJs, /maybePrepopulateMinimumCommitment/);
  assert.match(mainJs, /calculateSimulatorMinimumCommitmentInput/);
  assert.match(mainJs, /formatIcpCommitmentInputRoundedUp/);
  assert.match(mainJs, /state\.icpCommitmentUserEdited/);
  assert.match(mainJs, /maybePrepopulateMinimumCommitment\(\);\n    render\(\);/);
});
