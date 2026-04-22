export function setText(id, value) {
  const node = document.getElementById(id);
  if (node) node.textContent = value;
}


function renderStatusNode(statusNode, { loading = false, error = null } = {}) {
  if (!statusNode) return;
  statusNode.hidden = !loading && !error;
  statusNode.className = 'metric-status';
  statusNode.removeAttribute('title');
  statusNode.removeAttribute('aria-label');
  statusNode.textContent = '';
  if (loading) {
    statusNode.classList.add('metric-status--loading');
    statusNode.setAttribute('aria-label', 'Loading');
    return;
  }
  if (error) {
    statusNode.classList.add('metric-status--error');
    statusNode.textContent = '⚠';
    statusNode.title = error;
    statusNode.setAttribute('aria-label', error);
  }
}

export function setPaneValueText(id, { value = null, loading = false, error = null } = {}) {
  const valueNode = document.getElementById(id);
  if (valueNode) valueNode.textContent = value ?? '';
  renderStatusNode(document.getElementById(`${id}-status`), { loading, error });
}

export function setPaneValueTrustedHtml(id, { value = null, loading = false, error = null } = {}) {
  const valueNode = document.getElementById(id);
  if (valueNode) valueNode.innerHTML = value ?? '';
  renderStatusNode(document.getElementById(`${id}-status`), { loading, error });
}

export function setLink(id, { href, text, title = text } = {}) {
  const node = document.getElementById(id);
  if (!node) return;
  const valueNode = node.querySelector('span') || node;
  if (!href || !text) {
    node.removeAttribute('href');
    node.removeAttribute('title');
    valueNode.textContent = '';
    return;
  }
  node.href = href;
  node.title = title;
  valueNode.textContent = text;
}
