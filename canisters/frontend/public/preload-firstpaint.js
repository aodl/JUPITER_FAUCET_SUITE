(() => {
  const mobile = window.matchMedia("(max-width: 1024px)").matches;
  const preload = document.createElement("link");
  preload.rel = "preload";
  preload.as = "image";
  preload.type = "image/jpeg";
  preload.href = mobile
    ? "background-orbit/background-orbit-mobile-firstpaint.jpg?v=__ASSET_VERSION__"
    : "background-orbit/background-orbit-firstpaint.jpg?v=__ASSET_VERSION__";
  preload.setAttribute("fetchpriority", "high");
  document.head.appendChild(preload);
})();
