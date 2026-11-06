(() => {
  // Best effort only: pages may use custom editors or media implementations.
  let dirty = false;
  const streams = new Set(), contexts = new Set(), peers = new Set(), connections = new Map();
  document.addEventListener('input', e => { if (e.isTrusted) dirty = true; }, true);
  document.addEventListener('change', e => { if (e.isTrusted) dirty = true; }, true);
  // A submit may be prevented, save asynchronously, or fail. Only a new
  // document is reliable evidence that this document's edits can be discarded.
  for (const [name, closed] of [['WebSocket', 3], ['EventSource', 2]]) {
    const Original = window[name];
    if (Original) window[name] = new Proxy(Original, {
      construct(target,args,newTarget) {
        const connection = Reflect.construct(target,args,newTarget);
        // Reconnecting webapps can create many sockets. Tracking must not keep
        // otherwise unreachable connections alive for the document's lifetime.
        const reference = typeof WeakRef === 'function' ? new WeakRef(connection) : { deref: () => connection };
        connections.set(reference, closed);
        connection.addEventListener?.(name === 'WebSocket' ? 'close' : 'error', () => {
          if (connection.readyState === closed) connections.delete(reference);
        });
        return connection;
      }
    });
  }
  const media = navigator.mediaDevices;
  if (media) for (const name of ['getUserMedia', 'getDisplayMedia']) {
    const original = media[name]?.bind(media);
    if (original) media[name] = async (...args) => { const stream = await original(...args); streams.add(stream); return stream; };
  }
  for (const name of ['AudioContext', 'webkitAudioContext']) {
    const Original = window[name];
    if (Original) window[name] = new Proxy(Original, {construct(target,args,newTarget) { const context = Reflect.construct(target,args,newTarget); contexts.add(context); return context; }});
  }
  if (window.RTCPeerConnection) window.RTCPeerConnection = new Proxy(window.RTCPeerConnection, {
    construct(target,args,newTarget) { const peer = Reflect.construct(target,args,newTarget); peers.add(peer); return peer; }
  });
  window.__axolotlSleepBusy = () => {
    // CONNECTING/reconnecting connections also need their page to stay alive.
    let liveConnection = false;
    for (const [reference, closed] of connections) {
      const connection = reference.deref();
      if (connection && connection.readyState !== closed) liveConnection = true;
      else connections.delete(reference);
    }
    if (liveConnection) return true;
    if (dirty || [...document.querySelectorAll('audio,video')].some(m => !m.paused && !m.ended) ||
        [...streams].some(s => s.getTracks().some(t => t.readyState === 'live')) ||
        [...contexts].some(c => c.state === 'running') ||
        [...peers].some(p => ['new','connecting','connected'].includes(p.connectionState))) return true;
    for (const frame of document.querySelectorAll('iframe,frame')) {
      try { if (!frame.contentWindow.__axolotlSleepBusy || frame.contentWindow.__axolotlSleepBusy()) return true; }
      catch { return true; } // Cannot inspect cross-origin calls/editors safely.
    }
    return false;
  };
})();
