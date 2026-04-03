import test from 'node:test';
import assert from 'node:assert/strict';

import { setLink } from '../src/dom-helpers.js';

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
