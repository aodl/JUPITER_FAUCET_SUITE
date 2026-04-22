import test from 'node:test';
import assert from 'node:assert/strict';

import {
  setLink,
  setPaneValueText,
  setPaneValueTrustedHtml,
} from '../src/dom-helpers.js';

function withFakeDocument(nodeMap, fn) {
  const originalDocument = globalThis.document;
  globalThis.document = {
    getElementById(id) {
      return nodeMap.get(id) || null;
    },
  };
  try {
    fn();
  } finally {
    globalThis.document = originalDocument;
  }
}

function makeLinkNode() {
  const span = { textContent: '' };
  return {
    href: '',
    title: '',
    removeAttribute(name) {
      if (name === 'href') this.href = '';
      if (name === 'title') this.title = '';
    },
    querySelector(selector) {
      return selector === 'span' ? span : null;
    },
    get span() {
      return span;
    },
  };
}

function makeClassList() {
  const values = new Set();
  return {
    add(value) {
      values.add(value);
    },
    has(value) {
      return values.has(value);
    },
  };
}

function makeStatusNode() {
  return {
    hidden: false,
    className: '',
    classList: makeClassList(),
    textContent: '',
    title: '',
    ariaLabel: '',
    removeAttribute(name) {
      if (name === 'title') this.title = '';
      if (name === 'aria-label') this.ariaLabel = '';
    },
    setAttribute(name, value) {
      if (name === 'aria-label') this.ariaLabel = value;
    },
  };
}

function makeValueNode() {
  return {
    textContent: '',
    innerHTML: '',
  };
}

test('setLink clears stale visible text when a link becomes unavailable', () => {
  const node = makeLinkNode();
  const nodes = new Map([['stake-link', node]]);
  withFakeDocument(nodes, () => {
    setLink('stake-link', {
      href: 'https://example.com/account',
      text: 'aaaaa-aa',
      title: 'account',
    });
    assert.equal(node.href, 'https://example.com/account');
    assert.equal(node.title, 'account');
    assert.equal(node.span.textContent, 'aaaaa-aa');

    setLink('stake-link', {});
    assert.equal(node.href, '');
    assert.equal(node.title, '');
    assert.equal(node.span.textContent, '');
  });
});

test('setPaneValueText writes text content and exposes loading state without touching innerHTML', () => {
  const valueNode = makeValueNode();
  const statusNode = makeStatusNode();
  const nodes = new Map([
    ['stake-neuron-age', valueNode],
    ['stake-neuron-age-status', statusNode],
  ]);

  withFakeDocument(nodes, () => {
    setPaneValueText('stake-neuron-age', { value: '<b>safe</b>', loading: true });
  });

  assert.equal(valueNode.textContent, '<b>safe</b>');
  assert.equal(valueNode.innerHTML, '');
  assert.equal(statusNode.hidden, false);
  assert.equal(statusNode.ariaLabel, 'Loading');
  assert.equal(statusNode.textContent, '');
  assert.equal(statusNode.classList.has('metric-status--loading'), true);
});

test('setPaneValueTrustedHtml is the explicit raw-html path and still renders error status metadata', () => {
  const valueNode = makeValueNode();
  const statusNode = makeStatusNode();
  const nodes = new Map([
    ['stake-neuron-followees', valueNode],
    ['stake-neuron-followees-status', statusNode],
  ]);

  withFakeDocument(nodes, () => {
    setPaneValueTrustedHtml('stake-neuron-followees', {
      value: '<a href="https://example.com">followee</a>',
      error: 'followees unavailable',
    });
  });

  assert.equal(valueNode.innerHTML, '<a href="https://example.com">followee</a>');
  assert.equal(valueNode.textContent, '');
  assert.equal(statusNode.hidden, false);
  assert.equal(statusNode.textContent, '⚠');
  assert.equal(statusNode.title, 'followees unavailable');
  assert.equal(statusNode.ariaLabel, 'followees unavailable');
  assert.equal(statusNode.classList.has('metric-status--error'), true);
});
