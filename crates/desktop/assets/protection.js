// A policy belongs to the top-level website. Cross-origin frames use referrer as
// a conservative fallback; native rules always use the real top-level URL.
let topHost;
const ancestors=location.ancestorOrigins;
if (ancestors?.length) {try {topHost=new URL(ancestors[ancestors.length-1]).hostname;} catch {}}
try { topHost ||= top.location.hostname; } catch { try { topHost ||= new URL(document.referrer).hostname; } catch { topHost ||= location.hostname; } }
if (!policy.enabled || policy.exceptions.includes(topHost.toLowerCase().replace(/\.$/, ''))) return;
if (!/^https?:$/.test(location.protocol)) return;
const matchesDomain = (domain, h) => h === domain || h.endsWith('.' + domain);
const applies = (domains) => {
  if (!domains) return true;
  let positive = false, matched = false;
  for (const entry of domains.split(',')) {
    const negative = entry.startsWith('!') || entry.startsWith('~');
    const domain = negative ? entry.slice(1) : entry;
    if (!negative) positive = true;
    if (matchesDomain(domain, location.hostname)) { if (negative) return false; matched = true; }
  }
  return !positive || matched;
};
const selectors = new Set(), exceptions = new Set();
for (const line of filters) {
  const except = line.includes('#@#'), separator = except ? '#@#' : '##';
  const index = line.indexOf(separator);
  if (index < 0 || !applies(line.slice(0,index))) continue;
  const selector = line.slice(index + separator.length);
  // Extended/procedural filters and downloaded executable scriptlets are not run.
  if (!selector || selector.startsWith('+js(') || !CSS.supports('selector(' + selector + ')')) continue;
  (except ? exceptions : selectors).add(selector);
}
for (const selector of exceptions) selectors.delete(selector);
const style = document.createElement('style');
style.dataset.axolotlProtection = '';
style.textContent = [...selectors].map(s => s + '{display:none!important}').join('\n');
const mount = () => { if (!style.isConnected && document.documentElement) document.documentElement.append(style); };
mount();
new MutationObserver(mount).observe(document, {childList:true,subtree:true});

// YouTube returns ad metadata inside otherwise legitimate player responses.
// Remove only known ad fields, retaining streamingData, playabilityStatus and
// tracking required for ordinary playback. No media URLs are blocked here.
if (location.hostname === 'youtube.com' || location.hostname.endsWith('.youtube.com')) {
  const diagnostics={removedAdEntries:0,playerResponses:0};
  if (policy.diagnostics) Object.defineProperty(window,'__axolotlProtectionDiagnostics',{value:diagnostics});
  const prune = value => {
    if (!value || typeof value !== 'object') return value;
    for (const key of ['adPlacements','playerAds','adSlots','adBreakHeartbeatParams']) {
      if (Array.isArray(value[key])) diagnostics.removedAdEntries+=value[key].length;
      delete value[key];
    }
    if (value.streamingData) diagnostics.playerResponses++;
    if (value.playerResponse) prune(value.playerResponse);
    return value;
  };
  const originalParse = JSON.parse;
  JSON.parse = function(...args) { return prune(Reflect.apply(originalParse, this, args)); };
  const originalFetch = window.fetch;
  window.fetch = async function(...args) {
    const response = await Reflect.apply(originalFetch, this, args);
    const raw = typeof args[0] === 'string' ? args[0] : args[0]?.url;
    let player = false;
    try { const u = new URL(raw, location.href); player = (u.hostname === 'youtube.com' || u.hostname.endsWith('.youtube.com')) && u.pathname.startsWith('/youtubei/v1/player'); } catch {}
    if (!player || !response.ok) return response;
    try {
      const body = prune(originalParse(await response.clone().text()));
      const headers = new Headers(response.headers);headers.delete('content-length');headers.delete('content-encoding');
      return new Response(JSON.stringify(body), {status:response.status,statusText:response.statusText,headers});
    } catch { return response; }
  };
  let initial;
  const descriptor = Object.getOwnPropertyDescriptor(window, 'ytInitialPlayerResponse');
  if (!descriptor || descriptor.configurable) {
    initial = prune(window.ytInitialPlayerResponse);
    Object.defineProperty(window, 'ytInitialPlayerResponse', {configurable:true,enumerable:true,get:()=>initial,set:value=>{initial=prune(value)}});
  }
  const skip = () => {
    const player = document.querySelector('#movie_player');
    if (!player?.classList.contains('ad-showing')) return;
    const button = player.querySelector('.ytp-ad-skip-button, .ytp-ad-skip-button-modern, .ytp-skip-ad-button');
    if (button) button.click();
    const video = player.querySelector('video');
    // Never seek a normal video. Avoid live/infinite streams and use only the
    // player's explicit ad state, without changing mute/rate or calling play().
    if (video && Number.isFinite(video.duration) && video.duration > 0 && video.duration < 600) {
      try { video.currentTime = video.duration; } catch {}
    }
  };
  setInterval(skip, 250);
}
