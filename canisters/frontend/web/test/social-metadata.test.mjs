import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';

const indexHtmlUrl = new URL('../../public/index.html', import.meta.url);
const previewAssetUrl = new URL('../../public/og/preview-20260520.jpg', import.meta.url);
const indexHtml = readFileSync(indexHtmlUrl, 'utf8');

const canonicalOrigin = 'https://www.jupiter-faucet.com/';
const previewImageUrl = `${canonicalOrigin}og/preview-20260520.jpg`;
const redirectedOrigin = 'https://jupiter-faucet.com';

function metaContent(attribute, value) {
  const matches = [...indexHtml.matchAll(
    new RegExp(`<meta\\s+${attribute}="${value}"\\s+content="([^"]*)"\\s*/?>`, 'gu'),
  )];

  assert.equal(matches.length, 1, `expected exactly one ${attribute}="${value}" meta tag`);
  return matches[0][1];
}

test('canonical and social preview metadata use the direct production origin', () => {
  const canonicalMatches = [...indexHtml.matchAll(
    /<link\s+rel="canonical"\s+href="([^"]*)"\s*\/?>/gu,
  )];
  assert.equal(canonicalMatches.length, 1, 'expected exactly one canonical link');

  const canonicalUrl = canonicalMatches[0][1];
  const openGraphUrl = metaContent('property', 'og:url');
  const openGraphImage = metaContent('property', 'og:image');
  const openGraphSecureImage = metaContent('property', 'og:image:secure_url');
  const twitterImage = metaContent('name', 'twitter:image');

  assert.equal(canonicalUrl, canonicalOrigin);
  assert.equal(openGraphUrl, canonicalOrigin);
  assert.equal(openGraphImage, previewImageUrl);
  assert.equal(openGraphSecureImage, previewImageUrl);
  assert.equal(twitterImage, previewImageUrl);

  assert.equal(metaContent('property', 'og:image:type'), 'image/jpeg');
  assert.equal(metaContent('property', 'og:image:width'), '1200');
  assert.equal(metaContent('property', 'og:image:height'), '630');
  assert.notEqual(metaContent('property', 'og:image:alt').trim(), '');
  assert.notEqual(metaContent('name', 'twitter:image:alt').trim(), '');

  for (const value of [
    canonicalUrl,
    openGraphUrl,
    openGraphImage,
    openGraphSecureImage,
    twitterImage,
  ]) {
    assert.equal(value.startsWith(redirectedOrigin), false);
  }
});

test('the social preview metadata references an existing local asset', () => {
  assert.equal(existsSync(previewAssetUrl), true);
});
