import {
  accountIdentifierHex,
  bytesToHex,
  uint8ArrayFromOptBytes,
} from '../dashboard-data.js';
import { setLink, setPaneValueText, setPaneValueTrustedHtml, setText } from '../dom-helpers.js';
import { formatFolloweeLinks } from '../followee-links.js';
import { formatMaturityDisbursementLandingText, formatMaturityDisbursementStatus } from '../maturity-disbursement.js';
import { calculateAgeBonusBasisPointsFromAgingSince } from '../projection-simulator.js';
import { JUPITER_STAKING_ACCOUNT } from './config.js';
import {
  formatAgeBonusDisplay,
  formatAgeFromSeconds,
  formatIcpE8s,
  formatTimestampSeconds,
} from './view-formatters.js';

function isJupiterStakingAccount(account) {
  if (!account) return true;
  const owner = String(account.owner?.toText ? account.owner.toText() : account.owner);
  const subaccountHex = bytesToHex(uint8ArrayFromOptBytes(account.subaccount));
  return owner === JUPITER_STAKING_ACCOUNT.owner && subaccountHex === JUPITER_STAKING_ACCOUNT.subaccountHex;
}

function stakingAccountDisplayAddress(account) {
  if (isJupiterStakingAccount(account)) return JUPITER_STAKING_ACCOUNT.address;
  return accountIdentifierHex(account);
}

function stakingAccountExplorerAddress(account) {
  if (isJupiterStakingAccount(account)) return JUPITER_STAKING_ACCOUNT.explorerAccountHex;
  return accountIdentifierHex(account);
}

function updateLandingDisbursementStatus(text) {
  const node = document.getElementById('orbit-disbursement-status');
  if (!node) return;
  node.replaceChildren();
  if (!text) {
    node.hidden = true;
    return;
  }
  node.hidden = false;
  node.appendChild(document.createTextNode(`${text} `));
  const link = document.createElement('a');
  link.href = '#metric-stake';
  link.className = 'orbit-infographic-copy-link pane-external-link';
  link.textContent = 'More info';
  node.appendChild(link);
}

function setStatusNote(id, value) {
  const node = document.getElementById(id);
  if (!node) return;
  node.textContent = value || '';
  node.hidden = !value;
}

export function createStakePaneController({
  neuronId,
  simulatorController,
  setCopyButton,
  isNeuronLoaded = () => false,
}) {
  const renderHowItWorksAccount = () => {
    setCopyButton('copy-how-staking-account', () => JUPITER_STAKING_ACCOUNT.address);
    setCopyButton('copy-how-staking-account-identifier', () => JUPITER_STAKING_ACCOUNT.explorerAccountHex);
    setText('how-staking-account-address', JUPITER_STAKING_ACCOUNT.address);
    setText('how-staking-account-identifier', JUPITER_STAKING_ACCOUNT.explorerAccountHex);
    const stakingAccountLink = document.getElementById('how-staking-account-link');
    if (stakingAccountLink) {
      stakingAccountLink.href = `https://dashboard.internetcomputer.org/account/${JUPITER_STAKING_ACCOUNT.explorerAccountHex}`;
      stakingAccountLink.title = JUPITER_STAKING_ACCOUNT.address;
    }
    const stakingAccountIdentifierLink = document.getElementById('how-staking-account-identifier-link');
    if (stakingAccountIdentifierLink) {
      stakingAccountIdentifierLink.href = `https://dashboard.internetcomputer.org/account/${JUPITER_STAKING_ACCOUNT.explorerAccountHex}`;
      stakingAccountIdentifierLink.title = JUPITER_STAKING_ACCOUNT.explorerAccountHex;
    }
  };

  const renderStakePane = (data, neuron, { neuronLoading = false, neuronError = null, dataLoading = false } = {}) => {
    const stakingAccount = data?.status?.staking_account;
    const stakingAddress = stakingAccountDisplayAddress(stakingAccount);
    const stakingExplorerAddress = stakingAccountExplorerAddress(stakingAccount);
    setLink('stake-pane-account-link', {
      href: stakingExplorerAddress ? `https://dashboard.internetcomputer.org/account/${stakingExplorerAddress}` : '',
      text: stakingAddress,
    });
    setLink('stake-neuron-id-link', {
      href: `https://dashboard.internetcomputer.org/neuron/${neuronId.toString()}`,
      text: neuronId.toString(),
    });
    setPaneValueText('stake-pane-balance', dataLoading
      ? { loading: true }
      : data?.stakeE8s === null
        ? { error: data?.errors?.stake || 'Stake unavailable' }
        : { value: formatIcpE8s(data?.stakeE8s) });

    if (neuron) {
      const ageBonusBasisPoints = calculateAgeBonusBasisPointsFromAgingSince(
        neuron.aging_since_timestamp_seconds,
        Math.floor(Date.now() / 1000),
      ) ?? 0n;
      setPaneValueText('stake-neuron-maturity', { value: formatIcpE8s(neuron.maturity_e8s_equivalent) });
      setPaneValueText('stake-neuron-age', { value: formatAgeFromSeconds(neuron.aging_since_timestamp_seconds) });
      setPaneValueText('stake-neuron-age-bonus', { value: formatAgeBonusDisplay(ageBonusBasisPoints) });
      setPaneValueText('stake-neuron-public', { value: 'Yes' });
      setPaneValueText('stake-neuron-created', { value: formatTimestampSeconds(neuron.created_timestamp_seconds) });
      setPaneValueText('stake-neuron-refresh', { value: formatTimestampSeconds(neuron.voting_power_refreshed_timestamp_seconds?.[0]) });
      setPaneValueText('stake-neuron-disbursement', {
        value: formatMaturityDisbursementStatus(neuron, {
          formatIcpE8s,
          formatTimestampSeconds,
        }),
      });
      updateLandingDisbursementStatus(formatMaturityDisbursementLandingText(neuron, {
        formatTimestampSeconds,
      }));
      setPaneValueTrustedHtml('stake-neuron-followees', { value: formatFolloweeLinks(neuron) });
      simulatorController.setAgeBonus(ageBonusBasisPoints, true);
      return;
    }

    const fallback = neuronError
      ? { loading: false, error: neuronError }
      : neuronLoading || !isNeuronLoaded()
        ? { loading: true }
        : { error: 'Public neuron details unavailable' };
    setPaneValueText('stake-neuron-maturity', fallback);
    setPaneValueText('stake-neuron-age', fallback);
    setPaneValueText('stake-neuron-age-bonus', fallback);
    setPaneValueText('stake-neuron-public', fallback);
    setPaneValueText('stake-neuron-created', fallback);
    setPaneValueText('stake-neuron-refresh', fallback);
    setPaneValueText('stake-neuron-disbursement', fallback);
    if (!neuronLoading && !dataLoading) updateLandingDisbursementStatus(null);
    setPaneValueText('stake-neuron-followees', fallback);
  };

  const renderStakeNeuronStatus = ({ loading = false, error = null } = {}) => {
    if (loading || error) {
      setStatusNote('stake-neuron-note', '');
      return;
    }
    setStatusNote('stake-neuron-note', '');
  };

  return {
    renderHowItWorksAccount,
    renderStakeNeuronStatus,
    renderStakePane,
  };
}
