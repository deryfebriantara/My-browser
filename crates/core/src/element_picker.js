// Mode "Pick Element for Claude". Pengguna mengarahkan kursor (elemen di
// bawahnya disorot) lalu klik. Elemen itu disimpan di window.__mbPicked untuk
// dibaca get_selection. Esc membatalkan. Selama mode ini, klik dan tekan
// mouse tidak diteruskan ke halaman, supaya tombol atau link tidak ikut jalan.
(function () {
  if (window.__mbPicker) window.__mbPicker.stop();

  var ACCENT = '#d97757';
  var host = document.createElement('div');
  host.setAttribute('data-mb-overlay', '');
  host.style.cssText = 'position:fixed;inset:0;pointer-events:none;z-index:2147483647;';
  var root = host.attachShadow({ mode: 'closed' });
  root.innerHTML =
    '<style>' +
    '.box{position:fixed;display:none;border:2px solid ' + ACCENT + ';background:rgba(217,119,87,.14);border-radius:3px;box-sizing:border-box}' +
    '.tag,.hint{position:fixed;font:12px/1.4 -apple-system,system-ui,sans-serif;color:#fff;background:#1f1e1d;border-radius:6px;white-space:nowrap}' +
    '.tag{display:none;padding:3px 7px}' +
    '.hint{top:12px;left:50%;transform:translateX(-50%);padding:8px 12px;font-size:13px;box-shadow:0 4px 18px rgba(0,0,0,.3)}' +
    '</style><div class="box"></div><div class="tag"></div>' +
    '<div class="hint">Click an element to show Claude · Esc to cancel</div>';
  var box = root.querySelector('.box');
  var tag = root.querySelector('.tag');
  var hint = root.querySelector('.hint');
  (document.body || document.documentElement).appendChild(host);

  var current = null;

  function unique(sel) {
    try { return document.querySelectorAll(sel).length === 1; } catch (e) { return false; }
  }

  // Selector CSS yang menunjuk tepat satu elemen: id kalau unik, kalau tidak
  // jalur tag.kelas:nth-of-type dari elemen ke atas sampai unik.
  function selector(el) {
    if (el === document.body) return 'body';
    if (el.id && unique('#' + CSS.escape(el.id))) return '#' + CSS.escape(el.id);
    var parts = [];
    for (var e = el; e && e.nodeType === 1 && e !== document.documentElement; e = e.parentElement) {
      if (e !== el && e.id && unique('#' + CSS.escape(e.id))) {
        parts.unshift('#' + CSS.escape(e.id));
      } else {
        var part = e.tagName.toLowerCase();
        var classes = Array.prototype.filter.call(e.classList, function (c) { return c.length <= 40; }).slice(0, 2);
        if (classes.length) part += '.' + classes.map(CSS.escape).join('.');
        var parent = e.parentElement;
        if (parent) {
          var same = Array.prototype.filter.call(parent.children, function (c) { return c.tagName === e.tagName; });
          if (same.length > 1) part += ':nth-of-type(' + (same.indexOf(e) + 1) + ')';
        }
        parts.unshift(part);
      }
      var sel = parts.join(' > ');
      if (unique(sel)) return sel;
    }
    return parts.join(' > ');
  }

  function label(el) {
    var name = el.tagName.toLowerCase();
    if (el.id) name += '#' + el.id;
    var classes = Array.prototype.slice.call(el.classList, 0, 2);
    if (classes.length) name += '.' + classes.join('.');
    return name;
  }

  function show(el) {
    var r = el.getBoundingClientRect();
    box.style.display = 'block';
    box.style.left = r.left + 'px';
    box.style.top = r.top + 'px';
    box.style.width = r.width + 'px';
    box.style.height = r.height + 'px';
    tag.textContent = label(el) + '  ' + Math.round(r.width) + '×' + Math.round(r.height);
    tag.style.display = 'block';
    tag.style.left = Math.max(4, r.left) + 'px';
    tag.style.top = (r.top > 28 ? r.top - 26 : Math.min(r.bottom + 4, innerHeight - 26)) + 'px';
  }

  function target(ev) {
    var el = document.elementFromPoint(ev.clientX, ev.clientY);
    return el && el !== host ? el : null;
  }

  function block(ev) {
    ev.preventDefault();
    ev.stopPropagation();
    ev.stopImmediatePropagation();
  }

  function move(ev) {
    var el = target(ev);
    if (el && el !== current) {
      current = el;
      show(el);
    }
  }

  function scroll() {
    if (current) show(current);
  }

  function pick(ev) {
    block(ev);
    var el = target(ev) || current;
    if (!el) return;
    Object.defineProperty(window, '__mbPicked', {
      value: { el: el, selector: selector(el), at: Date.now() },
      configurable: true,
      writable: true,
      enumerable: false,
    });
    stop(true);
    acknowledge(el);
  }

  function key(ev) {
    if (ev.key === 'Escape') {
      block(ev);
      stop();
    }
  }

  var blocked = ['pointerdown', 'pointerup', 'mousedown', 'mouseup', 'dblclick', 'auxclick', 'contextmenu'];

  function stop(keepHost) {
    removeEventListener('mousemove', move, true);
    removeEventListener('click', pick, true);
    removeEventListener('keydown', key, true);
    removeEventListener('scroll', scroll, true);
    blocked.forEach(function (name) { removeEventListener(name, block, true); });
    if (!keepHost) host.remove();
    delete window.__mbPicker;
  }

  // Tanda singkat bahwa elemen sudah terpilih, lalu overlay dibuang.
  function acknowledge(el) {
    show(el);
    hint.textContent = 'Selected: ' + label(el) + ' · now tell Claude';
    setTimeout(function () { host.remove(); }, 1800);
  }

  addEventListener('mousemove', move, true);
  addEventListener('click', pick, true);
  addEventListener('keydown', key, true);
  addEventListener('scroll', scroll, true);
  blocked.forEach(function (name) { addEventListener(name, block, true); });
  Object.defineProperty(window, '__mbPicker', { value: { stop: stop }, configurable: true, enumerable: false });
})();
