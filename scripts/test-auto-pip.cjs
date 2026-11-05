// Exercise the exact all-frame script shipped to native webviews; no browser required.
const { readFileSync } = require('node:fs');
const { join } = require('node:path');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const { test } = require('node:test');
const source = readFileSync(join(__dirname, '../crates/desktop/assets/auto-pip.js'), 'utf8');

let frameSequence = 0;
function page({ child = false } = {}) {
  const messages = [], windowListeners = {}, documentListeners = {}, intervals = new Map();
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
    getComputedStyle: element => element.style,
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
  return { window, document, frame, messages, intervals, command, element, video, shadowRoot,
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

test('initializes in insecure contexts with getRandomValues and no randomUUID', () => {
  const p = page();
  assert.equal(typeof p.window.__axolotlAutoPip, 'function');
  const report = p.probe();
  assert.equal(report.frame, p.frame);
  assert.equal(report.area, 0);
});

test('chooses the largest eligible audible video by its visible viewport area', () => {
  const p = page();
  p.video({ rect: { left: -900, top: 0, right: 100, bottom: 800 } });
  const largest = p.video({ rect: { left: 50, top: 50, right: 850, bottom: 650 }, videoWidth: 1920, videoHeight: 1080 });
  p.video({ rect: { left: 0, top: 0, right: 300, bottom: 300 } });
  const result = p.probe();
  assert.equal(result.area, 800 * 600);
  assert.equal(result.aspect, 1920 / 1080);
  assert.equal(result.existing, false);
  p.select();
  assert.equal(marked(largest), 'target');
  assert.equal(largest.controls, true);
  assert.equal(p.messages.at(-1).kind, 'ready');
});

test('excludes muted, silent, paused, ended, disabled, unready, and hidden videos', () => {
  for (const options of [
    { muted: true }, { volume: 0 }, { paused: true }, { ended: true },
    { disablePictureInPicture: true }, { readyState: 1 }, { videoWidth: 0 },
    { webkitAudioDecodedByteCount: 0 }, { webkitAudioDecodedByteCount: undefined },
    { style: { display: 'none', visibility: 'visible', opacity: '1' } },
    { style: { display: 'block', visibility: 'hidden', opacity: '1' } },
    { style: { display: 'block', visibility: 'visible', opacity: '0' } },
    { rect: { left: 1100, top: 0, right: 1500, bottom: 400 } },
    { srcObject: { getAudioTracks: () => [] } },
    { srcObject: { getAudioTracks: () => [{ enabled: false, readyState: 'live' }] } },
    { srcObject: { getAudioTracks: () => [{ enabled: true, readyState: 'ended' }] } },
    { audioTracks: [{ enabled: false }] },
    { webkitAudioDecodedByteCount: undefined, mozHasAudio: false },
  ]) {
    const p = page(); p.video(options);
    assert.equal(p.probe().area, 0, JSON.stringify(options));
    p.select();
    assert.equal(p.messages.at(-1).kind, 'stopped');
    assert.equal(p.styles().length, 0);
  }
});

test('accepts enabled audio tracks, live audio streams, and positive engine audio evidence', () => {
  for (const options of [
    { audioTracks: [{ enabled: true }] },
    { srcObject: { getAudioTracks: () => [{ enabled: true, readyState: 'live' }] } },
    { webkitAudioDecodedByteCount: undefined, mozHasAudio: true },
    { webkitAudioDecodedByteCount: 10 },
  ]) {
    const p = page(); p.video(options);
    assert.ok(p.probe().area > 0);
  }
});

test('restore preserves page controls and preexisting attributes and removes injected styles', () => {
  for (const controls of [false, true]) {
    const p = page(), wrapper = p.element('section'), video = p.video({ controls }, wrapper);
    wrapper.setAttribute('data-axolotl-pip', 'original-branch');
    video.setAttribute('data-axolotl-pip', 'original-video');
    const pageStyle = p.element('style', p.document.head);
    pageStyle.textContent = 'video{border:1px solid red}';
    p.probe(); p.select();
    assert.equal(marked(wrapper), 'branch');
    assert.equal(marked(video), 'target');
    assert.equal(video.controls, true);
    assert.equal(p.styles().length, 2);
    assert.equal(p.intervals.size, 1);
    p.command('restore');
    assert.equal(marked(wrapper), 'original-branch');
    assert.equal(marked(video), 'original-video');
    assert.equal(marked(p.document.body), null);
    assert.equal(marked(p.document.documentElement), null);
    assert.equal(video.controls, controls);
    assert.deepEqual(p.styles(), [pageStyle]);
    assert.equal(pageStyle.textContent, 'video{border:1px solid red}');
    assert.equal(p.intervals.size, 0);
  }
});

test('stale restore cannot clear a newer selection', () => {
  const p = page(), video = p.video();
  p.probe('old'); p.select('old');
  p.probe('new'); p.select('new');
  p.command('restore', 'old');
  assert.equal(marked(video), 'target');
  assert.equal(video.controls, true);
  assert.equal(p.styles().length, 1);
  assert.equal(p.intervals.size, 1);
  p.command('restore', 'new');
  assert.equal(marked(video), null);
  assert.equal(video.controls, false);
  assert.equal(p.styles().length, 0);
});

test('relays commands and ready through cross-origin frames without reading their document', () => {
  const parent = page(), child = page({ child: true });
  const video = child.video(), { frame, downward, upward } = connect(parent, child);
  parent.probe('nested');
  assert.equal(child.messages.length, 0, 'Child candidates must route through their ancestors');
  assert.equal(parent.messages.at(-1).kind, 'candidate');
  assert.equal(parent.messages.at(-1).frame, child.id());
  assert.ok(parent.messages.at(-1).area > 0);
  parent.command('select', 'nested', { frame: child.id() });
  assert.equal(marked(video), 'target');
  assert.equal(marked(frame), 'target');
  assert.equal(upward.at(-1).data.kind, 'selected');
  assert.equal(upward.at(-1).origin, '*');
  assert.deepEqual(parent.messages.at(-1), { kind: 'ready', request: 'nested', frame: child.id() });
  assert.ok(downward.every(message => message.data.axolotlPip === 'secret' && message.origin === '*'));
  parent.command('restore', 'nested');
  assert.equal(marked(video), null); assert.equal(marked(frame), null);
  assert.equal(parent.styles().length, 0); assert.equal(child.styles().length, 0);
});

test('children without IPC relay discovery, ready, external PiP, and stopped through multiple ancestors', () => {
  const top = page(), middle = page({ child: true }), child = page({ child: true });
  const video = child.video(); connect(top, middle); connect(middle, child);
  assert.equal(child.window.ipc, undefined); assert.equal(middle.window.ipc, undefined);
  top.probe('no-child-ipc');
  const report = top.messages.find(message => message.frame === child.id());
  assert.ok(report.area > 0);
  top.command('select', 'no-child-ipc', { frame: child.id() });
  assert.deepEqual(top.messages.at(-1), { kind: 'ready', request: 'no-child-ipc', frame: child.id() });
  child.document.pictureInPictureElement = video;
  child.documentEvent('enterpictureinpicture');
  assert.deepEqual(top.messages.at(-1), { kind: 'external', frame: child.frame, existing: true });
  child.document.pictureInPictureElement = null;
  child.documentEvent('leavepictureinpicture');
  assert.equal(top.messages.at(-1).existing, false);
  video.ended = true;
  for (const tick of child.intervals.values()) tick();
  assert.deepEqual(top.messages.at(-1), { kind: 'stopped', request: 'no-child-ipc', frame: child.frame });
  assert.equal(middle.messages.length, 0); assert.equal(child.messages.length, 0);
  top.command('restore', 'no-child-ipc');
  assert.equal(marked(video), null); assert.equal(video.controls, false);
});

test('native child relay accepts only external and stopped messages from discovered frames', () => {
  const top = page(), child = page({ child: true });
  const { frame } = connect(top, child);
  for (const value of [
    { kind: 'candidate', request: 'x', frame: child.id(), area: 999999, aspect: 1, existing: false },
    { kind: 'ready', request: 'x', frame: child.id() },
    { kind: 'external', frame: child.frame, existing: 'true' },
    { kind: 'stopped', frame: child.frame },
  ]) top.event('message', { source: frame.contentWindow,
    data: { axolotlPip: 'secret', kind: 'native', value } });
  assert.equal(top.messages.length, 0);
  top.event('message', { source: {}, data: { axolotlPip: 'secret', kind: 'native',
    value: { kind: 'external', frame: child.frame, existing: true } } });
  assert.equal(top.messages.length, 0);
});

test('removed iframe ancestors report stopped even when their child can no longer send pagehide', () => {
  const top = page(), child = page({ child: true }), video = child.video();
  const { frame } = connect(top, child);
  top.probe('removed'); top.command('select', 'removed', { frame: child.id() });
  assert.equal(top.intervals.size, 1);
  frame.isConnected = false; frame.remove();
  for (const tick of top.intervals.values()) tick();
  assert.deepEqual(top.messages.at(-1), { kind: 'stopped', request: 'removed', frame: top.frame });
  assert.equal(video.ended, false);
  top.command('restore', 'removed');
  assert.equal(top.styles().length, 0); assert.equal(top.intervals.size, 0);
});

test('scales child viewport area into a tiny iframe so a visible top-level video wins', () => {
  const parent = page(), child = page({ child: true });
  const topVideo = parent.video({ rect: { left: 0, top: 0, right: 400, bottom: 300 } });
  child.video({ rect: { left: 0, top: 0, right: 1000, bottom: 800 }, videoWidth: 1920, videoHeight: 1080 });
  connect(parent, child, { rect: { left: 500, top: 0, right: 600, bottom: 80 } });
  parent.probe('sizes');
  const reports = parent.messages.filter(message => message.kind === 'candidate');
  const top = reports.find(message => message.frame === parent.id());
  const nested = reports.find(message => message.frame === child.id());
  assert.equal(top.area, 400 * 300);
  assert.equal(nested.area, 100 * 80);
  assert.equal(nested.aspect, 1920 / 1080);
  assert.ok(top.area > nested.area);
  parent.command('select', 'sizes', { frame: top.frame });
  assert.equal(marked(topVideo), 'target');
});

test('clips child candidate rectangles against iframe placement and every ancestor viewport', () => {
  const top = page(), middle = page({ child: true }), child = page({ child: true });
  child.video({ rect: { left: 500, top: 200, right: 1000, bottom: 800 } });
  connect(top, middle, { rect: { left: -200, top: 100, right: 800, bottom: 900 } });
  connect(middle, child, { rect: { left: 600, top: 400, right: 1600, bottom: 1200 } });
  top.probe('clipped');
  const report = top.messages.find(message => message.frame === child.id());
  // The video's left edge is 1100 in the middle frame, outside its viewport.
  assert.equal(report.area, 0);
  assert.equal(middle.messages.length, 0); assert.equal(child.messages.length, 0);
  const nestedVideo = child.document.querySelectorAll('video')[0];
  nestedVideo.rect = { left: 0, top: 0, right: 1000, bottom: 800 };
  top.probe('visible-corner');
  const corner = top.messages.find(message => message.frame === child.id() && message.request === 'visible-corner');
  // Middle iframe clips to x=600..1000,y=400..800; top maps that to
  // x=400..800,y=500..900, then clips to its y=800 viewport edge.
  assert.equal(corner.area, 400 * 300);
  assert.equal(corner.frame, child.id());
});

test('reports every local video and selects the smaller one that survives ancestor clipping', () => {
  const top = page(), child = page({ child: true });
  const locallyLargest = child.video({ rect: { left: 500, top: 0, right: 1000, bottom: 800 } });
  const visibleCorner = child.video({ rect: { left: 0, top: 0, right: 300, bottom: 300 } });
  connect(top, child, { rect: { left: 800, top: 0, right: 1800, bottom: 800 } });
  top.probe('all-videos');
  const reports = top.messages.filter(message => message.kind === 'candidate');
  const largest = reports.find(message => message.frame === child.id(0));
  const corner = reports.find(message => message.frame === child.id(1));
  assert.equal(largest.area, 0);
  assert.equal(corner.area, 200 * 300);
  top.command('select', 'all-videos', { frame: corner.frame });
  assert.equal(marked(visibleCorner), 'target');
  assert.equal(marked(locallyLargest), null);
  assert.equal(visibleCorner.controls, true);
  assert.equal(locallyLargest.controls, false);
  assert.deepEqual(top.messages.at(-1), { kind: 'ready', request: 'all-videos', frame: child.id(1) });
  top.command('restore', 'all-videos');
  assert.equal(marked(visibleCorner), null);
  assert.equal(visibleCorner.controls, false);
});

test('stale selects and delayed child selections cannot restyle canceled or newer requests', () => {
  const top = page(), child = page({ child: true }), video = child.video();
  const { frame } = connect(top, child);
  top.probe('old');
  top.command('select', 'old', { frame: child.id() });
  top.command('restore', 'old');
  const lateSelected = { data: { axolotlPip: 'secret', kind: 'selected', request: 'old', frame: child.id() },
    source: frame.contentWindow };
  top.event('message', lateSelected);
  assert.equal(marked(frame), null); assert.equal(top.styles().length, 0);
  top.command('select', 'old', { frame: child.id() });
  assert.equal(marked(video), null); assert.equal(child.styles().length, 0);
  top.probe('new');
  top.event('message', lateSelected);
  top.command('select', 'old', { frame: child.id() });
  assert.equal(marked(frame), null); assert.equal(marked(video), null);
  top.command('select', 'new', { frame: child.id() });
  assert.equal(marked(frame), 'target'); assert.equal(marked(video), 'target');
  top.command('restore', 'old');
  assert.equal(marked(frame), 'target'); assert.equal(marked(video), 'target');
  top.command('restore', 'new');
  assert.equal(marked(frame), null); assert.equal(marked(video), null);
});

test('hidden and offscreen ancestor iframes exclude candidates but preserve native PiP reports', () => {
  for (const frameOptions of [
    { style: { display: 'none', visibility: 'visible', opacity: '1' } },
    { style: { display: 'block', visibility: 'hidden', opacity: '1' } },
    { style: { display: 'block', visibility: 'visible', opacity: '0' } },
    { rect: { left: 1200, top: 0, right: 1800, bottom: 400 } },
  ]) {
    const top = page(), middle = page({ child: true }), child = page({ child: true });
    const video = child.video(); child.document.pictureInPictureElement = video;
    connect(top, middle, frameOptions); connect(middle, child);
    top.probe();
    const report = top.messages.find(message => message.frame === child.id());
    assert.equal(report.area, 0); assert.equal(report.existing, true);
    assert.equal(report.aspect, 640 / 360);
  }
  const top = page(), child = page({ child: true }), wrapper = top.element('section');
  wrapper.style.opacity = '0'; child.video();
  connect(top, child, { parent: wrapper });
  top.probe();
  assert.equal(top.messages.find(message => message.frame === child.id()).area, 0);
});

test('selection rechecks videos disabled or hidden after probing', () => {
  for (const mutate of [
    video => { video.disablePictureInPicture = true; },
    video => { video.style.display = 'none'; },
    video => { video.rect = { left: 1100, top: 0, right: 1500, bottom: 400 }; },
  ]) {
    const p = page(), video = p.video(); p.probe(); mutate(video); p.select();
    assert.equal(p.messages.at(-1).kind, 'stopped');
    assert.equal(marked(video), null); assert.equal(p.styles().length, 0);
  }
  const top = page(), child = page({ child: true }), video = child.video();
  const { frame } = connect(top, child); top.probe(); frame.style.display = 'none';
  top.command('select', 'request', { frame: child.id() });
  assert.equal(top.messages.at(-1).kind, 'stopped');
  assert.equal(marked(frame), null);
  top.command('restore');
  assert.equal(marked(video), null);
});

test('discovers shadow videos and restores styles and branch attributes in every tree', () => {
  const p = page(), host = p.element('video-player'), root = p.shadowRoot(host);
  const video = p.video({}, root);
  host.setAttribute('data-axolotl-pip', 'page-value');
  p.probe(); p.select();
  assert.equal(marked(video), 'target');
  assert.equal(marked(host), 'branch');
  assert.equal(root.querySelectorAll('style').length, 1);
  assert.equal(p.styles().length, 1);
  p.command('restore');
  assert.equal(marked(video), null);
  assert.equal(marked(host), 'page-value');
  assert.equal(root.querySelectorAll('style').length, 0);
  assert.equal(p.styles().length, 0);
  assert.equal(video.controls, false);
});

test('ignores unauthenticated and unrelated frame messages', () => {
  const p = page(), frame = p.element('iframe'); frame.contentWindow = {};
  for (const event of [
    { data: { axolotlPip: 'wrong', kind: 'selected', request: 'x' }, source: frame.contentWindow },
    { data: { axolotlPip: 'secret', kind: 'selected', request: 'x' }, source: {} },
  ]) p.event('message', event);
  assert.equal(marked(frame), null); assert.equal(p.messages.length, 0);
});

test('preserves existing native PiP including PiP started after probing', () => {
  for (const mode of ['standard', 'webkit']) {
    for (const startBeforeProbe of [true, false]) {
      const p = page(), video = p.video();
      const start = () => {
        if (mode === 'standard') p.document.pictureInPictureElement = video;
        else video.webkitPresentationMode = 'picture-in-picture';
      };
      if (startBeforeProbe) start();
      assert.equal(p.probe().existing, startBeforeProbe);
      if (!startBeforeProbe) start();
      p.select();
      assert.equal(marked(video), null); assert.equal(video.controls, false);
      assert.equal(p.styles().length, 0);
      assert.equal(p.messages.at(-1).kind, 'stopped');
      p.documentEvent(mode === 'standard' ? 'enterpictureinpicture' : 'webkitpresentationmodechanged');
      assert.equal(p.messages.at(-1).existing, true);
    }
  }
});

test('lifecycle reports source changes and page unload restores page state', () => {
  const p = page(), video = p.video();
  p.probe(); p.select();
  video.currentSrc = 'https://example.com/replacement.mp4';
  for (const tick of p.intervals.values()) tick();
  assert.equal(p.messages.at(-1).kind, 'stopped');
  p.event('pagehide');
  assert.equal(p.messages.at(-1).kind, 'external');
  assert.equal(p.messages.at(-1).existing, false);
  assert.equal(marked(video), null); assert.equal(video.controls, false);
  assert.equal(p.styles().length, 0); assert.equal(p.intervals.size, 0);
});
