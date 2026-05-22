import {
  advancedMemoUrlPrefillState,
  advancedMemoValidationMessages,
  buildAdvancedMemo,
  sanitizeCanisterPrincipalText,
  sanitizeNeuronIdText,
  shouldApplyAdvancedMemoUrlTargetValue,
} from '../advanced-memo-builder.js';

const PREFILL_CANISTER_NOTE = 'This memo helper simplifies constructing a memo from your chosen ID and a protocol canister that facilitates a specialised top-up flow.';
const PREFILL_NEURON_NOTE = 'This memo helper simplifies constructing a memo from your chosen ID and a public protocol neuron.';

export function initAdvancedMemoBuilder({ copyTextToClipboard } = {}) {
  const builder = document.getElementById('advanced-memo-builder');
  if (!builder || builder.dataset.bound === 'true') return;
  builder.dataset.bound = 'true';

  const modeFieldset = document.getElementById('memo-builder-mode-fieldset');
  const canisterFields = document.getElementById('memo-builder-canister-fields');
  const neuronFields = document.getElementById('memo-builder-neuron-fields');
  const optionalFields = document.getElementById('memo-builder-optional-fields');
  const canisterInput = document.getElementById('memo-builder-canister');
  const neuronInput = document.getElementById('memo-builder-neuron');
  const optionalInput = document.getElementById('memo-builder-optional');
  const optionalLabel = document.getElementById('memo-builder-optional-label');
  const builderTitle = document.getElementById('memo-builder-title');
  const prefillNote = document.getElementById('memo-builder-prefill-note');
  const safetyNotice = document.getElementById('memo-builder-safety-notice');
  const safetyTargetKind = document.getElementById('memo-builder-safety-target-kind');
  const safetyPrescriptionKind = document.getElementById('memo-builder-safety-prescription-kind');
  const canisterDashboardLink = document.getElementById('memo-builder-canister-dashboard-link');
  const urlContext = document.getElementById('memo-builder-url-context');
  const outputInput = document.getElementById('memo-builder-output');
  const copyButton = document.getElementById('memo-builder-copy');
  const messages = document.getElementById('memo-builder-messages');
  const defaultOptionalLabel = optionalLabel?.textContent || 'Optional outgoing transfer memo';
  const defaultBuilderTitle = builderTitle?.textContent || 'Memo Builder';
  let urlTargetMode = '';
  let urlTargetKind = '';
  let urlLocksTarget = false;
  let lastAppliedPrefillFragment = '';

  const fragmentParams = () => {
    const fragment = String(window.location.hash || '').replace(/^#/, '');
    const queryStart = fragment.indexOf('?');
    return queryStart >= 0 ? new URLSearchParams(fragment.slice(queryStart + 1)) : new URLSearchParams();
  };
  const setMode = (mode) => {
    const radio = builder.querySelector(`input[name="memo-builder-mode"][value="${mode}"]`);
    if (radio) radio.checked = true;
  };
  const applyUrlPrefill = () => {
    const currentFragment = String(window.location.hash || '');
    const params = fragmentParams();
    const canister = params.get('canister');
    const neuron = params.get('neuron');
    const label = (params.get('label') || '').slice(0, 30);
    const hasPrefillTarget = canister !== null || neuron !== null;
    const shouldApplyTargetValue = shouldApplyAdvancedMemoUrlTargetValue(currentFragment, lastAppliedPrefillFragment);
    const requestedMode = params.get('mode') || params.get('flow') || '';
    const urlPrefill = advancedMemoUrlPrefillState({ canister, neuron, requestedMode });
    const { sanitizedCanister, sanitizedNeuron, targetType: urlTargetType, displayTarget: urlDisplayTarget } = urlPrefill;
    if (optionalLabel) optionalLabel.textContent = label || (hasPrefillTarget ? 'Identifier' : defaultOptionalLabel);
    if (builderTitle) builderTitle.textContent = label ? `${label} Memo Builder` : defaultBuilderTitle;
    if (prefillNote) {
      prefillNote.hidden = !hasPrefillTarget;
      prefillNote.textContent = urlTargetType === 'neuron' ? PREFILL_NEURON_NOTE : PREFILL_CANISTER_NOTE;
    }
    if (safetyNotice) safetyNotice.hidden = !urlDisplayTarget;
    if (safetyTargetKind) safetyTargetKind.textContent = urlTargetType === 'neuron' ? 'protocol neuron' : 'protocol canister';
    if (safetyPrescriptionKind) safetyPrescriptionKind.textContent = urlTargetType === 'neuron' ? 'neuron' : 'canister';
    if (urlContext) {
      urlContext.textContent = urlDisplayTarget
        ? (label
          ? ` The ${urlTargetType} ID and term '${label}' were supplied in the URL.`
          : ` The ${urlTargetType} ID was supplied in the URL.`)
        : '';
    }
    if (canisterDashboardLink) {
      canisterDashboardLink.textContent = urlDisplayTarget;
      canisterDashboardLink.href = urlDisplayTarget
        ? `https://dashboard.internetcomputer.org/${urlTargetType}/${encodeURIComponent(urlDisplayTarget)}`
        : '#';
    }
    urlTargetMode = '';
    urlTargetKind = '';
    urlLocksTarget = false;
    if (sanitizedCanister) {
      urlTargetMode = urlPrefill.mode;
      urlTargetKind = urlTargetType;
      urlLocksTarget = true;
      setMode(urlTargetMode);
      if (canisterInput && shouldApplyTargetValue) canisterInput.value = sanitizedCanister;
    } else if (sanitizedNeuron) {
      urlTargetMode = urlPrefill.mode;
      urlTargetKind = urlTargetType;
      urlLocksTarget = true;
      setMode('neuron');
      if (neuronInput && shouldApplyTargetValue) neuronInput.value = sanitizedNeuron;
    } else if (hasPrefillTarget && shouldApplyTargetValue) {
      if (canisterInput) canisterInput.value = '';
      if (neuronInput) neuronInput.value = '';
    }
    lastAppliedPrefillFragment = currentFragment;
  };
  const currentMode = () => builder.querySelector('input[name="memo-builder-mode"]:checked')?.value || 'rawIcp';
  const optionalMemoText = () => optionalInput?.textContent || '';
  const setOptionalMemoText = (text) => {
    if (!optionalInput) return;
    optionalInput.textContent = text;
  };
  const optionalCaretOffset = () => {
    if (!optionalInput || !optionalInput.isContentEditable) return null;
    const selection = window.getSelection?.();
    if (!selection || selection.rangeCount === 0) return null;
    const range = selection.getRangeAt(0);
    if (!optionalInput.contains(range.endContainer)) return null;
    const prefix = range.cloneRange();
    prefix.selectNodeContents(optionalInput);
    prefix.setEnd(range.endContainer, range.endOffset);
    return prefix.toString().length;
  };
  const restoreOptionalCaret = (offset) => {
    if (!optionalInput || !optionalInput.isContentEditable || offset === null) return;
    const selection = window.getSelection?.();
    if (!selection) return;
    const walker = document.createTreeWalker(optionalInput, NodeFilter.SHOW_TEXT);
    let remaining = Math.max(0, offset);
    let node = walker.nextNode();
    while (node) {
      if (remaining <= node.textContent.length) {
        const range = document.createRange();
        range.setStart(node, remaining);
        range.collapse(true);
        selection.removeAllRanges();
        selection.addRange(range);
        return;
      }
      remaining -= node.textContent.length;
      node = walker.nextNode();
    }
    const range = document.createRange();
    range.selectNodeContents(optionalInput);
    range.collapse(false);
    selection.removeAllRanges();
    selection.addRange(range);
  };
  const renderOptionalMemoText = (result, caretOffset = null) => {
    if (!optionalInput) return;
    const currentText = optionalMemoText();
    const keptText = result.ok ? result.keptOptionalMemo : currentText;
    const mutedText = result.ok ? result.truncatedOptionalMemo : '';
    optionalInput.replaceChildren();
    const kept = document.createElement('span');
    kept.textContent = keptText;
    optionalInput.append(kept);
    if (mutedText) {
      const muted = document.createElement('span');
      muted.className = 'memo-builder-muted';
      muted.textContent = mutedText;
      optionalInput.append(muted);
    }
    restoreOptionalCaret(caretOffset);
  };
  const sanitizeTargetInputs = () => {
    if (canisterInput) canisterInput.value = sanitizeCanisterPrincipalText(canisterInput.value);
    if (neuronInput) neuronInput.value = sanitizeNeuronIdText(neuronInput.value);
  };
  const clearHiddenModeInputs = (mode) => {
    if (mode === 'cycles' && optionalMemoText()) setOptionalMemoText('');
    if (mode !== 'neuron' && neuronInput?.value) neuronInput.value = '';
  };
  const render = ({ preserveOptionalCaret = false } = {}) => {
    const caretOffset = preserveOptionalCaret ? optionalCaretOffset() : null;
    applyUrlPrefill();
    sanitizeTargetInputs();
    const mode = currentMode();
    clearHiddenModeInputs(mode);
    const result = buildAdvancedMemo({
      mode,
      canisterText: canisterInput?.value || '',
      neuronIdText: neuronInput?.value || '',
      optionalMemoText: optionalMemoText(),
    });
    if (modeFieldset) modeFieldset.hidden = Boolean(urlTargetMode);
    if (canisterFields) canisterFields.hidden = urlLocksTarget || mode === 'neuron';
    if (neuronFields) neuronFields.hidden = urlLocksTarget || mode !== 'neuron';
    const hasOptionalMemoField = mode !== 'cycles';
    if (optionalFields) optionalFields.hidden = !hasOptionalMemoField;
    if (outputInput) {
      outputInput.value = result.output;
      outputInput.placeholder = result.ok ? '' : 'Fix validation errors to generate a memo';
    }
    if (copyButton) copyButton.disabled = !result.ok || !result.output;
    if (messages) {
      const validationMessages = advancedMemoValidationMessages(result, {
        lockedTargetText: urlLocksTarget ? (canisterInput?.value || neuronInput?.value || '') : '',
        lockedTargetType: urlLocksTarget ? urlTargetKind : '',
      });
      messages.className = result.errors.length > 0 ? 'memo-builder-error' : result.warnings.length > 0 ? 'memo-builder-warning' : 'memo-builder-help';
      messages.textContent = validationMessages.join(' ');
    }
    renderOptionalMemoText(result, caretOffset);
  };

  builder.addEventListener('input', (evt) => {
    render({
      preserveOptionalCaret: Boolean(optionalInput && (evt.target === optionalInput || optionalInput.contains(evt.target))),
    });
  });
  builder.addEventListener('change', render);
  optionalInput?.addEventListener('keydown', (evt) => {
    if (evt.key === 'Enter') evt.preventDefault();
  });
  optionalInput?.addEventListener('paste', (evt) => {
    evt.preventDefault();
    const text = evt.clipboardData?.getData('text/plain') || '';
    document.execCommand?.('insertText', false, text.replace(/[\r\n]+/g, ' '));
  });
  copyButton?.addEventListener('click', async () => {
    const value = outputInput?.value || '';
    if (!value || typeof copyTextToClipboard !== 'function') return;
    const defaultText = copyButton.textContent || 'Copy memo';
    try {
      await copyTextToClipboard(value);
      copyButton.textContent = 'Copied';
      window.setTimeout(() => {
        copyButton.textContent = defaultText;
      }, 1200);
    } catch {
      copyButton.textContent = 'Copy failed';
      window.setTimeout(() => {
        copyButton.textContent = defaultText;
      }, 1500);
    }
  });
  window.addEventListener('hashchange', render);
  render();
}
