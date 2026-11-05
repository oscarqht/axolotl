// Exercise the exact all-frame script shipped to native webviews; no browser required.
const { readFileSync } = require('node:fs');
const { join } = require('node:path');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const source = readFileSync(join(__dirname, '../crates/desktop/assets/auto-pip.js'), 'utf8');

let frameSequence = 0;
function page({ child = false } = {}) {
  const messages = [], windowListeners = {}, documentListeners = {}, intervals = new Map(), observers = new Set();
  class MutationObserver {
    constructor(callback) { this.callback = callback; this.active = false; observers.add(this); }
    observe(target, options) { this.target = target; this.options = options; this.active = true; }
    disconnect() { this.active = false; }
    takeRecords() { return []; }
  }
  let nextInterval = 0;
  class Element {
    constructor(tag) {
      this.tagName = tag; this.children = []; this.attributes = new Map();
      this.style = { display: 'block', visibility: 'visible', opacity: '1' };
      this.isConnected = true;
      this.rect = { left: 0, top: 0, right: 1000, bottom: 800 };
    }
    appendChild(child) { child.parentElement = this; child.container = this; this.children.push(child); return child; }
    remove() {
      const parent = this.container;
      if (parent) parent.children.splice(parent.children.indexOf(this), 1);
      this.parentElement = this.container = null;
    }
    getRootNode() { return this.parentElement ? this.parentElement.getRootNode() : this; }
    getAttribute(name) { return this.attributes.has(name) ? this.attributes.get(name) : null; }
    setAttribute(name, value) { this.attributes.set(name, value); }
    removeAttribute(name) { this.attributes.delete(name); }
    querySelectorAll(query) {
      const descendants = this.children.flatMap(child => [child, ...child.querySelectorAll('*')]);
      return query === '*' ? descendants : descendants.filter(el => query.split(',').includes(el.tagName));
    }
    getBoundingClientRect() { return this.rect; }
  }
  const document = new Element('#document');
  document.documentElement = document.appendChild(new Element('html'));
  document.head = document.documentElement.appendChild(new Element('head'));
  document.body = document.documentElement.appendChild(new Element('body'));
  document.createElement = tag => new Element(tag);
  document.addEventListener = (name, listener) => { documentListeners[name] = listener; };
  // Real document elements stop parentElement traversal at <html>.
  document.documentElement.parentElement = null;
  document.documentElement.getRootNode = () => document;
  const sequence = ++frameSequence, frame = `${sequence}-0-0-0`;
  const window = {
    ipc: { postMessage: raw => {
      assert.ok(raw.startsWith('secret:pip:'));
      messages.push(JSON.parse(raw.slice('secret:pip:'.length)));
    } },
    addEventListener: (name, listener) => { windowListeners[name] = listener; },
  };
  window.top = window.parent = child ? {} : window;
  if (child) delete window.ipc;
  vm.runInNewContext(source.replace('__NONCE__', 'secret'), {
    window, document, crypto: { getRandomValues: values => { values[0] = sequence; return values; } }, innerWidth: 1000, innerHeight: 800,
    getComputedStyle: element => element.style, MutationObserver, queueMicrotask,
    setInterval: callback => { intervals.set(++nextInterval, callback); return nextInterval; },
    clearInterval: id => intervals.delete(id),
  });
  const command = (kind, request = 'request', extra = {}) => window.__axolotlAutoPip({ kind, request, ...extra });
  function element(tag, parent = document.body) { return parent.appendChild(new Element(tag)); }
  function video(options = {}, parent = document.body) {
    return Object.assign(element('video', parent), {
      muted: false, volume: 1, paused: false, ended: false, readyState: 4,
      videoWidth: 640, videoHeight: 360, webkitAudioDecodedByteCount: 100,
      controls: false, currentSrc: 'https://example.com/movie.mp4', srcObject: null,
      rect: { left: 0, top: 0, right: 640, bottom: 360 }, ...options,
    });
  }
  function shadowRoot(host) {
    const root = new Element('#shadow-root'); root.host = host; host.shadowRoot = root;
    root.appendChild = child => {
      Element.prototype.appendChild.call(root, child);
      child.parentElement = null; child.getRootNode = () => root;
      return child;
    };
    return root;
  }
  return { window, document, frame, messages, intervals, command, element, video, shadowRoot, observers,
    mutate: (records = [{ type: 'childList', target: document.body }]) => {
      for (const observer of observers) if (observer.active) observer.callback(records, observer);
    },
    event: (name, event = {}) => windowListeners[name](event),
    documentEvent: name => documentListeners[name](),
    styles: () => document.querySelectorAll('style'),
    id: (index = 0) => `${frame}:${index}`,
    probe: (request = 'request') => {
      command('probe', request);
      return messages.filter(message => message.kind === 'candidate' && message.request === request)
        .sort((a, b) => b.area - a.area)[0];
    },
    select: (request = 'request') => {
      const best = messages.filter(message => message.kind === 'candidate' && message.request === request)
        .sort((a, b) => b.area - a.area)[0];
      command('select', request, { frame: best?.frame || frame });
    },
  };
}

function marked(element) { return element.getAttribute('data-axolotl-pip'); }

function connect(parent, child, options = {}) {
  const frame = parent.element('iframe', options.parent || parent.document.body);
  Object.assign(frame, options);
  const downward = [], upward = [];
  const parentProxy = { postMessage: (data, origin) => {
    upward.push({ data, origin }); parent.event('message', { data, source: childProxy });
  } };
  const childProxy = { postMessage: (data, origin) => {
    downward.push({ data, origin }); child.event('message', { data, source: parentProxy });
  } };
  Object.defineProperty(childProxy, 'document', { get() { throw Error('cross origin'); } });
  frame.contentWindow = childProxy;
  child.window.parent = parentProxy; child.window.top = parent.window.top;
  return { frame, downward, upward };
}

module.exports = { page, marked, connect };
