// Injected into messages.google.com before the page's own scripts run.
// Bridges the web page to the native app: notifications, unread count and links.
(function () {
  'use strict';
  if (window.top !== window || location.hostname !== 'messages.google.com') return;
  if (window.__gmdInstalled) return;
  window.__gmdInstalled = true;

  function invoke(cmd, args) {
    try {
      const ipc = window.__TAURI_INTERNALS__;
      if (ipc && typeof ipc.invoke === 'function') {
        return Promise.resolve(ipc.invoke(cmd, args)).catch(function (e) {
          console.warn('[messages-desktop] ' + cmd + ' failed', e);
        });
      }
    } catch (e) {
      console.warn('[messages-desktop] ipc unavailable', e);
    }
    return Promise.resolve();
  }

  // ---------------------------------------------------------------------------
  // Notifications: route the page's notifications to native Windows toasts.
  // ---------------------------------------------------------------------------
  const live = new Map();
  let nextId = 1;

  function fire(target, type) {
    const ev = new Event(type, { cancelable: true });
    target.dispatchEvent(ev);
    const handler = target['on' + type];
    if (typeof handler === 'function') {
      try { handler.call(target, ev); } catch (e) { console.error(e); }
    }
  }

  class DesktopNotification extends EventTarget {
    constructor(title, options) {
      super();
      options = options || {};
      this.title = String(title == null ? '' : title);
      this.body = options.body ? String(options.body) : '';
      this.tag = options.tag || '';
      this.icon = options.icon || '';
      this.data = options.data === undefined ? null : options.data;
      this.silent = !!options.silent;
      this.onclick = null;
      this.onshow = null;
      this.onclose = null;
      this.onerror = null;
      this._id = nextId++;
      live.set(this._id, this);
      // Keep the map bounded; old notifications can no longer be clicked anyway.
      if (live.size > 100) live.delete(live.keys().next().value);
      const self = this;
      invoke('notify', { id: this._id, title: this.title, body: this.body }).then(function () {
        fire(self, 'show');
      });
    }
    close() {
      if (live.delete(this._id)) fire(this, 'close');
    }
    static get permission() { return 'granted'; }
    static get maxActions() { return 0; }
    static requestPermission(cb) {
      if (typeof cb === 'function') cb('granted');
      return Promise.resolve('granted');
    }
  }

  // Called by the app when a toast is clicked.
  window.__gmdNotificationClicked = function (id) {
    const n = live.get(id);
    if (!n) return;
    try { window.focus(); } catch (e) { /* ignore */ }
    fire(n, 'click');
    n.close();
  };

  Object.defineProperty(window, 'Notification', {
    value: DesktopNotification,
    writable: true,
    configurable: true,
  });

  // Notifications shown through the service worker registration also become toasts.
  if (window.ServiceWorkerRegistration && ServiceWorkerRegistration.prototype.showNotification) {
    ServiceWorkerRegistration.prototype.showNotification = function (title, options) {
      new DesktopNotification(title, options);
      return Promise.resolve();
    };
  }

  // Report notification permission as granted to permission queries too.
  if (navigator.permissions && navigator.permissions.query) {
    const origQuery = navigator.permissions.query.bind(navigator.permissions);
    navigator.permissions.query = function (desc) {
      if (desc && (desc.name === 'notifications' || desc.name === 'push')) {
        return Promise.resolve({ state: 'granted', status: 'granted', onchange: null,
          addEventListener() {}, removeEventListener() {} });
      }
      return origQuery(desc);
    };
  }

  // ---------------------------------------------------------------------------
  // Unread count: the title usually carries it, e.g. "(3) Messages". As a fallback,
  // count conversations the page marks unread.
  // ---------------------------------------------------------------------------
  let lastUnread = -1;
  function countUnread() {
    const m = /\((\d+)\)/.exec(document.title || '');
    if (m) return parseInt(m[1], 10);
    return document.querySelectorAll(
      'mws-conversation-list-item a[data-e2e-is-unread="true"], mws-conversation-list-item .unread'
    ).length;
  }
  function updateUnread() {
    let n = 0;
    try { n = countUnread(); } catch (e) { return; }
    if (n !== lastUnread) {
      lastUnread = n;
      invoke('set_unread', { count: n });
    }
  }
  setInterval(updateUnread, 2000);
  document.addEventListener('visibilitychange', updateUnread);

  // ---------------------------------------------------------------------------
  // Links: anything outside Messages opens in the default browser.
  // ---------------------------------------------------------------------------
  function isExternal(href) {
    try {
      const u = new URL(href, location.href);
      if (u.protocol === 'mailto:' || u.protocol === 'tel:') return true;
      if (u.protocol !== 'http:' && u.protocol !== 'https:') return false;
      return u.hostname !== 'messages.google.com';
    } catch (e) {
      return false;
    }
  }

  document.addEventListener('click', function (e) {
    if (e.defaultPrevented || e.button !== 0) return;
    const a = e.target && e.target.closest ? e.target.closest('a[href]') : null;
    if (!a) return;
    if (isExternal(a.href)) {
      e.preventDefault();
      e.stopPropagation();
      invoke('open_external', { url: a.href });
    }
  }, true);

  const origOpen = window.open;
  window.open = function (url) {
    if (url && isExternal(String(url))) {
      invoke('open_external', { url: new URL(String(url), location.href).href });
      return null;
    }
    return origOpen.apply(window, arguments);
  };
})();
