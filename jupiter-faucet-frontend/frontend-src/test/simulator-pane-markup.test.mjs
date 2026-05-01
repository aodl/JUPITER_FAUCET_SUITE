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

test('partners pane content has been removed', () => {
  assert.doesNotMatch(indexHtml, /id="nav-panel-partners"/);
  assert.doesNotMatch(indexHtml, /Life On Ledger/);
  assert.doesNotMatch(indexHtml, /WaterNeuron/);
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
  assert.match(simulator, /target canister/);
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
  assert.equal(attrValue(price, 'value'), '10.0');
  assert.equal(attrValue(apy, 'step'), '0.1');
  assert.equal(attrValue(apy, 'value'), '7.0');
  assert.match(simulator, /Daily burn \(T cycles\)/);
  assert.match(simulator, /APY \(%\)/);
  assert.doesNotMatch(simulator, /id="simulator-icp-commitment"[^>]*value="100\.0"/);
});

test('simulator no longer exposes a starting-buffer stat or copy', () => {
  const simulator = sectionMarkup('simulator');
  assert.doesNotMatch(simulator, /starting buffer/i);
  assert.doesNotMatch(simulator, /simulator-required-buffer/);
  assert.doesNotMatch(mainJs, /requiredStartingBufferCycles/);
  assert.doesNotMatch(mainJs, /one-year starting buffer/i);
});

test('simulator renders the cycles balance chart before the top-ups chart and clarifies APY copy', () => {
  const balanceIndex = mainJs.indexOf('<h3>Projected cycles balance</h3>');
  const topupsIndex = mainJs.indexOf('<h3>Projected CMC top-ups</h3>');
  assert.ok(balanceIndex >= 0, 'missing balance chart header');
  assert.ok(topupsIndex >= 0, 'missing top-ups chart header');
  assert.ok(balanceIndex < topupsIndex, 'balance chart should render before top-ups chart');
  assert.match(mainJs, /Projection uses the configured APY\. Exact APY depends on numerous factors/);
  assert.match(mainJs, /dashboard\.internetcomputer\.org\/neuron\/\$\{JUPITER_NEURON_ID\.toString\(\)\}/);
  assert.match(mainJs, /effective top-up APY discounts the current age-bonus component/);
  assert.match(mainJs, /first projected payout happens on day one/);
  assert.match(mainJs, /weekly-cadence one-year projection/);
  assert.match(mainJs, /Projected weekly CMC top-up cycles over one year/);
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

  assert.doesNotMatch(rail, /data-panel="metric-output"/);
  assert.doesNotMatch(rail, /data-panel="metric-rewards"/);
  assert.doesNotMatch(indexHtml, /id="nav-panel-metric-output"/);
  assert.doesNotMatch(indexHtml, /id="nav-panel-metric-rewards"/);
  assert.match(stake, /data-page="1"[\s\S]*Total Output/);
  assert.match(stake, /data-page="2"[\s\S]*Total Rewards/);
  assert.match(stake, /data-page="3"[\s\S]*D-QUORUM Route/);
  assert.match(stake, /aria-label="D-QUORUM route"/);
  assert.match(navbarJs, /key === "metric-output"[\s\S]*key: "metric-stake", page: 1/);
  assert.match(navbarJs, /key === "metric-rewards"[\s\S]*key: "metric-stake", page: 2/);
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
});

test('simulator displays T-cycle values with three decimal places and uses weekly chart copy', () => {
  assert.match(mainJs, /const thousandths = \(absolute \* 1000n\) \/ 1_000_000_000_000n;/);
  assert.match(mainJs, /padStart\(3, '0'\)/);
  assert.match(mainJs, /Weekly projection of cycles minted from the configured APY/);
  assert.match(mainJs, /Line samples the weekly cadence/);
});

test('metrics nav button closes an open pane before showing the metrics rail', () => {
  assert.match(navbarJs, /if \(backdrop\.classList\.contains\("is-open"\)\) \{/);
  assert.match(navbarJs, /metricsMenuOpen = true;[\s\S]*?closePanel\(\);/);
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
  assert.match(mainJs, /event\?\.detail\?\.key === 'metric-tracker'[\s\S]*hydrateTrackerFromLocationHash\(\{ submit: true \}\)/);
});

test('canister tracker displays cycles as T cycles and estimates burn per day', () => {
  assert.match(mainJs, /function formatCycles\(value\) \{\n  return formatTrillionCycles\(value\);/);
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
