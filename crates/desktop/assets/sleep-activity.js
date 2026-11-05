(() => {
  // Best effort only: pages may use custom editors or media implementations.
  let dirty = false;
  const streams = new Set(), contexts = new Set(), peers = new Set();
  document.addEventListener('input', e => { if (e.isTrusted) dirty = true; }, true);
  document.addEventListener('change', e => { if (e.isTrusted) dirty = true; }, true);
  document.addEventListener('submit', () => { dirty = false; }, true);
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
