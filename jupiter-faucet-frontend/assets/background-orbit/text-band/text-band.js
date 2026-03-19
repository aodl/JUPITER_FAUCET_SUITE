// Builds and animates a 3D text band that fills 360° with no gaps by
// distributing each glyph proportionally to its measured width.
(function () {
  const TEXT = "UNSTOPPABLE   \u2022   ";
  const ROTATION_SPEED = -0.0005; // degrees per millisecond
  const FPS = 10;
  const FRAME_INTERVAL_MS = 1000 / FPS;

  // Keep this in sync with your CSS breakpoint:
  // @media (max-width: 1024px) { .band-container { display:none; } }
  const MOBILE_MAX = 1024;

  function initBand() {
    const rig = document.getElementById("rig");
    const band = document.getElementById("band");
    const meas = document.getElementById("measure");

    if (!rig || !band || !meas) return;

    const mqDesktop = window.matchMedia(`(min-width: ${MOBILE_MAX + 1}px)`);

    const state = {
      items: [],
      angles: [],
      builtForDesktop: false,
    };

    let lastFrameMs = 0;
    let rebuildQueued = false;

    // Initial static tilt
    const now = Date.now();
    const rockX = 25 * Math.sin(0.0000001 * now);
    const rockZ = 25 * Math.cos(0.0000001 * now * 0.9 + Math.PI / 6);
    rig.style.transform = `rotateX(${rockX}deg) rotateZ(${rockZ}deg)`;

    function isBandVisible() {
      // If hidden by display:none (mobile CSS), this will be 0
      return band.getClientRects().length > 0;
    }

    function renderAt(ts) {
      for (let i = 0; i < state.items.length; i++) {
        const theta = (ts * ROTATION_SPEED + state.angles[i]) % 360;
        const rad = (theta * Math.PI) / 180;
        const el = state.items[i];

        el.style.transform =
          `translate(-50%, -50%) rotateY(${theta}deg) translateZ(var(--radius))`;

        const vis = Math.max(0, Math.cos(rad));
        el.style.opacity = String(vis ** 0.9);
        el.style.filter = `blur(${(1 - vis) * 1.2}px)`;
      }
    }

    function rebuild() {
      // Only build on desktop / when visible
      if (!mqDesktop.matches || !isBandVisible()) return false;

      const parts = [...TEXT, ...TEXT];

      // 1) Measure first (do NOT clear existing band yet)
      meas.innerHTML = "";

      const measureSpans = parts.map((char) => {
        const span = document.createElement("span");
        span.className = "char"; // okay with your current CSS
        span.textContent = char;
        meas.appendChild(span);
        return span;
      });

      const widths = measureSpans.map((sp) => sp.getBoundingClientRect().width);

      // Abort if fonts/layout not ready yet; keep old band intact
      if (!widths.some((w) => w > 0)) {
        return false;
      }

      const total = widths.reduce((a, b) => a + b, 0) || 1;
      const degPerPx = 360 / total;

      // 2) Build off-DOM fragment
      const frag = document.createDocumentFragment();
      const newItems = [];
      const newAngles = [];

      let acc = 0;
      for (let i = 0; i < parts.length; i++) {
        const span = document.createElement("span");
        span.className = "char band-char";
        span.textContent = parts[i];

        newItems.push(span);
        newAngles.push(acc);
        acc += widths[i] * degPerPx;

        frag.appendChild(span);
      }

      // 3) Swap in one go
      band.innerHTML = "";
      band.appendChild(frag);

      state.items = newItems;
      state.angles = newAngles;
      state.builtForDesktop = true;

      // 4) Immediately position them so there's no "bunched" flash
      renderAt(Date.now());

      return true;
    }

    function queueRebuild() {
      if (rebuildQueued) return;
      rebuildQueued = true;

      requestAnimationFrame(() => {
        rebuildQueued = false;
        rebuild();
      });
    }

    function animate(nowMs) {
      if (nowMs - lastFrameMs >= FRAME_INTERVAL_MS) {
        lastFrameMs = nowMs;

        if (mqDesktop.matches && isBandVisible()) {
          // If loaded on mobile then later shown on desktop, build lazily here
          if (state.items.length === 0) {
            rebuild();
          } else {
            renderAt(Date.now());
          }
        }
      }

      requestAnimationFrame(animate);
    }

    // Initial build attempt (desktop only; harmless if mobile)
    rebuild();

    // Rebuild only when crossing breakpoint (mobile <-> desktop)
    const onMqChange = () => {
      if (mqDesktop.matches) {
        // Just became desktop-visible
        queueRebuild();
      } else {
        // Just became mobile-hidden (optional cleanup)
        state.items = [];
        state.angles = [];
        band.innerHTML = "";
        state.builtForDesktop = false;
      }
    };

    if (mqDesktop.addEventListener) {
      mqDesktop.addEventListener("change", onMqChange);
    } else if (mqDesktop.addListener) {
      // Older browser fallback
      mqDesktop.addListener(onMqChange);
    }

    // Rebuild once when fonts are ready (important for accurate glyph widths)
    if (document.fonts && document.fonts.ready) {
      document.fonts.ready.then(() => {
        if (mqDesktop.matches) queueRebuild();
      }).catch(() => {});
    }

    requestAnimationFrame(animate);
  }

  document.addEventListener("DOMContentLoaded", initBand);
})();
