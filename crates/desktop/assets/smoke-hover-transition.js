// Sample the native CSS timeline without finishing the arrow animations along
// with the pill. Finishing the entire subtree would hide a second layout phase.
// Establish a compact CSS start even when another smoke check left the bar
// expanded. The host geometry/acknowledgment remains exercised by expand().
clearTimeout(collapseTimer);
clearTimeout(settleTimer);
bar.style.transition = "none";
bar.classList.remove("expanded");
expanded = false;
bar.getAnimations({ subtree: true }).forEach((animation) => animation.cancel());
bar.getBoundingClientRect();
bar.style.removeProperty("transition");
bar.getBoundingClientRect();
window.hoverTransitionSamples = null;
const hoverProbe = new MutationObserver(() => {
  if (!bar.classList.contains("expanded")) return;
  hoverProbe.disconnect();
  bar.getBoundingClientRect();
  const width = bar
    .getAnimations()
    .find((a) => a.transitionProperty === "width");
  if (width) width.pause();
  window.hoverTransitionSamples = [0, 40, 80, 120, 180, 240].map((time) => {
    if (width) width.currentTime = time;
    metrics();
    return {
      time,
      width: bar.getBoundingClientRect().width,
      overflow: bar.classList.contains("overflow"),
      faded:
        bar.classList.contains("fade-left") ||
        bar.classList.contains("fade-right"),
      arrowWidth: [...bar.querySelectorAll(".arrow")].reduce(
        (sum, a) => sum + a.getBoundingClientRect().width,
        0,
      ),
    };
  });
  width?.finish();
});
hoverProbe.observe(bar, { attributes: true, attributeFilter: ["class"] });
pinned = true;
expand(true);
