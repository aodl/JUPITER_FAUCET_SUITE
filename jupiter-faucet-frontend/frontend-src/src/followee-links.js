const ALPHA_VOTE_NEURON_ID = '2947465672511369';

export function escapeHtml(value) {
  return String(value)
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('\"', '&quot;')
    .replaceAll("'", '&#39;');
}

export function formatFolloweeLinks(neuron) {
  const follows = [...new Set(neuron.followees.flatMap((entry) => entry[1].followees.map((followee) => followee.id.toString())))];
  if (!follows.length) return 'None';
  return follows.map((id) => {
    const label = id === ALPHA_VOTE_NEURON_ID ? 'αlpha-vote' : id;
    const title = id === ALPHA_VOTE_NEURON_ID ? `${label} (${id})` : id;
    return `<a class="pane-external-link mono" href="https://dashboard.internetcomputer.org/neuron/${escapeHtml(id)}" target="_blank" rel="noopener noreferrer" title="${escapeHtml(title)}">${escapeHtml(label)}</a>`;
  }).join(', ');
}
