// navbar.js
// - Fades navbar in when scrolling up or near the top
// - Fades out when scrolling down
// - Clicking nav items opens panel sections
// - Each panel section can have 3 "pages" switched by dot buttons
// - Hash #about / #how-it-works auto-opens that section
// - Navbar stays visible whenever a pane is open
// - Swipe gestures to navigate between pages in touch devices

(function () {
  const VISIBILITY_SCROLL_THRESHOLD = 10; // px from top
  const SCROLL_DELTA_TOLERANCE = 4; // px before we consider it real movement
  const SWIPE_THRESHOLD = 50; // Minimum distance (in pixels) for a valid swipe

  let lastScrollY = window.scrollY || 0;

  function initNavbar() {
    const navbar = document.getElementById("navbar");
    // IMPORTANT: only bind buttons that actually open panels
    const panelTriggers = Array.from(document.querySelectorAll("a[data-panel]"));

    const backdrop = document.getElementById("nav-panel-backdrop");
    const closeBtn = document.querySelector(".nav-panel-close");
    const sections = Array.from(document.querySelectorAll(".nav-panel-section"));

    if (!navbar || panelTriggers.length === 0 || !backdrop || !closeBtn || sections.length === 0) {
      return;
    }

    // remember trigger to restore focus on close
    let lastTriggerBtn = null;

    // move focus into the opened panel (dot first, else close button)
    function focusControlsForSection(sectionEl) {
      if (!sectionEl) return;

      const dot =
        sectionEl.querySelector(".nav-panel-dot.is-active") ||
        sectionEl.querySelector(".nav-panel-dot");

      requestAnimationFrame(() => {
        (dot || closeBtn)?.focus?.();
      });
    }

    // ---- Scroll behaviour: show/hide navbar ----
    function updateNavbarVisibility() {
      const currentY = window.scrollY || 0;

      // If a pane is open, navbar must always be visible
      if (backdrop.classList.contains("is-open")) {
        navbar.classList.add("navbar--visible");
        lastScrollY = currentY;
        return;
      }

      // Always show near the very top of the page
      if (currentY <= VISIBILITY_SCROLL_THRESHOLD) {
        navbar.classList.add("navbar--visible");
        lastScrollY = currentY;
        return;
      }

      const delta = currentY - lastScrollY;

      if (Math.abs(delta) < SCROLL_DELTA_TOLERANCE) return;

      if (delta > 0) {
        navbar.classList.remove("navbar--visible"); // scrolling down
      } else {
        navbar.classList.add("navbar--visible"); // scrolling up
      }

      lastScrollY = currentY;
    }

    if (window.scrollY <= VISIBILITY_SCROLL_THRESHOLD) {
      navbar.classList.add("navbar--visible");
    }
    window.addEventListener("scroll", updateNavbarVisibility, { passive: true });

    // ---- Helpers: section + active button ----
    function setActiveSection(key) {
      sections.forEach((section) => {
        section.classList.toggle(
          "nav-panel-section--active",
          section.getAttribute("data-panel") === key
        );
      });
    }

    function setActiveButton(key) {
      panelTriggers.forEach((btn) => {
        btn.classList.toggle("nav-item--active", btn.getAttribute("data-panel") === key);
      });
    }

    // ---- Pagination (dots) ----
    function activatePage(sectionEl, pageIndex) {
      if (!sectionEl) return;

      const pages = Array.from(sectionEl.querySelectorAll(".nav-panel-page"));
      const dots = Array.from(sectionEl.querySelectorAll(".nav-panel-dot"));

      if (pages.length === 0 || dots.length === 0) return;

      const clamped = Math.max(0, Math.min(pageIndex, pages.length - 1));

      pages.forEach((p, i) => p.classList.toggle("is-active", i === clamped));
      dots.forEach((d, i) => {
        const isActive = i === clamped;
        d.classList.toggle("is-active", isActive);
        d.setAttribute("aria-selected", isActive ? "true" : "false");
      });
    }

    // Dot clicks (delegated)
    backdrop.addEventListener("click", (evt) => {
      const dot = evt.target.closest && evt.target.closest(".nav-panel-dot");
      if (!dot) return;

      const sectionEl = dot.closest(".nav-panel-section");
      const page = Number(dot.getAttribute("data-page"));
      if (!Number.isFinite(page)) return;

      activatePage(sectionEl, page);
    });

    backdrop.addEventListener("focusin", (evt) => {
      const dot = evt.target.closest?.(".nav-panel-dot");
      if (!dot) return;

      const sectionEl = dot.closest(".nav-panel-section");
      const page = Number(dot.getAttribute("data-page"));
      if (!Number.isFinite(page)) return;

      activatePage(sectionEl, page);
    });

    // Arrow-key support (left/right) for paging dots
    backdrop.addEventListener("keydown", (evt) => {
      if (!backdrop.classList.contains("is-open")) return;

      if (evt.key !== "ArrowLeft" && evt.key !== "ArrowRight") return;

      // Prefer the section containing the currently focused dot, otherwise the active panel
      const focusedDot = document.activeElement?.closest?.(".nav-panel-dot");
      const sectionEl =
        focusedDot?.closest?.(".nav-panel-section") ||
        backdrop.querySelector(".nav-panel-section--active");

      if (!sectionEl) return;

      const dots = Array.from(sectionEl.querySelectorAll(".nav-panel-dot"));
      if (dots.length === 0) return;

      const activeIndex = Math.max(
        0,
        dots.findIndex(
          (d) => d.classList.contains("is-active") || d.getAttribute("aria-selected") === "true"
        )
      );

      const dir = evt.key === "ArrowRight" ? 1 : -1;
      const nextIndex = (activeIndex + dir + dots.length) % dots.length;

      activatePage(sectionEl, nextIndex);

      // Keep focus on the control the user is interacting with
      dots[nextIndex].focus();

      evt.preventDefault();
    });

    // ---- Open/close panel ----
    function openPanel(key) {
      if (!key) return;

      setActiveSection(key);
      backdrop.classList.add("is-open");
      navbar.classList.add("navbar--visible");

      // Reset to page 1 whenever opening a section
      const sectionEl = sections.find((s) => s.getAttribute("data-panel") === key);
      activatePage(sectionEl, 0);

      // move focus into the dialog so keyboard works immediately
      focusControlsForSection(sectionEl);
    }

    function closePanel() {
      backdrop.classList.remove("is-open");
      panelTriggers.forEach((btn) => btn.classList.remove("nav-item--active"));
      sections.forEach((section) => {
        section.classList.remove("nav-panel-section--active");
        // optional: clear page state
        section.querySelectorAll(".nav-panel-page").forEach((p) => p.classList.remove("is-active"));
        section.querySelectorAll(".nav-panel-dot").forEach((d) => {
          d.classList.remove("is-active");
          d.setAttribute("aria-selected", "false");
        });
      });

      updateNavbarVisibility();

      // restore focus to the trigger button that opened the panel
      requestAnimationFrame(() => {
        lastTriggerBtn?.focus?.();
      });
    }

    function handleTriggerClick(btn) {
      // remember for focus restore
      lastTriggerBtn = btn;

      const key = btn.getAttribute("data-panel");

      if (btn.classList.contains("nav-item--active") && backdrop.classList.contains("is-open")) {
        closePanel();
        return;
      }

      setActiveButton(key);
      openPanel(key);
    }

    panelTriggers.forEach((btn) => {
      btn.addEventListener("click", () => handleTriggerClick(btn));
    });

    closeBtn.addEventListener("click", closePanel);

    // Close when clicking outside the panel (but not when clicking inside it)
    backdrop.addEventListener("click", (evt) => {
      if (evt.target === backdrop) closePanel();
    });

    document.addEventListener("keydown", (evt) => {
      if (evt.key === "Escape" && backdrop.classList.contains("is-open")) closePanel();
    });

    // ---- Hash fragment support ----
    function applyHash(hash) {
      const key = hash ? hash.replace(/^#/, "") : "";
      if (!key) return;

      const matchingTrigger = panelTriggers.find((btn) => btn.getAttribute("data-panel") === key);
      const matchingSection = sections.find((section) => section.getAttribute("data-panel") === key);
      if (!matchingTrigger || !matchingSection) return;

      // so close restores focus reasonably after hash-open
      lastTriggerBtn = matchingTrigger;

      setActiveButton(key);
      openPanel(key);
    }

    applyHash(window.location.hash);
    window.addEventListener("hashchange", () => applyHash(window.location.hash));

    // ---- Swipe navigation for touch devices ----
    let touchStartX = 0;  // Store starting X position of touch
    let touchEndX = 0;    // Store ending X position of touch

    // Function to detect swipe direction and trigger navigation
    function handleSwipe() {
      const activeSection = backdrop.querySelector(".nav-panel-section--active");
      const dots = Array.from(activeSection.querySelectorAll(".nav-panel-dot"));
      const activeDot = dots.find((dot) => dot.classList.contains("is-active"));

      if (!activeDot) return;

      const activeIndex = dots.indexOf(activeDot);

      // Determine swipe direction
      if (touchStartX - touchEndX > SWIPE_THRESHOLD) {
        // Swipe left -> go to the next section
        const nextIndex = (activeIndex + 1) % dots.length;  // Wrap around
        activatePage(activeSection, nextIndex);
      } else if (touchEndX - touchStartX > SWIPE_THRESHOLD) {
        // Swipe right -> go to the previous section
        const prevIndex = (activeIndex - 1 + dots.length) % dots.length; // Wrap around
        activatePage(activeSection, prevIndex);
      }
    }

    // Handle touch start (record starting position)
    backdrop.addEventListener("touchstart", (e) => {
      const touch = e.touches[0];
      touchStartX = touch.pageX;
    });

    // Handle touch end (calculate swipe direction)
    backdrop.addEventListener("touchend", (e) => {
      const touch = e.changedTouches[0];
      touchEndX = touch.pageX;
      handleSwipe();
    });
  }

  document.addEventListener("DOMContentLoaded", initNavbar);
})();
