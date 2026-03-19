// background-orbit.js
(function () {
  // LCM of 400, 600, 1200 = 1200s
  const PERIOD_SECONDS = 1200;

  function init() {
    const svg = document.getElementById("background-orbit");
    if (!svg || typeof svg.setCurrentTime !== "function") {
      return;
    }

    const nowSeconds = Date.now() / 1000;
    const t = nowSeconds % PERIOD_SECONDS;
    svg.setCurrentTime(t);
  }

  document.addEventListener("DOMContentLoaded", init);
})();
