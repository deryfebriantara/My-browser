// Pengisi login: badan fungsi async yang dijalankan di content world
// "mb-passwords" pada frame utama. Argumen: origin, username, password.
// Hanya mengisi kalau halaman masih di origin yang sama dengan entri, jadi
// password tidak pernah terisi di situs lain walaupun halaman berpindah
// sejak pengguna memilih entri.
if (location.origin !== origin) {
  return JSON.stringify({ error: 'Halaman sudah berpindah ke situs lain; password tidak diisi.' });
}
function visible(el) {
  var r = el.getBoundingClientRect(), s = getComputedStyle(el);
  return r.width > 0 && r.height > 0 && s.visibility !== 'hidden' && s.display !== 'none' && !el.disabled && !el.readOnly;
}
function isIdentifier(el) {
  var t = (el.type || 'text').toLowerCase();
  return t === 'text' || t === 'email' || t === 'tel';
}
// Setter bawaan + event input/change, supaya framework seperti React ikut
// melihat nilainya.
function set(el, value) {
  var setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set;
  el.focus();
  setter.call(el, value);
  el.dispatchEvent(new Event('input', { bubbles: true }));
  el.dispatchEvent(new Event('change', { bubbles: true }));
}
var pw = Array.prototype.filter.call(document.querySelectorAll('input[type=password]'), visible)[0] || null;
var scope = (pw && pw.form) || document;
var user = null;
var auto = scope.querySelector('input[autocomplete~="username"], input[autocomplete~="email"]');
if (auto && visible(auto)) user = auto;
if (!user) {
  var inputs = Array.prototype.slice.call(scope.querySelectorAll('input'));
  for (var i = (pw ? inputs.indexOf(pw) : inputs.length) - 1; i >= 0; i--) {
    if (isIdentifier(inputs[i]) && visible(inputs[i])) { user = inputs[i]; break; }
  }
}
if (!pw && !(user && username)) {
  return JSON.stringify({ error: 'Tidak ada kolom login di halaman ini.' });
}
if (user && username) set(user, username);
if (pw) set(pw, password);
return JSON.stringify({ filled_password: !!pw, filled_username: !!(user && username) });
