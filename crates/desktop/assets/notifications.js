// Badge-only support is local to this page; no system permission is implied.
(() => {
  const post = window.ipc.postMessage.bind(window.ipc);
  const NativeNotification = window.Notification;
  class BadgeNotification extends EventTarget {
    static get permission() { return 'granted'; }
    static async requestPermission(callback) {
      // No OS prompt is needed for an in-app unread badge.
      callback?.('granted');
      return 'granted';
    }
    constructor(title, options = {}) {
      super();
      this.title = String(title);
      this.body = String(options.body ?? '');
      this.tag = String(options.tag ?? '');
      this.icon = String(options.icon ?? '');
      this.data = options.data ?? null;
      this.onclick = this.onclose = this.onerror = this.onshow = null;
      this.native = null;
      // Preserve native delivery when the engine already has OS permission.
      // Missing APIs, denied permissions and unsupported constructors still
      // support badges, without claiming or requesting native permission.
      if (NativeNotification?.permission === 'granted') {
        try {
          this.native = new NativeNotification(this.title, options);
          for (const type of ['click', 'close', 'error', 'show']) {
            this.native.addEventListener(type, () => this.emit(type));
          }
        } catch { /* WKWebView may expose an unusable constructor. */ }
      }
      post('__NONCE__:notification');
      if (!this.native) queueMicrotask(() => this.emit('show'));
    }
    emit(type) {
      const event = new Event(type);
      this.dispatchEvent(event);
      this['on' + type]?.call(this, event);
    }
    close() {
      if (this.native) this.native.close();
      else if (!this.closed) { this.closed = true; this.emit('close'); }
    }
  }
  window.Notification = BadgeNotification;
})();
