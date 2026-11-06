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

function bridge() {
  const listeners = {}, messages = [];
  const window = {
    ipc: { postMessage: value => messages.push(value) },
    addEventListener: (type, listener, options) => { listeners[type] = { listener, options }; },
  };
  vm.runInNewContext(source, { window, innerWidth: 800 });
  return {
    messages,
    fire(type, options = {}) {
      const event = { isTrusted: true, ctrlKey: false, deltaMode: 0, deltaX: 0, deltaY: 0,
        touches: [], preventDefault() { this.prevented = true; },
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
