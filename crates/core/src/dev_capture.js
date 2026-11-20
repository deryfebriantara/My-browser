// Alat developer: mencatat console, error JavaScript, dan request fetch/XHR
// halaman supaya bisa dibaca Claude lewat console_logs dan network_log.
//
// Dipasang di awal setiap dokumen main frame. Catatan hidup selama dokumen
// itu hidup (memuat ulang halaman = catatan baru) dan dibatasi jumlahnya
// supaya tetap hemat memory. Isi respons hanya disimpan untuk situs lokal,
// karena isi respons situs sungguhan bisa berisi data pribadi.
(function () {
  if (window.__mbDev) return;
  var MAX_LOGS = 200, MAX_NET = 300, MAX_TEXT = 500, MAX_STACK = 2000, MAX_BODY = 4000;
  var local = location.protocol === 'file:' ||
    /^(localhost|127(\.\d+){3}|\[::1\]|.+\.localhost|.+\.local|.+\.test|10(\.\d+){3}|192\.168(\.\d+){2}|172\.(1[6-9]|2\d|3[01])(\.\d+){2})$/.test(location.hostname);
  var dev = { logs: [], net: [], local: local, seq: 0 };
  Object.defineProperty(window, '__mbDev', { value: dev, enumerable: false });

  function now() { return Math.round(performance.now()); }
  function clip(s, n) { s = String(s); return s.length > n ? s.slice(0, n) + '…' : s; }
  function text(v) {
    if (typeof v === 'string') return v;
    // Di WebKit, Error.stack tidak memuat pesannya; stack dicatat terpisah.
    if (v instanceof Error) return String(v);
    try { return JSON.stringify(v); } catch (e) { return String(v); }
  }
  function push(list, max, item) {
    item.seq = ++dev.seq;
    list.push(item);
    if (list.length > max) list.shift();
    return item;
  }
  function absolute(url) {
    try { return new URL(url, location.href).href; } catch (e) { return String(url); }
  }
  function log(level, args, extra) {
    var entry = { t: now(), level: level, message: clip(Array.prototype.map.call(args, text).join(' '), MAX_TEXT) };
    for (var k in extra || {}) if (extra[k] !== undefined) entry[k] = extra[k];
    push(dev.logs, MAX_LOGS, entry);
  }

  ['log', 'info', 'warn', 'error', 'debug'].forEach(function (level) {
    var original = console[level];
    console[level] = function () {
      try { log(level, arguments); } catch (e) {}
      return original.apply(this, arguments);
    };
  });

  addEventListener('error', function (ev) {
    var el = ev.target;
    if (el && el !== window && (el.src || el.href)) {
      log('error', ['Gagal memuat ' + (el.tagName || '').toLowerCase() + ': ' + (el.src || el.href)]);
      return;
    }
    log('error', [ev.message || 'Error'], {
      source: ev.filename ? ev.filename + ':' + ev.lineno + ':' + ev.colno : undefined,
      stack: ev.error && ev.error.stack ? clip(ev.error.stack, MAX_STACK) : undefined,
      uncaught: true,
    });
  }, true);

  addEventListener('unhandledrejection', function (ev) {
    var r = ev.reason;
    log('error', ['Unhandled promise rejection: ' + text(r)], {
      stack: r && r.stack ? clip(r.stack, MAX_STACK) : undefined,
      uncaught: true,
    });
  });

  function keepBody(type) { return local && /json|text|xml|javascript|html/.test(type || ''); }

  var originalFetch = window.fetch;
  if (originalFetch) {
    window.fetch = function (input, init) {
      var method = (init && init.method) || (input && input.method) || 'GET';
      var url = typeof input === 'string' ? input : (input && input.url) || String(input);
      var entry = push(dev.net, MAX_NET, { type: 'fetch', method: String(method).toUpperCase(), url: absolute(url), start: now() });
      return originalFetch.apply(this, arguments).then(function (res) {
        entry.status = res.status;
        entry.duration = now() - entry.start;
        entry.content_type = res.headers.get('content-type') || '';
        if (keepBody(entry.content_type)) {
          res.clone().text().then(function (b) { entry.body = clip(b, MAX_BODY); }, function () {});
        }
        return res;
      }, function (err) {
        entry.error = String(err);
        entry.duration = now() - entry.start;
        throw err;
      });
    };
  }

  var open = XMLHttpRequest.prototype.open, send = XMLHttpRequest.prototype.send;
  XMLHttpRequest.prototype.open = function (method, url) {
    this.__mbEntry = { type: 'xhr', method: String(method).toUpperCase(), url: absolute(url) };
    return open.apply(this, arguments);
  };
  XMLHttpRequest.prototype.send = function () {
    var xhr = this, entry = xhr.__mbEntry;
    if (entry) {
      entry.start = now();
      push(dev.net, MAX_NET, entry);
      xhr.addEventListener('loadend', function () {
        entry.status = xhr.status;
        entry.duration = now() - entry.start;
        entry.content_type = xhr.getResponseHeader('content-type') || '';
        if (xhr.status === 0) entry.error = 'Request gagal (jaringan, CORS, atau dibatalkan)';
        if ((xhr.responseType === '' || xhr.responseType === 'text') && keepBody(entry.content_type)) {
          entry.body = clip(xhr.responseText, MAX_BODY);
        }
      });
    }
    return send.apply(this, arguments);
  };
})();
