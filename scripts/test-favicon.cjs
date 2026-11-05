// Exercise the actual Wry initialization script without network or dependencies.
const { readFileSync } = require('node:fs');
const { join } = require('node:path');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const { test } = require('node:test');
const source = readFileSync(join(__dirname, '../crates/desktop/assets/input-bridge.js'), 'utf8')
  .replace('__NONCE__', 'secret').replaceAll('__LINK_MODIFIER__', 'event.metaKey && !event.ctrlKey')
  .replace('__GESTURES__', '');

function favicon({ href, page = 'https://accounts.google.com/select', frame = false, tainted = false } = {}) {
  const listeners = {}, images = [], messages = [], requests = [];
  const location = new URL(page);
  const window = {
    ipc: { postMessage: value => messages.push(value) },
    addEventListener: (type, callback) => { listeners[type] = callback; },
  };
  window.top = frame ? {} : window;
  class Image {
    constructor() { images.push(this); }
    set src(value) { this.url = value; requests.push({ url: value, crossOrigin: this.crossOrigin }); }
    get src() { return this.url; }
  }
  const document = {
    querySelector: () => href === undefined ? null : { href },
    createElement: () => ({
      getContext: () => ({ drawImage() {} }),
      toDataURL() { if (tainted) throw new Error('SecurityError: tainted canvas'); return 'data:image/png;base64,small-png'; },
    }),
  };
  vm.runInNewContext(source, { window, document, location, Image, URL });
  listeners.load();
  return { images, requests, messages };
}

test('same-origin favicons use ordinary image mode, including Google favicon fallback', () => {
  for (const href of [undefined, 'https://accounts.google.com/icon.png']) {
    const b = favicon({ href });
    assert.equal(b.requests.length, 1);
    assert.equal(b.requests[0].crossOrigin, undefined);
    b.images[0].onload();
    assert.deepEqual(b.messages, ['secret:favicon:small-png']);
  }
});

test('unreadable cross-origin icons go directly to the page-origin fallback', () => {
  const b = favicon({ href: 'https://cdn.example.com/icon.png' });
  assert.deepEqual(b.requests, [{ url: 'https://accounts.google.com/favicon.ico', crossOrigin: undefined }]);
});

test('failed declared icons fall back once, with no CORS mode or retry loop', () => {
  const b = favicon({ href: 'https://accounts.google.com/icon.png' });
  b.images[0].onerror();
  b.images[0].onerror();
  assert.deepEqual(b.requests.map(r => r.url), [
    'https://accounts.google.com/icon.png', 'https://accounts.google.com/favicon.ico',
  ]);
  assert.ok(b.requests.every(r => r.crossOrigin === undefined));
});

test('embedded PNG icons still reach chrome, while canvas security failures stay contained', () => {
  const b = favicon({ href: 'data:image/png;base64,fixture' });
  assert.equal(b.requests[0].url, 'data:image/png;base64,fixture');
  b.images[0].onload();
  assert.deepEqual(b.messages, ['secret:favicon:small-png']);
  const blocked = favicon({ tainted: true });
  assert.doesNotThrow(() => blocked.images[0].onload());
  assert.deepEqual(blocked.messages, []);
});

test('subframes and non-HTTP documents never request a favicon', () => {
  for (const options of [{ frame: true }, { page: 'about:blank' }, { page: 'file:///tmp/page.html' }]) {
    const b = favicon(options);
    assert.deepEqual(b.requests, []);
    assert.deepEqual(b.messages, []);
  }
});
