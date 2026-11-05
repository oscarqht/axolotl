// Installed in every frame. Keep the original video/stream in its document;
// the native host moves this webview into a floating window after selection.
(() => {
  const nonce = '__NONCE__';
  // Wry exposes IPC only in the main WKWebView frame. Child frames must
  // communicate through their ancestors rather than reading window.ipc.
  const post = window === window.top ? window.ipc.postMessage.bind(window.ipc) : null;
  // getRandomValues also works in insecure contexts such as about:blank.
  const frameId = [...crypto.getRandomValues(new Uint32Array(4))].join('-');
  let candidates = new Map(), currentRequest = null, selection = null, timer = null;
  const send = value => {
    if (post) post(nonce + ':pip:' + JSON.stringify(value));
    else window.parent.postMessage({axolotlPip:nonce, kind:'native', value}, '*');
  };
  const roots = () => {
    const result = [document];
    for (let i = 0; i < result.length; i++)
      for (const el of result[i].querySelectorAll('*')) if (el.shadowRoot) result.push(el.shadowRoot);
    return result;
  };
  const frames = () => roots().flatMap(root => [...root.querySelectorAll('iframe,frame')]);
  const nativePip = () => roots().some(root => root.pictureInPictureElement ||
    [...root.querySelectorAll('video')].some(v => v.webkitPresentationMode === 'picture-in-picture'));
  const audible = v => {
    if (v.muted || v.volume <= 0 || v.paused || v.ended || v.readyState < 2 || !v.videoWidth) return false;
    if (v.srcObject?.getAudioTracks) return v.srcObject.getAudioTracks().some(t => t.enabled && t.readyState === 'live');
    if (v.audioTracks?.length) return [...v.audioTracks].some(t => t.enabled);
    if (typeof v.webkitAudioDecodedByteCount === 'number') return v.webkitAudioDecodedByteCount > 0;
    if (typeof v.mozHasAudio === 'boolean') return v.mozHasAudio;
    return false; // Unknown audio is not evidence of an audible video.
  };
  const emptyRect = () => ({left:0, top:0, right:0, bottom:0});
  const rectArea = r => Math.max(0, r.right - r.left) * Math.max(0, r.bottom - r.top);
  const clip = (r, bounds) => ({left:Math.max(r.left, bounds.left), top:Math.max(r.top, bounds.top),
    right:Math.max(Math.max(r.left, bounds.left), Math.min(r.right, bounds.right)),
    bottom:Math.max(Math.max(r.top, bounds.top), Math.min(r.bottom, bounds.bottom))});
  const viewport = () => ({width:innerWidth, height:innerHeight});
  const visibleRect = el => {
    for (let node = el; node; node = node.parentElement || node.getRootNode().host) {
      const s = getComputedStyle(node);
      if (s.display === 'none' || s.visibility !== 'visible' || Number(s.opacity) === 0) return emptyRect();
    }
    return clip(el.getBoundingClientRect(), {left:0, top:0, right:innerWidth, bottom:innerHeight});
  };
  const area = el => rectArea(visibleRect(el));
  // Only the top frame reports candidates to native code. Each ancestor maps
  // the child's clipped CSS rectangle into its iframe and clips it again, so
  // a large player inside a tiny/offscreen embed cannot outrank a visible one.
  const report = value => {
    value = {...value, viewport:viewport()};
    if (window === window.top) send(value);
    else window.parent.postMessage({axolotlPip:nonce, kind:'report', value}, '*');
  };
  const mapFrameRect = (frame, value) => {
    const r = value.rect, size = value.viewport, bounds = frame.getBoundingClientRect();
    if (!r || !size || ![r.left,r.top,r.right,r.bottom,size.width,size.height].every(Number.isFinite) ||
        size.width <= 0 || size.height <= 0) return emptyRect();
    const width = bounds.right - bounds.left, height = bounds.bottom - bounds.top;
    return clip({left:bounds.left + r.left / size.width * width,
      top:bounds.top + r.top / size.height * height,
      right:bounds.left + r.right / size.width * width,
      bottom:bounds.top + r.bottom / size.height * height}, visibleRect(frame));
  };
  const restore = () => {
    if (timer !== null) clearInterval(timer);
    timer = null;
    if (!selection) return;
    for (const [el, previous] of selection.attributes) {
      if (previous === null) el.removeAttribute('data-axolotl-pip');
      else el.setAttribute('data-axolotl-pip', previous);
    }
    for (const style of selection.styles) style.remove();
    if (selection.video) selection.video.controls = selection.controls;
    selection = null;
  };
  const isolate = (el, request, video) => {
    restore();
    const attributes = [], styles = [], trees = new Set();
    for (let node = el; node; node = node.parentElement || node.getRootNode().host) {
      attributes.push([node, node.getAttribute('data-axolotl-pip')]);
      node.setAttribute('data-axolotl-pip', node === el ? 'target' : 'branch');
      trees.add(node.getRootNode());
    }
    const css = `:root{overflow:hidden!important;background:#000!important}
[data-axolotl-pip="branch"]{display:block!important;position:fixed!important;inset:0!important;margin:0!important;padding:0!important;border:0!important;width:100vw!important;height:100vh!important;min-width:0!important;min-height:0!important;max-width:none!important;max-height:none!important;transform:none!important;filter:none!important;perspective:none!important;contain:none!important;overflow:visible!important;clip:auto!important;clip-path:none!important;visibility:visible!important;opacity:1!important;background:#000!important}
[data-axolotl-pip="branch"]>:not([data-axolotl-pip]):not(style){display:none!important}
[data-axolotl-pip="target"]{display:block!important;position:fixed!important;inset:0!important;margin:0!important;padding:0!important;border:0!important;width:100vw!important;height:100vh!important;min-width:0!important;min-height:0!important;max-width:none!important;max-height:none!important;transform:none!important;object-fit:contain!important;visibility:visible!important;opacity:1!important;z-index:2147483647!important;background:#000!important}`;
    for (const root of trees) {
      const style = document.createElement('style'); style.textContent = css;
      (root.head || root).appendChild(style); styles.push(style);
    }
    selection = {request, attributes, styles, video, controls:video?.controls};
    if (video) video.controls = true;
    const src = video?.currentSrc, stream = video?.srcObject;
    timer = setInterval(() => {
      // An iframe can disappear before its child can relay pagehide. Monitor
      // every isolated ancestor as well as the original media element.
      if (!el.isConnected || (video && (video.ended || video.currentSrc !== src || video.srcObject !== stream)))
        send({kind:'stopped', request, frame:frameId});
    }, 500);
  };
  const selected = (request, frame) => {
    if (window === window.top) send({kind:'ready', request, frame});
    else window.parent.postMessage({axolotlPip:nonce, kind:'selected', request, frame}, '*');
  };
  const broadcast = command => {
    for (const frame of frames()) frame.contentWindow?.postMessage({axolotlPip:nonce, ...command}, '*');
  };
  const command = value => {
    if (!value || typeof value.request !== 'string') return;
    if (value.kind === 'probe') {
      currentRequest = value.request;
      candidates = new Map();
      let index = 0;
      for (const root of roots()) for (const video of root.querySelectorAll('video')) {
        const id = `${frameId}:${index++}`;
        const size = audible(video) && !video.disablePictureInPicture ? area(video) : 0;
        if (size <= 0) continue;
        candidates.set(id, video);
        // Report every video: ancestor clipping can reverse the local ranking.
        report({kind:'candidate', request:value.request, frame:id, area:size,
          rect:visibleRect(video), aspect:video.videoWidth / video.videoHeight, existing:nativePip()});
      }
      if (!candidates.size) report({kind:'candidate', request:value.request, frame:frameId, area:0,
        rect:emptyRect(), aspect:1, existing:nativePip()});
    } else if (value.kind === 'select') {
      if (value.request !== currentRequest) return;
      if (value.frame === frameId || value.frame?.startsWith(frameId + ':')) {
        const candidate = candidates.get(value.frame);
        if (candidate?.isConnected && audible(candidate) && !candidate.disablePictureInPicture && area(candidate) > 0 && !nativePip()) {
          isolate(candidate, value.request, candidate); selected(value.request, value.frame);
        } else send({kind:'stopped', request:value.request, frame:value.frame});
      }
    } else if (value.kind === 'restore') {
      // An old asynchronous completion cannot undo a newer selection, and a
      // delayed child ready cannot restyle ancestors after cancellation.
      if (currentRequest === value.request) {
        currentRequest = null; candidates.clear();
        if (selection?.request === value.request) restore();
      }
    }
    broadcast(value);
  };
  window.__axolotlAutoPip = command;
  window.addEventListener('message', event => {
    const value = event.data;
    if (!value || value.axolotlPip !== nonce) return;
    if (event.source === window.parent && window !== window.top) command(value);
    else if (value.kind === 'selected' || value.kind === 'report' || value.kind === 'native') {
      const frame = frames().find(frame => frame.contentWindow === event.source);
      if (!frame) return;
      if (value.kind === 'native') {
        const child = value.value;
        if (typeof child?.frame !== 'string') return;
        if ((child.kind === 'external' && typeof child.existing === 'boolean') ||
            (child.kind === 'stopped' && typeof child.request === 'string')) send(child);
      } else if (value.kind === 'report') {
        const child = value.value;
        if (child?.kind !== 'candidate' || typeof child.request !== 'string' || typeof child.frame !== 'string' || child.request !== currentRequest) return;
        const rect = mapFrameRect(frame, child);
        report({...child, rect, area:rectArea(rect)});
      } else if (value.request !== currentRequest || currentRequest === null) return;
      else if (area(frame) > 0) {
        isolate(frame, value.request, null); selected(value.request, value.frame);
      } else send({kind:'stopped', request:value.request, frame:value.frame});
    }
  });
  for (const type of ['enterpictureinpicture','leavepictureinpicture','webkitpresentationmodechanged'])
    document.addEventListener(type, () => send({kind:'external', frame:frameId, existing:nativePip()}), true);
  window.addEventListener('pagehide', () => {
    if (selection) send({kind:'stopped', request:selection.request, frame:frameId});
    send({kind:'external', frame:frameId, existing:false});
    currentRequest = null; candidates.clear(); restore();
  });
})();
