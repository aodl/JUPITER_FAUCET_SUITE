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
