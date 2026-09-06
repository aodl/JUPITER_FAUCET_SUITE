import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';

const source = readFileSync(new URL('../../public/diagram-viewer.js', import.meta.url), 'utf8');

const runViewer = (search) => {
  const title = { textContent: 'Protocol diagram' };
  const image = { src: '', alt: '', hidden: true };
  const error = { hidden: true };
  const document = {
    title: 'Jupiter Faucet diagram',
    querySelector(selector) {
      return {
        '#diagram-viewer-title': title,
        '#diagram-viewer-image': image,
        '#diagram-viewer-error': error,
      }[selector];
    },
  };

  runInNewContext(source, {
    document,
    URLSearchParams,
    window: { location: { search } },
  });

  return { document, error, image, title };
};

test('diagram viewer selects only whitelisted SVG assets', () => {
  const expected = {
    overview: 'jupiter-faucet-overview.svg',
    topups: 'perpetual-canister-topups.svg',
    disburser: 'disburser.svg',
    faucet: 'faucet.svg',
    relay: 'relay.svg',
  };

  for (const [diagram, file] of Object.entries(expected)) {
    const result = runViewer(`?diagram=${diagram}&v=frontend-test123`);
    assert.equal(result.image.src, `/${file}?v=frontend-test123`);
    assert.equal(result.image.hidden, false);
    assert.equal(result.error.hidden, true);
    assert.match(result.document.title, / · Jupiter Faucet$/);
  }
});

test('diagram viewer rejects unknown diagrams and unsafe asset versions', () => {
  const unknown = runViewer('?diagram=unknown');
  assert.equal(unknown.image.hidden, true);
  assert.equal(unknown.error.hidden, false);
  assert.equal(unknown.title.textContent, 'Diagram unavailable');

  const unsafeVersion = runViewer('?diagram=faucet&v=../../untrusted');
  assert.equal(unsafeVersion.image.src, '/faucet.svg');
});
