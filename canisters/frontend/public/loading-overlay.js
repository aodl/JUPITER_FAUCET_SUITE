(function () {
  const overlay = document.getElementById("page-loading-overlay");
  const title = document.getElementById("page-loading-title");
  if (!overlay || !title) return;

  overlay.classList.add("is-active");

  function enableFullBackgroundAfterOverlayPaint() {
    const enable = function () {
      if (document.body) {
        document.body.classList.add("background-orbit-enhanced");
      }
    };

    if (typeof window.requestAnimationFrame === "function") {
      window.requestAnimationFrame(function () {
        window.requestAnimationFrame(enable);
      });
      return;
    }

    window.setTimeout(enable, 0);
  }

  enableFullBackgroundAfterOverlayPaint();

  const phrases = [
    "Infinite Cycles Begin Here",
    "Autonomous top-ups for unstoppable software",
    "Cycles keep canisters alive",
    "Keep every canister fueled"
  ];
  const startedAt = Date.now();
  const minVisibleMs = 2000;
  const maxVisibleMs = 60000;
  let phraseIndex = Math.floor(Math.random() * phrases.length);
  let progress = 8;
  let animationFrame = 0;
  let finished = false;

  title.textContent = phrases[phraseIndex];

  function setProgress(value) {
    progress = Math.max(progress, Math.min(100, value));
    overlay.style.setProperty("--loader-progress", progress.toFixed(1));
  }

  function animateProgress() {
    if (finished) return;

    const elapsedMs = Date.now() - startedAt;
    const easedProgress = 8 + (96 - 8) * (1 - Math.exp(-elapsedMs / 2600));
    setProgress(Math.min(96, easedProgress));
    animationFrame = window.requestAnimationFrame(animateProgress);
  }

  animationFrame = window.requestAnimationFrame(animateProgress);

  const phraseTimer = window.setInterval(function () {
    phraseIndex = (phraseIndex + 1) % phrases.length;
    title.classList.remove("is-swiping-in");
    title.classList.add("is-swiping-out");

    window.setTimeout(function () {
      title.textContent = phrases[phraseIndex];
      title.classList.remove("is-swiping-out");
      title.classList.add("is-swiping-in");
    }, 240);
  }, 2100);

  function finish() {
    if (finished) return;
    finished = true;
    window.cancelAnimationFrame(animationFrame);
    window.clearInterval(phraseTimer);
    window.clearTimeout(maxVisibleTimer);
    setProgress(100);

    const elapsedMs = Date.now() - startedAt;
    const waitMs = Math.max(0, Math.min(minVisibleMs - elapsedMs, maxVisibleMs - elapsedMs));
    window.setTimeout(function () {
      overlay.classList.add("is-fading");
      window.setTimeout(function () {
        overlay.remove();
      }, 1500);
    }, waitMs);
  }

  const maxVisibleTimer = window.setTimeout(finish, maxVisibleMs);

  if (document.readyState === "complete") {
    finish();
  } else {
    window.addEventListener("load", finish, { once: true });
  }
})();
