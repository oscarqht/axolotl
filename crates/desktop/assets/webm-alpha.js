// WebKit drops WebM BlockAdditional alpha. Decode that stream with bundled
// libvpx, mask DOM videos, and composite RGBA for canvas/WebGL video uploads.
// No native fetch/IPC: CSP, same-origin and CORS continue to apply.
(() => {
  'use strict';
  const MAX_BYTES = 32 * 1024 * 1024, MAX_PIXELS = 64 * 1024 * 1024;
  function demux(bytes) {
    const data = new Uint8Array(bytes);
    if (data.length > MAX_BYTES) throw Error('WebM too large');
    function vint(p, keep = false) {
      const first = data[p];
      if (!first) throw Error('Invalid EBML integer');
      let n = 1;
      while (!(first & (128 >> (n - 1)))) n++;
      if (n > (keep ? 4 : 8) || p + n > data.length) throw Error('Invalid EBML width');
      let value = keep ? first : first & (255 >> n);
      let unknown = !keep && value === (255 >> n);
      for (let i = 1; i < n; i++) {
        value = value * 256 + data[p + i];
        unknown = unknown && data[p + i] === 255;
      }
      return {n, value, unknown};
    }
    function elements(start, end) {
      const result = [];
      for (let p = start; p < end;) {
        const id = vint(p, true), size = vint(p + id.n), begin = p + id.n + size.n;
        const stop = size.unknown ? end : begin + size.value;
        if (!Number.isSafeInteger(stop) || stop > end || stop < begin) throw Error('Truncated EBML');
        // Only Segment may have an unknown size; other unknown masters need a
        // schema-aware streaming parser and are deliberately left native.
        if (size.unknown && id.value !== 0x18538067) throw Error('Streaming WebM');
        result.push({id: id.value, begin, end: stop});
        if (result.length > 100000) throw Error('Too many EBML elements');
        p = stop;
      }
      return result;
    }
    const children = e => elements(e.begin, e.end);
    const find = (list, id) => list.find(e => e.id === id);
    function uint(e, fallback = 0) {
      if (!e) return fallback;
      if (e.end - e.begin > 6) throw Error('Oversized EBML integer');
      let v = 0;
      for (let i = e.begin; i < e.end; i++) v = v * 256 + data[i];
      return v;
    }
    const segment = find(elements(0, data.length), 0x18538067);
    if (!segment) return null;
    const top = children(segment), tracks = find(top, 0x1654ae6b);
    if (!tracks) return null;
    let track;
    for (const entry of children(tracks).filter(e => e.id === 0xae)) {
      const fields = children(entry), video = find(fields, 0xe0);
      if (!video || uint(find(fields, 0x83)) !== 1) continue;
      const v = children(video);
      if (uint(find(v, 0x53c0)) !== 1) continue;
      const codec = find(fields, 0x86);
      const name = codec && new TextDecoder().decode(data.subarray(codec.begin, codec.end));
      if (!['V_VP8', 'V_VP9'].includes(name)) continue;
      track = {number: uint(find(fields, 0xd7)), width: uint(find(v, 0xb0)), height: uint(find(v, 0xba)), codec: name === 'V_VP8' ? 'vp8' : 'vp09.00.10.08'};
      break;
    }
    if (!track) return null;
    if (!track.width || !track.height || track.width * track.height > 4 * 1024 * 1024) throw Error('Large alpha frame');
    const info = find(top, 0x1549a966);
    const scale = info ? uint(find(children(info), 0x2ad7b1), 1000000) : 1000000;
    const frames = [];
    for (const cluster of top.filter(e => e.id === 0x1f43b675)) {
      const list = children(cluster), time = uint(find(list, 0xe7));
      for (const group of list.filter(e => e.id === 0xa0)) {
        const fields = children(group), block = find(fields, 0xa1), additions = find(fields, 0x75a1);
        if (!block || !additions) continue;
        const num = vint(block.begin), p = block.begin + num.n;
        if (num.value !== track.number) continue;
        if (p + 3 > block.end || (data[p + 2] & 6)) throw Error('Unsupported laced alpha');
        const relative = (data[p] << 8 | data[p + 1]) << 16 >> 16;
        for (const more of children(additions).filter(e => e.id === 0xa6)) {
          const extra = children(more), alpha = find(extra, 0xa5);
          if (!alpha || uint(find(extra, 0xee), 1) !== 1) continue;
          const packet = data.subarray(alpha.begin, alpha.end);
          if (!packet.length) throw Error('Empty alpha packet');
          // Alpha and color keyframes need not coincide. Inspect the alpha header.
          let key;
          if (track.codec === 'vp8') key = !(packet[0] & 1);
          else {
            let bit = 0;
            const read = () => { if (bit >= packet.length * 8) throw Error('VP9 header'); return packet[bit >> 3] >> (7 - (bit++ & 7)) & 1; };
            if ((read() << 1 | read()) !== 2) throw Error('VP9 marker');
            const profile = read() | read() << 1;
            if (profile === 3) read();
            key = !read() && !read();
          }
          frames.push({timestamp: Math.round((time + relative) * scale / 1000), key, data: packet});
          if (frames.length > 600 || frames.length * track.width * track.height > MAX_PIXELS) throw Error('Alpha animation too large');
        }
      }
    }
    return frames.length ? {...track, frames} : null;
  }
  if (typeof module === 'object' && module.exports) { module.exports = {demux}; return; }
  if (typeof alphaCodecs === 'undefined' || !globalThis.WebAssembly) return;
  const states = new WeakMap(), tracked = new Set(), watched = new WeakSet(), resources = new Map();
  const nativeDraw = CanvasRenderingContext2D.prototype.drawImage;
  let pixels = 0, maskBytes = 0, decodeQueue = Promise.resolve();
  function remember(video, state) {
    state.ref = new WeakRef(video); tracked.add(state.ref); states.set(video, state);
  }
  function* videos() {
    for (const ref of tracked) {
      const video = ref.deref();
      if (video) yield video; else tracked.delete(ref);
    }
  }
  function release(resource) {
    if (--resource.refs !== 0) return;
    resources.delete(resource.key); resource.abort.abort();
    pixels -= resource.pixels; maskBytes -= resource.bytes;
    resource.images.clear();
  }
  const finalized = new FinalizationRegistry(release);
  async function read(url, signal, credentials) {
    const response = await fetch(url, {signal, credentials});
    if (!response.ok || +response.headers.get('content-length') > MAX_BYTES) throw Error('WebM fetch');
    const reader = response.body.getReader(), chunks = [];
    let length = 0;
    try {
      for (;;) {
        const {value, done} = await reader.read();
        if (done) break;
        length += value.length;
        if (length > MAX_BYTES) throw Error('WebM too large');
        chunks.push(value);
      }
    } finally { await reader.cancel(); }
    const bytes = new Uint8Array(length);
    let offset = 0;
    for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.length; }
    return bytes;
  }
  function restoreMask(video, state) {
    if (!state.applied) return;
    for (const [name, value, priority] of state.styles) {
      if (value) video.style.setProperty(name, value, priority);
      else video.style.removeProperty(name);
    }
    state.applied = false;
  }
  function dispose(video) {
    const state = states.get(video);
    if (!state) return;
    states.delete(video);
    tracked.delete(state.ref); finalized.unregister(state);
    if (state.resource) release(state.resource);
    if (state.callback) video.cancelVideoFrameCallback(state.callback);
    restoreMask(video, state);
  }
  async function loadMasks(resource, url, credentials) {
    const track = demux(await read(url, resource.abort.signal, credentials));
    if (!track || resource.abort.signal.aborted) return null;
    const amount = track.frames.length * track.width * track.height;
    if (pixels + amount > 4 * MAX_PIXELS) throw Error('Document alpha budget');
    pixels += amount; resource.pixels = amount;
    const codec = track.codec === 'vp8' ? 'vp8' : 'vp9';
    const wasmBinary = Uint8Array.from(atob(alphaWasm[codec]), c => c.charCodeAt(0));
    const decoder = await alphaCodecs[codec]({wasmBinary, videoFormat: {width: track.width, height: track.height, displayWidth: track.width, displayHeight: track.height}});
    const canvas = document.createElement('canvas');
    canvas.width = track.width; canvas.height = track.height;
    const ctx = canvas.getContext('2d');
    const image = ctx.createImageData(track.width, track.height), masks = [];
    decoder.init(() => {});
    try {
      for (let i = 0; i < track.frames.length; i++) {
        if (resource.abort.signal.aborted) return null;
        const frame = track.frames[i], packet = frame.data;
        let valid;
        decoder.processFrame(packet.buffer.slice(packet.byteOffset, packet.byteOffset + packet.byteLength), ok => { valid = ok; });
        const decoded = decoder.frameBuffer;
        if (!valid || !decoded) throw Error('Alpha decode failed');
        const {cropLeft, cropTop} = decoded.format;
        for (let y = 0; y < track.height; y++) {
          for (let x = 0; x < track.width; x++) {
            const offset = (y * track.width + x) * 4;
            image.data[offset] = image.data[offset + 1] = image.data[offset + 2] = 255;
            image.data[offset + 3] = decoded.y.bytes[(y + cropTop) * decoded.y.stride + x + cropLeft];
          }
        }
        ctx.putImageData(image, 0, 0);
        const mask = canvas.toDataURL();
        if (maskBytes + mask.length * 2 > 64 * 1024 * 1024) throw Error('Mask budget');
        resource.bytes += mask.length * 2; maskBytes += mask.length * 2;
        masks.push({time: frame.timestamp / 1000000, mask});
        decoder.recycleFrame(decoded); decoder.frameBuffer = null;
        // Yield between small batches; unrelated page input remains responsive.
        if (i % 4 === 3) await new Promise(resolve => setTimeout(resolve, 0));
      }
    } finally { decoder._ogv_video_decoder_destroy(); }
    if (!masks.length || resource.abort.signal.aborted) return null;
    // A page may prohibit data: images in CSP. Fail before applying a mask
    // rather than making its video invisible with an unloadable CSS image.
    const test = new Image(); test.src = masks[0].mask;
    await test.decode();
    masks.sort((a, b) => a.time - b.time);
    return {track: {width: track.width, height: track.height}, masks};
  }
  async function attach(video) {
    const url = video.currentSrc;
    if (!url || states.get(video)?.url === url) return;
    dispose(video);
    if (!/\.webm(?:[?#]|$)|^data:video\/webm|^blob:/i.test(url) && ![...video.querySelectorAll('source')].some(s => s.src === url && s.type.includes('webm'))) return;
    // Native controls and existing masks must not themselves become masked.
    if (video.isConnected && getComputedStyle(video).maskImage !== 'none') return;
    const state = {url, styles: [], applied: false, connected: video.isConnected, generation: 0};
    remember(video, state);
    try {
      const credentials = video.crossOrigin === 'use-credentials' ? 'include' : 'same-origin';
      const key = credentials + ':' + url;
      let resource = resources.get(key);
      if (!resource) {
        if (resources.size >= 16) throw Error('Too many alpha resources');
        resource = {key, refs: 0, pixels: 0, bytes: 0, images: new Map(), abort: new AbortController()};
        resources.set(key, resource);
        // Serialize decoders so many poses cannot allocate WASM heaps at once.
        resource.promise = decodeQueue.then(() => resource.abort.signal.aborted ? null : loadMasks(resource, url, credentials));
        decodeQueue = resource.promise.then(() => {}, () => {});
      }
      resource.refs++; state.resource = resource; finalized.register(video, resource, state);
      const decoded = await resource.promise;
      if (states.get(video) !== state) return;
      if (!decoded) {
        dispose(video); remember(video, {url, styles: [], applied: false}); return;
      }
      const {track, masks} = decoded;
      // Engines that already expose alpha must not have it applied twice.
      const probe = document.createElement('canvas');
      probe.width = track.width; probe.height = track.height;
      const context = probe.getContext('2d', {willReadFrequently: true});
      try {
        nativeDraw.call(context, video, 0, 0);
        const rgba = context.getImageData(0, 0, probe.width, probe.height).data;
        for (let i = 3; i < rgba.length; i += 4) if (rgba[i] < 255) {
          dispose(video); remember(video, {url, styles: [], applied: false}); return;
        }
      } catch { /* A cross-origin native video can be tainted even when the alpha fetch passed CORS. */ }
      const properties = ['mask-image', 'mask-size', 'mask-position', 'mask-repeat', 'mask-mode'];
      state.styles = properties.map(name => [name, video.style.getPropertyValue(name), video.style.getPropertyPriority(name)]);
      let last = -1;
      function paint(time) {
        let index = 0;
        for (let i = 1; i < masks.length && masks[i].time <= time + 0.001; i++) index = i;
        state.index = index;
        if (video.controls) { restoreMask(video, state); return; }
        if (!video.isConnected) return;
        const css = getComputedStyle(video), fit = css.objectFit;
        let sizing = fit === 'fill' ? '100% 100%' : fit;
        if (fit === 'none' || fit === 'scale-down') {
          const scale = fit === 'none' ? 1 : Math.min(1, video.clientWidth / track.width, video.clientHeight / track.height);
          sizing = `${track.width * scale}px ${track.height * scale}px`;
        }
        video.style.setProperty('mask-size', sizing);
        video.style.setProperty('mask-position', css.objectPosition);
        video.style.setProperty('mask-repeat', 'no-repeat');
        video.style.setProperty('mask-mode', 'alpha');
        if (!state.applied || last !== index) video.style.setProperty('mask-image', `url("${masks[index].mask}")`);
        last = index; state.applied = true;
      }
      state.paint = paint;
      state.decoded = decoded;
      state.compose = time => {
        paint(time);
        if (state.captureTime === time || state.pending || video.readyState < 2) return;
        const index = state.index;
        let entry = resource.images.get(index);
        if (!entry) {
          const image = new Image(); image.src = masks[index].mask;
          entry = {image, ready: image.decode()};
          resource.images.set(index, entry);
          while (resource.images.size > 3) resource.images.delete(resource.images.keys().next().value);
        }
        state.captureTime = time; state.pending = true;
        const generation = state.generation;
        const frame = state.spare || document.createElement('canvas');
        frame.width = track.width; frame.height = track.height;
        const context = frame.getContext('2d');
        // Capture color now, before awaiting PNG decoding, so the matte and
        // color always belong to the same frame even if native playback advances.
        nativeDraw.call(context, video, 0, 0);
        entry.ready.then(() => {
          if (states.get(video) !== state || state.generation !== generation) return;
          context.globalCompositeOperation = 'destination-in';
          nativeDraw.call(context, entry.image, 0, 0);
          context.globalCompositeOperation = 'source-over';
          state.spare = state.output; state.output = frame;
        }).catch(() => {}).finally(() => { if (state.generation === generation) state.pending = false; });
      };
      const tick = (_, metadata) => {
        if (states.get(video) !== state) return;
        state.compose(metadata.mediaTime);
        if (states.get(video) === state) state.callback = video.requestVideoFrameCallback(tick);
      };
      state.compose(video.currentTime);
      if (states.get(video) === state) state.callback = video.requestVideoFrameCallback(tick);
    } catch {
      // Unsupported, inaccessible or malformed sources retain native playback.
      if (states.get(video) === state) {
        dispose(video);
        remember(video, {url, styles: [], applied: false});
      }
    }
  }
  function watch(video) {
    if (watched.has(video)) return;
    watched.add(video);
    video.addEventListener('loadeddata', () => { void attach(video); });
    video.addEventListener('seeked', () => {
      const state = states.get(video);
      if (state) { state.generation++; state.captureTime = undefined; state.pending = false; state.compose?.(video.currentTime); }
    });
    video.addEventListener('emptied', () => dispose(video));
  }
  function source(video) {
    if (!(video instanceof HTMLVideoElement)) return video;
    watch(video);
    if (video.readyState >= 2) void attach(video);
    const state = states.get(video);
    state?.compose?.(video.currentTime);
    return state?.output || video;
  }
  // A detached video never dispatches load events to document. Catch explicit
  // playback and texture uploads, without changing play()'s native promise.
  const nativePlay = HTMLMediaElement.prototype.play;
  HTMLMediaElement.prototype.play = function(...args) {
    if (this instanceof HTMLVideoElement) { watch(this); if (this.readyState >= 2) void attach(this); }
    return Reflect.apply(nativePlay, this, args);
  };
  function wrap(prototype, name, index) {
    if (!prototype?.[name]) return;
    const original = prototype[name];
    prototype[name] = function(...args) {
      if (index === undefined) {
        for (let i = 0; i < args.length; i++) if (args[i] instanceof HTMLVideoElement) args[i] = source(args[i]);
      } else if (args[index] instanceof HTMLVideoElement) args[index] = source(args[index]);
      return Reflect.apply(original, this, args);
    };
  }
  wrap(CanvasRenderingContext2D.prototype, 'drawImage', 0);
  wrap(CanvasRenderingContext2D.prototype, 'createPattern', 0);
  if (globalThis.OffscreenCanvasRenderingContext2D) {
    wrap(OffscreenCanvasRenderingContext2D.prototype, 'drawImage', 0);
    wrap(OffscreenCanvasRenderingContext2D.prototype, 'createPattern', 0);
  }
  for (const type of [globalThis.WebGLRenderingContext, globalThis.WebGL2RenderingContext]) {
    for (const method of ['texImage2D', 'texSubImage2D', 'texImage3D', 'texSubImage3D']) wrap(type?.prototype, method);
  }
  if (globalThis.createImageBitmap) {
    const original = globalThis.createImageBitmap;
    globalThis.createImageBitmap = function(image, ...args) { return Reflect.apply(original, this, [source(image), ...args]); };
  }
  document.addEventListener('loadeddata', event => {
    if (event.target instanceof HTMLVideoElement) { watch(event.target); void attach(event.target); }
  }, true);
  new MutationObserver(() => {
    for (const video of videos()) {
      const state = states.get(video);
      if (state?.connected && !video.isConnected) dispose(video);
      else if (state && video.isConnected) { state.connected = true; state.paint?.(video.currentTime); }
    }
    for (const video of document.querySelectorAll('video')) if (video.readyState >= 2) { watch(video); void attach(video); }
  }).observe(document, {subtree: true, childList: true, attributes: true, attributeFilter: ['src', 'controls']});
  addEventListener('pagehide', () => { for (const video of videos()) dispose(video); });
  addEventListener('pageshow', () => { for (const video of document.querySelectorAll('video')) if (video.readyState >= 2) void attach(video); });
})();
