// WebKit drops WebM BlockAdditional alpha. Decode that elementary stream with
// WebCodecs, then mask the *original* video, retaining its layout and media API.
// No native fetch/IPC: CSP, same-origin and CORS continue to apply.
(() => {
  'use strict';
  const MAX_BYTES = 32 * 1024 * 1024, MAX_PIXELS = 32 * 1024 * 1024;
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
  if (!globalThis.VideoDecoder || !globalThis.EncodedVideoChunk || !HTMLVideoElement.prototype.requestVideoFrameCallback) return;
  const states = new Map(), resources = new Map();
  let pixels = 0, maskBytes = 0;
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
  function dispose(video) {
    const state = states.get(video);
    if (!state) return;
    states.delete(video);
    const resource = state.resource;
    if (resource && --resource.refs === 0) {
      resources.delete(resource.key);
      resource.abort.abort();
      if (resource.decoder?.state !== 'closed') resource.decoder?.close();
      pixels -= resource.pixels;
      maskBytes -= resource.bytes;
    }
    if (state.callback) video.cancelVideoFrameCallback(state.callback);
    if (state.applied) {
      for (const [name, value, priority] of state.styles) {
        if (value) video.style.setProperty(name, value, priority);
        else video.style.removeProperty(name);
      }
    }
  }
  async function loadMasks(resource, url, credentials) {
    const track = demux(await read(url, resource.abort.signal, credentials));
    if (!track || resource.abort.signal.aborted) return null;
    const amount = track.frames.length * track.width * track.height;
    if (pixels + amount > 4 * MAX_PIXELS) throw Error('Document alpha budget');
    pixels += amount; resource.pixels = amount;
    const config = {codec: track.codec, codedWidth: track.width, codedHeight: track.height};
    if (!(await VideoDecoder.isConfigSupported(config)).supported) throw Error('Alpha codec unavailable');
    if (resource.abort.signal.aborted) return null;
    const canvas = document.createElement('canvas');
    canvas.width = track.width; canvas.height = track.height;
    const ctx = canvas.getContext('2d', {willReadFrequently: true});
    const masks = []; let failure;
    const decoder = resource.decoder = new VideoDecoder({
      error(error) { failure = error; },
      output(frame) {
        try {
          ctx.drawImage(frame, 0, 0);
          const image = ctx.getImageData(0, 0, canvas.width, canvas.height);
          for (let i = 0; i < image.data.length; i += 4) image.data[i + 3] = image.data[i];
          ctx.putImageData(image, 0, 0);
          const mask = canvas.toDataURL();
          if (maskBytes + mask.length * 2 > MAX_BYTES) throw Error('Mask budget');
          resource.bytes += mask.length * 2; maskBytes += mask.length * 2;
          masks.push({time: frame.timestamp / 1000000, mask});
        } catch (error) { failure = error; }
        finally { frame.close(); }
      }
    });
    decoder.configure(config);
    for (let i = 0; i < track.frames.length; i++) {
      if (resource.abort.signal.aborted) return null;
      const frame = track.frames[i];
      decoder.decode(new EncodedVideoChunk({type: frame.key ? 'key' : 'delta', timestamp: frame.timestamp, data: frame.data}));
      if (i % 16 === 15) await new Promise(resolve => setTimeout(resolve, 0));
      if (failure) throw failure;
    }
    await decoder.flush(); decoder.close(); resource.decoder = null;
    if (failure) throw failure;
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
    if (video.controls || getComputedStyle(video).maskImage !== 'none') return;
    const state = {url, styles: [], applied: false};
    states.set(video, state);
    try {
      const credentials = video.crossOrigin === 'use-credentials' ? 'include' : 'same-origin';
      const key = credentials + ':' + url;
      let resource = resources.get(key);
      if (!resource) {
        if (resources.size >= 4) throw Error('Too many alpha resources');
        resource = {key, refs: 0, pixels: 0, bytes: 0, abort: new AbortController()};
        resources.set(key, resource);
        resource.promise = loadMasks(resource, url, credentials);
      }
      resource.refs++; state.resource = resource;
      const decoded = await resource.promise;
      if (states.get(video) !== state) return;
      if (!decoded) {
        dispose(video); states.set(video, {url, styles: [], applied: false}); return;
      }
      const {track, masks} = decoded;
      // Engines that already expose alpha must not have it applied twice.
      const probe = document.createElement('canvas');
      probe.width = track.width; probe.height = track.height;
      const context = probe.getContext('2d', {willReadFrequently: true});
      try {
        context.drawImage(video, 0, 0);
        const rgba = context.getImageData(0, 0, probe.width, probe.height).data;
        for (let i = 3; i < rgba.length; i += 4) if (rgba[i] < 255) {
          dispose(video); states.set(video, {url, styles: [], applied: false}); return;
        }
      } catch { /* A cross-origin native video can be tainted even when the alpha fetch passed CORS. */ }
      const properties = ['mask-image', 'mask-size', 'mask-position', 'mask-repeat', 'mask-mode'];
      state.styles = properties.map(name => [name, video.style.getPropertyValue(name), video.style.getPropertyPriority(name)]);
      let last = -1;
      function paint(time) {
        if (video.controls) { dispose(video); return; }
        let index = 0;
        for (let i = 1; i < masks.length && masks[i].time <= time + 0.001; i++) index = i;
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
        if (last !== index) video.style.setProperty('mask-image', `url("${masks[index].mask}")`);
        last = index; state.applied = true;
      }
      state.paint = paint;
      const tick = (_, metadata) => {
        if (states.get(video) !== state) return;
        paint(metadata.mediaTime);
        if (states.get(video) === state) state.callback = video.requestVideoFrameCallback(tick);
      };
      paint(video.currentTime);
      if (states.get(video) === state) state.callback = video.requestVideoFrameCallback(tick);
    } catch {
      // Unsupported, inaccessible or malformed sources retain native playback.
      if (states.get(video) === state) {
        dispose(video);
        states.set(video, {url, styles: [], applied: false});
      }
    }
  }
  document.addEventListener('loadeddata', event => { if (event.target instanceof HTMLVideoElement) void attach(event.target); }, true);
  document.addEventListener('seeked', event => states.get(event.target)?.paint?.(event.target.currentTime), true);
  document.addEventListener('emptied', event => dispose(event.target), true);
  new MutationObserver(() => {
    for (const video of states.keys()) if (!video.isConnected || video.controls) dispose(video);
    for (const video of document.querySelectorAll('video')) if (video.readyState >= 2) void attach(video);
  }).observe(document, {subtree: true, childList: true, attributes: true, attributeFilter: ['src', 'controls']});
  addEventListener('pagehide', () => { for (const video of states.keys()) dispose(video); });
  addEventListener('pageshow', () => { for (const video of document.querySelectorAll('video')) if (video.readyState >= 2) void attach(video); });
})();
