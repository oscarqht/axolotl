// Dependency-free checks against the exact initialization script shipped to Wry.
const { readFileSync } = require('node:fs');
const { join } = require('node:path');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const { test } = require('node:test');
const source = readFileSync(join(__dirname, '../crates/desktop/assets/input-bridge.js'), 'utf8');

function bridge(mac = true) {
  const listeners = {}, messages = [];
  class Element {
    constructor(href, download = false) { this.href = href; this.download = download; this.baseURI = 'https://example.com/dir/page'; }
    matches() { return this.href !== null; }
    getAttribute() { return this.href; }
    hasAttribute(name) { return name === 'download' && this.download; }
  }
  const window = {
    ipc: { postMessage: value => messages.push(value) },
    addEventListener: (type, listener) => { listeners[type] = listener; },
  };
  vm.runInNewContext(source.replace('__NONCE__', 'secret')
    .replaceAll('__LINK_MODIFIER__', mac ? 'event.metaKey && !event.ctrlKey' : 'event.ctrlKey && !event.metaKey')
    .replace('__GESTURES__', ''), { window, Element, URL });
  function click(options = {}, href = '../next?q=1#part', download = false) {
    const link = new Element(href, download);
    const event = { isTrusted: true, defaultPrevented: false, button: 0, metaKey: mac, ctrlKey: !mac,
      shiftKey: false, altKey: false, composedPath: () => [new Element(null), link],
      preventDefault() { this.defaultPrevented = true; }, stopImmediatePropagation() { this.stopped = true; }, ...options };
    listeners.click(event);
    return event;
  }
  return { click, messages, listeners };
}

test('trusted Cmd+click resolves a nested link and cancels native navigation/popup exactly once', () => {
  const b = bridge();
  const e = b.click();
  assert.equal(e.defaultPrevented, true);
  assert.equal(e.stopped, true);
  assert.deepEqual(b.messages, ['secret:open-link:https://example.com/next?q=1#part']);
});

test('ordinary clicks, macOS Ctrl+click, other buttons and synthetic events are untouched', () => {
  for (const opts of [{ metaKey: false }, { metaKey: false, ctrlKey: true }, { button: 1 },
    { button: 2 }, { shiftKey: true }, { altKey: true }, { ctrlKey: true },
    { isTrusted: false }, { defaultPrevented: true }, { composedPath: () => [] }]) {
    const b = bridge();
    const e = b.click(opts);
    assert.equal(e.stopped, undefined);
    assert.deepEqual(b.messages, []);
  }
});

test('unsafe URLs and downloads cannot enter sibling routing', () => {
  for (const href of ['javascript:alert(1)', 'data:text/html,x', 'file:///etc/passwd', 'about:config', 'mailto:x@example.com', 'http://[']) {
    const b = bridge();
    assert.equal(b.click({}, href).defaultPrevented, false);
    assert.deepEqual(b.messages, []);
  }
  const b = bridge();
  assert.equal(b.click({}, '/file', true).defaultPrevented, false);
  assert.deepEqual(b.messages, []);
});

test('relative, fragment, absolute and blank links retain destination semantics', () => {
  for (const [href, expected] of [['#section', 'https://example.com/dir/page#section'],
    ['/next', 'https://example.com/next'], ['https://example.org/a', 'https://example.org/a'], ['about:blank', 'about:blank']]) {
    const b = bridge(); b.click({}, href);
    assert.deepEqual(b.messages, ['secret:open-link:' + expected]);
  }
});

test('non-macOS uses Ctrl+click rather than the platform meta key', () => {
  const b = bridge(false); assert.equal(b.click().defaultPrevented, true);
  const other = bridge(false); assert.equal(other.click({ ctrlKey: false, metaKey: true }).defaultPrevented, false);
});

test('existing trusted shortcuts remain authenticated and ordinary keys stay untouched', () => {
  const b = bridge();
  const event = { isTrusted: true, metaKey: true, key: 'l',
    preventDefault() { this.prevented = true; }, stopImmediatePropagation() {} };
  b.listeners.keydown(event);
  assert.equal(event.prevented, true);
  assert.deepEqual(b.messages, ['secret:address']);
  b.listeners.keydown({ ...event, isTrusted: false });
  assert.equal(b.messages.length, 1);
});

test('settings shortcut uses the platform modifier and rejects untrusted or modified input', () => {
  for (const mac of [true, false]) {
    for (const overrides of [null, { isTrusted: false }, { altKey: true }, { shiftKey: true },
      { metaKey: !mac, ctrlKey: mac }, { metaKey: true, ctrlKey: true }]) {
      const b = bridge(mac);
      const event = { isTrusted: true, key: ',', metaKey: mac, ctrlKey: !mac, altKey: false, shiftKey: false,
        preventDefault() { this.prevented = true; }, stopImmediatePropagation() { this.stopped = true; }, ...overrides };
      b.listeners.keydown(event);
      assert.deepEqual(b.messages, overrides === null ? ['secret:settings'] : []);
      assert.equal(Boolean(event.prevented), overrides === null);
    }
  }
});


test('Option+click opens the popup and resolves nested links while preserving Cmd+click', () => {
  const b = bridge();
  const e = b.click({ metaKey: false, altKey: true });
  assert.equal(e.defaultPrevented, true);
  assert.equal(e.stopped, true);
  assert.deepEqual(b.messages, ['secret:popup-link:https://example.com/next?q=1#part']);
  b.click();
  assert.equal(b.messages[1], 'secret:open-link:https://example.com/next?q=1#part');
  for (const opts of [{ metaKey: true }, { ctrlKey: true }, { shiftKey: true },
    { button: 1 }, { isTrusted: false }, { defaultPrevented: true }]) {
    const other = bridge();
    assert.equal(other.click({ metaKey: false, altKey: true, ...opts }).stopped, undefined);
    assert.deepEqual(other.messages, []);
  }
});

test('popup links reject unsafe schemes and downloads, including target blank navigation', () => {
  for (const href of ['javascript:alert(1)', 'data:text/html,x', 'file:///etc/passwd', 'about:config', 'mailto:x@example.com']) {
    const b = bridge();
    assert.equal(b.click({ metaKey: false, altKey: true }, href).defaultPrevented, false);
    assert.deepEqual(b.messages, []);
  }
  const b = bridge();
  assert.equal(b.click({ metaKey: false, altKey: true }, '/file', true).defaultPrevented, false);
});

function key(b, options = {}) {
  const event = { isTrusted: true, key: 'T', metaKey: true, ctrlKey: false, altKey: false, shiftKey: true,
    preventDefault() { this.prevented = true; }, stopImmediatePropagation() { this.stopped = true; }, ...options };
  b.listeners.keydown(event);
  return event;
}

test('Cmd+Shift+T creates a space and Cmd+T remains a tab shortcut', () => {
  const b = bridge();
  assert.equal(key(b).prevented, true);
  assert.deepEqual(b.messages, ['secret:new-space']);
  assert.equal(key(b, { key: 't', shiftKey: false }).prevented, true);
  assert.deepEqual(b.messages, ['secret:new-space', 'secret:new-tab']);
  for (const opts of [{ isTrusted: false }, { altKey: true }, { metaKey: false }, { ctrlKey: true },
    { key: 'r' }, { metaKey: false, ctrlKey: true }]) {
    const other = bridge();
    assert.equal(key(other, opts).prevented, undefined);
    assert.deepEqual(other.messages, []);
  }
  const windows = bridge(false);
  key(windows, { metaKey: false, ctrlKey: true });
  assert.deepEqual(windows.messages, ['secret:new-space']);
});

test('trusted Escape reaches the shell without swallowing ordinary page behavior', () => {
  const b = bridge();
  const e = key(b, { key: 'Escape', metaKey: false, shiftKey: false });
  assert.equal(e.prevented, undefined);
  assert.deepEqual(b.messages, ['secret:escape']);
  key(b, { key: 'Escape', metaKey: false, shiftKey: false, isTrusted: false });
  key(b, { key: 'Escape', metaKey: false, shiftKey: true });
  assert.equal(b.messages.length, 1);
});
