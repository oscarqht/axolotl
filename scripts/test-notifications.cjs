const { readFileSync } = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const { test } = require('node:test');
const source = readFileSync(require('node:path').join(__dirname, '../crates/desktop/assets/notifications.js'), 'utf8');
function setup(native) {
  const messages = [];
  const window = { Notification: native, ipc: {postMessage: m => messages.push(m)} };
  vm.runInNewContext(source.replace('__NONCE__', 'secret'), { window, EventTarget, Event, queueMicrotask });
  return { Notification: window.Notification, messages };
}
test('missing native API supplies usable badge Notification and permission callback/promise', async () => {
  const { Notification, messages } = setup();
  assert.equal(Notification.permission, 'granted');
  let permission;
  assert.equal(await Notification.requestPermission(p => permission = p), 'granted');
  assert.equal(permission, 'granted');
  const n = new Notification('Inbox', {body: 'Message', data: {id: 3}});
  assert.equal(n.body, 'Message');
  assert.equal(n.data.id, 3);
  let shown = 0, closed = 0;
  n.onshow = () => shown++;
  n.addEventListener('close', () => closed++);
  await Promise.resolve();
  n.close(); n.close();
  assert.equal(shown, 1);
  assert.equal(closed, 1);
  assert.deepEqual(messages, ['secret:notification']);
});
test('granted native delivery is forwarded exactly once; native close events still work', () => {
  let delivered = 0;
  class Native extends EventTarget {
    static permission = 'granted';
    constructor(title, options) { super(); delivered++; assert.equal(title, 'Inbox'); assert.equal(options.body, 'Message'); }
    close() { this.dispatchEvent(new Event('close')); }
  }
  const b = setup(Native), n = new b.Notification('Inbox', {body:'Message'});
  let closed = 0;
  n.onclose = () => closed++;
  n.close();
  assert.equal(closed, 1);
  assert.equal(delivered, 1);
  assert.equal(b.messages.length, 1);
});
test('denied/default permission and throwing constructors fall back to badges without OS prompts', () => {
  for (const permission of ['denied', 'default', 'granted']) {
    class Native { static permission = permission; constructor() { throw Error('unavailable'); } }
    const b = setup(Native);
    new b.Notification('one'); new b.Notification('two');
    assert.deepEqual(b.messages, ['secret:notification', 'secret:notification']);
  }
});
