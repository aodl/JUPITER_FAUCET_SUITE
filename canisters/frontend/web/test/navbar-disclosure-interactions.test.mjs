import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';

const __dirname = dirname(fileURLToPath(import.meta.url));
const navbarJs = readFileSync(resolve(__dirname, '../../public/navbar.js'), 'utf8');

class FakeClassList {
  constructor(owner, initial = '') {
    this.owner = owner;
    this.values = new Set(initial.split(/\s+/).filter(Boolean));
  }

  add(value) {
    this.values.add(value);
    this.sync();
  }

  remove(value) {
    this.values.delete(value);
    this.sync();
  }

  contains(value) {
    return this.values.has(value);
  }

  toggle(value, force) {
    const shouldAdd = force === undefined ? !this.values.has(value) : Boolean(force);
    if (shouldAdd) this.values.add(value);
    else this.values.delete(value);
    this.sync();
    return shouldAdd;
  }

  sync() {
    this.owner.attributes.class = Array.from(this.values).join(' ');
  }
}

class FakeElement {
  constructor(tagName, attrs = {}) {
    this.tagName = tagName.toUpperCase();
    this.attributes = {};
    this.children = [];
    this.parentElement = null;
    this.listeners = new Map();
    this.hidden = false;
    this.textContent = '';
    this.isContentEditable = false;
    this.rect = { left: 0, top: 0, right: 0, bottom: 0, width: 0, height: 0 };
    this.classList = new FakeClassList(this);
    Object.entries(attrs).forEach(([name, value]) => this.setAttribute(name, value));
  }

  get id() {
    return this.attributes.id || '';
  }

  get dataset() {
    const data = {};
    Object.entries(this.attributes).forEach(([name, value]) => {
      if (!name.startsWith('data-')) return;
      const key = name
        .slice(5)
        .replace(/-([a-z])/g, (_, char) => char.toUpperCase());
      data[key] = value;
    });
    return data;
  }

  get isConnected() {
    return Boolean(this.ownerDocument?.contains?.(this));
  }

  setAttribute(name, value) {
    this.attributes[name] = String(value);
    if (name === 'class') this.classList = new FakeClassList(this, String(value));
    if (name === 'hidden') this.hidden = true;
  }

  getAttribute(name) {
    return this.attributes[name] ?? null;
  }

  removeAttribute(name) {
    delete this.attributes[name];
    if (name === 'hidden') this.hidden = false;
  }

  appendChild(child) {
    child.parentElement = this;
    this.children.push(child);
    return child;
  }

  addEventListener(type, handler) {
    if (!this.listeners.has(type)) this.listeners.set(type, []);
    this.listeners.get(type).push(handler);
  }

  dispatchEvent(event) {
    event.target ||= this;
    event.currentTarget = this;
    for (const handler of this.listeners.get(event.type) || []) handler(event);
    if (event.bubbles !== false && this.parentElement) this.parentElement.dispatchEvent(event);
    return !event.defaultPrevented;
  }

  focus() {
    if (this.hidden || this.hasHiddenAncestor() || this.isConnected === false) return;
    this.ownerDocument.activeElement = this;
  }

  contains(node) {
    for (let current = node; current; current = current.parentElement) {
      if (current === this) return true;
    }
    return false;
  }

  closest(selector) {
    for (let current = this; current; current = current.parentElement) {
      if (current.matches(selector)) return current;
    }
    return null;
  }

  querySelector(selector) {
    return this.querySelectorAll(selector)[0] || null;
  }

  querySelectorAll(selector) {
    const matches = [];
    const visit = (node) => {
      node.children.forEach((child) => {
        if (child.matches(selector)) matches.push(child);
        visit(child);
      });
    };
    visit(this);
    return matches;
  }

  matches(selector) {
    return selector.split(',').some((part) => this.matchesSingle(part.trim()));
  }

  matchesSingle(selector) {
    const attrMatch = selector.match(/^([a-z]+)?\[([^=\]]+)(?:="([^"]+)")?\]$/i);
    if (attrMatch) {
      const [, tag, attr, value] = attrMatch;
      if (tag && this.tagName.toLowerCase() !== tag.toLowerCase()) return false;
      if (!(attr in this.attributes)) return false;
      return value === undefined || this.attributes[attr] === value;
    }

    const classAttrMatch = selector.match(/^\.([a-z0-9_-]+)\[([^=]+)="([^"]+)"\]$/i);
    if (classAttrMatch) {
      const [, className, attr, value] = classAttrMatch;
      return this.classList.contains(className) && this.attributes[attr] === value;
    }

    if (selector.startsWith('#')) return this.id === selector.slice(1);
    if (selector.startsWith('.')) return this.classList.contains(selector.slice(1));
    return this.tagName.toLowerCase() === selector.toLowerCase();
  }

  getBoundingClientRect() {
    if (this.hidden || this.hasHiddenAncestor()) return zeroRect();
    if (this.id === 'actions-menu') return popoverRect(this, 'start');
    if (this.id === 'metrics-menu') return popoverRect(this, 'end');
    return this.rect;
  }

  hasHiddenAncestor() {
    for (let current = this; current; current = current.parentElement) {
      if (current.hidden) return true;
    }
    return false;
  }
}

class FakeDocument extends FakeElement {
  constructor() {
    super('#document');
    this.ownerDocument = this;
    this.body = new FakeElement('body');
    this.body.ownerDocument = this;
    this.appendChild(this.body);
    this.activeElement = this.body;
  }

  createElement(tagName) {
    const element = new FakeElement(tagName);
    element.ownerDocument = this;
    return element;
  }

  getElementById(id) {
    return this.querySelector(`#${id}`);
  }

  elementFromPoint(x, y) {
    const all = [];
    const visit = (node) => {
      node.children.forEach((child) => {
        all.push(child);
        visit(child);
      });
    };
    visit(this);
    return all.reverse().find((node) => {
      if (node.hidden || node.hasHiddenAncestor()) return false;
      const rect = node.getBoundingClientRect();
      return x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom;
    }) || null;
  }
}

function zeroRect() {
  return { left: 0, top: 0, right: 0, bottom: 0, width: 0, height: 0 };
}

function rect(left, top, width, height) {
  return { left, top, width, height, right: left + width, bottom: top + height };
}

function popoverRect(menu, align) {
  const disclosure = menu.closest('.nav-disclosure');
  const parentRect = disclosure.getBoundingClientRect();
  const width = menu.id === 'metrics-menu' ? 380 : 92;
  const left = align === 'end' ? parentRect.right - width : parentRect.left;
  return rect(left, parentRect.bottom + 8, width, 116);
}

function event(type, extra = {}) {
  return {
    type,
    bubbles: true,
    defaultPrevented: false,
    preventDefault() {
      this.defaultPrevented = true;
    },
    ...extra,
  };
}

function append(parent, tag, attrs = {}, text = '') {
  const child = parent.ownerDocument.createElement(tag);
  Object.entries(attrs).forEach(([name, value]) => child.setAttribute(name, value));
  child.textContent = text;
  parent.appendChild(child);
  return child;
}

function addPanel(document, key, pages = 2) {
  const section = append(document.body, 'article', {
    class: 'nav-panel-section',
    'data-panel': key,
    id: `nav-panel-${key}`,
  });
  for (let i = 0; i < pages; i += 1) {
    append(section, 'div', { class: 'nav-panel-page', 'data-page': String(i) });
  }
  for (let i = 0; i < pages; i += 1) {
    append(section, 'button', { class: 'nav-panel-dot', 'data-page': String(i) });
  }
  return section;
}

function setupNavbar(width = 1440, initialHash = '') {
  const document = new FakeDocument();
  const windowEvents = new Map();
  const location = { pathname: '/', search: '', hash: initialHash };
  const historyStack = [initialHash];
  let historyIndex = 0;

  const window = {
    innerWidth: width,
    scrollY: 0,
    location,
    setTimeout(handler) {
      handler();
    },
    addEventListener(type, handler) {
      if (!windowEvents.has(type)) windowEvents.set(type, []);
      windowEvents.get(type).push(handler);
    },
    dispatchWindowEvent(type) {
      for (const handler of windowEvents.get(type) || []) handler(event(type, { bubbles: false }));
    },
  };
  window.history = {
    pushState(_state, _title, url) {
      location.hash = url.startsWith('#') ? url : '';
      historyStack.splice(historyIndex + 1);
      historyStack.push(location.hash);
      historyIndex = historyStack.length - 1;
    },
    replaceState(_state, _title, url) {
      location.hash = url.startsWith('#') ? url : '';
      historyStack[historyIndex] = location.hash;
    },
    back() {
      if (historyIndex === 0) return;
      historyIndex -= 1;
      location.hash = historyStack[historyIndex];
      window.dispatchWindowEvent('popstate');
    },
    forward() {
      if (historyIndex >= historyStack.length - 1) return;
      historyIndex += 1;
      location.hash = historyStack[historyIndex];
      window.dispatchWindowEvent('popstate');
    },
  };

  const context = {
    window,
    document,
    history: window.history,
    CustomEvent: class CustomEvent {
      constructor(type, init = {}) {
        this.type = type;
        this.detail = init.detail;
        this.bubbles = false;
      }
    },
    requestAnimationFrame(handler) {
      handler();
    },
  };

  const navbar = append(document.body, 'header', { class: 'navbar', id: 'navbar' });
  navbar.rect = rect(0, 0, width, 60);
  const nav = append(navbar, 'nav', { class: 'navbar-inner' });
  const links = append(nav, 'div', { class: 'nav-links' });
  append(links, 'a', { href: '/#intro', class: 'nav-brand' });
  const aboutLink = append(links, 'a', { href: '#about', class: 'nav-item', 'data-panel': 'about' }, 'About');
  const howLink = append(links, 'a', { href: '#how-it-works', class: 'nav-item', 'data-panel': 'how-it-works' }, 'How');
  append(links, 'a', { href: '#simulator', class: 'nav-item', 'data-panel': 'simulator' }, 'Simulator');
  const domainsLink = append(links, 'a', { href: '#domains', class: 'nav-item', 'data-panel': 'domains' }, 'Domains');
  const actions = append(nav, 'div', { class: 'nav-disclosure', 'data-nav-group': 'actions' });
  actions.rect = rect(width - 230, 16, 72, 28);
  const actionsButton = append(actions, 'button', {
    id: 'actions-menu-toggle',
    class: 'nav-item nav-disclosure-toggle',
    type: 'button',
    'aria-expanded': 'false',
    'aria-controls': 'actions-menu',
  }, 'Actions');
  actionsButton.rect = rect(width - 230, 16, 72, 28);
  const actionsMenu = append(actions, 'div', {
    id: 'actions-menu',
    class: 'nav-popover action-rail',
    hidden: '',
  });
  append(actionsMenu, 'a', { href: '#simulator', class: 'metric-rail-link nav-item', 'data-panel': 'simulator' }, 'Plan');
  append(actionsMenu, 'a', { href: '#memo-builder', class: 'metric-rail-link nav-item', 'data-panel': 'memo-builder' }, 'Commit');
  append(actionsMenu, 'a', { href: '#relay-setup', class: 'metric-rail-link nav-item', 'data-panel': 'relay-setup' }, 'Optimize');

  const metrics = append(nav, 'div', {
    class: 'nav-disclosure nav-disclosure--end',
    'data-nav-group': 'metrics',
  });
  metrics.rect = rect(width - 142, 16, 74, 28);
  const metricsButton = append(metrics, 'button', {
    id: 'metrics-menu-toggle',
    class: 'nav-item nav-disclosure-toggle',
    type: 'button',
    'aria-expanded': 'false',
    'aria-controls': 'metrics-menu',
  }, 'Metrics');
  metricsButton.rect = rect(width - 142, 16, 74, 28);
  const metricsMenu = append(metrics, 'div', {
    id: 'metrics-menu',
    class: 'nav-popover metric-rail',
    hidden: '',
  });
  append(metricsMenu, 'p', { class: 'metric-rail-subtitle', id: 'landing-next-run' }, 'Next historian run approx. 26 Jul 2026, 09:04 BST.');
  append(metricsMenu, 'a', { href: '#metric-stake', class: 'metric-rail-link nav-item', 'data-panel': 'metric-stake' }, 'Jupiter Stake');
  append(metricsMenu, 'a', { href: '#metric-commitments', class: 'metric-rail-link nav-item', 'data-panel': 'metric-commitments' }, 'Patron Commitments');
  append(metricsMenu, 'a', { href: '#metric-tracker', class: 'metric-rail-link nav-item', 'data-panel': 'metric-tracker' }, 'Track Memos');

  const backdrop = append(document.body, 'div', { class: 'nav-panel-backdrop', id: 'nav-panel-backdrop' });
  append(backdrop, 'button', { class: 'nav-panel-close' });
  ['about', 'how-it-works', 'memo-builder', 'simulator', 'domains', 'relay-setup', 'metric-stake', 'metric-commitments', 'metric-tracker'].forEach((key) => {
    addPanel(document, key, key === 'how-it-works' ? 4 : 2);
  });
  const howSection = document.querySelector('.nav-panel-section[data-panel="how-it-works"]');
  const howPageTwoLink = append(howSection, 'a', {
    href: '/#how-it-works:2',
    class: 'pane-link',
    'data-panel': 'how-it-works',
  }, 'Prepare page');
  const memoBuilderPrefillLink = append(howSection, 'a', {
    href: '/#memo-builder?canister=u2qkp-aqaaa-aaaar-qb7ea-cai&title=Relay%20Canister&label=Optional%20Donor%20Name',
    class: 'pane-link',
    'data-panel': 'memo-builder',
  }, 'Relay memo builder');
  append(document.body, 'aside', {
    id: 'orbit-disbursement-status',
    class: 'orbit-disbursement-status',
    hidden: '',
  });

  const vmContext = vm.createContext(context);
  vm.runInContext(navbarJs, vmContext);
  document.dispatchEvent(event('DOMContentLoaded', { bubbles: false }));

  return {
    document,
    window,
    actionsButton,
    actionsMenu,
    metricsButton,
    metricsMenu,
    backdrop,
    aboutLink,
    howLink,
    domainsLink,
    howPageTwoLink,
    memoBuilderPrefillLink,
  };
}

function click(element) {
  element.dispatchEvent(event('click'));
}

function keydown(document, key) {
  document.dispatchEvent(event('keydown', { key, target: document.body }));
}

function center(rectangle) {
  return {
    x: rectangle.left + rectangle.width / 2,
    y: rectangle.top + rectangle.height / 2,
  };
}

function activeSection(document) {
  return document.querySelector('.nav-panel-section--active');
}

function activePage(section) {
  return Array.from(section.querySelectorAll('.nav-panel-page'))
    .find((page) => page.classList.contains('is-active'));
}

for (const width of [1440, 1024, 861, 720]) {
  test(`navbar disclosures align and remain clickable at ${width}px`, () => {
    const env = setupNavbar(width);
    const actionsRect = env.actionsButton.getBoundingClientRect();
    const metricsRect = env.metricsButton.getBoundingClientRect();

    click(env.actionsButton);
    assert.equal(env.actionsMenu.hidden, false);
    assert.equal(env.metricsMenu.hidden, true);
    assert.equal(env.actionsButton.getAttribute('aria-expanded'), 'true');
    assert.equal(env.metricsButton.getAttribute('aria-expanded'), 'false');
    assert.ok(Math.abs(env.actionsMenu.getBoundingClientRect().left - actionsRect.left) <= 2);
    assert.ok(env.actionsMenu.getBoundingClientRect().top >= actionsRect.bottom);
    const metricsCenter = center(metricsRect);
    assert.equal(env.document.elementFromPoint(metricsCenter.x, metricsCenter.y), env.metricsButton);

    click(env.metricsButton);
    assert.equal(env.actionsMenu.hidden, true);
    assert.equal(env.metricsMenu.hidden, false);
    assert.equal(env.actionsButton.getAttribute('aria-expanded'), 'false');
    assert.equal(env.metricsButton.getAttribute('aria-expanded'), 'true');
    const actionsCenter = center(actionsRect);
    assert.equal(env.document.elementFromPoint(actionsCenter.x, actionsCenter.y), env.actionsButton);
    const metricsMenuRect = env.metricsMenu.getBoundingClientRect();
    assert.ok(Math.abs(metricsMenuRect.right - metricsRect.right) <= 2);
    assert.ok(metricsMenuRect.top >= metricsRect.bottom);

    click(env.metricsButton);
    assert.equal(env.actionsMenu.hidden, true);
    assert.equal(env.metricsMenu.hidden, true);
    assert.equal(env.backdrop.classList.contains('is-open'), false);
    assert.equal(env.actionsButton.getAttribute('aria-expanded'), 'false');
    assert.equal(env.metricsButton.getAttribute('aria-expanded'), 'false');
  });
}

test('navbar disclosure and panel state transitions are authoritative', () => {
  const env = setupNavbar();
  const planLink = env.actionsMenu.querySelector('a[data-panel="simulator"]');
  const stakeLink = env.metricsMenu.querySelector('a[data-panel="metric-stake"]');

  click(env.actionsButton);
  click(env.actionsButton);
  assert.equal(env.actionsMenu.hidden, true);
  assert.equal(env.backdrop.classList.contains('is-open'), false);

  click(env.actionsButton);
  click(planLink);
  assert.equal(env.actionsMenu.hidden, true);
  assert.equal(env.backdrop.classList.contains('is-open'), true);
  assert.equal(env.document.querySelector('.nav-panel-section--active').getAttribute('data-panel'), 'simulator');
  click(env.actionsButton);
  assert.equal(env.backdrop.classList.contains('is-open'), false);
  assert.equal(env.window.location.hash, '');

  click(env.metricsButton);
  click(stakeLink);
  assert.equal(env.metricsMenu.hidden, true);
  assert.equal(env.backdrop.classList.contains('is-open'), true);
  assert.equal(env.document.querySelector('.nav-panel-section--active').getAttribute('data-panel'), 'metric-stake');
  click(env.metricsButton);
  assert.equal(env.backdrop.classList.contains('is-open'), false);
  assert.equal(env.metricsButton.getAttribute('aria-expanded'), 'false');

  click(env.actionsButton);
  keydown(env.document, 'Escape');
  assert.equal(env.actionsMenu.hidden, true);

  click(env.metricsButton);
  env.document.body.dispatchEvent(event('click'));
  assert.equal(env.metricsMenu.hidden, true);
});

test('navbar hash navigation opens panels without reopening dropdowns', () => {
  const env = setupNavbar();
  const planLink = env.actionsMenu.querySelector('a[data-panel="simulator"]');
  const stakeLink = env.metricsMenu.querySelector('a[data-panel="metric-stake"]');

  click(env.actionsButton);
  click(planLink);
  assert.equal(env.window.location.hash, '#simulator');
  assert.equal(env.actionsMenu.hidden, true);
  click(env.metricsButton);
  assert.equal(env.window.location.hash, '');
  assert.equal(env.metricsMenu.hidden, false);
  click(stakeLink);
  assert.equal(env.window.location.hash, '#metric-stake');
  assert.equal(env.metricsMenu.hidden, true);

  env.window.history.back();
  assert.equal(env.window.location.hash, '');
  assert.equal(env.backdrop.classList.contains('is-open'), false);
  assert.equal(env.actionsMenu.hidden, true);

  env.window.history.forward();
  assert.equal(env.window.location.hash, '#metric-stake');
  assert.equal(env.document.querySelector('.nav-panel-section--active').getAttribute('data-panel'), 'metric-stake');
  assert.equal(env.metricsMenu.hidden, true);

  keydown(env.document, 'Escape');
  assert.equal(env.window.location.hash, '');
  assert.equal(env.backdrop.classList.contains('is-open'), false);
});

test('Relay route keeps Actions ownership through direct load and history', () => {
  const env = setupNavbar(1440, '#how-it-works:2');

  assert.equal(activeSection(env.document).getAttribute('data-panel'), 'how-it-works');
  assert.equal(activePage(activeSection(env.document)).getAttribute('data-page'), '2');
  assert.equal(env.actionsMenu.hidden, true);
  assert.equal(env.actionsButton.classList.contains('nav-item--active'), true);

  click(env.actionsButton);
  assert.equal(env.backdrop.classList.contains('is-open'), false);
  assert.equal(env.window.location.hash, '');

  click(env.actionsButton);
  click(env.actionsMenu.querySelector('a[href="#simulator"]'));
  click(env.actionsButton);
  click(env.actionsButton);
  click(env.actionsMenu.querySelector('a[href="#memo-builder"]'));
  assert.equal(env.window.location.hash, '#memo-builder');
  env.window.history.back();
  assert.equal(env.window.location.hash, '');
  assert.equal(env.backdrop.classList.contains('is-open'), false);
  env.window.history.forward();
  assert.equal(env.window.location.hash, '#memo-builder');
  assert.equal(env.actionsButton.classList.contains('nav-item--active'), true);
  assert.equal(env.actionsMenu.hidden, true);
});

test('opening transient disclosures clears stale panel hashes', () => {
  const cases = [
    ['about to actions', 'aboutLink', 'actionsButton', 'actionsMenu'],
    ['about to metrics', 'aboutLink', 'metricsButton', 'metricsMenu'],
    ['domains to actions', 'domainsLink', 'actionsButton', 'actionsMenu'],
    ['relay route to metrics', null, 'metricsButton', 'metricsMenu', '#how-it-works:2'],
    ['metrics panel to actions', 'metricsButton', 'actionsButton', 'actionsMenu', null, 'a[data-panel="metric-stake"]'],
    ['actions panel to metrics', 'actionsButton', 'metricsButton', 'metricsMenu', null, 'a[data-panel="simulator"]'],
  ];

  for (const [, openerKey, menuButtonKey, menuKey, initialHash, childSelector] of cases) {
    const env = setupNavbar(1440, initialHash || '');
    if (openerKey && childSelector) {
      click(env[openerKey]);
      click((openerKey === 'actionsButton' ? env.actionsMenu : env.metricsMenu).querySelector(childSelector));
    } else if (openerKey) {
      click(env[openerKey]);
    }

    assert.equal(env.backdrop.classList.contains('is-open'), true);
    assert.notEqual(env.window.location.hash, '');

    click(env[menuButtonKey]);
    assert.equal(env.backdrop.classList.contains('is-open'), false);
    assert.equal(env[menuKey].hidden, false);
    assert.equal(env.window.location.hash, '');
    env.window.history.back();
    assert.equal(env.backdrop.classList.contains('is-open'), false);
  }
});

test('direct panel triggers toggle closed but same-section page links navigate', () => {
  const env = setupNavbar();

  click(env.aboutLink);
  assert.equal(env.window.location.hash, '#about');
  click(env.aboutLink);
  assert.equal(env.backdrop.classList.contains('is-open'), false);
  assert.equal(env.window.location.hash, '');

  click(env.howLink);
  assert.equal(env.window.location.hash, '#how-it-works');
  click(env.howLink);
  assert.equal(env.backdrop.classList.contains('is-open'), false);

  click(env.howLink);
  click(env.howPageTwoLink);
  assert.equal(env.window.location.hash, '#how-it-works:2');
  assert.equal(env.backdrop.classList.contains('is-open'), true);
  assert.equal(activePage(activeSection(env.document)).getAttribute('data-page'), '2');
  assert.equal(env.actionsButton.classList.contains('nav-item--active'), true);
});

test('panel links preserve memo builder prefill query parameters', () => {
  const env = setupNavbar();

  click(env.howLink);
  click(env.memoBuilderPrefillLink);

  assert.equal(
    env.window.location.hash,
    '#memo-builder?canister=u2qkp-aqaaa-aaaar-qb7ea-cai&title=Relay%20Canister&label=Optional%20Donor%20Name',
  );
  assert.equal(activeSection(env.document).getAttribute('data-panel'), 'memo-builder');
});

test('closing child panels restores focus to visible parent toggles', () => {
  const env = setupNavbar();

  click(env.actionsButton);
  click(env.actionsMenu.querySelector('a[data-panel="simulator"]'));
  keydown(env.document, 'Escape');
  assert.equal(env.document.activeElement, env.actionsButton);

  click(env.metricsButton);
  click(env.metricsMenu.querySelector('a[data-panel="metric-stake"]'));
  keydown(env.document, 'Escape');
  assert.equal(env.document.activeElement, env.metricsButton);

  click(env.aboutLink);
  keydown(env.document, 'Escape');
  assert.equal(env.document.activeElement, env.aboutLink);
});

test('orbit status visibility uses body metrics menu state', () => {
  const env = setupNavbar();
  const status = env.document.getElementById('orbit-disbursement-status');
  status.hidden = false;

  assert.equal(env.document.body.classList.contains('metrics-menu-open'), false);
  click(env.metricsButton);
  assert.equal(env.document.body.classList.contains('metrics-menu-open'), true);
  click(env.metricsMenu.querySelector('a[data-panel="metric-stake"]'));
  assert.equal(env.document.body.classList.contains('metrics-menu-open'), false);
  assert.equal(env.backdrop.classList.contains('is-open'), true);
});
