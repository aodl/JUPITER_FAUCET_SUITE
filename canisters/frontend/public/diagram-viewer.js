const diagrams = Object.freeze({
  overview: {
    file: 'jupiter-faucet-overview.svg',
    title: 'Jupiter Faucet suite overview',
    alt: 'Diagram showing the Jupiter Faucet suite architecture, funding flows, observability, rewards, and recovery controls',
  },
  topups: {
    file: 'perpetual-canister-topups.svg',
    title: 'Perpetual canister top-ups',
    alt: 'Flow diagram showing ICP committed to permanent neuron stake becoming recurring maturity and cycles for a target canister',
  },
  disburser: {
    file: 'disburser.svg',
    title: 'Disburser maturity routing',
    alt: 'Flow diagram showing how the Jupiter Disburser routes base maturity and age-bonus maturity',
  },
  faucet: {
    file: 'faucet.svg',
    title: 'Faucet payout targets',
    alt: 'Flow diagram showing the Jupiter Faucet memo-directed payout targets',
  },
  relay: {
    file: 'relay.svg',
    title: 'Relay allocation loop',
    alt: 'Flow diagram showing Relay funding, cycles allocation, managed canister top-ups, and surplus routing',
  },
});

const params = new URLSearchParams(window.location.search);
const diagram = diagrams[params.get('diagram') ?? ''];
const title = document.querySelector('#diagram-viewer-title');
const image = document.querySelector('#diagram-viewer-image');
const error = document.querySelector('#diagram-viewer-error');

if (!diagram) {
  title.textContent = 'Diagram unavailable';
  error.hidden = false;
} else {
  const requestedVersion = params.get('v') ?? '';
  const assetVersion = /^[a-zA-Z0-9._-]+$/.test(requestedVersion) ? requestedVersion : '';
  const versionSuffix = assetVersion ? `?v=${encodeURIComponent(assetVersion)}` : '';

  document.title = `${diagram.title} · Jupiter Faucet`;
  title.textContent = diagram.title;
  image.src = `/${diagram.file}${versionSuffix}`;
  image.alt = diagram.alt;
  image.hidden = false;
}
