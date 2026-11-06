// Dependency-free checks against the page input bridge shipped to native webviews.
const { readFileSync } = require('node:fs');
const { join } = require('node:path');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const { test } = require('node:test');
const source = readFileSync(join(__dirname, '../crates/desktop/assets/input-bridge.js'), 'utf8')
  .replace('__NONCE__', 'test-nonce')
  .replaceAll('__LINK_MODIFIER__', 'event.metaKey && !event.ctrlKey')
  .replace('__GESTURES__', readFileSync(join(__dirname, '../crates/desktop/assets/gestures.js'), 'utf8'));

class Element {
  constructor({ left = 0, width = 200, content = 600, overflow = 'auto', direction = 'ltr' } = {}) {
    Object.assign(this, { scrollLeft: left, clientWidth: width, scrollWidth: content,
      style: { overflowX: overflow, direction } });
  }
}
function bridge(root = new Element({ content: 200 })) {
  const listeners = {}, messages = [];
  const window = {
    ipc: { postMessage: value => messages.push(value) },
    addEventListener: (type, listener, options) => { listeners[type] = { listener, options }; },
  };
  let now = 0;
  vm.runInNewContext(source, { window, innerWidth: 800, Element,
    document: { scrollingElement: root }, getComputedStyle: node => node.style,
    performance: { now: () => now } });
  return {
    messages,
    idle() { now += 301; },
    fire(type, options = {}) {
      const event = { isTrusted: true, ctrlKey: false, deltaMode: 0, deltaX: 0, deltaY: 0,
        touches: [], composedPath: () => [root], preventDefault() { this.prevented = true; },
        stopImmediatePropagation() { this.stopped = true; }, ...options };
      listeners[type].listener(event);
      return event;
    },
    listeners,
  };
}
const point = (x, y = 0) => [{ clientX: x, clientY: y }];

test('page touch swipes go right/back and left/forward, once per gesture', () => {
  for (const [x, command] of [[70, 'history-back'], [-70, 'history-forward']]) {
    const b = bridge();
    b.fire('touchstart', { touches: point(100) });
    assert.equal(b.fire('touchmove', { touches: point(100 + x) }).prevented, true);
    b.fire('touchmove', { touches: point(100 + x * 2) });
    b.fire('touchmove', { touches: point(100 - x) });
    assert.deepEqual(b.messages, ['test-nonce:' + command]);
    b.fire('touchend');
    b.fire('touchmove', { touches: point(500) });
    assert.equal(b.messages.length, 1);
    b.fire('touchstart', { touches: point(100) });
    b.fire('touchmove', { touches: point(100 + x) });
    assert.deepEqual(b.messages, ['test-nonce:' + command, 'test-nonce:' + command]);
  }
});

test('touch thresholds, horizontal precedence and vertical scrolling stay unchanged', () => {
  const b = bridge();
  b.fire('touchstart', { touches: point(0) });
  assert.equal(b.fire('touchmove', { touches: point(12) }).prevented, undefined);
  assert.equal(b.fire('touchmove', { touches: point(60) }).prevented, true);
  assert.deepEqual(b.messages, []);
  assert.equal(b.fire('touchmove', { touches: point(70, 50) }).prevented, undefined);
  assert.equal(b.fire('touchmove', { touches: point(20, 80) }).prevented, undefined);
  b.fire('touchmove', { touches: point(61) });
  assert.deepEqual(b.messages, ['test-nonce:history-back']);
  assert.equal(b.listeners.touchmove.options.passive, false);
  assert.equal(b.listeners.touchmove.options.capture, true);
});

test('synthetic input, multi-touch and cancelled touches cannot navigate', () => {
  const b = bridge();
  b.fire('touchstart', { isTrusted: false, touches: point(0) });
  b.fire('touchmove', { touches: point(100) });
  b.fire('touchstart', { touches: point(0) });
  b.fire('touchmove', { isTrusted: false, touches: point(100) });
  b.fire('touchmove', { touches: [...point(100), ...point(200)] });
  b.fire('touchcancel');
  b.fire('touchmove', { touches: point(100) });
  b.fire('touchstart', { touches: [...point(0), ...point(100)] });
  b.fire('touchmove', { touches: point(200) });
  assert.deepEqual(b.messages, []);
});

test('wheel deltas retain system direction and normalize pixel, line and page modes', () => {
  const b = bridge();
  for (const [deltaX, deltaY, deltaMode, x, y] of [
    [70, 2, 0, 70, 2], [-70, -2, 0, -70, -2], [5, 1, 1, 80, 16], [1, 0, 2, 800, 0],
  ]) {
    const e = b.fire('wheel', { deltaX, deltaY, deltaMode });
    assert.equal(e.prevented, true);
    assert.equal(e.stopped, true);
    assert.equal(b.messages.at(-1), `test-nonce:wheel:${x}:${y}`);
  }
  assert.equal(b.listeners.wheel.options.passive, false);
  assert.equal(b.listeners.wheel.options.capture, true);
});

test('vertical, diagonal, zero, pinch and synthetic wheel input remain untouched', () => {
  const b = bridge();
  for (const options of [{ deltaY: 100 }, { deltaX: 10, deltaY: 10 }, {},
    { ctrlKey: true, deltaX: 100 }, { isTrusted: false, deltaX: 100 }]) {
    const e = b.fire('wheel', options);
    assert.equal(e.prevented, undefined);
    assert.equal(e.stopped, undefined);
  }
  assert.deepEqual(b.messages, []);
});


test('wheel leaves root and nested horizontal scrolling to the page in both directions', () => {
  for (const rootScroll of [true, false]) {
    const scroller = new Element({ left: 100 });
    const b = bridge(rootScroll ? scroller : undefined);
    for (const deltaX of [80, -80]) {
      const e = b.fire('wheel', { deltaX, composedPath: () => [scroller] });
      assert.equal(e.prevented, undefined);
      assert.equal(e.stopped, undefined);
    }
    assert.deepEqual(b.messages, []);
  }
});

test('wheel navigates only at the directional edge, checking outer scrollers too', () => {
  const inner = new Element({ left: 400 });
  const outer = new Element({ left: 50 });
  const b = bridge(outer);
  assert.equal(b.fire('wheel', { deltaX: 80, composedPath: () => [inner] }).prevented, undefined);
  outer.scrollLeft = 400;
  b.idle();
  assert.equal(b.fire('wheel', { deltaX: 80, composedPath: () => [inner] }).prevented, true);
  b.idle();
  assert.equal(b.fire('wheel', { deltaX: -80, composedPath: () => [inner] }).prevented, undefined);
  inner.scrollLeft = outer.scrollLeft = 0;
  b.idle();
  assert.equal(b.fire('wheel', { deltaX: -80, composedPath: () => [inner] }).prevented, true);
});

test('scrolling wheel momentum cannot navigate when it reaches an edge', () => {
  const root = new Element({ left: 350 });
  const b = bridge(root);
  b.fire('wheel', { deltaX: 80 });
  root.scrollLeft = 400;
  assert.equal(b.fire('wheel', { deltaX: 80 }).prevented, undefined);
  assert.deepEqual(b.messages, []);
  b.idle();
  assert.equal(b.fire('wheel', { deltaX: 80 }).prevented, true);
});

test('RTL negative scroll offsets, shadow paths and hidden overflow are handled', () => {
  const rtl = new Element({ left: -100, direction: 'rtl' });
  const b = bridge();
  for (const deltaX of [-80, 80]) {
    assert.equal(b.fire('wheel', { deltaX, composedPath: () => [rtl, {}] }).prevented, undefined);
  }
  rtl.scrollLeft = -400;
  b.idle();
  assert.equal(b.fire('wheel', { deltaX: -80, composedPath: () => [rtl] }).prevented, true);
  rtl.scrollLeft = 0;
  b.idle();
  assert.equal(b.fire('wheel', { deltaX: 80, composedPath: () => [rtl] }).prevented, true);
  b.idle();
  const hidden = new Element({ left: 100, overflow: 'hidden' });
  assert.equal(b.fire('wheel', { deltaX: 80, composedPath: () => [hidden] }).prevented, true);
});

test('touch scrolls both directions and stays with the page until the next gesture', () => {
  for (const dx of [-80, 80]) {
    const inner = new Element({ left: 100 });
    const b = bridge();
    b.fire('touchstart', { touches: point(100), composedPath: () => [inner] });
    assert.equal(b.fire('touchmove', { touches: point(100 + dx) }).prevented, undefined);
    inner.scrollLeft = dx < 0 ? 400 : 0;
    assert.equal(b.fire('touchmove', { touches: point(100 + dx * 2) }).prevented, undefined);
    assert.deepEqual(b.messages, []);
    b.fire('touchend');
    b.fire('touchstart', { touches: point(100), composedPath: () => [inner] });
    assert.equal(b.fire('touchmove', { touches: point(100 + dx) }).prevented, true);
    assert.deepEqual(b.messages, ['test-nonce:' + (dx < 0 ? 'history-forward' : 'history-back')]);
  }
});

test('document root with visible overflow scrolls, including small initial touch moves', () => {
  const root = new Element({ left: 100, overflow: 'visible' });
  const b = bridge(root);
  assert.equal(b.fire('wheel', { deltaX: -80, composedPath: () => [] }).prevented, undefined);
  b.fire('touchstart', { touches: point(100) });
  assert.equal(b.fire('touchmove', { touches: point(104) }).prevented, undefined);
  root.scrollLeft = 0;
  assert.equal(b.fire('touchmove', { touches: point(180) }).prevented, undefined);
  assert.deepEqual(b.messages, []);
});
