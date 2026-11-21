// Penangkap login untuk pengelola password. Dipasang di frame utama, di
// content world "mb-passwords": script halaman tidak bisa melihat kode ini
// atau mengirim pesan ke handler mbPassword, dan origin pengirim ditentukan
// WebKit dari frame-nya, bukan dari isi pesan.
//
// Saat form login dikirim (submit, klik tombol login, atau Enter di kolom
// password), username dan password dikirim ke aplikasi, yang lalu
// menawarkan untuk menyimpannya.
(function () {
  var lastIdentifier = '';
  var lastSent = null;
  var SKIP_BUTTON = /show|hide|tampil|sembunyi|lihat|reveal|toggle|cancel|batal|lupa|forgot/i;
  var LOGIN_BUTTON = /log ?in|sign ?in|masuk|login|continue|lanjut|next|berikut|submit|kirim/i;

  function isIdentifier(el) {
    var t = (el.type || 'text').toLowerCase();
    return el.tagName === 'INPUT' && (t === 'text' || t === 'email' || t === 'tel');
  }

  // Login bertahap (mis. Google): username diketik di langkah pertama,
  // password di langkah berikutnya tanpa kolom username.
  document.addEventListener('change', function (e) {
    if (isIdentifier(e.target) && e.target.value) lastIdentifier = e.target.value;
  }, true);

  function usernameFor(pw) {
    var scope = pw.form || document;
    var auto = scope.querySelector('input[autocomplete~="username"], input[autocomplete~="email"]');
    if (auto && auto.value) return auto.value;
    var inputs = Array.prototype.slice.call(scope.querySelectorAll('input'));
    for (var i = inputs.indexOf(pw) - 1; i >= 0; i--) {
      if (isIdentifier(inputs[i]) && inputs[i].value) return inputs[i].value;
    }
    return lastIdentifier;
  }

  // Form daftar atau ganti password punya beberapa kolom password; yang
  // disimpan adalah password baru.
  function pick(fields) {
    if (fields.length >= 3) return fields[1];
    if (fields.length === 2 && fields[0].value !== fields[1].value) return fields[1];
    return fields[0];
  }

  function capture(scope) {
    var fields = Array.prototype.filter.call((scope || document).querySelectorAll('input[type=password]'), function (el) {
      return el.value;
    });
    if (!fields.length) return;
    var pw = pick(fields);
    var username = usernameFor(pw);
    var key = username + '\u0000' + pw.value;
    if (key === lastSent) return;
    lastSent = key;
    webkit.messageHandlers.mbPassword.postMessage(JSON.stringify({
      username: String(username).slice(0, 256),
      password: String(pw.value).slice(0, 1024),
    }));
  }

  document.addEventListener('submit', function (e) { capture(e.target); }, true);
  document.addEventListener('click', function (e) {
    var button = e.target.closest && e.target.closest('button, input[type=submit], input[type=button], [role=button]');
    if (!button) return;
    var label = (button.innerText || button.value || '') + ' ' + (button.getAttribute('aria-label') || '');
    if (SKIP_BUTTON.test(label)) return;
    var form = button.form || button.closest('form');
    if (form) capture(form);
    else if (LOGIN_BUTTON.test(label)) capture(document);
  }, true);
  document.addEventListener('keydown', function (e) {
    if (e.key === 'Enter' && e.target && e.target.type === 'password') capture(e.target.form || document);
  }, true);
})();
