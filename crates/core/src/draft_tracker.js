// Mencatat kolom yang benar-benar diketik pengguna (event input dengan
// isTrusted), supaya tab dengan teks yang belum dikirim tidak ditidurkan.
// Berjalan di content world terpisah: script halaman tidak melihat catatan
// ini. Isi teks tidak pernah dikirim ke mana pun; browser hanya menanyakan
// "masih ada draft?" lewat window.__mbDraft.pending().
(() => {
  if (window.__mbDraft) return;

  // Kolom satu baris yang menampung teks bebas. Password tidak dihitung.
  const TEXT_TYPES = new Set(['text', 'search', 'email', 'url', 'tel', 'number']);
  // Kolom -> URL saat terakhir diketik.
  const typed = new Map();

  const editingHost = (el) => {
    let host = el;
    while (host.parentElement && host.parentElement.isContentEditable) host = host.parentElement;
    return host;
  };

  const fieldOf = (target) => {
    if (target instanceof HTMLTextAreaElement) return target;
    if (target instanceof HTMLInputElement) return TEXT_TYPES.has(target.type) ? target : null;
    if (target instanceof HTMLElement && target.isContentEditable) return editingHost(target);
    return null;
  };

  // composedPath()[0]: elemen asli walaupun berada di dalam shadow DOM terbuka.
  const origin = (event) => (event.composedPath && event.composedPath()[0]) || event.target;

  document.addEventListener('input', (event) => {
    if (!event.isTrusted) return;
    const field = fieldOf(origin(event));
    if (field) typed.set(field, location.href);
  }, true);

  // Form dikirim: isinya sudah sampai ke situs.
  document.addEventListener('submit', (event) => {
    for (const field of typed.keys()) {
      if (event.target instanceof HTMLFormElement && event.target.contains(field)) typed.delete(field);
    }
  }, true);

  // Enter di kolom satu baris biasanya mengirim (pencarian, chat sederhana).
  document.addEventListener('keydown', (event) => {
    if (event.key !== 'Enter' || event.isComposing) return;
    const field = origin(event);
    if (field instanceof HTMLInputElement) typed.delete(field);
  }, true);

  const hasText = (field) => {
    const text = field.isContentEditable ? field.textContent : field.value;
    return /\S/.test(text || '');
  };

  const pending = () => {
    for (const [field, href] of typed) {
      // Kolom satu baris yang ditinggal karena halaman berpindah (misalnya
      // memilih saran pencarian) tidak dihitung; teks panjang di textarea
      // atau editor tetap dihitung selama kolomnya masih ada.
      const stale = field instanceof HTMLInputElement && href !== location.href;
      if (!field.isConnected || stale || !hasText(field)) {
        typed.delete(field);
        continue;
      }
      return true;
    }
    return false;
  };

  Object.defineProperty(window, '__mbDraft', { value: Object.freeze({ pending }) });
})();
