import { readOpt } from '../candid-opt.js';
import { renderEmptyChart, renderLineChart } from '../chart-rendering.js';
import { buildSimulatorProjection } from '../projection-simulator.js';
import { SIMULATOR_DEFAULTS } from './config.js';
import { SIMULATOR_HASH_PREFIX, simulatorHashForPrefill, simulatorPrefillFromHash } from './hash-routes.js';
import {
  DASH,
  formatAgeBonusDisplay,
  formatBasisPointsAsPercent,
  formatCompactTrillionCycles,
  formatCycles,
  formatIcpE8s,
  formatIcpXdrRateDisplay,
  formatIcpXdrRateInput,
  formatIcpXdrRateSource,
  formatTimestampSeconds,
  formatTrillionCycles,
} from './view-formatters.js';

const ICP_TENTH_E8S = 10_000_000n;
const SIMULATOR_INPUT_CONSTRAINTS = {
  'simulator-icp-commitment': { min: 1, fractionDigits: 1 },
  'simulator-daily-burn': { min: 0, fractionDigits: 4 },
  'simulator-icp-price': { min: 0.1, fractionDigits: 1 },
  'simulator-apy': { min: 0, fractionDigits: 1 },
};

export function formatIcpCommitmentInputRoundedUp(e8s) {
  if (e8s === null || e8s === undefined) return null;
  const asBigInt = typeof e8s === 'bigint' ? e8s : BigInt(e8s);
  const roundedTenths = (asBigInt + ICP_TENTH_E8S - 1n) / ICP_TENTH_E8S;
  const clampedTenths = roundedTenths < 10n ? 10n : roundedTenths;
  return `${clampedTenths / 10n}.${(clampedTenths % 10n).toString()}`;
}

export function formatDailyBurnInputFromCyclesPerDay(value) {
  if (value === null || value === undefined) return null;
  const cycles = typeof value === 'bigint' ? value : BigInt(value);
  const tenThousandths = (cycles * 10_000n + 500_000_000_000n) / 1_000_000_000_000n;
  return `${tenThousandths / 10_000n}.${(tenThousandths % 10_000n).toString().padStart(4, '0')}`;
}

export function clampSimulatorInputValue(value, constraint) {
  if (!constraint) return value;
  const text = String(value ?? '').replace(/,/g, '').replace(/[^\d.]/g, '');
  const firstDot = text.indexOf('.');
  const whole = (firstDot === -1 ? text : text.slice(0, firstDot)).replace(/^0+(?=\d)/, '');
  const rawFraction = firstDot === -1 ? '' : text.slice(firstDot + 1).replace(/\./g, '');
  const fraction = rawFraction.slice(0, constraint.fractionDigits);
  const normalised = firstDot === -1 ? whole : `${whole}.${fraction}`;
  if (!normalised || normalised === '.') return '';

  const numeric = Number(normalised);
  if (Number.isFinite(numeric) && numeric < constraint.min) {
    return constraint.min.toFixed(constraint.fractionDigits).replace(/\.?0+$/, '');
  }
  return normalised.startsWith('.') ? `0${normalised}` : normalised;
}

export function createSimulatorController({ copyTextToClipboard, neuronId }) {
  const state = {
    initialised: false,
    ageBonusBasisPoints: 0n,
    ageBonusAvailable: false,
    icpPriceUserEdited: false,
    icpCommitmentUserEdited: false,
    icpXdrRateSnapshot: null,
  };

  const simulatorInputConstraint = (input) => (input?.id ? SIMULATOR_INPUT_CONSTRAINTS[input.id] : null);

  const sanitiseSimulatorInput = (input) => {
    const constraint = simulatorInputConstraint(input);
    if (!constraint) return false;
    const cleanValue = clampSimulatorInputValue(input.value, constraint);
    if (input.value === cleanValue) return false;
    input.value = cleanValue;
    return true;
  };

  const simulatorInputValue = (id) => document.getElementById(id)?.value ?? '';

  const simulatorProjectionInputValue = (id, fallback) => {
    const value = simulatorInputValue(id);
    const normalised = value.endsWith('.') ? value.slice(0, -1) : value;
    return normalised || fallback;
  };

  const setSimulatorText = (id, text, title = '') => {
    const node = document.getElementById(id);
    if (!node) return;
    node.textContent = text;
    if (title) node.title = title;
    else node.removeAttribute?.('title');
  };

  const setSimulatorStatus = (message, kind = '') => {
    const status = document.getElementById('simulator-status');
    if (!status) return;
    status.textContent = message;
    status.hidden = !message;
    status.className = kind ? `pane-status-note tracker-status-note tracker-status-note--${kind}` : 'pane-status-note tracker-status-note';
  };

  const clearSimulatorSummary = () => {
    [
      'simulator-cycles-per-icp',
      'simulator-icp-xdr-source',
      'simulator-annual-topup-icp',
      'simulator-annual-topup-cycles',
      'simulator-annual-burn-cycles',
      'simulator-age-bonus',
      'simulator-effective-apy',
      'simulator-year-end-balance',
      'simulator-required-commitment',
    ].forEach((id) => setSimulatorText(id, DASH));
  };

  const readSimulatorInputs = () => ({
    icpCommitment: simulatorProjectionInputValue('simulator-icp-commitment', '1.0'),
    dailyBurnTrillionCycles: simulatorProjectionInputValue('simulator-daily-burn', SIMULATOR_DEFAULTS.dailyBurnTrillionCycles),
    assumedIcpPrice: simulatorProjectionInputValue('simulator-icp-price', SIMULATOR_DEFAULTS.assumedIcpPrice),
    annualApyPercent: simulatorProjectionInputValue('simulator-apy', SIMULATOR_DEFAULTS.annualApyPercent),
    ageBonusBasisPoints: state.ageBonusBasisPoints,
  });

  const calculateSimulatorMinimumCommitmentInput = () => {
    const projection = buildSimulatorProjection({
      icpCommitment: '1.0',
      dailyBurnTrillionCycles: simulatorInputValue('simulator-daily-burn') || SIMULATOR_DEFAULTS.dailyBurnTrillionCycles,
      assumedIcpPrice: simulatorInputValue('simulator-icp-price') || SIMULATOR_DEFAULTS.assumedIcpPrice,
      annualApyPercent: simulatorInputValue('simulator-apy') || SIMULATOR_DEFAULTS.annualApyPercent,
      ageBonusBasisPoints: state.ageBonusBasisPoints,
    });
    if (!projection.ok || projection.summary.requiredCommitmentE8s === null) return null;
    return formatIcpCommitmentInputRoundedUp(projection.summary.requiredCommitmentE8s);
  };

  const maybePrepopulateMinimumCommitment = () => {
    if (state.icpCommitmentUserEdited) return;
    const input = document.getElementById('simulator-icp-commitment');
    if (!input) return;
    const minimumCommitment = calculateSimulatorMinimumCommitmentInput();
    if (minimumCommitment && input.value !== minimumCommitment) {
      input.value = minimumCommitment;
    }
  };

  const simulatorShareHashFromInputs = () => simulatorHashForPrefill({
    dailyBurn: simulatorProjectionInputValue('simulator-daily-burn', SIMULATOR_DEFAULTS.dailyBurnTrillionCycles),
    icpCommitment: simulatorProjectionInputValue('simulator-icp-commitment', '1.0'),
    assumedIcpPrice: simulatorProjectionInputValue('simulator-icp-price', SIMULATOR_DEFAULTS.assumedIcpPrice),
    annualApyPercent: simulatorProjectionInputValue('simulator-apy', SIMULATOR_DEFAULTS.annualApyPercent),
  });

  const simulatorShareUrlFromInputs = () => {
    const hash = simulatorShareHashFromInputs();
    const url = new URL(window.location.href);
    url.hash = hash;
    return url.toString();
  };

  const bindSimulatorShareUrlButton = () => {
    const button = document.getElementById('simulator-copy-url');
    if (!button || button.dataset.bound === 'true') return;
    button.dataset.bound = 'true';
    const defaultText = button.textContent || 'Copy to URL';
    button.addEventListener('click', async () => {
      const hash = simulatorShareHashFromInputs();
      const url = simulatorShareUrlFromInputs();
      history.replaceState(null, '', hash);
      try {
        await copyTextToClipboard(url);
        button.textContent = 'Copied to URL';
        window.setTimeout(() => {
          button.textContent = defaultText;
        }, 1200);
      } catch {
        button.textContent = 'Copy failed';
        window.setTimeout(() => {
          button.textContent = defaultText;
        }, 1500);
      }
    });
  };

  const renderSimulatorCharts = (projection) => {
    const wrapper = document.getElementById('simulator-chart-wrapper');
    if (!wrapper) return;
    const buckets = projection?.buckets || [];
    const weeklyTopupCycles = projection?.summary?.weeklyTopupCycles ?? 0n;
    wrapper.innerHTML = `
      <div class="tracker-chart-card">
        <div class="tracker-chart-header">
          <h3>Projected cycles balance</h3>
          <span>Line samples the weekly cadence, assuming the first projected payout happens on day one, then shows cumulative projected top-ups minus the configured burn.</span>
        </div>
        ${renderLineChart({
          buckets,
          valueKey: 'projectedBalanceCycles',
          emptyMessage: 'No balance projection is available for these inputs.',
          ariaLabel: 'Projected weekly cycles balance over one year',
          valueFormatter: formatTrillionCycles,
          pointLabelBuilder: (bucket) => `${bucket.label}: ${formatTrillionCycles(bucket.projectedBalanceCycles)} projected balance after ${formatTrillionCycles(bucket.projectedBurnCycles)} burned in bucket`,
          showAllTicks: false,
        })}
      </div>
      <div class="tracker-chart-card simulator-topup-headline-card">
        <div class="tracker-chart-header tracker-chart-header--headline">
          <h3>Projected weekly top-ups: ${formatCompactTrillionCycles(weeklyTopupCycles)}</h3>
          <span>Per weekly CMC top-up, based on the configured APY.</span>
        </div>
      </div>`;
  };

  const render = () => {
    const projection = buildSimulatorProjection(readSimulatorInputs());
    const assumption = document.getElementById('simulator-assumption-note');
    if (assumption) {
      const dashboardHref = `https://dashboard.internetcomputer.org/neuron/${neuronId.toString()}`;
      const ageBonusCopy = state.ageBonusAvailable
        ? `The effective top-up APY discounts the current age-bonus component (${formatAgeBonusDisplay(state.ageBonusBasisPoints)}) because Jupiter Faucet routes that component to SNS holders instead of CMC top-ups.`
        : 'Neuron age-bonus details are still loading; the projection temporarily assumes no age-bonus diversion.';
      const rateSnapshot = state.icpXdrRateSnapshot;
      const rateCopy = rateSnapshot
        ? `The ICP/XDR input is prefilled from historian’s daily XRC cache at ${formatIcpXdrRateDisplay(rateSnapshot)}${rateSnapshot.fetched_at_ts ? `, fetched ${formatTimestampSeconds(rateSnapshot.fetched_at_ts)}` : ''}.`
        : 'No cached XRC ICP/XDR rate is available yet; edit the ICP/XDR input manually until historian completes its next refresh.';
      assumption.innerHTML = `Projection uses the configured APY. Exact APY depends on numerous factors — consult the <a class="pane-external-link" href="${dashboardHref}" target="_blank" rel="noopener noreferrer">dashboard</a> for the current annualised rewards estimate. ${ageBonusCopy} ${rateCopy} It assumes 1T cycles per ICP/XDR price unit and a weekly-cadence one-year projection.`;
    }

    if (!projection.ok) {
      clearSimulatorSummary();
      setSimulatorStatus(projection.errors.join(' '), 'error');
      const wrapper = document.getElementById('simulator-chart-wrapper');
      if (wrapper) wrapper.innerHTML = renderEmptyChart('Enter valid simulator inputs to render the projection.');
      return;
    }

    const { summary } = projection;
    setSimulatorText('simulator-cycles-per-icp', formatTrillionCycles(summary.cyclesPerIcp));
    setSimulatorText(
      'simulator-icp-xdr-source',
      formatIcpXdrRateSource(
        state.icpXdrRateSnapshot,
        state.icpPriceUserEdited,
      ),
      state.icpXdrRateSnapshot?.fetched_at_ts ? `Fetched ${formatTimestampSeconds(state.icpXdrRateSnapshot.fetched_at_ts)}` : '',
    );
    setSimulatorText('simulator-annual-topup-icp', formatIcpE8s(summary.annualTopupE8s));
    setSimulatorText('simulator-annual-topup-cycles', formatTrillionCycles(summary.annualTopupCycles));
    setSimulatorText('simulator-annual-burn-cycles', formatTrillionCycles(summary.annualBurnCycles));
    setSimulatorText('simulator-age-bonus', state.ageBonusAvailable ? formatAgeBonusDisplay(summary.ageBonusBasisPoints) : 'Loading; assuming 0.0%');
    setSimulatorText('simulator-effective-apy', formatBasisPointsAsPercent(summary.effectiveApyBasisPoints));
    setSimulatorText('simulator-year-end-balance', formatTrillionCycles(summary.yearEndBalanceCycles));
    setSimulatorText('simulator-required-commitment', summary.requiredCommitmentE8s === null ? DASH : formatIcpE8s(summary.requiredCommitmentE8s));
    if (summary.annualTopupCycles >= summary.annualBurnCycles) {
      const surplus = summary.annualTopupCycles - summary.annualBurnCycles;
      setSimulatorStatus(`At these assumptions the commitment covers the configured annual burn, with a projected annual surplus of ${formatTrillionCycles(surplus)}.`, '');
    } else {
      const shortfall = summary.annualBurnCycles - summary.annualTopupCycles;
      const required = summary.requiredCommitmentE8s === null ? 'a larger commitment' : formatIcpE8s(summary.requiredCommitmentE8s);
      setSimulatorStatus(`At these assumptions the canister is underfunded by ${formatTrillionCycles(shortfall)} per year. Increase the commitment to at least ${required} for an indefinite weekly projection.`, 'error');
    }
    renderSimulatorCharts(projection);
  };

  const bind = () => {
    const form = document.getElementById('commitment-simulator-form');
    if (!form || state.initialised) return;
    state.initialised = true;
    bindSimulatorShareUrlButton();
    [
      ['simulator-daily-burn', SIMULATOR_DEFAULTS.dailyBurnTrillionCycles],
      ['simulator-apy', SIMULATOR_DEFAULTS.annualApyPercent],
    ].forEach(([id, value]) => {
      const input = document.getElementById(id);
      if (input && !input.value) input.value = value;
    });
    maybePrepopulateMinimumCommitment();
    form.addEventListener('submit', (event) => event.preventDefault());
    form.addEventListener('input', (event) => {
      if (event.target instanceof HTMLInputElement) {
        sanitiseSimulatorInput(event.target);
      }
      if (event.target?.id === 'simulator-icp-price') {
        state.icpPriceUserEdited = true;
      }
      if (event.target?.id === 'simulator-icp-commitment') {
        state.icpCommitmentUserEdited = true;
      }
      render();
    });
    form.addEventListener('change', render);
    render();
  };

  const openPanel = () => {
    const simulatorSection = document.querySelector('.nav-panel-section[data-panel="simulator"]');
    const simulatorAlreadyOpen = document.body.classList.contains('nav-panel-open')
      && simulatorSection?.classList.contains('nav-panel-section--active');
    if (simulatorAlreadyOpen) return;

    const trigger = document.querySelector('a[data-panel="simulator"]');
    if (trigger) {
      const currentHash = window.location.hash;
      trigger.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, view: window }));
      if (currentHash.startsWith(SIMULATOR_HASH_PREFIX)) {
        history.replaceState(null, '', currentHash);
      }
      return;
    }
    window.location.hash = '#simulator';
  };

  const applyPrefill = ({
    dailyBurn,
    icpCommitment,
    assumedIcpPrice,
    annualApyPercent,
  }) => {
    openPanel();
    window.setTimeout(() => {
      const burnInput = document.getElementById('simulator-daily-burn');
      if (burnInput && dailyBurn) burnInput.value = clampSimulatorInputValue(dailyBurn, SIMULATOR_INPUT_CONSTRAINTS['simulator-daily-burn']);
      const commitmentInput = document.getElementById('simulator-icp-commitment');
      if (commitmentInput && icpCommitment) {
        commitmentInput.value = clampSimulatorInputValue(icpCommitment, SIMULATOR_INPUT_CONSTRAINTS['simulator-icp-commitment']);
        state.icpCommitmentUserEdited = true;
      }
      const priceInput = document.getElementById('simulator-icp-price');
      if (priceInput && assumedIcpPrice) {
        priceInput.value = clampSimulatorInputValue(assumedIcpPrice, SIMULATOR_INPUT_CONSTRAINTS['simulator-icp-price']);
        state.icpPriceUserEdited = true;
      }
      const apyInput = document.getElementById('simulator-apy');
      if (apyInput && annualApyPercent) apyInput.value = clampSimulatorInputValue(annualApyPercent, SIMULATOR_INPUT_CONSTRAINTS['simulator-apy']);
      render();
    }, 0);
  };

  const hydrateFromLocationHash = () => {
    const prefill = simulatorPrefillFromHash();
    if (!prefill) return false;
    applyPrefill(prefill);
    return true;
  };

  const applyIcpXdrRateFromStatus = (status) => {
    const snapshot = readOpt(status?.icp_xdr_rate);
    state.icpXdrRateSnapshot = snapshot || null;
    const formatted = formatIcpXdrRateInput(snapshot);
    if (formatted && !state.icpPriceUserEdited) {
      const input = document.getElementById('simulator-icp-price');
      if (input && input.value !== formatted) {
        input.value = formatted;
      }
    }
    maybePrepopulateMinimumCommitment();
    render();
  };

  const setAgeBonus = (ageBonusBasisPoints, available = true) => {
    state.ageBonusBasisPoints = ageBonusBasisPoints ?? 0n;
    state.ageBonusAvailable = available;
    maybePrepopulateMinimumCommitment();
    render();
  };

  return {
    state,
    applyIcpXdrRateFromStatus,
    bind,
    hydrateFromLocationHash,
    render,
    setAgeBonus,
  };
}
