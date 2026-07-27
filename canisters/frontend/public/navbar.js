// navbar.js
// - Fades navbar in when scrolling up or near the top
// - Clicking nav items opens panel sections
// - Each panel section can have pages switched by dot buttons
// - Hash routes open content panels, while transient dropdowns do not alter the hash
// - Navbar stays visible whenever a menu or pane is open
// - Swipe gestures navigate between pages on touch devices

(function () {
  const VISIBILITY_SCROLL_THRESHOLD = 10; // px from top
  const SCROLL_DELTA_TOLERANCE = 4; // px before we consider it real movement
  const SWIPE_THRESHOLD = 50; // Minimum distance (in pixels) for a valid swipe
  const CLOSED_NAV_STATE = Object.freeze({
    openMenu: null,
    openPanel: null,
    panelOwner: null,
    panelPage: 0,
  });

  let lastScrollY = window.scrollY || 0;

  function initNavbar() {
    const navbar = document.getElementById("navbar");
    const brandLink = document.querySelector(".nav-brand");
    const panelTriggers = Array.from(document.querySelectorAll("a[data-panel]"));
    const actionsToggle = document.getElementById("actions-menu-toggle");
    const metricsToggle = document.getElementById("metrics-menu-toggle");
    const actionsMenu = document.getElementById("actions-menu");
    const metricsMenu = document.getElementById("metrics-menu");
    const backdrop = document.getElementById("nav-panel-backdrop");
    const closeBtn = document.querySelector(".nav-panel-close");
    const sections = Array.from(document.querySelectorAll(".nav-panel-section"));

    if (!navbar || panelTriggers.length === 0 || !backdrop || !closeBtn || sections.length === 0) {
      return;
    }

    let lastTriggerBtn = null;
    let pointerDownOnBackdrop = false;
    let navState = { ...CLOSED_NAV_STATE };
    let lastAppliedHash = "";
    let renderedPanelKey = null;
    let renderedPanelPage = null;

    function isMetricPanelKey(key) {
      return /^metric-/.test(key || "");
    }

    function groupForPanelRoute(key, page = 0, trigger = null) {
      const disclosure = trigger?.closest?.("[data-nav-group]");
      const triggerGroup = disclosure?.getAttribute?.("data-nav-group");
      if (triggerGroup === "actions" || triggerGroup === "metrics") return triggerGroup;
      if (isMetricPanelKey(key)) return "metrics";
      if (
        key === "simulator" ||
        key === "relay-setup" ||
        key === "memo-builder" ||
        (key === "how-it-works" && page === 3)
      ) {
        return "actions";
      }
      return null;
    }

    function activeSection() {
      return sections.find((section) => section.getAttribute("data-panel") === navState.openPanel);
    }

    function panelHashFor(key, pageIndex = 0) {
      return pageIndex > 0 ? `#${key}:${pageIndex}` : `#${key}`;
    }

    function setMenuHidden(menu, hidden) {
      if (!menu) return;
      menu.hidden = hidden;
    }

    function hasHiddenAncestor(node) {
      for (let current = node; current; current = current.parentElement) {
        if (current.hidden) return true;
      }
      return false;
    }

    function canRestoreFocus(node) {
      if (!node || typeof node.focus !== "function") return false;
      if (node.isConnected === false) return false;
      if (hasHiddenAncestor(node)) return false;
      return true;
    }

    function focusReturnTarget(trigger) {
      const group = trigger
        ?.closest?.("[data-nav-group]")
        ?.getAttribute?.("data-nav-group");
      if (group === "actions") return actionsToggle;
      if (group === "metrics") return metricsToggle;
      return trigger;
    }

    function clearPanelPages(section) {
      section.querySelectorAll(".nav-panel-page").forEach((page) => {
        page.classList.remove("is-active");
      });
      section.querySelectorAll(".nav-panel-dot").forEach((dot) => {
        dot.classList.remove("is-active");
        dot.setAttribute("aria-selected", "false");
      });
    }

    function activatePage(sectionEl, pageIndex, { syncHash = false, emitPageChange = true } = {}) {
      if (!sectionEl) return 0;

      const pages = Array.from(sectionEl.querySelectorAll(".nav-panel-page"));
      const dots = Array.from(sectionEl.querySelectorAll(".nav-panel-dot"));
      if (pages.length === 0 || dots.length === 0) return 0;

      const clamped = Math.max(0, Math.min(pageIndex, pages.length - 1));
      pages.forEach((page, index) => page.classList.toggle("is-active", index === clamped));
      dots.forEach((dot, index) => {
        const isActive = index === clamped;
        dot.classList.toggle("is-active", isActive);
        dot.setAttribute("aria-selected", isActive ? "true" : "false");
      });

      if (syncHash) {
        const key = sectionEl.getAttribute("data-panel");
        const nextHash = key ? panelHashFor(key, clamped) : "";
        if (nextHash && window.location.hash !== nextHash) {
          history.pushState(null, "", nextHash);
          lastAppliedHash = nextHash;
        }
      }

      if (emitPageChange) {
        document.dispatchEvent(new CustomEvent("navpanel:pagechange", {
          detail: {
            key: sectionEl.getAttribute("data-panel"),
            page: clamped,
          },
        }));
      }
      return clamped;
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

    function renderNavState({ focusPanel = false, syncPanelHash = false } = {}) {
      const panelOpen = Boolean(navState.openPanel);
      const actionsDisclosureVisible = navState.openMenu === "actions";
      const metricsDisclosureVisible = navState.openMenu === "metrics";
      const openingPanelKey = panelOpen && navState.openPanel !== renderedPanelKey
        ? navState.openPanel
        : null;
      const panelRouteChanged = panelOpen &&
        (navState.openPanel !== renderedPanelKey || navState.panelPage !== renderedPanelPage);
      const panelOwner = panelOpen
        ? groupForPanelRoute(navState.openPanel, navState.panelPage)
        : navState.panelOwner;

      setMenuHidden(actionsMenu, !actionsDisclosureVisible);
      setMenuHidden(metricsMenu, !metricsDisclosureVisible);

      actionsToggle?.setAttribute("aria-expanded", actionsDisclosureVisible ? "true" : "false");
      metricsToggle?.setAttribute("aria-expanded", metricsDisclosureVisible ? "true" : "false");
      actionsToggle?.classList.toggle(
        "nav-item--active",
        actionsDisclosureVisible || panelOwner === "actions"
      );
      metricsToggle?.classList.toggle(
        "nav-item--active",
        metricsDisclosureVisible || panelOwner === "metrics"
      );

      panelTriggers.forEach((trigger) => {
        trigger.classList.toggle(
          "nav-item--active",
          Boolean(navState.openPanel) &&
            trigger.getAttribute("data-panel") === navState.openPanel
        );
      });

      sections.forEach((section) => {
        const isActive = section.getAttribute("data-panel") === navState.openPanel;
        section.classList.toggle("nav-panel-section--active", isActive);
        if (!isActive) clearPanelPages(section);
      });

      backdrop.classList.toggle("is-open", panelOpen);
      document.body.classList.toggle("nav-panel-open", panelOpen);
      document.body.classList.toggle("metrics-menu-open", metricsDisclosureVisible);
      if (actionsDisclosureVisible || metricsDisclosureVisible || panelOpen) {
        navbar.classList.add("navbar--visible");
      }

      if (panelOpen) {
        const sectionEl = activeSection();
        navState.panelPage = activatePage(sectionEl, navState.panelPage, {
          syncHash: syncPanelHash,
          emitPageChange: panelRouteChanged,
        });
        navState.panelOwner = groupForPanelRoute(navState.openPanel, navState.panelPage);
        if (focusPanel) focusControlsForSection(sectionEl);
        if (openingPanelKey) {
          document.dispatchEvent(new CustomEvent("navpanel:open", {
            detail: { key: navState.openPanel },
          }));
        }
        renderedPanelKey = navState.openPanel;
        renderedPanelPage = navState.panelPage;
      } else {
        renderedPanelKey = null;
        renderedPanelPage = null;
      }
    }

    function clearPanelHash() {
      if (!window.location.hash) return;
      const cleanUrl = `${window.location.pathname}${window.location.search}`;
      history.replaceState(null, "", cleanUrl);
      lastAppliedHash = "";
    }

    function setClosedState({ syncHash = true, restoreFocus = true } = {}) {
      const previousOwner = navState.panelOwner ||
        groupForPanelRoute(navState.openPanel, navState.panelPage);
      navState = { ...CLOSED_NAV_STATE };
      if (syncHash) clearPanelHash();
      renderNavState();
      updateNavbarVisibility();
      if (restoreFocus) {
        requestAnimationFrame(() => {
          const fallback =
            previousOwner === "actions"
              ? actionsToggle
              : previousOwner === "metrics"
                ? metricsToggle
                : closeBtn;
          const target = canRestoreFocus(lastTriggerBtn) ? lastTriggerBtn : fallback;
          if (canRestoreFocus(target)) target.focus();
        });
      }
    }

    function setMenuState(group) {
      if (navState.openPanel) clearPanelHash();
      navState = {
        ...CLOSED_NAV_STATE,
        openMenu: group,
      };
      renderNavState();
    }

    function setPanelState(key, page = 0, owner = null, { syncHash = true, focusPanel = true, hashOverride = "" } = {}) {
      if (!key) return;
      navState = {
        openMenu: null,
        openPanel: key,
        panelOwner: owner || groupForPanelRoute(key, page),
        panelPage: page,
      };
      if (syncHash) {
        const nextHash = hashOverride || panelHashFor(key, page);
        if (nextHash && window.location.hash !== nextHash) {
          history.pushState(null, "", nextHash);
          lastAppliedHash = nextHash;
        }
      }
      renderNavState({ focusPanel });
    }

    function updateNavbarVisibility() {
      const currentY = window.scrollY || 0;

      const setVisible = (visible) => {
        navbar.classList.toggle("navbar--visible", visible);
      };

      if (navState.openMenu || navState.openPanel) {
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
    renderNavState();
    window.addEventListener("scroll", updateNavbarVisibility, { passive: true });

    function isTextEditingTarget(target) {
      if (!target?.closest) return false;
      return Boolean(
        target.closest("input, textarea, select, [contenteditable]") ||
          target.isContentEditable
      );
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
          navState.panelPage = page;
          navState.panelOwner = groupForPanelRoute(navState.openPanel, page);
          renderNavState({ syncPanelHash: true });
        }
        return;
      }

      const dot = evt.target.closest && evt.target.closest(".nav-panel-dot");
      if (!dot) return;

      const sectionEl = dot.closest(".nav-panel-section");
      const page = Number(dot.getAttribute("data-page"));
      if (!Number.isFinite(page)) return;

      navState.panelPage = page;
      navState.panelOwner = groupForPanelRoute(navState.openPanel, page);
      renderNavState({ syncPanelHash: true });
    });

    backdrop.addEventListener("focusin", (evt) => {
      const dot = evt.target.closest?.(".nav-panel-dot");
      if (!dot) return;

      const sectionEl = dot.closest(".nav-panel-section");
      const page = Number(dot.getAttribute("data-page"));
      if (!Number.isFinite(page)) return;

      navState.panelPage = page;
      navState.panelOwner = groupForPanelRoute(navState.openPanel, page);
      renderNavState();
    });

    function handlePanelArrowKeydown(evt) {
      if (!navState.openPanel) return;
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
          (dot) => dot.classList.contains("is-active") || dot.getAttribute("aria-selected") === "true"
        )
      );
      const dir = evt.key === "ArrowRight" ? 1 : -1;
      const nextIndex = (activeIndex + dir + dots.length) % dots.length;

      navState.panelPage = nextIndex;
      navState.panelOwner = groupForPanelRoute(navState.openPanel, nextIndex);
      renderNavState({ syncPanelHash: true });
      dots[nextIndex].focus();
      evt.preventDefault();
    }

    function panelTargetFromHash(hash) {
      const hashText = String(hash || "");
      const hashStart = hashText.indexOf("#");
      const fragment = hashStart >= 0 ? hashText.slice(hashStart + 1) : hashText;
      const fullHash = fragment ? `#${fragment}` : "";
      const route = fragment.split("?")[0];
      const pageMatch = route.match(/^([^:]+):(\d+)$/);
      const key = pageMatch ? pageMatch[1] : route;
      const page = pageMatch ? Number(pageMatch[2]) : 0;
      if (key.startsWith("metric-tracker-")) return { key: "metric-tracker", page: 0, hash: fullHash };
      if (key.startsWith("simulator-")) return { key: "simulator", page: 0, hash: fullHash };
      if (key === "metric-registered") return { key: "metric-commitments", page: 0, hash: fullHash };
      if (key === "metric-output") return { key: "metric-stake", page: 1, hash: fullHash };
      if (key === "metric-rewards") return { key: "metric-stake", page: 2, hash: fullHash };
      return { key, page: Number.isFinite(page) ? page : 0, hash: fullHash };
    }

    function applyHash(hash) {
      if (hash === lastAppliedHash) return;
      lastAppliedHash = hash || "";
      const { key, page } = panelTargetFromHash(hash);
      if (!key) {
        navState = { ...CLOSED_NAV_STATE };
        renderNavState();
        updateNavbarVisibility();
        return;
      }

      const matchingSection = sections.find((section) => section.getAttribute("data-panel") === key);
      if (!matchingSection) return;

      const owner = groupForPanelRoute(key, page);
      lastTriggerBtn =
        owner === "metrics"
          ? metricsToggle
          : owner === "actions"
            ? actionsToggle
            : panelTriggers.find((trigger) => trigger.getAttribute("data-panel") === key);
      setPanelState(key, page, owner, { syncHash: false, focusPanel: false });
    }

    panelTriggers.forEach((trigger) => {
      trigger.addEventListener("click", (evt) => {
        evt.preventDefault();
        const key = trigger.getAttribute("data-panel");
        const hrefTarget = panelTargetFromHash(trigger.getAttribute("href"));
        const page = hrefTarget.key === key ? hrefTarget.page : 0;
        const owner = groupForPanelRoute(key, page, trigger);
        const isDirectNavbarTrigger = Boolean(trigger.closest("#navbar")) &&
          !trigger.closest(".nav-popover");
        const samePanel = navState.openPanel === key && navState.panelPage === page;
        lastTriggerBtn = focusReturnTarget(trigger);
        if (isDirectNavbarTrigger && samePanel) {
          setClosedState();
          return;
        }
        setPanelState(key, page, owner, { hashOverride: hrefTarget.hash });
      });
    });

    actionsToggle?.addEventListener("click", (evt) => {
      evt.preventDefault();
      lastTriggerBtn = actionsToggle;
      if (navState.openMenu === "actions" || navState.panelOwner === "actions") {
        setClosedState();
        return;
      }
      setMenuState("actions");
    });

    metricsToggle?.addEventListener("click", (evt) => {
      evt.preventDefault();
      lastTriggerBtn = metricsToggle;
      if (navState.openMenu === "metrics" || navState.panelOwner === "metrics") {
        setClosedState();
        return;
      }
      setMenuState("metrics");
    });

    brandLink?.addEventListener("click", () => {
      setClosedState({ syncHash: false, restoreFocus: false });
    });

    closeBtn.addEventListener("click", () => setClosedState());

    backdrop.addEventListener("pointerdown", (evt) => {
      pointerDownOnBackdrop = evt.target === backdrop;
    });

    backdrop.addEventListener("click", (evt) => {
      const shouldClose = evt.target === backdrop && pointerDownOnBackdrop;
      pointerDownOnBackdrop = false;
      if (shouldClose) setClosedState();
    });

    document.addEventListener("click", (evt) => {
      if (!navState.openMenu) return;
      const openDisclosure = document.querySelector(
        `.nav-disclosure[data-nav-group="${navState.openMenu}"]`
      );
      if (openDisclosure?.contains?.(evt.target)) return;
      setClosedState({ syncHash: false, restoreFocus: false });
    });

    document.addEventListener("keydown", (evt) => {
      if (evt.key === "Escape" && (navState.openMenu || navState.openPanel)) {
        setClosedState();
      }
    });
    document.addEventListener("keydown", handlePanelArrowKeydown);

    applyHash(window.location.hash);
    window.addEventListener("hashchange", () => applyHash(window.location.hash));
    window.addEventListener("popstate", () => applyHash(window.location.hash));

    let touchStartX = 0;
    let touchEndX = 0;

    function handleSwipe() {
      const sectionEl = backdrop.querySelector(".nav-panel-section--active");
      if (!sectionEl) return;

      const dots = Array.from(sectionEl.querySelectorAll(".nav-panel-dot"));
      const activeDot = dots.find((dot) => dot.classList.contains("is-active"));
      if (!activeDot) return;

      const activeIndex = dots.indexOf(activeDot);
      if (touchStartX - touchEndX > SWIPE_THRESHOLD) {
        navState.panelPage = (activeIndex + 1) % dots.length;
        navState.panelOwner = groupForPanelRoute(navState.openPanel, navState.panelPage);
        renderNavState({ syncPanelHash: true });
      } else if (touchEndX - touchStartX > SWIPE_THRESHOLD) {
        navState.panelPage = (activeIndex - 1 + dots.length) % dots.length;
        navState.panelOwner = groupForPanelRoute(navState.openPanel, navState.panelPage);
        renderNavState({ syncPanelHash: true });
      }
    }

    backdrop.addEventListener("touchstart", (evt) => {
      const touch = evt.touches[0];
      touchStartX = touch.pageX;
    });

    backdrop.addEventListener("touchend", (evt) => {
      const touch = evt.changedTouches[0];
      touchEndX = touch.pageX;
      handleSwipe();
    });
  }

  document.addEventListener("DOMContentLoaded", initNavbar);
})();
