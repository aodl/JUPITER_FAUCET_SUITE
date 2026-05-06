// background-orbit.js
(function () {
  // LCM of 400, 600, 1200 = 1200s
  const PERIOD_SECONDS = 1200;
  const INFOGRAPHIC_CONFIG = [
    {
      label: "Particle",
      text: "Automated disbursals keep flowing, minting new ICP from voting rewards, powering downstream smart contracts.",
      target: "particle",
      targetX: 453,
      targetY: 237,
      markerRadius: 8,
      lineStartX: 391,
      lineStartY: 201,
      textLeftVw: 6.5,
      textTopVw: 10,
      textWidthVw: 49,
      fontSizeVw: 2.3,
      fontSizeMaxRem: 2.05,
      lineWidth: 0.5,
    },
    {
      label: "Black hole",
      text: "Disbursals are orchestrated by immutable (unmodifiable) smart contracts, aka 'blackholed'.\nCOMING SOON ...",
      target: "point",
      targetX: 408,
      targetY: 451,
      markerRadius: 81,
      lineStartX: 295,
      lineStartY: 581,
      textLeftVw: 8,
      textTopVw: 58,
      textWidthVw: 32.5,
      fontSizeVw: 2.3,
      fontSizeMaxRem: 2.25,
      lineWidth: 0.7,
    },
    {
      label: "Jupiter",
      text: "Disbursed ICP is automatically converted into cycles, forming a giant, unstoppable faucet.",
      target: "point",
      targetX: 687,
      targetY: 646,
      markerRadius: 140,
      lineStartX: 409,
      lineStartY: 630,
      textLeftVw: 8,
      textTopVw: 57,
      textWidthVw: 32,
      fontSizeVw: 2.15,
      fontSizeMaxRem: 1.95,
      lineWidth: 1.2,
    },
    {
      label: "Top ups",
      text: "Cycles are permanently routed to canisters that were declared by Jupiter Faucet users, removing or reducing economic dependency on developers, and the risk of service disruption/deletion.",
      target: "point",
      targetX: 648,
      targetY: 841,
      markerRadius: 58,
      lineStartX: 370,
      lineStartY: 725,
      textLeftVw: 5.5,
      textTopVw: 54.5,
      textWidthVw: 42,
      fontSizeVw: 2.2,
      fontSizeMaxRem: 2.25,
      lineWidth: 1.2,
    },
  ];
  const PARTICLE_DURATIONS_SECONDS = [600, 400, 1200];
  const TOUCH_ACTIVE_MS = 5000;
  const SVG_NS = "http://www.w3.org/2000/svg";

  function easeInOut(value) {
    if (value <= 0) {
      return 0;
    }

    if (value >= 1) {
      return 1;
    }

    return value * value * (3 - 2 * value);
  }

  function interpolate(start, end, progress) {
    return start + (end - start) * progress;
  }

  function animatedSvgPosition(element, overlaySvg, fallback) {
    if (
      !element ||
      !overlaySvg ||
      typeof element.getScreenCTM !== "function" ||
      typeof overlaySvg.getScreenCTM !== "function" ||
      typeof overlaySvg.createSVGPoint !== "function"
    ) {
      return fallback;
    }

    const elementMatrix = element.getScreenCTM();
    const overlayMatrix = overlaySvg.getScreenCTM();
    if (!elementMatrix || !overlayMatrix) {
      return fallback;
    }

    const point = overlaySvg.createSVGPoint();
    point.x = elementMatrix.e;
    point.y = elementMatrix.f;

    try {
      const localPoint = point.matrixTransform(overlayMatrix.inverse());
      return { x: localPoint.x, y: localPoint.y };
    } catch (_error) {
      return fallback;
    }
  }

  function setLine(line, marker, start, target, progress, markerRadius, lineWidth) {
    const eased = easeInOut(progress);
    const x2 = interpolate(start.x, target.x, eased);
    const y2 = interpolate(start.y, target.y, eased);

    line.setAttribute("x1", String(start.x));
    line.setAttribute("y1", String(start.y));
    line.setAttribute("x2", x2.toFixed(2));
    line.setAttribute("y2", y2.toFixed(2));
    line.setAttribute("stroke-width", String(lineWidth));
    line.style.opacity = progress > 0.01 ? "1" : "0";
    marker.setAttribute("cx", target.x.toFixed(2));
    marker.setAttribute("cy", target.y.toFixed(2));
    marker.setAttribute("r", (markerRadius * eased).toFixed(2));
    marker.style.opacity = progress > 0.01 ? "1" : "0";
  }

  function createSvgElement(name, attributes) {
    const element = document.createElementNS(SVG_NS, name);
    Object.entries(attributes).forEach(([key, value]) => {
      element.setAttribute(key, String(value));
    });

    return element;
  }

  function ensureOverlayPath(overlaySvg, path) {
    if (!overlaySvg || !path) {
      return null;
    }

    let defs = overlaySvg.querySelector("defs");
    if (!defs) {
      defs = createSvgElement("defs", {});
      overlaySvg.insertBefore(defs, overlaySvg.firstChild);
    }

    let overlayPath = overlaySvg.querySelector("#orbit-infographic-swirl-1");
    if (!overlayPath) {
      overlayPath = createSvgElement("path", {
        id: "orbit-infographic-swirl-1",
        d: path.getAttribute("d"),
      });
      defs.appendChild(overlayPath);
    }

    return overlayPath;
  }

  function addParticleHotspot(layer, id, durationSeconds, onActivate, onDeactivate, onClick) {
    if (!layer) {
      return null;
    }

    if (layer.querySelector(`#${id}`)) {
      return null;
    }

    const hotspot = createSvgElement("circle", {
      id,
      class: "orbit-infographic-particle-hotspot",
      r: 14,
      fill: "transparent",
      opacity: 0,
      "data-duration-seconds": durationSeconds,
    });
    const motion = createSvgElement("animateMotion", {
      dur: `${durationSeconds}s`,
      begin: "0s",
      repeatCount: "indefinite",
      rotate: "auto",
    });
    const mpath = document.createElementNS(SVG_NS, "mpath");
    mpath.setAttribute("href", "#orbit-infographic-swirl-1");
    motion.appendChild(mpath);
    hotspot.appendChild(motion);
    hotspot.addEventListener("mouseenter", () => onActivate(durationSeconds));
    hotspot.addEventListener("mouseleave", onDeactivate);
    hotspot.addEventListener("click", (event) => {
      event.preventDefault();
      onClick(durationSeconds);
    });
    layer.appendChild(hotspot);

    return hotspot;
  }

  function initInfographic(svg, nowSeconds) {
    const copy = document.getElementById("orbit-infographic-copy");
    const line = document.getElementById("orbit-infographic-line");
    const marker = document.getElementById("orbit-infographic-marker");
    const overlaySvg = document.querySelector(".orbit-infographic-lines");
    const hotspotLayer = document.getElementById("orbit-infographic-hotspots");
    const path = document.getElementById("swirl-1");

    if (!copy || !line || !marker || !overlaySvg || !hotspotLayer) {
      return;
    }
    ensureOverlayPath(overlaySvg, path);

    const config = INFOGRAPHIC_CONFIG.map((item) => ({ ...item }));
    const state = {
      hoverIndex: null,
      clickIndex: null,
      displayIndex: null,
      activeParticleHotspot: null,
      clickedParticleHotspot: null,
      clickedUntilMs: 0,
      progress: 0,
      reduceMotion: window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches,
    };
    overlaySvg.setCurrentTime(nowSeconds % PERIOD_SECONDS);

    function activateHover(index, particleHotspot) {
      clearClickActivation();
      state.hoverIndex = index;
      if (particleHotspot) {
        state.activeParticleHotspot = particleHotspot;
      }
    }

    function deactivateHover(index) {
      if (state.hoverIndex === index) {
        state.hoverIndex = null;
      }
    }

    function activateClick(index, particleHotspot) {
      state.clickIndex = index;
      state.clickedUntilMs = Date.now() + TOUCH_ACTIVE_MS;
      if (particleHotspot) {
        state.clickedParticleHotspot = particleHotspot;
      }
    }

    function activeIndex() {
      if (state.clickIndex !== null && Date.now() <= state.clickedUntilMs) {
        return state.clickIndex;
      }

      state.clickIndex = null;
      state.clickedParticleHotspot = null;
      return state.hoverIndex;
    }

    function activeParticleHotspot() {
      if (state.clickIndex === 0 && state.clickedParticleHotspot) {
        return state.clickedParticleHotspot;
      }

      return state.activeParticleHotspot;
    }

    function clearClickActivation() {
      state.clickIndex = null;
      state.clickedParticleHotspot = null;
      state.clickedUntilMs = 0;
    }

    config.forEach((item, index) => {
      if (item.target === "particle") {
        return;
      }

      const hotspot = createSvgElement("circle", {
        class: "orbit-infographic-hotspot",
        "data-index": index,
        cx: item.targetX,
        cy: item.targetY,
        r: item.markerRadius,
      });
      hotspotLayer.appendChild(hotspot);
      hotspot.addEventListener("mouseenter", () => {
        activateHover(index);
      });
      hotspot.addEventListener("mouseleave", () => {
        deactivateHover(index);
      });
      hotspot.addEventListener("click", (event) => {
        event.preventDefault();
        activateClick(index);
      });
    });

    const particleHotspots = PARTICLE_DURATIONS_SECONDS.map((durationSeconds, index) => (
      addParticleHotspot(
        hotspotLayer,
        `orbit-infographic-particle-hotspot-${index}`,
        durationSeconds,
        (activeDurationSeconds) => {
          activateHover(0, particleHotspots[index]);
        },
        () => {
          deactivateHover(0);
        },
        () => activateClick(0, particleHotspots[index]),
      )
    )).filter(Boolean);

    document.querySelectorAll(".neon-top, .neon-ups").forEach((label) => {
      label.addEventListener("mouseenter", () => {
        activateHover(3);
      });
      label.addEventListener("mouseleave", () => {
        deactivateHover(3);
      });
      label.addEventListener("click", (event) => {
        event.preventDefault();
        activateClick(3);
      });
    });

    document.addEventListener("click", (event) => {
      const target = event.target;
      if (!(target instanceof Element)) {
        clearClickActivation();
        return;
      }

      if (
        target.closest(".orbit-infographic-hotspot") ||
        target.closest(".orbit-infographic-particle-hotspot") ||
        target.closest(".neon-top") ||
        target.closest(".neon-ups")
      ) {
        return;
      }

      clearClickActivation();
    });

    function render() {
      overlaySvg.setCurrentTime(svg.getCurrentTime());

      config.forEach((item, index) => {
        const hotspot = hotspotLayer.querySelector(`.orbit-infographic-hotspot[data-index="${index}"]`);
        if (!hotspot) {
          return;
        }

        const target = { x: item.targetX, y: item.targetY };
        hotspot.setAttribute("cx", target.x.toFixed(2));
        hotspot.setAttribute("cy", target.y.toFixed(2));
        hotspot.setAttribute("r", String(item.markerRadius));
      });

      const currentActiveIndex = activeIndex();
      const desiredProgress = currentActiveIndex === null ? 0 : 1;
      if (state.reduceMotion) {
        state.progress = desiredProgress;
      } else {
        state.progress = interpolate(state.progress, desiredProgress, 0.16);
        if (Math.abs(state.progress - desiredProgress) < 0.01) {
          state.progress = desiredProgress;
        }
      }

      if (currentActiveIndex !== null) {
        const item = config[currentActiveIndex];
        const target = item.target === "particle"
          ? animatedSvgPosition(activeParticleHotspot(), overlaySvg, { x: item.targetX, y: item.targetY })
          : { x: item.targetX, y: item.targetY };
        const start = { x: item.lineStartX, y: item.lineStartY };

        state.displayIndex = currentActiveIndex;
        copy.textContent = item.text;
        copy.style.left = `${item.textLeftVw}dvw`;
        copy.style.top = `${item.textTopVw}dvw`;
        copy.style.width = `${item.textWidthVw}dvw`;
        copy.style.fontSize = `min(${item.fontSizeMaxRem}rem, ${item.fontSizeVw}dvw)`;
        copy.classList.add("is-visible");
        setLine(line, marker, start, target, state.progress, item.markerRadius, item.lineWidth);
      } else {
        copy.classList.remove("is-visible");
        if (state.displayIndex !== null) {
          const item = config[state.displayIndex];
          const target = item.target === "particle"
            ? animatedSvgPosition(activeParticleHotspot(), overlaySvg, { x: item.targetX, y: item.targetY })
            : { x: item.targetX, y: item.targetY };
          const start = { x: item.lineStartX, y: item.lineStartY };
          setLine(line, marker, start, target, state.progress, item.markerRadius, item.lineWidth);
        }
      }

      requestAnimationFrame(render);
    }

    requestAnimationFrame(render);
  }

  function init() {
    const svg = document.getElementById("background-orbit");
    if (!svg || typeof svg.setCurrentTime !== "function") {
      return;
    }

    const nowSeconds = Date.now() / 1000;
    const t = nowSeconds % PERIOD_SECONDS;
    svg.setCurrentTime(t);
    initInfographic(svg, nowSeconds);
  }

  document.addEventListener("DOMContentLoaded", init);
})();
