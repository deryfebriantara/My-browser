// Pemblokir bawaan: melewati iklan video YouTube.
//
// Iklan video YouTube dikirim dari server yang sama dengan videonya, jadi
// content blocker tidak bisa memblokirnya. Script ini dipasang di setiap
// halaman (WKUserScript tidak bisa dibatasi per situs) tetapi hanya bekerja
// di youtube.com, termasuk player YouTube yang disematkan di situs lain.
//
// Selama player menandai iklan (kelas `ad-showing`): tombol "Lewati" diklik
// begitu muncul, dan video dibisukan serta dipercepat 16x supaya iklan yang
// tidak bisa dilewati selesai dalam satu-dua detik. Setelah iklan selesai,
// kecepatan dan suara dikembalikan seperti sebelum iklan.
(function () {
  if (!/(^|\.)youtube\.com$/.test(location.hostname) || window.__mbAdSkip) return;
  window.__mbAdSkip = true;

  var SKIP = [
    '.ytp-skip-ad-button',
    '.ytp-ad-skip-button',
    '.ytp-ad-skip-button-modern',
    '.ytp-ad-skip-button-slot button',
  ].join(',');
  // Keadaan video pengguna yang terakhir terlihat di luar iklan.
  var user = null;
  var inAd = false;

  function restore(player, video) {
    var rate = user ? user.rate : null;
    var muted = user ? user.muted : null;
    if (rate === null && typeof player.getPlaybackRate === 'function') rate = player.getPlaybackRate();
    if (muted === null && typeof player.isMuted === 'function') muted = player.isMuted();
    video.playbackRate = rate || 1;
    video.muted = !!muted;
  }

  function tick() {
    var overlay = document.querySelector('.ytp-ad-overlay-close-button');
    if (overlay) overlay.click();

    var player = document.querySelector('.html5-video-player');
    if (!player) return;
    var video = player.querySelector('video');
    if (player.classList.contains('ad-showing')) {
      inAd = true;
      var skip = player.querySelector(SKIP);
      if (skip) skip.click();
      if (video) {
        video.muted = true;
        video.playbackRate = 16;
      }
      return;
    }
    if (!video) return;
    if (inAd) {
      inAd = false;
      restore(player, video);
    }
    user = { rate: video.playbackRate, muted: video.muted };
  }

  setInterval(tick, 250);
})();
