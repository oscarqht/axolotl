const send = (v) => window.ipc.postMessage(JSON.stringify(v)),
  bar = document.querySelector("#bar"),
  tabs = document.querySelector("#tabs");
let state = { tabs: [] },
  expanded = false,
  pinned = false,
  drag = null,
  suppress = false,
  collapseTimer,
  preview = null,
  scrollTimer = null;
function metrics() {
  send({
    type: "metrics",
    midpoints: [...tabs.children].map((c) => {
      let r = c.getBoundingClientRect();
      return r.x + r.width / 2;
    }),
  });
  bar.classList.toggle("overflow", tabs.scrollWidth > tabs.clientWidth + 1);
}
function expand(value) {
  clearTimeout(collapseTimer);
  if (
    !value &&
    (drag ||
      state.editing ||
      (document.hasFocus() && bar.contains(document.activeElement)) ||
      bar.matches(":hover") ||
      pinned)
  )
    return;
  if (expanded === value) return;
  expanded = value;
  bar.classList.toggle("expanded", value);
  send({ type: "expand", value });
  requestAnimationFrame(() => {
    reveal();
    metrics();
  });
}
function collapse() {
  clearTimeout(collapseTimer);
  collapseTimer = setTimeout(() => expand(false), 450);
}
function reveal() {
  tabs
    .querySelector(".active")
    ?.scrollIntoView({ block: "nearest", inline: "nearest" });
}
bar.addEventListener("pointerenter", (e) => {
  if (e.pointerType !== "touch") expand(true);
});
bar.addEventListener("pointerleave", () => {
  if (drag && !drag.active) drag = null;
  collapse();
});
bar.addEventListener("dragstart", (e) => e.preventDefault());
bar.addEventListener("focusin", () => expand(true));
bar.addEventListener("focusout", collapse);
window.addEventListener("blur", () => {
  if (state.editing) send({ type: "blur", tab: state.editing });
  pinned = false;
  collapse();
});
document.querySelector("#count").onclick = () => {
  pinned = !pinned;
  expand(true);
};
document.querySelector("#add").onclick = () => send({ type: "new" });
document.querySelector("#prev").onclick = () =>
  tabs.scrollBy({ left: -190, behavior: "smooth" });
document.querySelector("#next").onclick = () =>
  tabs.scrollBy({ left: 190, behavior: "smooth" });
tabs.onscroll = metrics;
new ResizeObserver(metrics).observe(tabs);
window.updateBar = (s) => {
  const old = state;
  state = s;
  if (!old.tabs.length && s.expanded) expand(true);
  document.querySelector("#count").textContent =
    `${s.tabs.findIndex((t) => t.id === s.active) + 1}/${s.tabs.length}`;
  if (
    old.tabs.map((t) => t.id).join(",") !== s.tabs.map((t) => t.id).join(",") ||
    old.editing !== s.editing
  ) {
    const focused =
        old.editing === s.editing &&
        document.activeElement?.tagName === "INPUT",
      draft = focused ? document.activeElement.value : null;
    tabs.replaceChildren();
    for (const t of s.tabs) {
      const chip = document.createElement("div");
      chip.className = "chip" + (t.id === s.active ? " active" : "");
      chip.dataset.id = t.id;
      chip.setAttribute("role", "tab");
      chip.setAttribute("aria-selected", t.id === s.active);
      chip.tabIndex = 0;
      chip.title = t.title || t.url || "New tab";
      if (s.editing === t.id) {
        chip.classList.add("editing");
        const input = document.createElement("input");
        input.setAttribute("aria-label", "Address");
        input.value = draft ?? t.url;
        input.onkeydown = (e) => {
          if (e.key === "Enter") {
            send({ type: "navigate", tab: t.id, value: input.value });
            e.preventDefault();
          }
          if (e.key === "Escape") {
            send({ type: "cancel" });
            e.preventDefault();
            e.stopPropagation();
          }
        };
        chip.append(input);
        requestAnimationFrame(() => {
          input.focus();
          if (draft === null) input.select();
        });
      } else {
        const img = document.createElement("img");
        img.alt = "";
        img.draggable = false;
        try {
          const u = new URL(t.url);
          if (["http:", "https:"].includes(u.protocol))
            img.src = u.origin + "/favicon.ico";
          else img.hidden = true;
        } catch {
          img.hidden = true;
        }
        img.onerror = () => (img.hidden = true);
        const label = document.createElement("span");
        label.className = "label";
        label.textContent = t.title || t.url || "New tab";
        chip.append(img, label);
      }
      const close = document.createElement("button");
      close.className = "close";
      close.textContent = "×";
      close.title = "Close tab";
      close.setAttribute("aria-label", "Close " + (t.title || t.url || "tab"));
      close.onclick = (e) => {
        e.stopPropagation();
        send({ type: "close", tab: t.id });
      };
      chip.append(close);
      chip.onclick = (e) => {
        if (!suppress && !e.target.closest("input,button"))
          send({ type: "select", tab: t.id });
      };
      chip.ondblclick = (e) => {
        if (!e.target.closest("input,button"))
          send({ type: "edit", tab: t.id });
      };
      chip.onkeydown = (e) => {
        if (e.target.tagName === "INPUT") return;
        if (e.key === "Enter" || e.key === " ")
          send({ type: "select", tab: t.id });
        if (e.key === "F2") send({ type: "edit", tab: t.id });
        if (e.key === "ArrowRight" || e.key === "ArrowLeft") {
          e.preventDefault();
          send({ type: "swipe", forward: e.key === "ArrowRight" });
        }
      };
      tabs.append(chip);
    }
    requestAnimationFrame(() => {
      reveal();
      metrics();
      window.previewSlot(preview);
    });
  }
  for (const [index, chip] of [...tabs.children].entries()) {
    const t = s.tabs[index];
    chip.title = t.title || t.url || "New tab";
    const label = chip.querySelector(".label");
    if (label) label.textContent = t.title || t.url || "New tab";
    const image = chip.querySelector("img");
    if (image && old.tabs[index]?.url !== t.url) {
      try {
        const url = new URL(t.url);
        image.hidden = !["http:", "https:"].includes(url.protocol);
        if (!image.hidden) image.src = url.origin + "/favicon.ico";
      } catch {
        image.hidden = true;
      }
    }

    const active = Number(chip.dataset.id) === s.active;
    chip.classList.toggle("active", active);
    chip.setAttribute("aria-selected", active);
  }
  if (old.active !== s.active)
    requestAnimationFrame(() => {
      reveal();
      metrics();
    });
  if (s.editing) expand(true);
  else if (old.editing) {
    document.activeElement?.blur();
    pinned = false;
    collapse();
  }
};
window.previewSlot = (index) => {
  preview = index;
  [...tabs.children].forEach((c, i) => c.classList.toggle("slot", i === index));
  document
    .querySelector("#add")
    .classList.toggle("slot", index === state.tabs.length);
};
bar.addEventListener("pointerdown", (e) => {
  if (
    !e.isTrusted ||
    e.button !== 0 ||
    e.target.closest("input,.close,#add,.arrow,#count")
  )
    return;
  const chip = e.target.closest(".chip"),
    handle = e.target.closest("#handle");
  if (!chip && !handle) return;
  drag = {
    id: chip ? Number(chip.dataset.id) : state.active,
    whole: !!handle,
    x: e.clientX,
    y: e.clientY,
    active: false,
    pointer: e.pointerId,
  };
  suppress = false;
});
bar.addEventListener("pointermove", (e) => {
  if (!drag || e.pointerId !== drag.pointer) return;
  if (!(e.buttons & 1)) {
    end(e, true);
    return;
  }
  if (!drag.active && Math.hypot(e.clientX - drag.x, e.clientY - drag.y) < 5)
    return;
  if (!drag.active) {
    drag.active = true;
    bar.setPointerCapture(e.pointerId);
    suppress = true;
    send({
      type: "drag-start",
      tab: drag.id,
      whole: drag.whole,
      x: drag.x,
      y: drag.y,
    });
    bar.classList.add("dragging");
    scrollTimer = setInterval(() => {
      if (!drag?.active) return;
      const r = tabs.getBoundingClientRect();
      if (drag.lastY < r.top - 12 || drag.lastY > r.bottom + 12) return;
      const direction =
        drag.lastX < r.left + 24 ? -1 : drag.lastX > r.right - 24 ? 1 : 0;
      if (direction) tabs.scrollBy({ left: direction * 16 });
    }, 40);
  }
  drag.lastX = e.clientX;
  drag.lastY = e.clientY;
  send({ type: "drag-move", x: e.clientX, y: e.clientY });
});
function end(e, cancel = false) {
  if (!drag) return;
  if (drag.active)
    send({
      type: cancel ? "drag-cancel" : "drag-end",
      x: e.clientX,
      y: e.clientY,
    });
  drag = null;
  clearInterval(scrollTimer);
  scrollTimer = null;
  bar.classList.remove("dragging");
  setTimeout(() => (suppress = false), 100);
  pinned = false;
  collapse();
}
bar.addEventListener("pointerup", (e) => end(e));
bar.addEventListener("pointercancel", (e) => end(e, true));
bar.addEventListener("lostpointercapture", (e) => end(e, true));
let touchSwipe = null;
document.addEventListener(
  "touchstart",
  (e) => {
    if (
      e.isTrusted &&
      e.touches.length === 1 &&
      !e.target.closest(".chip,input,#handle")
    ) {
      const t = e.touches[0];
      touchSwipe = { x: t.clientX, y: t.clientY, fired: false };
    }
  },
  { passive: true },
);
document.addEventListener(
  "touchmove",
  (e) => {
    if (!touchSwipe || !e.isTrusted || e.touches.length !== 1) return;
    const t = e.touches[0],
      x = t.clientX - touchSwipe.x,
      y = t.clientY - touchSwipe.y;
    if (Math.abs(x) > 12 && Math.abs(x) > Math.abs(y) * 1.4) {
      e.preventDefault();
      if (!touchSwipe.fired && Math.abs(x) > 60) {
        touchSwipe.fired = true;
        send({ type: "swipe", forward: x < 0 });
      }
    }
  },
  { passive: false },
);
document.addEventListener("touchend", () => (touchSwipe = null));
document.addEventListener("touchcancel", () => (touchSwipe = null));
document.addEventListener(
  "wheel",
  (e) => {
    if (!e.isTrusted || e.ctrlKey || Math.abs(e.deltaX) <= Math.abs(e.deltaY))
      return;
    e.preventDefault();
    e.stopImmediatePropagation();
    const scale = e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? innerWidth : 1;
    send({ type: "wheel", x: e.deltaX * scale, y: e.deltaY * scale });
  },
  { passive: false },
);
document.addEventListener("keydown", (e) => {
  if (!e.isTrusted) return;
  if (e.key === "Escape") {
    end(e, true);
    pinned = false;
    send({ type: "cancel" });
    document.activeElement?.blur();
    expand(false);
  }
  if ((e.metaKey || e.ctrlKey) && !e.altKey && !e.shiftKey) {
    let type = { l: "edit", t: "new", w: "close", r: "reload" }[
      e.key.toLowerCase()
    ];
    if (type) {
      e.preventDefault();
      if (type === "edit" && document.activeElement?.tagName === "INPUT") {
        document.activeElement.select();
        return;
      }
      send({ type, tab: state.active });
    }
  }
});
send({ type: "ready" });
