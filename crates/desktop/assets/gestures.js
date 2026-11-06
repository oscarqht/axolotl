// Runs inside the private input-bridge closure on every document/frame.
// Capture horizontal input before page handlers; leave vertical scrolling alone.
let touch = null;
window.addEventListener(
  "wheel",
  (event) => {
    if (!event.isTrusted || event.ctrlKey) return;
    const scale =
      event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? innerWidth : 1;
    const x = event.deltaX * scale,
      y = event.deltaY * scale;
    if (Math.abs(x) > Math.abs(y) && Math.abs(x) > 0) {
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
    touch = { x: t.clientX, y: t.clientY, fired: false };
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
