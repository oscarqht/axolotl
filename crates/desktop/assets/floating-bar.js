function updateBadge(parent, count) {
  let badge = parent.querySelector('.notification-badge');
  if (!count) { badge?.remove(); return; }
  if (!badge) {
    badge = document.createElement('span');
    badge.className = 'notification-badge';
    badge.setAttribute('aria-hidden', 'true');
    parent.insertBefore(badge, parent.querySelector(".close"));
  }
  badge.textContent = count > 99 ? '99+' : String(count);
  badge.title = `${count} unread notifications`;
}
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
  scrollTimer = null,
  settleTimer = null,
  transitionEpoch = 0,
  pendingStart = null,
  renaming = null,
  menu = null,
  forceRebuild = false;
function metrics() {
  send({
    type: "metrics",
    midpoints: [...tabs.children].map((c) => {
      let r = c.getBoundingClientRect();
      return r.x + r.width / 2;
    }),
  });
  // Overflow is a property of the final expanded layout, not the animated
  // viewport. Otherwise a fitting chip introduces arrows mid-expansion, then
  // spends another 180ms removing them after the pill reaches its final width.
  const style = getComputedStyle(bar);
  const rowStyle = getComputedStyle(tabs);
  const number = (value) => parseFloat(value) || 0;
  const fixed =
    document.querySelector("#handle").getBoundingClientRect().width +
    document.querySelector("#add").getBoundingClientRect().width +
    document.querySelector("#shield").getBoundingClientRect().width + 4 +
    number(style.paddingLeft) +
    number(style.paddingRight) +
    number(style.borderLeftWidth) +
    number(style.borderRightWidth) +
    4 * number(style.columnGap);
  const target = (state.expandedWidth ?? state.surfaceWidth ?? innerWidth) - 8;
  const capacity = Math.max(0, target - fixed);
  const required =
    [...tabs.children].reduce((sum, chip) => {
      const basis = number(getComputedStyle(chip).flexBasis);
      return (
        sum +
        (chip.classList.contains("editing") ? Math.min(basis, capacity) : basis)
      );
    }, 0) +
    Math.max(0, tabs.children.length - 1) * number(rowStyle.columnGap);
  const overflow = required > capacity + 1;
  bar.classList.toggle("overflow", overflow);
  bar.classList.toggle("fade-left", overflow && tabs.scrollLeft > 1);
  bar.classList.toggle(
    "fade-right",
    overflow && tabs.scrollLeft + tabs.clientWidth < tabs.scrollWidth - 1,
  );
}
function scheduleSettle(value, epoch) {
  clearTimeout(settleTimer);
  const settle = () => {
    if (epoch === transitionEpoch && expanded === value) {
      // Occluded WKWebViews may freeze CSS timelines. Commit their endpoint
      // before allowing the native surface to shrink, including reduced motion.
      bar
        .getAnimations({ subtree: true })
        .forEach((animation) => animation.finish());
      reveal();
      metrics();
      send({ type: "settled", value });
    }
  };
  settleTimer = setTimeout(
    settle,
    matchMedia("(prefers-reduced-motion: reduce)").matches ? 0 : 320,
  );
}
function expand(value) {
  clearTimeout(collapseTimer);
  if (
    !value &&
    (drag ||
      state.editing ||
      renaming ||
      menu ||
      (document.hasFocus() &&
        document.activeElement?.closest("#count,#tabs,.arrow")) ||
      [...bar.querySelectorAll("#count,#tabs,.arrow")].some((area) =>
        area.matches(":hover"),
      ) ||
      pinned)
  )
    return;
  if (expanded === value) return;
  expanded = value;
  const epoch = ++transitionEpoch;
  clearTimeout(settleTimer);
  send({ type: "expand", value });
  const start = () => {
    if (epoch !== transitionEpoch) return;
    // Wait for the host to acknowledge its wide bounds, keeping the compact
    // pill centered in the meantime. Use an explicit pixel endpoint so a delayed
    // viewport resize cannot change the tween's interpolation basis.
    if (value && !(state.expanded || state.editing)) return;
    pendingStart = null;
    // Establish the starting layout even if the native child is occluded.
    bar.getBoundingClientRect();
    bar.classList.toggle("expanded", value);
    document.querySelector("#count").tabIndex = value ? -1 : 0;
    document.querySelector("#count").setAttribute("aria-hidden", String(value));
    scheduleSettle(value, epoch);
    // Focus immediately after making the row visible. WebKit can pause frame
    // callbacks in occluded windows, including while native siblings are built.
    const input = tabs.querySelector("input");
    if (value && state.editing && input && document.activeElement !== input) {
      input.focus();
      input.select();
    }
    reveal();
    metrics();
  };
  pendingStart = start;
  start();
}
window.addEventListener("resize", () => pendingStart?.());
bar.addEventListener("transitionend", (event) => {
  if (
    event.target === bar &&
    event.propertyName === "width" &&
    bar.classList.contains("expanded") === expanded &&
    Math.abs(
      bar.getBoundingClientRect().width -
        (expanded
          ? parseFloat(bar.style.getPropertyValue("--expanded-width"))
          : 160),
    ) < 1
  ) {
    clearTimeout(settleTimer);
    reveal();
    metrics();
    send({ type: "settled", value: expanded });
  }
});
function collapse() {
  clearTimeout(collapseTimer);
  collapseTimer = setTimeout(() => expand(false), 450);
}
function reveal() {
  tabs
    .querySelector(".active")
    ?.scrollIntoView({ block: "nearest", inline: "nearest" });
}
for (const area of bar.querySelectorAll("#count,#tabs")) {
  area.addEventListener("pointerenter", (event) => {
    if (event.pointerType !== "touch" && !drag?.whole) expand(true);
  });
  area.addEventListener("pointerleave", collapse);
}
for (const arrow of bar.querySelectorAll(".arrow")) {
  arrow.addEventListener("pointerleave", collapse);
}
bar.addEventListener("pointerleave", () => {
  if (drag && !drag.active) drag = null;
  collapse();
});
bar.addEventListener("dragstart", (e) => e.preventDefault());
bar.addEventListener("focusin", (event) => {
  if (event.target.closest("#count,#tabs,.arrow")) expand(true);
  else collapse();
});
bar.addEventListener("focusout", collapse);
window.addEventListener("blur", () => {
  closeMenu();
  if (state.editing) send({ type: "blur", tab: state.editing });
  pinned = false;
  collapse();
});
document.querySelector("#count").onclick = () => {
  pinned = !pinned;
  expand(true);
};
document.querySelector("#shield").onclick = () => send({type:"protection", tab:state.active});
document.querySelector("#add").onclick = () => send({ type: "new" });
document.querySelector("#prev").onclick = () =>
  tabs.scrollBy({ left: -190, behavior: "smooth" });
document.querySelector("#next").onclick = () =>
  tabs.scrollBy({ left: 190, behavior: "smooth" });
tabs.onscroll = metrics;
new ResizeObserver(metrics).observe(tabs);
window.updateBar = (s) => {
  const shield=document.querySelector('#shield'), active=s.tabs.find(t=>t.id===s.active);
  shield.disabled=!active?.protectionSite || !active?.protectionGlobal || active?.protectionReady===false;
  shield.style.color=active?.protection ? 'var(--accent)' : 'var(--muted)';
  shield.setAttribute('aria-pressed',String(!!active?.protection));
  shield.title=active?.protectionReady===false ? 'Applying protection…' : active?.protectionSite ? (active.protectionGlobal ? `${active.protection?'Protection on':'Protection off'} for ${active.protectionSite}. Click to ${active.protection?'disable':'enable'} and reload.` : 'Protection is off in Settings.') : 'Protection is available on websites.';
  shield.setAttribute('aria-label',shield.title);
  const old = state;
  state = s;
  bar.classList.toggle("editing", s.editing != null || renaming !== null);
  if (s.expanded || s.editing) {
    const width = s.expandedWidth ?? s.surfaceWidth;
    bar.style.setProperty("--expanded-width", `${width - 8}px`);
    if (
      width !== (old.expandedWidth ?? old.surfaceWidth) &&
      expanded &&
      bar.classList.contains("expanded")
    ) {
      scheduleSettle(true, ++transitionEpoch);
    }
  }
  if (!old.tabs.length && s.expanded) {
    expand(true);
    // A rebuilt surface has no previous hover-leave timer. Retain expansion
    // only if its chip region is still being used, rather than for handle/+.
    collapse();
  }
  document.querySelector("#count").textContent =
    `${s.tabs.findIndex((t) => t.id === s.active) + 1}/${s.tabs.length}`;
  const countButton = document.querySelector("#count");
  const total = s.tabs.reduce((sum, t) => sum + (t.notifications || 0), 0);
  updateBadge(countButton, total);
  countButton.setAttribute("aria-label", `Show tabs${total ? `, ${total} unread notifications` : ''}`);
  if (
    old.tabs.map((t) => t.id).join(",") !== s.tabs.map((t) => t.id).join(",") ||
    old.editing !== s.editing ||
    forceRebuild
  ) {
    forceRebuild = false;
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
        bindHistory(input, t.id);
        input.onkeydown = (e) => {
          if (historyKey(e, input, t.id)) return;
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
          if (document.activeElement !== input) {
            input.focus();
            if (draft === null) input.select();
          }
        });
      } else if (renaming === t.id) {
        chip.classList.add("renaming");
        const input = document.createElement("input");
        input.setAttribute("aria-label", "Tab title");
        input.placeholder = "Page title (empty to reset)";
        input.value = t.customTitle ?? t.title ?? "";
        input.onkeydown = (e) => {
          if (e.key === "Enter") {
            send({ type: "set-title", tab: t.id, value: input.value });
            stopRenaming();
            e.preventDefault();
          }
          if (e.key === "Escape") {
            stopRenaming();
            e.preventDefault();
            e.stopPropagation();
          }
        };
        input.onblur = () => stopRenaming();
        chip.append(input);
        requestAnimationFrame(() => {
          input.focus();
          input.select();
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
      chip.oncontextmenu = (e) => {
        e.preventDefault();
        if (e.target.closest("input")) return;
        stopRenaming();
        menu = { tab: t.id, x: e.clientX };
        send({ type: "menu-open", tab: t.id });
        renderMenu();
      };
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
    reveal();
    metrics();
    window.previewSlot(preview);
  }
  for (const [index, chip] of [...tabs.children].entries()) {
    const t = s.tabs[index];
    chip.title = t.title || t.url || "New tab";
    updateBadge(chip, t.notifications || 0);
    chip.setAttribute("aria-label", `${chip.title}${t.notifications ? `, ${t.notifications} unread notifications` : ''}`);
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
  if (old.active !== s.active) {
    reveal();
    metrics();
  }
  renderMenu();
  if (s.editing) expand(true);
  else if (old.editing) {
    window.clearHistory();
    document.activeElement?.blur();
    pinned = false;
    collapse();
  }
  pendingStart?.();
};
function stopRenaming() {
  if (renaming === null) return;
  renaming = null;
  forceRebuild = true;
  window.updateBar(state);
  collapse();
}
function closeMenu() {
  if (!menu) return;
  menu = null;
  send({ type: "menu-close" });
  renderMenu();
  collapse();
}
// The host grows the native surface upward once it acknowledges `state.menu`;
// only then is there room to draw the menu without clipping.
function renderMenu() {
  const el = document.querySelector("#menu"),
    t = menu && state.tabs.find((tab) => tab.id === menu.tab);
  if (menu && !t) menu = null;
  document.body.classList.toggle("menu", !!menu && !!state.menu);
  el.hidden = !(menu && state.menu);
  if (el.hidden) {
    el.replaceChildren();
    return;
  }
  const act = (fn) => () => {
    const id = menu.tab;
    closeMenu();
    fn(id);
  };
  const groups = [
    [
      ["Back", !t.canBack, (id) => send({ type: "back", tab: id })],
      ["Forward", !t.canForward, (id) => send({ type: "forward", tab: id })],
      ["Refresh", !t.canReload, (id) => send({ type: "reload", tab: id })],
    ],
    [
      [
        "Set title…",
        false,
        (id) => {
          renaming = id;
          forceRebuild = true;
          window.updateBar(state);
        },
      ],
    ],
    [["Pin to header", false, (id) => send({ type: "pin", tab: id })]],
    [["Close", false, (id) => send({ type: "close", tab: id })]],
  ];
  el.replaceChildren(
    ...groups.flatMap((items, index) => [
      ...(index ? [document.createElement("hr")] : []),
      ...items.map(([label, disabled, fn]) => {
        const b = document.createElement("button");
        b.type = "button";
        b.setAttribute("role", "menuitem");
        b.textContent = label;
        b.disabled = disabled;
        b.onclick = act(fn);
        return b;
      }),
    ]),
  );
  el.style.left =
    Math.max(4, Math.min(menu.x, innerWidth - el.offsetWidth - 4)) + "px";
}
window.previewSlot = (index) => {
  preview = index;
  [...tabs.children].forEach((c, i) => c.classList.toggle("slot", i === index));
  document
    .querySelector("#add")
    .classList.toggle("slot", index === state.tabs.length);
};
document.addEventListener(
  "pointerdown",
  (e) => {
    if (menu && !e.target.closest("#menu")) closeMenu();
  },
  true,
);
bar.addEventListener("pointerdown", (e) => {
  if (
    !e.isTrusted ||
    e.button !== 0 ||
    e.target.closest("input,.close,#add,#shield,.arrow,#count")
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
    closeMenu();
    end(e, true);
    pinned = false;
    send({ type: "cancel" });
    document.activeElement?.blur();
    expand(false);
  }
  if (e.shiftKey && !e.altKey && e.key.toLowerCase() === "t" &&
      (navigator.platform.includes("Mac") ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey)) {
    e.preventDefault();
    e.stopImmediatePropagation();
    send({ type: "new-space" });
    return;
  }
  if ((e.metaKey || e.ctrlKey) && !e.altKey && !e.shiftKey) {
    if (e.key === "," && !(navigator.platform.includes("Mac") ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey)) return;
    let type = { l: "edit", t: "new", w: "close", r: "reload", ",": "settings" }[
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
