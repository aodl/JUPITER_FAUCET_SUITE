import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';

const __dirname = dirname(fileURLToPath(import.meta.url));
const indexHtml = readFileSync(resolve(__dirname, '../../assets/index.html'), 'utf8');
const metricsCss = readFileSync(resolve(__dirname, '../../assets/metrics.css'), 'utf8');
const mainJs = readFileSync(resolve(__dirname, '../src/main.js'), 'utf8');
const navbarJs = readFileSync(resolve(__dirname, '../../assets/navbar.js'), 'utf8');
const navbarCss = readFileSync(resolve(__dirname, '../../assets/navbar.css'), 'utf8');

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

test('top navbar exposes Simulator and no longer exposes Partners', () => {
  assert.match(indexHtml, /<a href="#simulator" class="nav-item" data-panel="simulator">Simulator<\/a>/);
  assert.doesNotMatch(indexHtml, /data-panel="partners"/i);
  assert.doesNotMatch(indexHtml, />Partners<\/a>/i);
});

test('orbit scene includes hoverable infographic callouts', () => {
  const orbitCss = readFileSync(resolve(__dirname, '../../assets/background-orbit/background-orbit.css'), 'utf8');
  const orbitJs = readFileSync(resolve(__dirname, '../../assets/background-orbit/background-orbit.js'), 'utf8');

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
  assert.match(orbitJs, /Disbursals are orchestrated by immutable \(unmodifiable\) smart contracts, aka 'blackholed'\.\\nCOMING SOON \.\.\./);
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
  assert.match(mainJs, /function calculateResponsiveTablePageSize\(viewportHeight = window\.innerHeight\)/);
  assert.match(mainJs, /Math\.min\(TABLE_MAX_PAGE_SIZE, Math\.max\(TABLE_MIN_PAGE_SIZE, estimatedRows\)\)/);
  assert.match(mainJs, /const pageSize = currentTablePageSize\(\);[\s\S]*state\.items\.slice\(start, start \+ pageSize\)/);
  assert.match(mainJs, /registeredPageSize: currentTablePageSize\(\)/);
  assert.match(mainJs, /window\.addEventListener\('resize'/);
  assert.doesNotMatch(mainJs, /const TABLE_PAGE_SIZE = 6;/);
});

test('About pane uses a single consolidated page', () => {
  const about = sectionMarkup('about');
  assert.match(about, /<strong>Jupiter Faucet<\/strong> is a perpetual cycles top-up protocol/);
  assert.match(about, /href="https:\/\/internetcomputer\.org\/"[^>]*>Internet Computer<\/a>/);
  assert.match(about, /href="https:\/\/learn\.internetcomputer\.org\/hc\/en-us\/articles\/34573913497108-Cycles"[^>]*>Internet Computer cycles guide<\/a>/);
  assert.match(about, /one-off operation/);
  assert.match(about, /data-panel="how-it-works"[^>]*>How It Works<\/a>/);
  assert.match(about, /The core components will be blackholed/);
  assert.match(about, /network dependency prevents disbursals\s*or other core functionality/);
  assert.match(about, /blackhole themself again once service resumes/);
  assert.match(about, /data-panel="source"[^>]*>open source<\/a>/);
  assert.match(about, /data-panel="governance"[^>]*>decentralize control<\/a>/);
  assert.doesNotMatch(about, /fixed\s*corner links at the bottom of the page/);
  assert.doesNotMatch(about, /Status:/);
  assert.doesNotMatch(about, /planned launch within/);
  assert.doesNotMatch(about, /nav-panel-dots/);
  assert.doesNotMatch(about, /data-page="1"/);
});

test('Source and Governance panes expose subnet context', () => {
  const source = sectionMarkup('source');
  const governance = sectionMarkup('governance');
  assert.match(source, /source-pane-subnet-link pane-external-link[^>]*>Subnet pzp6e<\/a>/);
  assert.match(source, /network\/subnets\/pzp6e-ekpqk-3c5x7-2h6so-njoeq-mt45d-h3h6c-q3mxf-vpeq5-fk5o7-yae/);
  assert.match(navbarCss, /\.source-pane-canister \{[\s\S]*position: relative;[\s\S]*\}/);
  assert.match(navbarCss, /\.source-pane-subnet-link \{[\s\S]*position: absolute;[\s\S]*right: 16px;[\s\S]*\}/);
  assert.match(governance, /All Jupiter Faucet suite canisters reside on either the/);
  assert.match(governance, /network\/subnets\/pzp6e-ekpqk-3c5x7-2h6so-njoeq-mt45d-h3h6c-q3mxf-vpeq5-fk5o7-yae[^>]*>Fiduciary<\/a>/);
  assert.match(governance, /network\/subnets\/x33ed-h457x-bsgyx-oqxqf-6pzwv-wkhzr-rm2j3-npodi-purzm-n66cg-gae[^>]*>SNS subnet<\/a>/);
  assert.match(governance, /both composed of over 30 nodes/);
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
  assert.match(indexHtml, /\.how-it-works-guide-card\.is-send-step \{[\s\S]*grid-row: 1 \/ span 2;[\s\S]*\}/);
  assert.match(howItWorks, /set the transaction memo to your declared canister ID/);
  assert.doesNotMatch(howItWorks, /Transfer ICP to the long-form ICRC-1 staking account address displayed above/);
  assert.doesNotMatch(howItWorks, /While stake commitments can be made today/);
  assert.match(howItWorks, /data-panel="metric-tracker"[^>]*>canister tracker<\/a>/);
  assert.match(howItWorks, /data-panel="simulator"[^>]*>simulator<\/a>/);
  assert.match(howItWorks, /newly minted <strong>IO<\/strong> \(a liquid staking protocol that will be launched alongside Jupiter Faucet\)/);
  assert.match(howItWorks, /<strong>0%–19%<\/strong> distributed to <strong>SNS JUP stakers<\/strong>/);
  assert.match(howItWorks, /<strong>0%–1%<\/strong> restaked into/);
  assert.match(howItWorks, /pre-requisite for truly unstoppable canisters is a secure and decentralized network/);
  assert.match(howItWorks, /data-panel="metric-commitments"[^>]*>committed ICP<\/a>/);
  assert.match(howItWorks, /dashboard\.internetcomputer\.org\/account\/22594ba982e201a96a8e3e51105ac412221a30f231ec74bb320322deccb5061d[^>]*>staking account<\/a>/);
  assert.match(howItWorks, /dashboard\.internetcomputer\.org\/neuron\/11614578985374291210[^>]*>neuron<\/a>/);
  assert.match(howItWorks, /data-page-target="0"[^>]*>rules described<\/a>/);
  assert.match(howItWorks, /Contributions must meet the requirements in order to be counted/);
  assert.match(howItWorks, /transactions of at least 1 ICP featuring a memo that declares a valid canister principal/);
  assert.match(howItWorks, /dashboard\.internetcomputer\.org\/account\/4d6afc06456fc7d5e5d6c9096a12ca60182a9fdb4ee50c4ff2feb2112c86222f[^>]*>rewards account<\/a>/);
  assert.match(howItWorks, /data-panel="governance"[^>]*>Governance<\/a>/);
  assert.match(navbarJs, /data-page-target/);
});

test('How it works pane no longer contains the simulator slide', () => {
  const howItWorks = sectionMarkup('how-it-works');
  assert.doesNotMatch(howItWorks, /commitment-simulator-form/);
  assert.doesNotMatch(howItWorks, /Commitment simulator/);
  assert.match(howItWorks, /data-page="0"/);
  assert.match(howItWorks, /data-page="1"/);
  assert.doesNotMatch(howItWorks, /data-page="2"/);
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
  assert.equal(attrValue(burn, 'step'), '0.001');
  assert.equal(attrValue(burn, 'value'), '0.001');
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
  assert.match(mainJs, /'simulator-daily-burn': \{ min: 0, fractionDigits: 3 \}/);
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
  assert.match(mainJs, /dashboard\.internetcomputer\.org\/neuron\/\$\{JUPITER_NEURON_ID\.toString\(\)\}/);
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

  assert.match(simulator, /Age bonus diverted/);
  assert.match(simulator, /id="simulator-age-bonus"/);
  assert.match(simulator, /Effective top-up APY/);
  assert.match(simulator, /id="simulator-effective-apy"/);
  assert.match(simulator, /ICP\/XDR rate source/);
  assert.match(simulator, /id="simulator-icp-xdr-source"/);
  assert.match(stake, /Age bonus diverted/);
  assert.match(stake, /id="stake-neuron-age-bonus"/);
  assert.match(mainJs, /calculateAgeBonusBasisPointsFromAgingSince/);
  assert.match(mainJs, /simulatorState\.ageBonusBasisPoints/);
});

test('Total Output and Total Rewards are pages of Jupiter Stake rather than metric rail buttons', () => {
  const stake = sectionMarkup('metric-stake');
  const rail = indexHtml.slice(indexHtml.indexOf('<aside class="metric-rail"'), indexHtml.indexOf('</aside>') + '</aside>'.length);

  assert.match(rail, /id="landing-next-run"[\s\S]*Jupiter Stake/);
  assert.match(mainJs, /setText\('landing-next-run', subtitle\);/);
  assert.match(rail, /Jupiter Stake[\s\S]*Patron Commitments[\s\S]*Declared Canisters[\s\S]*Track Canisters/);
  assert.doesNotMatch(rail, /Target Canisters/);
  assert.doesNotMatch(rail, />Commitments<\/span>/);
  assert.doesNotMatch(rail, /data-panel="metric-output"/);
  assert.doesNotMatch(rail, /data-panel="metric-rewards"/);
  assert.doesNotMatch(indexHtml, /id="nav-panel-metric-output"/);
  assert.doesNotMatch(indexHtml, /id="nav-panel-metric-rewards"/);
  assert.match(indexHtml, /id="nav-panel-metric-commitments"[\s\S]*Patron Commitments/);
  assert.match(indexHtml, /id="nav-panel-metric-registered"[\s\S]*Declared Canisters/);
  assert.match(stake, /data-page="1"[\s\S]*Total Output/);
  assert.match(stake, /data-page="2"[\s\S]*Total Rewards/);
  assert.match(stake, /data-page="3"[\s\S]*D-QUORUM Route/);
  assert.match(stake, /aria-label="D-QUORUM route"/);
  assert.match(navbarJs, /key === "metric-output"[\s\S]*key: "metric-stake", page: 1/);
  assert.match(navbarJs, /key === "metric-rewards"[\s\S]*key: "metric-stake", page: 2/);
});

test('Patron Commitments table omits redundant category column', () => {
  const commitments = sectionMarkup('metric-commitments');
  assert.match(commitments, /<th>Timestamp<\/th>[\s\S]*<th>Amount<\/th>[\s\S]*<th>Declared canister<\/th>/);
  assert.doesNotMatch(commitments, /<th>Category<\/th>/);
  assert.match(commitments, /<td colspan="3" class="empty-cell">Loading…<\/td>/);
  assert.doesNotMatch(mainJs, /formatCommitmentOutcome/);
  assert.doesNotMatch(mainJs, /commitmentOutcomeCategory/);
  assert.match(mainJs, /renderCommitmentsPane\(data\)[\s\S]*<td>\$\{formatCommitmentTarget\(item\)\}<\/td>[\s\S]*paneEmptyMessage\(data, 'recent', 'No commitments indexed yet\.'\),\s*3,/);
});

test('Tracker results render chart controls and graphs before explanatory text', () => {
  const rangeControlsIndex = mainJs.indexOf('${renderTrackerRangeControls()}');
  const chartWrapperIndex = mainJs.indexOf('<div class="tracker-chart-wrapper" id="tracker-chart-wrapper"></div>');
  const summaryGridIndex = mainJs.indexOf('<dl class="pane-detail-grid tracker-summary-grid">');
  const showingNoteIndex = mainJs.indexOf('Showing ${escapeHtml(rangeLabel)} using');

  assert.ok(rangeControlsIndex >= 0, 'missing tracker range controls render');
  assert.ok(chartWrapperIndex > rangeControlsIndex, 'chart wrapper should render after range controls');
  assert.ok(summaryGridIndex > chartWrapperIndex, 'summary text should render below charts');
  assert.ok(showingNoteIndex > summaryGridIndex, 'explanatory text should render below summary');
  assert.match(metricsCss, /\.tracker-chart-wrapper \{[\s\S]*margin: 16px 0 20px;/);
});



test('paged nav panel content keeps a stable panel height while preserving overflow scrolling', () => {
  assert.match(navbarCss, /\.nav-panel \{[\s\S]*height: min\(720px, calc\(100dvh - 112px\)\);[\s\S]*overflow: hidden;[\s\S]*\}/);
  assert.match(navbarCss, /\.nav-panel-page\.is-active \{[\s\S]*display: flex;[\s\S]*flex: 1;[\s\S]*overflow: auto;[\s\S]*\}/);
  assert.match(navbarCss, /\.nav-panel-page\.is-active > \.nav-panel-scroll-region \{[\s\S]*padding-right: 0;[\s\S]*\}/);
  assert.doesNotMatch(navbarCss, /\.nav-panel-page\.is-active \{[\s\S]*max-height: 40vh;[\s\S]*\}/);
  assert.match(navbarCss, /\.nav-panel-scroll-region \{[\s\S]*overflow: auto;[\s\S]*\}/);
  assert.doesNotMatch(navbarCss, /\.nav-panel-scroll-region \{[\s\S]*max-height: min\(46vh, calc\(100dvh - 220px\)\);[\s\S]*\}/);
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

test('simulator displays T-cycle values with three decimal places and uses weekly headline copy', () => {
  assert.match(mainJs, /const thousandths = \(absolute \* 1000n\) \/ 1_000_000_000_000n;/);
  assert.match(mainJs, /padStart\(3, '0'\)/);
  assert.match(mainJs, /formatCompactTrillionCycles/);
  assert.match(mainJs, /Per weekly CMC top-up, based on the configured APY/);
  assert.match(mainJs, /Line samples the weekly cadence/);
});

test('metrics nav button closes an open pane before showing the metrics rail', () => {
  assert.match(navbarJs, /if \(backdrop\.classList\.contains\("is-open"\)\) \{/);
  assert.match(navbarJs, /metricsMenuOpen = true;[\s\S]*?closePanel\(\);/);
});

test('pane fragment navigation participates in browser history', () => {
  assert.match(navbarJs, /history\.pushState\(null, "", `#\$\{key\}`\);/);
  assert.match(navbarJs, /window\.addEventListener\("popstate", \(\) => applyHash\(window\.location\.hash\)\);/);
  assert.match(navbarJs, /if \(!key\) \{[\s\S]*closePanel\(\{ syncHash: false, restoreFocus: false \}\);/);
  assert.match(navbarJs, /function closePanel\(\{ syncHash = true, restoreFocus = true \} = \{\}\)/);
  assert.match(navbarJs, /if \(syncHash\) \{[\s\S]*clearPanelHash\(\);[\s\S]*\}/);
  assert.doesNotMatch(navbarJs, /history\.replaceState\(null, "", `#\$\{key\}`\);/);
});

test('canister tracker defaults to all loaded history', () => {
  assert.match(mainJs, /const trackerState = \{[\s\S]*?range: 'all'/);
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
  assert.match(mainJs, /applySimulatorIcpXdrRateFromStatus/);
  assert.match(mainJs, /readOpt\(status\?\.icp_xdr_rate\)/);
  assert.match(mainJs, /formatIcpXdrRateInput/);
  assert.match(mainJs, /simulatorState\.icpPriceUserEdited/);
  assert.match(mainJs, /historian’s daily XRC cache/);
  assert.match(mainJs, /No cached XRC ICP\/XDR rate is available yet/);
  assert.match(mainJs, /formatIcpXdrRateSource/);
  assert.match(mainJs, /Historian XRC cache:/);
  assert.match(mainJs, /Fetched \$\{formatTimestampSeconds/);
  assert.match(mainJs, /formatIcpXdrRateSource\(snapshot, manualOverride = false\)/);
  assert.match(mainJs, /Manual override; \$\{cacheText\}/);
  assert.match(mainJs, /formatIcpXdrRateSource\(\s*simulatorState\.icpXdrRateSnapshot,\s*simulatorState\.icpPriceUserEdited,\s*\)/);
});


test('canister tracker links use shareable metric-tracker fragments', () => {
  assert.match(mainJs, /const TRACKER_HASH_PREFIX = '#metric-tracker-'/);
  assert.match(mainJs, /trackerHashForPrincipal/);
  assert.match(mainJs, /trackerPrincipalFromHash/);
  assert.match(navbarJs, /key\.startsWith\("metric-tracker-"\)/);
  assert.match(indexHtml, /href="#metric-tracker-uccpi-cqaaa-aaaar-qby3q-cai"/);
});

test('metric tracker hash deep links submit once on cold load and panel open', () => {
  assert.match(mainJs, /let lastTrackerHashSubmitPrincipal = ''/);
  assert.match(mainJs, /hydrateTrackerFromLocationHash\(\{ submit: true \}\);/);
  assert.match(mainJs, /submit && lastTrackerHashSubmitPrincipal !== principalText/);
  assert.match(mainJs, /lastTrackerHashSubmitPrincipal = principalText/);
  assert.match(mainJs, /replaceTrackerLocationHash\(principal\.toText\(\)\);/);
  assert.match(mainJs, /event\?\.detail\?\.key === 'metric-tracker'[\s\S]*hydrateTrackerFromLocationHash\(\{ submit: true \}\)/);
});

test('canister tracker displays cycles as T cycles and estimates burn per day', () => {
  assert.match(mainJs, /function formatCycles\(value\) \{\n  return formatTrillionCycles\(value\);/);
  assert.match(mainJs, /function trackerCyclesChartPoints\(data\)/);
  assert.match(mainJs, /return sortedCycleSamples\(data\)\.map/);
  assert.match(mainJs, /function trackerCyclesPointLabel\(point\)/);
  assert.match(mainJs, /formatTimestampNanos\(point\.timestampNanos\)/);
  assert.match(mainJs, /pointLabelBuilder: trackerCyclesPointLabel/);
  assert.match(mainJs, /xDomainBuckets: timelineBuckets/);
  assert.match(mainJs, /xTickBuckets: timelineBuckets/);
  assert.doesNotMatch(mainJs, /Line shows each loaded cycles probe/);
  assert.match(mainJs, /cyclesProbeIssueNote/);
  assert.match(mainJs, /cyclesStatus\.kind === 'error'/);
  assert.match(mainJs, /cyclesStatus\.kind === 'notAvailable'/);
  assert.match(mainJs, /Estimated observed cycles burned\/day/);
  assert.match(mainJs, /downward balance changes between all loaded cycle probes/);
  assert.match(mainJs, /const estimatedObservedCyclesBurnedPerDay = estimateCyclesBurnedPerDay\(data\);/);
  assert.match(mainJs, /estimateCyclesBurnedPerDay/);
  assert.match(mainJs, /formatTrillionCyclesPerDay/);
});

test('simulator prepopulates commitment from calculated break-even minimum', () => {
  assert.match(mainJs, /maybePrepopulateSimulatorMinimumCommitment/);
  assert.match(mainJs, /calculateSimulatorMinimumCommitmentInput/);
  assert.match(mainJs, /formatIcpCommitmentInputRoundedUp/);
  assert.match(mainJs, /simulatorState\.icpCommitmentUserEdited/);
  assert.match(mainJs, /maybePrepopulateSimulatorMinimumCommitment\(\);\n  renderCommitmentSimulator\(\);/);
});
