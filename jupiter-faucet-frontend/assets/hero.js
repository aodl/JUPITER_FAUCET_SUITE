
(function () {
  const els = document.querySelectorAll(".bottom-fade-widget");

  // Values in vw units (e.g. 50vw means 50% of viewport width)
  const fadeStartVw = 10; // start fading when within 50vw of bottom
  const fadeEndVw = 0;   // fully visible when within 10vw of bottom

  function clamp(value, min, max) {
    return Math.min(Math.max(value, min), max);
  }

  function updateBottomFade() {
    const scrollTop = window.scrollY || window.pageYOffset;
    const viewportHeight = window.innerHeight;
    const fullHeight = document.documentElement.scrollHeight;
    const distanceToBottom = fullHeight - (scrollTop + viewportHeight);

    // Convert vw -> px
    const vwPx = window.innerWidth / 100;
    const fadeStart = fadeStartVw * vwPx;
    const fadeEnd = fadeEndVw * vwPx;

    const progress = clamp(
      (fadeStart - distanceToBottom) / (fadeStart - fadeEnd),
      0,
      1
    );

    els.forEach((el) => {
      el.style.opacity = progress;

      // Optional visibility toggle (so hidden elements are truly hidden)
      el.style.visibility = progress > 0 ? "visible" : "hidden";
      el.style.pointerEvents = progress > 0.05 ? "auto" : "none";
    });
  }

  window.addEventListener("scroll", updateBottomFade, { passive: true });
  window.addEventListener("resize", updateBottomFade);

  function initBottomFade() {
    requestAnimationFrame(updateBottomFade);
  }

  if (document.readyState === "complete") {
    initBottomFade();
  } else {
    window.addEventListener("load", initBottomFade, { once: true });
  }
})();
