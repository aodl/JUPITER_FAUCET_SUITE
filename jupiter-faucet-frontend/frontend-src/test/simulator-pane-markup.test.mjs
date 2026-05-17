import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';

const __dirname = dirname(fileURLToPath(import.meta.url));
const indexHtml = readFileSync(resolve(__dirname, '../../assets/index.html'), 'utf8');
const metricsCss = readFileSync(resolve(__dirname, '../../assets/metrics.css'), 'utf8');
const bootstrapJs = readFileSync(resolve(__dirname, '../src/app/bootstrap.js'), 'utf8');
const advancedMemoControllerJs = readFileSync(resolve(__dirname, '../src/app/advanced-memo-controller.js'), 'utf8');
const configJs = readFileSync(resolve(__dirname, '../src/app/config.js'), 'utf8');
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
  assert.doesNotMatch(orbitJs, /statusSlot: "orbit-disbursement-status"/);
  assert.doesNotMatch(orbitJs, /__JUPITER_ORBIT_DISBURSEMENT_TEXT__/);
  assert.match(indexHtml, /class="orbit-disbursement-status" id="orbit-disbursement-status" hidden aria-live="polite"/);
  assert.match(orbitCss, /\.metric-rail:not\(\.metric-rail--visible\) ~ \.orbit-disbursement-status \{[\s\S]*display: none;[\s\S]*\}/);
  assert.match(orbitCss, /\.orbit-disbursement-status \{[\s\S]*top: 22dvw;[\s\S]*left: 6\.5dvw;[\s\S]*width: min\(23dvw, 16rem\);[\s\S]*color: rgba\(255, 255, 255, 0\.56\);[\s\S]*font-size: 11px;[\s\S]*line-height: 1\.35;[\s\S]*font-weight: 400;[\s\S]*\}/);
  assert.match(orbitCss, /\.orbit-disbursement-status::before \{[\s\S]*width: min\(8dvw, 7rem\);[\s\S]*\}/);
  assert.match(orbitCss, /\.orbit-disbursement-status::before \{[\s\S]*float: right;[\s\S]*shape-outside: polygon\(100% 0, 100% 100%, 0 100%\);[\s\S]*\}/);
  assert.doesNotMatch(orbitCss, /\.orbit-disbursement-status \{[^}]*background:/);
  assert.doesNotMatch(orbitCss, /\.orbit-disbursement-status \{[^}]*border:/);
  assert.doesNotMatch(orbitCss, /\.orbit-disbursement-status \{[^}]*padding:/);
  assert.doesNotMatch(orbitCss, /\.orbit-disbursement-status \{[^}]*text-shadow:/);
  assert.match(orbitCss, /\.orbit-disbursement-status \.orbit-infographic-copy-link \{[\s\S]*color: inherit;[\s\S]*\}/);
  assert.match(orbitCss, /\.orbit-disbursement-status \.orbit-infographic-copy-link:hover,[\s\S]*\.orbit-disbursement-status \.orbit-infographic-copy-link:focus \{[\s\S]*color: #fff;[\s\S]*\}/);
  assert.match(orbitJs, /Disbursals are orchestrated by immutable \(unmodifiable\) smart contracts, aka 'blackholed'\."/);
  assert.doesNotMatch(orbitJs, /COMING SOON/);
  assert.match(orbitJs, /ctaLabel: "MORE INFO"/);
  assert.match(orbitJs, /ctaPanel: "governance"/);
  assert.match(orbitJs, /link\.dataset\.panel = item\.ctaPanel/);
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
  assert.match(about, /The core components will be blackholed/);
  assert.match(about, /network dependency prevents disbursals\s*or other core functionality/);
  assert.match(about, /blackhole themself again once service resumes/);
  assert.match(about, /data-panel="source"[^>]*>open source<\/a>/);
  assert.match(about, /data-panel="governance"[^>]*>decentralize control<\/a>/);
  assert.doesNotMatch(about, /fixed\s*corner links at the bottom of the page/);
  assert.doesNotMatch(about, /Status:/);
  assert.doesNotMatch(about, /planned launch within/);
  assert.match(about, /class="nav-panel-page is-active" data-page="0"/);
  assert.match(about, /class="nav-panel-page" data-page="1"[\s\S]*<h2 class="about-projects-title">Projects Powered by Jupiter Faucet<\/h2>[\s\S]*More coming soon!/);
  assert.match(about, /data-page-target="0"[^>]*>social channels<\/a>[\s\S]*class="about-project-grid"/);
  assert.match(about, /class="about-project-preview" href="https:\/\/jupiter-faucet\.com\/"/);
  assert.match(about, /src="\/social-preview\.jpg"[\s\S]*<strong>Jupiter Faucet<\/strong>/);
  assert.match(about, /Yes, Jupiter Faucet powers itself!/);
  assert.match(about, /<p class="about-project-canisters-title">Canisters<\/p>/);
  assert.match(about, /data-tracker-principal="uccpi-cqaaa-aaaar-qby3q-cai"[\s\S]*>Disburser<\/a>/);
  assert.match(about, /data-tracker-principal="acjuz-liaaa-aaaar-qb4qq-cai"[\s\S]*>Faucet<\/a>/);
  assert.match(about, /data-tracker-principal="j5gs6-uiaaa-aaaar-qb5cq-cai"[\s\S]*>Historian<\/a>/);
  assert.match(about, /data-tracker-principal="afisn-gqaaa-aaaar-qb4qa-cai"[\s\S]*>Lifeline<\/a>/);
  assert.match(about, /data-tracker-principal="alk7f-5aaaa-aaaar-qb4ra-cai"[\s\S]*>SNS Rewards<\/a>/);
  assert.match(about, /data-tracker-principal="jufzc-caaaa-aaaar-qb5da-cai"[\s\S]*>Frontend<\/a>/);
  assert.match(about, /class="about-project-card about-project-card--soon"[\s\S]*More coming soon!/);
  assert.match(about, /href="https:\/\/github\.com\/aodl\/JUPITER_FAUCET_SUITE\/pulls"[^>]*>raise a pull request<\/a>/);
  assert.match(about, /<div class="nav-panel-dots" role="tablist" aria-label="About pages">/);
  assert.match(about, /data-page="1" aria-label="Projects Powered by Jupiter Faucet"/);
  assert.match(metricsCss, /\.about-projects-slide \{[\s\S]*overflow: visible;[\s\S]*\}/);
  assert.doesNotMatch(metricsCss, /\.about-projects-slide \{[\s\S]*min-height: 100%;[\s\S]*\}/);
  assert.match(metricsCss, /\.about-project-grid \{[\s\S]*grid-template-columns: repeat\(2, minmax\(0, calc\(\(100% - 12px\) \/ 2\)\)\);[\s\S]*\}/);
  assert.match(metricsCss, /\.about-project-card \{[\s\S]*container-type: inline-size;[\s\S]*\}/);
  assert.doesNotMatch(metricsCss, /\.about-project-card \{[\s\S]*aspect-ratio: 1 \/ 1;[\s\S]*\}/);
  assert.match(metricsCss, /\.about-project-preview img \{[\s\S]*height: clamp\(128px, 38cqw, 160px\);[\s\S]*object-fit: cover;[\s\S]*\}/);
});

test('Source and Governance panes expose subnet context', () => {
  const source = sectionMarkup('source');
  const governance = sectionMarkup('governance');
  assert.match(source, /source-pane-subnet-link pane-external-link[^>]*>Subnet pzp6e<\/a>/);
  assert.match(source, /network\/subnets\/pzp6e-ekpqk-3c5x7-2h6so-njoeq-mt45d-h3h6c-q3mxf-vpeq5-fk5o7-yae/);
  assert.match(source, /data-source-total-memory="uccpi-cqaaa-aaaar-qby3q-cai"/);
  assert.match(source, /data-source-heap-memory="uccpi-cqaaa-aaaar-qby3q-cai"/);
  assert.match(source, /data-source-stable-memory="uccpi-cqaaa-aaaar-qby3q-cai"/);
  assert.match(navbarCss, /\.source-pane-canister \{[\s\S]*position: relative;[\s\S]*\}/);
  assert.match(navbarCss, /\.source-pane-subnet-link \{[\s\S]*position: absolute;[\s\S]*right: 16px;[\s\S]*\}/);
  assert.match(governance, /All Jupiter Faucet suite canisters reside on either the/);
  assert.match(governance, /network\/subnets\/pzp6e-ekpqk-3c5x7-2h6so-njoeq-mt45d-h3h6c-q3mxf-vpeq5-fk5o7-yae[^>]*>Fiduciary<\/a>/);
  assert.match(governance, /network\/subnets\/x33ed-h457x-bsgyx-oqxqf-6pzwv-wkhzr-rm2j3-npodi-purzm-n66cg-gae[^>]*>SNS subnet<\/a>/);
  assert.match(governance, /both composed of over 30 nodes/);
  assert.match(governance, /moving toward SNS DAO control/);
  assert.match(governance, /data-panel="source"[^>]*>open source<\/a>/);
  assert.match(governance, /data-panel="source"[^>]*>reproducible builds<\/a>/);
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
  assert.match(howItWorks, /memo-builder-safety-notice[\s\S]*<strong>Rewards:<\/strong>[\s\S]*JUP SNS tokens will be minted/);
  assert.match(howItWorks, /JUP SNS tokens will be minted[\s\S]*While the Jupiter Faucet SNS rewards components are still being finalised/);
  assert.match(howItWorks, /data-panel="metric-commitments"[^>]*>committed ICP<\/a>/);
  assert.match(howItWorks, /dashboard\.internetcomputer\.org\/account\/22594ba982e201a96a8e3e51105ac412221a30f231ec74bb320322deccb5061d[^>]*>staking account<\/a>/);
  assert.match(howItWorks, /dashboard\.internetcomputer\.org\/neuron\/11614578985374291210[^>]*>neuron<\/a>/);
  assert.match(howItWorks, /data-page-target="0"[^>]*>rules described<\/a>/);
  assert.match(howItWorks, /Contributions must meet the requirements in order to be counted/);
  assert.match(howItWorks, /transactions of at least 1 ICP featuring a memo that declares a canister ID or neuron ID/);
  assert.match(howItWorks, /dashboard\.internetcomputer\.org\/account\/4d6afc06456fc7d5e5d6c9096a12ca60182a9fdb4ee50c4ff2feb2112c86222f[^>]*>rewards account<\/a>/);
  assert.match(howItWorks, /data-panel="governance"[^>]*>Governance<\/a>/);
  assert.match(navbarJs, /data-page-target/);
});

test('How it works pane includes advanced usage memo builder without restoring simulator controls', () => {
  const howItWorks = sectionMarkup('how-it-works');
  assert.doesNotMatch(howItWorks, /commitment-simulator-form/);
  assert.doesNotMatch(howItWorks, /Commitment simulator/);
  assert.match(howItWorks, /data-page="0"/);
  assert.match(howItWorks, /data-page="1"/);
  assert.match(howItWorks, /data-page="2"/);
  assert.match(howItWorks, /data-page="3"/);
  assert.match(howItWorks, /Advanced Usage/);
  assert.match(howItWorks, /three memo-directed flows/);
  assert.match(howItWorks, /plain declared canister ID/);
  assert.match(howItWorks, /memo builder on the next slide/);
  assert.match(howItWorks, /<strong>Developer tip:<\/strong> You can adjust the <code>canister<\/code>\/<code>neuron<\/code>\s*and <code>label<\/code> parameters/);
  assert.match(howItWorks, /customise the memo helper\s*form on the next slide for a smoother user experience/);
  assert.doesNotMatch(howItWorks, /next slide, for a smoother/);
  assert.match(howItWorks, /<a class="memo-builder-tip-url pane-external-link mono" href="\/#how-it-works:3\?canister=%7Bprotocol%20canister%20ID%7D&amp;label=%7Bcustom%20label%7D">/);
  assert.match(howItWorks, /memo-builder-tip-url pane-external-link mono[\s\S]*\/#how-it-works:3\?canister=/);
  assert.match(howItWorks, /memo-builder-placeholder[^>]*>\{protocol canister ID\}<\/span>&amp;label=/);
  assert.match(howItWorks, /memo-builder-placeholder[^>]*>\{custom label\}<\/span>/);
  assert.doesNotMatch(howItWorks, /custom identifier label/);
  assert.match(metricsCss, /\.memo-builder-tip-url \{[\s\S]*overflow-wrap: anywhere;[\s\S]*\}/);
  assert.match(metricsCss, /\.memo-builder-placeholder \{[\s\S]*color: rgba\(255, 226, 168, 0\.78\);[\s\S]*\}/);
  assert.doesNotMatch(howItWorks, /memo helper utility coming soon/);
  assert.doesNotMatch(howItWorks, /This enables more specialized designs/);
  assert.doesNotMatch(howItWorks, /ONICAI/);
  assert.match(howItWorks, /32 characters/);
  assert.match(howItWorks, /Memo Builder/);
  assert.match(howItWorks, /id="memo-builder-title"[^>]*>Memo Builder<\/h3>/);
  assert.match(howItWorks, /id="memo-builder-prefill-note"[^>]*hidden/);
  assert.match(howItWorks, /This memo helper simplifies constructing a memo from your chosen ID/);
  assert.match(howItWorks, /protocol\s+canister that facilitates a specialised top-up flow/);
  assert.match(howItWorks, /id="memo-builder-safety-notice"[^>]*hidden/);
  assert.match(howItWorks, /<strong>Health &amp; Safety Notice:<\/strong>/);
  assert.match(howItWorks, /identified\s+the controller of the <span id="memo-builder-safety-target-kind">protocol canister<\/span>/);
  assert.match(howItWorks, /id="memo-builder-canister-dashboard-link"[^>]*href="#"/);
  assert.match(howItWorks, /DAO or reputable pre-DAO dev team who prescribe this\s*<span id="memo-builder-safety-prescription-kind">canister<\/span>/);
  assert.match(howItWorks, /Jupiter Faucet\s+is not responsible for lost funds resulting from user indiscretion/);
  assert.match(howItWorks, /id="memo-builder-url-context"/);
  assert.match(howItWorks, /id="memo-builder-mode-fieldset"/);
  assert.match(howItWorks, /value="rawIcp" checked/);
  assert.doesNotMatch(howItWorks, /value="cycles" checked/);
  assert.match(howItWorks, /Cycles top-up canister/);
  assert.match(howItWorks, /value="rawIcp"/);
  assert.match(howItWorks, /Canister default account/);
  assert.match(howItWorks, /Public neuron staking account/);
  assert.match(howItWorks, /Canister default account[\s\S]*Public neuron staking account[\s\S]*Cycles top-up canister/);
  assert.doesNotMatch(howItWorks, />Neuron staking account<\/span>/);
  assert.match(howItWorks, /id="memo-builder-canister"/);
  assert.match(howItWorks, /id="memo-builder-canister"[^>]*pattern="\[A-Za-z2-7-\]\*"/);
  assert.match(howItWorks, /id="memo-builder-neuron-fields" hidden/);
  assert.match(howItWorks, /Declared public neuron ID/);
  assert.doesNotMatch(howItWorks, />Declared neuron ID<\/label>/);
  assert.match(howItWorks, /id="memo-builder-neuron"/);
  assert.match(howItWorks, /id="memo-builder-neuron"[^>]*pattern="\[0-9\]\*"/);
  assert.match(howItWorks, /id="memo-builder-optional-fields" hidden/);
  assert.match(howItWorks, /id="memo-builder-optional-label"[^>]*>Optional outgoing transfer memo<\/label>/);
  assert.match(howItWorks, /id="memo-builder-optional"[^>]*contenteditable="plaintext-only"/);
  assert.match(howItWorks, /id="memo-builder-optional"[^>]*role="textbox"/);
  assert.match(howItWorks, /id="memo-builder-optional"[^>]*aria-labelledby="memo-builder-optional-label"/);
  assert.doesNotMatch(howItWorks, /memo-builder-input-wrap/);
  assert.doesNotMatch(howItWorks, /memo-builder-input-overlay/);
  assert.doesNotMatch(howItWorks, /id="memo-builder-preview"/);
  assert.doesNotMatch(howItWorks, /Used text:/);
  assert.doesNotMatch(howItWorks, /Truncated text:/);
  assert.doesNotMatch(howItWorks, /id="memo-builder-status"/);
  assert.doesNotMatch(howItWorks, /id="memo-builder-availability"/);
  assert.doesNotMatch(howItWorks, /ASCII bytes used/);
  assert.doesNotMatch(howItWorks, /bytes available for optional memo text/);
  assert.doesNotMatch(howItWorks, /id="memo-builder-remove-hyphens"/);
  assert.doesNotMatch(howItWorks, /id="memo-builder-remove-optional-hyphens"/);
  assert.doesNotMatch(howItWorks, />Remove hyphens<\/button>/);
  assert.match(howItWorks, /id="memo-builder-output"[^>]*readonly/);
  assert.match(howItWorks, /id="memo-builder-copy"[^>]*>Copy memo<\/button>/);
  assert.match(howItWorks, /Use the generated memo as described in the/);
  assert.match(howItWorks, /href="#how-it-works"[^>]*data-page-target="0"[^>]*>basic instructions<\/a>/);
  assert.match(howItWorks, /\(in place of the "declared canister ID"\) to make your ICP commitment and initiate\s+perpetual top-ups/);
  assert.match(howItWorks, /Use the generated memo[\s\S]*For more information about this memo builder see/);
  assert.match(howItWorks, /For more information about this memo builder see[\s\S]*href="#how-it-works:2"[^>]*data-page-target="2"[^>]*>Advanced Usage<\/a>/);
  assert.doesNotMatch(howItWorks, /Use the copied memo/);
  assert.doesNotMatch(howItWorks, /target="_blank"[^>]*>Advanced Usage<\/a>/);
  assert.doesNotMatch(howItWorks, /target="_blank"[^>]*>basic instructions<\/a>/);
  assert.match(navbarJs, /route\.match\(\/\^\(\[\^:\]\+\):\(\\d\+\)\$\/\)/);
  assert.match(navbarJs, /panelHashFor\(key, clamped\)/);
  assert.match(mainJs, /params\.get\('canister'\)/);
  assert.match(mainJs, /params\.get\('neuron'\)/);
  assert.match(mainJs, /params\.get\('mode'\)/);
  assert.match(mainJs, /params\.get\('label'\)/);
  assert.match(mainJs, /\(params\.get\('label'\) \|\| ''\)\.slice\(0, 30\)/);
  assert.doesNotMatch(mainJs, /params\.get\('Label'\)/);
  assert.doesNotMatch(mainJs, /params\.get\('optionalLabel'\)/);
  assert.doesNotMatch(mainJs, /params\.get\('memoLabel'\)/);
  assert.doesNotMatch(mainJs, /params\.get\('memo_label'\)/);
  assert.match(mainJs, /builderTitle\.textContent = label \? `\$\{label\} Memo Builder` : defaultBuilderTitle/);
  assert.match(mainJs, /const hasPrefillTarget = canister !== null \|\| neuron !== null/);
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
  assert.match(mainJs, /safetyNotice\.hidden = !urlDisplayTarget/);
  assert.match(mainJs, /safetyTargetKind\.textContent = urlTargetType === 'neuron' \? 'protocol neuron' : 'protocol canister'/);
  assert.match(mainJs, /safetyPrescriptionKind\.textContent = urlTargetType === 'neuron' \? 'neuron' : 'canister'/);
  assert.match(mainJs, /urlContext\.textContent = urlDisplayTarget/);
  assert.match(mainJs, /The \$\{urlTargetType\} ID and term '\$\{label\}' were supplied in the URL/);
  assert.match(mainJs, /The \$\{urlTargetType\} ID was supplied in the URL/);
  assert.match(mainJs, /canisterDashboardLink\.textContent = urlDisplayTarget/);
  assert.match(mainJs, /dashboard\.internetcomputer\.org\/\$\{urlTargetType\}\/\$\{encodeURIComponent\(urlDisplayTarget\)\}/);
  assert.doesNotMatch(mainJs, /requestedMode !== 'cycles' && hasCustomLabel/);
  assert.match(mainJs, /urlTargetMode = urlPrefill\.mode/);
  assert.match(mainJs, /urlLocksTarget = true/);
  assert.match(mainJs, /canisterInput && shouldApplyTargetValue/);
  assert.match(mainJs, /neuronInput && shouldApplyTargetValue/);
  assert.match(mainJs, /hasPrefillTarget && shouldApplyTargetValue/);
  assert.match(mainJs, /lastAppliedPrefillFragment = currentFragment/);
  assert.match(mainJs, /if \(sanitizedCanister\) \{[\s\S]*\} else if \(sanitizedNeuron\) \{/);
  assert.match(mainJs, /\|\| 'rawIcp'/);
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

  assert.match(simulator, /Age bonus diverted/);
  assert.match(simulator, /id="simulator-age-bonus"/);
  assert.match(simulator, /Effective top-up APY/);
  assert.match(simulator, /id="simulator-effective-apy"/);
  assert.match(simulator, /ICP\/XDR rate source/);
  assert.match(simulator, /id="simulator-icp-xdr-source"/);
  assert.match(stake, /Age bonus diverted/);
  assert.match(stake, /id="stake-neuron-age-bonus"/);
  assert.match(stake, /Maturity disbursal/);
  assert.match(stake, /id="stake-neuron-disbursement"/);
  assert.match(mainJs, /formatMaturityDisbursementStatus/);
  assert.match(mainJs, /formatMaturityDisbursementLandingText/);
  assert.match(mainJs, /updateLandingDisbursementStatus/);
  assert.match(mainJs, /void ensureNeuronDetailsLoaded\(data\);/);
  assert.match(mainJs, /link\.href = '#metric-stake'/);
  assert.match(mainJs, /link\.textContent = 'More info'/);
  assert.match(nnsGovernanceDidJs, /maturity_disbursements_in_progress/);
  assert.match(mainJs, /calculateAgeBonusBasisPointsFromAgingSince/);
  assert.match(mainJs, /state\.ageBonusBasisPoints/);
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
  assert.match(commitments, /<h3 class="pane-section-title">Declared Canisters<\/h3>[\s\S]*<th>Timestamp<\/th>[\s\S]*<th>Amount<\/th>[\s\S]*<th>Declared<\/th>/);
  assert.match(commitments, /<h3 class="pane-section-title">Declared Raw ICP Canisters<\/h3>[\s\S]*<th>Memo<\/th>/);
  assert.match(commitments, /<h3 class="pane-section-title">Declared Neurons<\/h3>[\s\S]*<th>Declared<\/th>[\s\S]*<th>Memo<\/th>/);
  assert.match(commitments, /aria-label="Patron Commitment pages"[\s\S]*aria-label="Declared Neurons"/);
  assert.match(commitments, /href="\/#how-it-works"[^>]*data-panel="how-it-works"[^>]*>How It Works<\/a> for qualifying commitment rules/);
  assert.doesNotMatch(commitments, /private neurons cannot be refreshed by the faucet top-up process/);
  assert.doesNotMatch(commitments, /<th>Category<\/th>/);
  assert.match(commitments, /<td colspan="3" class="empty-cell">Loading…<\/td>/);
  assert.match(commitments, /<td colspan="4" class="empty-cell">Loading…<\/td>/);
  assert.doesNotMatch(mainJs, /formatCommitmentOutcome/);
  assert.doesNotMatch(mainJs, /commitmentOutcomeCategory/);
  assert.match(mainJs, /const renderCommitmentsPane = \(data\) => \{[\s\S]*rawMemo === undefined \|\| rawMemo === null[\s\S]*commitments-raw[\s\S]*rawIcpMemoText\(item\)[\s\S]*commitments-neurons[\s\S]*neuronMemoText\(item\)/);
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
  assert.match(navbarJs, /if \(backdrop\.classList\.contains\("is-open"\)\) \{/);
  assert.match(navbarJs, /metricsMenuOpen = true;[\s\S]*?closePanel\(\);/);
});

test('pane fragment navigation participates in browser history', () => {
  assert.match(navbarJs, /history\.pushState\(null, "", panelHashFor\(key, 0\)\);/);
  assert.match(navbarJs, /function panelHashFor\(key, pageIndex = 0\)/);
  assert.match(navbarJs, /return pageIndex > 0 \? `#\$\{key\}:\$\{pageIndex\}` : `#\$\{key\}`;/);
  assert.match(navbarJs, /window\.addEventListener\("popstate", \(\) => applyHash\(window\.location\.hash\)\);/);
  assert.match(navbarJs, /if \(!key\) \{[\s\S]*closePanel\(\{ syncHash: false, restoreFocus: false \}\);/);
  assert.match(navbarJs, /function closePanel\(\{ syncHash = true, restoreFocus = true \} = \{\}\)/);
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
  assert.match(navbarJs, /backdrop\.addEventListener\("focusin", \(evt\) => \{[\s\S]*activatePage\(sectionEl, page\);[\s\S]*\}\);/);
  assert.doesNotMatch(navbarJs, /backdrop\.addEventListener\("focusin", \(evt\) => \{[\s\S]*activatePage\(sectionEl, page, \{ syncHash: true \}\);[\s\S]*\}\);/);
});

test('canister tracker defaults to all loaded history', () => {
  assert.match(mainJs, /const state = \{[\s\S]*?range: 'all'/);
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
  assert.match(mainJs, /trackerPrincipalFromHash/);
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
  assert.match(mainJs, /let lastHashSubmitPrincipal = ''/);
  assert.match(mainJs, /trackerController\.hydrateFromLocationHash\(\{ submit: true \}\);/);
  assert.match(mainJs, /submit && lastHashSubmitPrincipal !== principalText/);
  assert.match(mainJs, /lastHashSubmitPrincipal = principalText/);
  assert.match(mainJs, /replaceLocationHash\(principal\.toText\(\)\);/);
  assert.match(mainJs, /event\?\.detail\?\.key === 'metric-tracker'[\s\S]*trackerController\.hydrateFromLocationHash\(\{ submit: true \}\)/);
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
  assert.match(mainJs, /oldest and newest loaded/);
  assert.match(mainJs, /renderCyclesProbeInfoNote/);
  assert.match(mainJs, /using canister log cycles/);
  assert.match(mainJs, /const estimatedObservedCyclesBurnedPerDay = estimateCyclesBurnedPerDay\(data\);/);
  assert.match(trackerCyclesJs, /estimateCyclesBurnedPerDay/);
  assert.match(mainJs, /formatTrillionCyclesPerDay/);
  assert.match(mainJs, /renderTrackerLogs\(data\)/);
  assert.match(mainJs, /data-simulator-prefill="true"/);
  assert.match(metricsCss, /\.tracker-log-details\s*\{/);
});

test('simulator prepopulates commitment from calculated break-even minimum', () => {
  assert.match(mainJs, /maybePrepopulateMinimumCommitment/);
  assert.match(mainJs, /calculateSimulatorMinimumCommitmentInput/);
  assert.match(mainJs, /formatIcpCommitmentInputRoundedUp/);
  assert.match(mainJs, /state\.icpCommitmentUserEdited/);
  assert.match(mainJs, /maybePrepopulateMinimumCommitment\(\);\n    render\(\);/);
});
