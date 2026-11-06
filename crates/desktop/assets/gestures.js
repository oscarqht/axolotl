// Runs inside the private input-bridge closure on every document/frame.
// Leave scrollable horizontal input (and all vertical input) with the page.
const canScrollHorizontally = (path, delta) => {
  const root = document.scrollingElement;
  return [...path, root].some((node) => {
    if (!(node instanceof Element)) return false;
    const style = getComputedStyle(node);
    if (!/^(auto|scroll|overlay)$/.test(style.overflowX) &&
        !(node === root && style.overflowX === "visible")) return false;
    const range = node.scrollWidth - node.clientWidth;
    if (range <= 0) return false;
    // RTL scrollers use negative scrollLeft, with zero at the right edge.
    const min = style.direction === "rtl" ? -range : 0;
    const max = style.direction === "rtl" ? 0 : range;
    return delta < 0 ? node.scrollLeft > min + 0.5 : node.scrollLeft < max - 0.5;
  });
};
let wheelLast = -Infinity;
let wheelScrolling = false;
let touch = null;
window.addEventListener(
  "wheel",
  (event) => {
    if (!event.isTrusted || event.ctrlKey) return;
    const scale =
      event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? innerWidth : 1;
    const x = event.deltaX * scale,
      y = event.deltaY * scale;
    const now = performance.now();
    if (now - wheelLast > 300) wheelScrolling = false;
    wheelLast = now;
    if (Math.abs(x) > Math.abs(y) && Math.abs(x) > 0) {
      // Keep scrolling/momentum with the page until the gesture goes idle.
      wheelScrolling ||= canScrollHorizontally(event.composedPath(), x);
      if (wheelScrolling) return;
      event.preventDefault();
      event.stopImmediatePropagation();
      send("wheel:" + x + ":" + y);
    }
  },
  { capture: true, passive: false },
);
window.addEventListener(
  "touchstart",
  (event) => {
    if (!event.isTrusted || event.touches.length !== 1) {
      touch = null;
      return;
    }
    const t = event.touches[0];
    touch = { x: t.clientX, y: t.clientY, lastX: t.clientX,
      path: event.composedPath(), scrolling: false, fired: false };
  },
  { capture: true, passive: true },
);
window.addEventListener(
  "touchmove",
  (event) => {
    if (!event.isTrusted || !touch || event.touches.length !== 1) return;
    const t = event.touches[0],
      x = t.clientX - touch.x,
      y = t.clientY - touch.y;
    const delta = touch.lastX - t.clientX;
    touch.lastX = t.clientX;
    if (!touch.fired && Math.abs(x) > Math.abs(y) && delta !== 0) {
      touch.scrolling ||= canScrollHorizontally(touch.path, delta);
    }
    if (touch.scrolling) return;
    if (Math.abs(x) > Math.abs(y) * 1.4 && Math.abs(x) > 12) {
      event.preventDefault();
      event.stopImmediatePropagation();
      if (!touch.fired && Math.abs(x) > 60) {
        touch.fired = true;
        send(x < 0 ? "history-forward" : "history-back");
      }
    }
  },
  { capture: true, passive: false },
);
window.addEventListener("touchend", () => (touch = null), { capture: true });
window.addEventListener("touchcancel", () => (touch = null), { capture: true });
