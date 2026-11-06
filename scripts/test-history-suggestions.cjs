// Exercise the exact floating history script shipped to the native webview.
const { readFileSync } = require('node:fs');
const { join } = require('node:path');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const { test } = require('node:test');
const source = readFileSync(join(__dirname, '../crates/desktop/assets/floating-history.js'), 'utf8');
const items = [
  { title: 'First', url: 'https://example.test/first' },
  { title: 'Second', url: 'https://example.test/second' },
];

function history() {
  const messages = [], classes = new Set();
  function element() {
    return {
      attributes: {}, children: [], hidden: true, value: '', isConnected: true,
      setAttribute(name, value) { this.attributes[name] = value; },
      removeAttribute(name) { delete this.attributes[name]; },
      append(...nodes) { this.children.push(...nodes); },
      replaceChildren(...nodes) { this.children = nodes; },
      querySelector() { return this.children.find(node => node.attributes['aria-selected'] === 'true'); },
      scrollIntoView() {},
    };
  }
  const list = element(), input = element(), window = {};
  const context = vm.createContext({ window, send: message => messages.push(JSON.parse(JSON.stringify(message))), document: {
    querySelector: () => list, createElement: element,
    body: { classList: { toggle(name, enabled) { enabled ? classes.add(name) : classes.delete(name); } } },
  } });
  vm.runInContext(source, context);
  context.input = input;
  vm.runInContext("bindHistory(input, 'tab')", context);
  function type(value) { input.value = value; input.oninput(); }
  function key(key, options = {}) {
    context.event = { key, preventDefault() { this.prevented = true; }, stopPropagation() {}, ...options };
    return vm.runInContext("historyKey(event, input, 'tab')", context);
  }
  type('first');
  window.updateHistory('first', items);
  messages.length = 0;
  return { messages, list, input, window, type, key, classes };
}

test('typing retains visible results without a native close/reopen event', () => {
  const h = history(), rows = h.list.children;
  h.type('second');
  assert.equal(h.list.hidden, false);
  assert.equal(h.classes.has('history'), true);
  assert.equal(h.input.attributes['aria-expanded'], 'true');
  assert.equal(h.list.children, rows);
  assert.deepEqual(h.messages, [{ type: 'history-query', tab: 'tab', value: 'second' }]);
  h.window.updateHistory('second', [items[1]]);
  assert.equal(h.list.hidden, false);
  assert.equal(h.list.children.length, 1);
  assert.equal(h.list.children[0].children[0].textContent, 'Second');
});

test('stale searches cannot replace retained results', () => {
  const h = history(), rows = h.list.children;
  h.type('second');
  h.window.updateHistory('first', []);
  assert.equal(h.list.children, rows);
  assert.equal(h.list.hidden, false);
});

test('empty and whitespace input close immediately and reject pending results', () => {
  for (const value of ['', '   ']) {
    const h = history();
    h.type('second');
    h.type(value);
    assert.equal(h.list.hidden, true);
    assert.equal(h.input.attributes['aria-expanded'], 'false');
    h.window.updateHistory('second', items);
    h.window.updateHistory(value, items);
    assert.equal(h.list.hidden, true);
  }
});

test('completed search with no matches closes the panel', () => {
  const h = history();
  h.type('missing');
  assert.equal(h.list.hidden, false);
  h.window.updateHistory('missing', []);
  assert.equal(h.list.hidden, true);
  assert.equal(h.input.attributes['aria-expanded'], 'false');
});

test('Escape rejects pending results until another input event', () => {
  const h = history();
  h.type('second');
  assert.equal(h.key('Escape'), true);
  h.window.updateHistory('second', items);
  assert.equal(h.list.hidden, true);
  h.type('second');
  h.window.updateHistory('second', items);
  assert.equal(h.list.hidden, false);
});

test('blur, host clearing and a disconnected editor reject delayed results', () => {
  for (const dismiss of [h => h.input.onblur(), h => h.window.clearHistory()]) {
    const h = history();
    h.type('second');
    dismiss(h);
    h.window.updateHistory('second', items);
    assert.equal(h.list.hidden, true);
  }
  const h = history(), rows = h.list.children;
  h.input.isConnected = false;
  h.window.updateHistory('first', []);
  assert.equal(h.list.children, rows);
});

test('retained rows support arrow selection, Enter and click navigation', () => {
  const h = history();
  h.type('second');
  assert.equal(h.key('ArrowUp'), true);
  assert.equal(h.input.attributes['aria-activedescendant'], 'history-1');
  assert.equal(h.key('Enter'), true);
  assert.deepEqual(h.messages.at(-1), { type: 'navigate', tab: 'tab', value: items[1].url });
  assert.equal(h.key('ArrowDown'), true);
  assert.equal(h.input.attributes['aria-activedescendant'], 'history-0');
  h.list.children[1].onclick();
  assert.equal(h.messages.at(-1).value, items[1].url);
  h.window.updateHistory('second', [items[1]]);
  assert.equal(h.input.attributes['aria-activedescendant'], undefined);
  assert.equal(h.key('Enter'), false);
});

test('composition keys are not consumed', () => {
  const h = history();
  assert.equal(h.key('ArrowDown', { isComposing: true }), false);
  assert.equal(h.input.attributes['aria-activedescendant'], undefined);
});
