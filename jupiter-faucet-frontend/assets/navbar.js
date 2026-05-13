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
    const panelTriggers = Array.from(document.querySelectorAll("a[data-panel]"));
    const metricsToggle = document.getElementById("metrics-menu-toggle");
    const backdrop = document.getElementById("nav-panel-backdrop");
    const metricRail = document.getElementById("landing-live-summary");
    const closeBtn = document.querySelector(".nav-panel-close");
    const sections = Array.from(document.querySelectorAll(".nav-panel-section"));

    if (!navbar || panelTriggers.length === 0 || !backdrop || !closeBtn || sections.length === 0) {
      return;
    }

    let lastTriggerBtn = null;
    let activePanelKey = "";
    let metricsMenuOpen = false;
    let pointerDownOnBackdrop = false;

    function isMetricPanelKey(key) {
      return /^metric-/.test(key || "");
    }

    function syncMetricsUi() {
      const panelOpen = backdrop.classList.contains("is-open");
      const metricPanelOpen = panelOpen && isMetricPanelKey(activePanelKey);
      const shouldShowRail = navbar.classList.contains("navbar--visible") && metricsMenuOpen && !panelOpen;
      metricRail?.classList.toggle("metric-rail--visible", shouldShowRail);
      if (metricsToggle) {
        metricsToggle.classList.toggle("nav-item--active", metricsMenuOpen || metricPanelOpen);
        metricsToggle.setAttribute("aria-expanded", shouldShowRail ? "true" : "false");
      }
    }

    function focusControlsForSection(sectionEl) {
      if (!sectionEl) return;

      requestAnimationFrame(() => {
        const dot =
          sectionEl.querySelector(".nav-panel-dot.is-active") ||
          sectionEl.querySelector(".nav-panel-dot");
        (dot || closeBtn)?.focus?.();
      });
    }

    function updateNavbarVisibility() {
      const currentY = window.scrollY || 0;

      const setVisible = (visible) => {
        navbar.classList.toggle("navbar--visible", visible);
        syncMetricsUi();
      };

      if (backdrop.classList.contains("is-open")) {
        setVisible(true);
        lastScrollY = currentY;
        return;
      }

      if (currentY <= VISIBILITY_SCROLL_THRESHOLD) {
        setVisible(true);
        lastScrollY = currentY;
        return;
      }

      const delta = currentY - lastScrollY;
      if (Math.abs(delta) < SCROLL_DELTA_TOLERANCE) return;

      setVisible(delta <= 0);
      lastScrollY = currentY;
    }

    if (window.scrollY <= VISIBILITY_SCROLL_THRESHOLD) {
      navbar.classList.add("navbar--visible");
    }
    syncMetricsUi();
    window.addEventListener("scroll", updateNavbarVisibility, { passive: true });

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

    function activePageIndex(sectionEl) {
      const pages = Array.from(sectionEl?.querySelectorAll?.(".nav-panel-page") || []);
      const activeIndex = pages.findIndex((page) => page.classList.contains("is-active"));
      return activeIndex >= 0 ? activeIndex : 0;
    }

    function panelHashFor(key, pageIndex = 0) {
      return pageIndex > 0 ? `#${key}:${pageIndex}` : `#${key}`;
    }

    function isTextEditingTarget(target) {
      if (!target?.closest) return false;
      return Boolean(
        target.closest("input, textarea, select, [contenteditable]") ||
          target.isContentEditable
      );
    }

    function activatePage(sectionEl, pageIndex, { syncHash = false } = {}) {
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
      if (syncHash) {
        const key = sectionEl.getAttribute("data-panel");
        if (key) {
          const nextHash = panelHashFor(key, clamped);
          if (window.location.hash !== nextHash) history.pushState(null, "", nextHash);
        }
      }
    }

    backdrop.addEventListener("click", (evt) => {
      const pageLink = evt.target.closest && evt.target.closest("[data-page-target]");
      if (pageLink) {
        const sectionEl =
          pageLink.closest(".nav-panel-section") ||
          backdrop.querySelector(".nav-panel-section--active");
        const page = Number(pageLink.getAttribute("data-page-target"));
        if (sectionEl && Number.isFinite(page)) {
          evt.preventDefault();
          activatePage(sectionEl, page, { syncHash: true });
        }
        return;
      }

      const dot = evt.target.closest && evt.target.closest(".nav-panel-dot");
      if (!dot) return;

      const sectionEl = dot.closest(".nav-panel-section");
      const page = Number(dot.getAttribute("data-page"));
      if (!Number.isFinite(page)) return;

      activatePage(sectionEl, page, { syncHash: true });
    });

    backdrop.addEventListener("focusin", (evt) => {
      const dot = evt.target.closest?.(".nav-panel-dot");
      if (!dot) return;

      const sectionEl = dot.closest(".nav-panel-section");
      const page = Number(dot.getAttribute("data-page"));
      if (!Number.isFinite(page)) return;

      activatePage(sectionEl, page);
    });

    function handlePanelArrowKeydown(evt) {
      if (!backdrop.classList.contains("is-open")) return;
      if (evt.key !== "ArrowLeft" && evt.key !== "ArrowRight") return;
      if (isTextEditingTarget(evt.target) || isTextEditingTarget(document.activeElement)) return;

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

      activatePage(sectionEl, nextIndex, { syncHash: true });
      dots[nextIndex].focus();
      evt.preventDefault();
    }

    function clearPanelHash() {
      if (!window.location.hash) return;
      const cleanUrl = `${window.location.pathname}${window.location.search}`;
      history.replaceState(null, "", cleanUrl);
    }

    function openPanel(key) {
      if (!key) return;

      activePanelKey = key;
      if (isMetricPanelKey(key)) {
        metricsMenuOpen = true;
      }
      setActiveSection(key);
      backdrop.classList.add("is-open");
      document.body.classList.add("nav-panel-open");
      navbar.classList.add("navbar--visible");
      syncMetricsUi();

      const sectionEl = sections.find((s) => s.getAttribute("data-panel") === key);
      activatePage(sectionEl, activePageIndex(sectionEl));
      focusControlsForSection(sectionEl);
      document.dispatchEvent(new CustomEvent("navpanel:open", {
        detail: { key },
      }));
    }

    function closePanel({ syncHash = true, restoreFocus = true } = {}) {
      backdrop.classList.remove("is-open");
      document.body.classList.remove("nav-panel-open");
      activePanelKey = "";
      panelTriggers.forEach((btn) => btn.classList.remove("nav-item--active"));
      sections.forEach((section) => {
        section.classList.remove("nav-panel-section--active");
        section.querySelectorAll(".nav-panel-page").forEach((p) => p.classList.remove("is-active"));
        section.querySelectorAll(".nav-panel-dot").forEach((d) => {
          d.classList.remove("is-active");
          d.setAttribute("aria-selected", "false");
        });
      });

      if (syncHash) {
        clearPanelHash();
      }
      updateNavbarVisibility();
      syncMetricsUi();

      if (restoreFocus) {
        requestAnimationFrame(() => {
          lastTriggerBtn?.focus?.();
        });
      }
    }

    function handleTriggerClick(btn) {
      lastTriggerBtn = btn;
      const key = btn.getAttribute("data-panel");

      if (btn.classList.contains("nav-item--active") && backdrop.classList.contains("is-open")) {
        closePanel();
        return;
      }

      if (!isMetricPanelKey(key)) {
        metricsMenuOpen = false;
      }
      setActiveButton(key);
      openPanel(key);
    }

    panelTriggers.forEach((btn) => {
      btn.addEventListener("click", (evt) => {
        evt.preventDefault();
        const key = btn.getAttribute("data-panel");
        if (key && window.location.hash !== panelHashFor(key, 0)) {
          history.pushState(null, "", panelHashFor(key, 0));
        }
        handleTriggerClick(btn);
      });
    });

    metricsToggle?.addEventListener("click", (evt) => {
      evt.preventDefault();
      lastTriggerBtn = metricsToggle;
      if (backdrop.classList.contains("is-open")) {
        metricsMenuOpen = true;
        closePanel();
        syncMetricsUi();
        return;
      }
      metricsMenuOpen = !metricsMenuOpen;
      syncMetricsUi();
    });

    closeBtn.addEventListener("click", closePanel);

    backdrop.addEventListener("pointerdown", (evt) => {
      pointerDownOnBackdrop = evt.target === backdrop;
    });

    backdrop.addEventListener("click", (evt) => {
      const shouldClose = evt.target === backdrop && pointerDownOnBackdrop;
      pointerDownOnBackdrop = false;
      if (shouldClose) closePanel();
    });

    document.addEventListener("keydown", (evt) => {
      if (evt.key === "Escape" && backdrop.classList.contains("is-open")) closePanel();
    });
    document.addEventListener("keydown", handlePanelArrowKeydown);

    function panelTargetFromHash(hash) {
      const fragment = hash ? hash.replace(/^#/, "") : "";
      const route = fragment.split("?")[0];
      const pageMatch = route.match(/^([^:]+):(\d+)$/);
      const key = pageMatch ? pageMatch[1] : route;
      const page = pageMatch ? Number(pageMatch[2]) : 0;
      if (key.startsWith("metric-tracker-")) return { key: "metric-tracker", page: 0 };
      if (key.startsWith("simulator-")) return { key: "simulator", page: 0 };
      if (key === "metric-output") return { key: "metric-stake", page: 1 };
      if (key === "metric-rewards") return { key: "metric-stake", page: 2 };
      return { key, page: Number.isFinite(page) ? page : 0 };
    }

    function applyHash(hash) {
      const { key, page } = panelTargetFromHash(hash);
      if (!key) {
        if (backdrop.classList.contains("is-open")) {
          closePanel({ syncHash: false, restoreFocus: false });
        }
        return;
      }

      const matchingTrigger = panelTriggers.find((btn) => btn.getAttribute("data-panel") === key);
      const matchingSection = sections.find((section) => section.getAttribute("data-panel") === key);
      if (!matchingTrigger || !matchingSection) return;

      lastTriggerBtn = isMetricPanelKey(key) ? metricsToggle || matchingTrigger : matchingTrigger;
      if (isMetricPanelKey(key)) {
        metricsMenuOpen = true;
      }
      setActiveButton(key);
      openPanel(key);
      activatePage(matchingSection, page);
    }

    applyHash(window.location.hash);
    window.addEventListener("hashchange", () => applyHash(window.location.hash));
    window.addEventListener("popstate", () => applyHash(window.location.hash));

    let touchStartX = 0;
    let touchEndX = 0;

    function handleSwipe() {
      const activeSection = backdrop.querySelector(".nav-panel-section--active");
      if (!activeSection) return;

      const dots = Array.from(activeSection.querySelectorAll(".nav-panel-dot"));
      const activeDot = dots.find((dot) => dot.classList.contains("is-active"));
      if (!activeDot) return;

      const activeIndex = dots.indexOf(activeDot);

      if (touchStartX - touchEndX > SWIPE_THRESHOLD) {
        const nextIndex = (activeIndex + 1) % dots.length;
        activatePage(activeSection, nextIndex, { syncHash: true });
      } else if (touchEndX - touchStartX > SWIPE_THRESHOLD) {
        const prevIndex = (activeIndex - 1 + dots.length) % dots.length;
        activatePage(activeSection, prevIndex, { syncHash: true });
      }
    }

    backdrop.addEventListener("touchstart", (e) => {
      const touch = e.touches[0];
      touchStartX = touch.pageX;
    });

    backdrop.addEventListener("touchend", (e) => {
      const touch = e.changedTouches[0];
      touchEndX = touch.pageX;
      handleSwipe();
    });
  }

  document.addEventListener("DOMContentLoaded", initNavbar);
})();
