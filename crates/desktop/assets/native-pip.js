// macOS only. Invoked by the host on an in-app tab/space visibility transition.
// No polling, DOM mutations, stream extraction, or webview reparenting.
(() => {
  const send = state => window.ipc.postMessage('__NONCE__:pip:' + state);
  const watching = new WeakSet();
  const presented = new Set();
  let requestGeneration = 0;
  const watch = video => {
    if (watching.has(video)) return;
    watching.add(video);
    const changed = () => {
      const wasPresented = presented.has(video);
      if (video.webkitPresentationMode === 'picture-in-picture') presented.add(video);
      else presented.delete(video);
      if (presented.size) send('entered');
      else if (wasPresented) send('left');
    };
    video.addEventListener('webkitpresentationmodechanged', changed);
    video.addEventListener('enterpictureinpicture', changed);
    video.addEventListener('leavepictureinpicture', changed);
  };
  document.addEventListener('loadedmetadata', e => {
    if (e.target instanceof HTMLVideoElement) watch(e.target);
  }, true);
  document.addEventListener('play', e => {
    if (e.target instanceof HTMLVideoElement) watch(e.target);
  }, true);
  window.__axolotlNativePip = () => {
    const generation = ++requestGeneration;
    const videos = [...document.querySelectorAll('video')];
    videos.forEach(watch);
    if (videos.some(v => v.webkitPresentationMode === 'picture-in-picture')) {
      send('entered');
      return;
    }
    const candidates = videos.filter(v => !v.paused && !v.ended && v.readyState >= 2 &&
      !v.disablePictureInPicture && v.webkitSupportsPresentationMode?.('picture-in-picture'));
    const area = v => { const r = v.getBoundingClientRect(); return r.width * r.height; };
    candidates.sort((a, b) => area(b) - area(a));
    const video = candidates[0];
    if (!video) { send('unavailable'); return; }
    try {
      video.webkitSetPresentationMode('picture-in-picture');
      // A rejected prefixed request can be silent. Release the pending reservation once.
      setTimeout(() => {
        if (generation === requestGeneration && !presented.size && video.webkitPresentationMode !== 'picture-in-picture') send('unavailable');
      }, 1500);
    } catch (_) { send('unavailable'); }
  };
})();
