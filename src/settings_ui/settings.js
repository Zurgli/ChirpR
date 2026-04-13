(function () {
  const state = window.__CHIRPR_INITIAL_STATE || {};
  const tabButtons = document.querySelectorAll("[data-tab-button]");
  const tabPanels = document.querySelectorAll("[data-tab-panel]");
  const startupWarning = document.getElementById("startup-warning");
  const structuredError = document.getElementById("structured-error");
  const rawError = document.getElementById("raw-error");
  const rawToml = document.getElementById("rawToml");
  const modelStatus = document.getElementById("modelStatus");
  const saveButton = document.getElementById("saveButton");
  const cancelButton = document.getElementById("cancelButton");
  const wordOverrides = document.getElementById("wordOverrides");
  const addWordOverride = document.getElementById("addWordOverride");
  const fieldIds = [
    "primaryShortcut",
    "recordingMode",
    "injectionMode",
    "pasteMode",
    "clipboardBehavior",
    "clipboardClearDelay",
    "audioFeedback",
    "audioFeedbackVolume",
    "recordingOverlay",
    "overlayIndicator",
    "maxRecordingDuration",
    "language",
    "postProcessing",
    "startSoundPath",
    "stopSoundPath",
    "errorSoundPath",
    "sttBackend",
    "parakeetModel",
    "parakeetQuantization",
    "onnxProviders",
    "threads",
    "modelTimeout",
  ];

  let activeTab = "general";
  let refreshTimer = null;
  let saveSuccessFlashTimer = null;

  const SOUND_PREVIEW_VISUAL_MS = 1150;

  function clearSoundPreviewVisual(button) {
    if (button._soundPreviewTimer != null) {
      clearTimeout(button._soundPreviewTimer);
      button._soundPreviewTimer = null;
    }
    button.classList.remove("is-sound-previewing");
    button.removeAttribute("aria-busy");
  }

  function clearAllSoundPreviewVisuals() {
    document.querySelectorAll(".sound-preview-btn").forEach((btn) => clearSoundPreviewVisual(btn));
  }

  function $(id) {
    return document.getElementById(id);
  }

  function showBanner(node, message) {
    if (!message) {
      node.textContent = "";
      node.classList.add("hidden");
      return;
    }
    node.textContent = message;
    node.classList.remove("hidden");
  }

  function sendIpc(message) {
    const payload = JSON.stringify(message);
    if (window.ipc && typeof window.ipc.postMessage === "function") {
      window.ipc.postMessage(payload);
      return;
    }
    if (
      window.chrome &&
      window.chrome.webview &&
      typeof window.chrome.webview.postMessage === "function"
    ) {
      window.chrome.webview.postMessage(payload);
      return;
    }
    console.warn("ChirpR settings: IPC unavailable (cannot reach host)");
  }

  function setActiveTab(tab) {
    activeTab = tab;
    tabButtons.forEach((button) => {
      button.classList.toggle("is-active", button.dataset.tabButton === tab);
    });
    tabPanels.forEach((panel) => {
      panel.classList.toggle("is-active", panel.dataset.tabPanel === tab);
    });

  }

  function syncWordOverrideAddButton() {
    const rows = wordOverrides.querySelectorAll(".word-override-row");
    const allowAdd =
      rows.length === 0 ||
      Array.from(rows).every((row) => {
        const spoken = row.querySelector(".override-spoken")?.value?.trim() ?? "";
        return spoken.length > 0;
      });
    addWordOverride.disabled = !allowAdd;
  }

  function ensureWordOverrideMinimumRow() {
    if (wordOverrides.querySelectorAll(".word-override-row").length === 0) {
      wordOverrides.appendChild(createWordOverrideRow({ spoken: "", replacement: "" }));
    }
    syncWordOverrideAddButton();
  }

  function createWordOverrideRow(entry) {
    const row = document.createElement("tr");
    row.className = "word-override-row";
    row.innerHTML = `
      <td><input type="text" class="override-spoken" spellcheck="false" aria-label="Spoken"></td>
      <td><input type="text" class="override-replacement" spellcheck="false" aria-label="Replacement"></td>
      <td class="word-overrides-col-actions"><button class="secondary-button remove-word-override" type="button" title="Remove row">×</button></td>
    `;
    const spokenInput = row.querySelector(".override-spoken");
    spokenInput.value = entry.spoken || "";
    row.querySelector(".override-replacement").value = entry.replacement || "";
    const onSpokenChange = () => syncWordOverrideAddButton();
    spokenInput.addEventListener("input", onSpokenChange);
    spokenInput.addEventListener("change", onSpokenChange);
    row.querySelector(".remove-word-override").addEventListener("click", () => {
      row.remove();
      ensureWordOverrideMinimumRow();
    });
    return row;
  }

  function renderWordOverrides(entries) {
    wordOverrides.innerHTML = "";
    const filtered = (entries || []).filter((e) => (e.spoken || "").trim() !== "");
    const list = filtered.length > 0 ? filtered : [{ spoken: "", replacement: "" }];
    list.forEach((entry) => {
      wordOverrides.appendChild(createWordOverrideRow(entry));
    });
    syncWordOverrideAddButton();
  }

  const OVERLAY_PREVIEW_STYLES = ["dot", "halo_soft", "sine_eye_double"];

  let sinePreviewRaf = null;

  function prefersReducedMotion() {
    return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  }

  function stopSinePreviewMotion() {
    if (sinePreviewRaf !== null) {
      cancelAnimationFrame(sinePreviewRaf);
      sinePreviewRaf = null;
    }
  }

  const SINE_PREVIEW_TAU = Math.PI * 2;

  /**
   * Same geometry as `recording_overlay::sine_trace_points` (28 samples, half-sine envelope^1.2,
   * y = center + amplitude * envelope * sin(elapsed*2.1 + step*0.51 + phase_offset)).
   */
  function sineTracePoints(elapsedSec, phaseOffset, left, top, width, height) {
    const centerY = top + height / 2;
    const insetX = Math.round(width * 0.06);
    const drawLeft = left + insetX;
    const drawWidth = Math.max(width - insetX * 2, 1);
    const amplitude = Math.max(height * 0.34, 1.0);
    const points = [];
    for (let step = 0; step < 28; step++) {
      const progress = step / 27;
      const x = drawLeft + progress * drawWidth;
      const envelope = Math.max(Math.sin(progress * Math.PI), 0) ** 1.2;
      const y =
        centerY +
        amplitude * envelope * Math.sin(elapsedSec * 2.1 + step * 0.51 + phaseOffset);
      points.push([Math.round(x), Math.round(y)]);
    }
    return points;
  }

  function pointsToSvgPath(points) {
    if (points.length === 0) {
      return "";
    }
    let d = `M ${points[0][0]} ${points[0][1]}`;
    for (let i = 1; i < points.length; i++) {
      d += ` L ${points[i][0]} ${points[i][1]}`;
    }
    return d;
  }

  /** Matches `current_indicator_metrics` pulse blend used for pen alpha in `draw_sine_eye_double_antialiased`. */
  function sinePreviewPhaseBlend(elapsedSec) {
    const wave = Math.sin(elapsedSec * SINE_PREVIEW_TAU * 0.6);
    return (wave + 1) * 0.5;
  }

  function drawSinePreviewPaths(svg, elapsedSec) {
    const paths = svg.querySelectorAll("path");
    if (paths.length < 2) {
      return;
    }
    const W = 36;
    const H = 14;
    const left = 0;
    const top = 0;
    const phaseBlend = sinePreviewPhaseBlend(elapsedSec);
    const alphaPrimary = (110 + phaseBlend * 130) / 255;
    const alphaSecondary = (90 + phaseBlend * 110) / 255;
    const strokeW = Math.max(H * 0.16, 1);

    const primary = sineTracePoints(elapsedSec, 0, left, top, W, H);
    const secondary = sineTracePoints(elapsedSec, SINE_PREVIEW_TAU / 3, left, top, W, H);
    paths[0].setAttribute("d", pointsToSvgPath(primary));
    paths[1].setAttribute("d", pointsToSvgPath(secondary));
    paths[0].setAttribute("stroke", `rgba(228, 82, 63, ${alphaPrimary})`);
    paths[1].setAttribute("stroke", `rgba(110, 168, 245, ${alphaSecondary})`);
    paths[0].setAttribute("stroke-width", String(strokeW));
    paths[1].setAttribute("stroke-width", String(strokeW));
  }

  function startSinePreviewMotion() {
    stopSinePreviewMotion();
    const prev = $("overlayIndicatorPreview");
    if (!prev || prev.dataset.style !== "sine_eye_double") {
      return;
    }
    const svg = prev.querySelector('svg[data-preview="sine_eye_double"]');
    if (!svg) {
      return;
    }

    if (prefersReducedMotion()) {
      drawSinePreviewPaths(svg, 0);
      return;
    }

    const t0 = performance.now();
    function frame(now) {
      const holder = $("overlayIndicatorPreview");
      if (!holder || holder.dataset.style !== "sine_eye_double") {
        sinePreviewRaf = null;
        return;
      }
      const el = holder.querySelector('svg[data-preview="sine_eye_double"]');
      if (!el) {
        sinePreviewRaf = null;
        return;
      }
      const elapsedSec = (now - t0) / 1000;
      drawSinePreviewPaths(el, elapsedSec);
      sinePreviewRaf = requestAnimationFrame(frame);
    }
    sinePreviewRaf = requestAnimationFrame(frame);
  }

  function syncOverlayIndicatorPreview() {
    const sel = $("overlayIndicator");
    const prev = $("overlayIndicatorPreview");
    if (!sel || !prev) {
      return;
    }
    const v = (sel.value || "").toLowerCase();
    prev.dataset.style = OVERLAY_PREVIEW_STYLES.includes(v) ? v : "sine_eye_double";
    stopSinePreviewMotion();
    if (prev.dataset.style === "sine_eye_double") {
      startSinePreviewMotion();
    }
  }

  function applyForm(form) {
    fieldIds.forEach((id) => {
      const node = $(id);
      if (!node) {
        return;
      }
      const key = id.charAt(0).toLowerCase() + id.slice(1);
      if (node.type === "checkbox") {
        node.checked = Boolean(form[key]);
      } else {
        node.value = form[key] ?? "";
      }
    });
    renderWordOverrides(form.wordOverrides || []);
    syncOverlayIndicatorPreview();
  }

  function collectForm() {
    const form = {};
    fieldIds.forEach((id) => {
      const node = $(id);
      const key = id.charAt(0).toLowerCase() + id.slice(1);
      form[key] = node.type === "checkbox" ? node.checked : node.value;
    });
    form.wordOverrides = Array.from(wordOverrides.querySelectorAll(".word-override-row"))
      .map((row) => ({
        spoken: row.querySelector(".override-spoken").value,
        replacement: row.querySelector(".override-replacement").value,
      }))
      .filter((entry) => entry.spoken.trim() !== "");
    return form;
  }

  function refreshModelStatusSoon() {
    clearTimeout(refreshTimer);
    refreshTimer = setTimeout(() => {
      sendIpc({
        kind: "refresh_model_status",
        form: collectForm(),
      });
    }, 180);
  }

  window.__chirprReceive = function (message) {
    switch (message.kind) {
      case "replace_sound_path": {
        const field = $(message.field);
        if (field) {
          field.value = message.value || "";
        }
        showBanner(structuredError, "");
        break;
      }
      case "update_model_status":
        modelStatus.textContent = message.message || "";
        break;
      case "set_structured_error":
        showBanner(structuredError, message.message || "");
        break;
      case "set_raw_error":
        showBanner(rawError, message.message || "");
        break;
      case "reload_ui":
        if (message.state) {
          applyForm(message.state.form || {});
          rawToml.value = message.state.rawToml || "";
          modelStatus.textContent = message.state.modelStatus || "";
          showBanner(startupWarning, message.state.loadWarning || "");
        }
        showBanner(structuredError, "");
        showBanner(rawError, "");
        break;
      case "save_succeeded":
        saveButton.textContent = "Saved";
        saveButton.classList.add("is-save-success");
        if (saveSuccessFlashTimer) clearTimeout(saveSuccessFlashTimer);
        saveSuccessFlashTimer = setTimeout(() => {
          saveButton.textContent = "Save";
          saveButton.classList.remove("is-save-success");
          saveSuccessFlashTimer = null;
        }, 1100);
        break;
      default:
        break;
    }
  };

  tabButtons.forEach((button) => {
    button.addEventListener("click", () => setActiveTab(button.dataset.tabButton));
  });

  document.querySelectorAll(".browse-button").forEach((button) => {
    button.addEventListener("click", () => {
      sendIpc({
        kind: "browse_sound",
        field: button.dataset.browseField,
      });
    });
  });

  document.querySelectorAll(".sound-preview-btn").forEach((button) => {
    button.addEventListener("click", () => {
      const fieldId = button.dataset.soundField;
      const role = button.dataset.soundRole;
      if (!fieldId || !role) {
        return;
      }
      clearAllSoundPreviewVisuals();
      button.classList.add("is-sound-previewing");
      button.setAttribute("aria-busy", "true");
      const input = $(fieldId);
      sendIpc({
        kind: "preview_sound",
        role,
        path: input ? input.value : "",
      });
      button._soundPreviewTimer = setTimeout(() => {
        clearSoundPreviewVisual(button);
      }, SOUND_PREVIEW_VISUAL_MS);
    });
  });

  addWordOverride.addEventListener("click", () => {
    if (addWordOverride.disabled) {
      return;
    }
    wordOverrides.appendChild(createWordOverrideRow({ spoken: "", replacement: "" }));
    syncWordOverrideAddButton();
  });

  saveButton.addEventListener("click", () => {
    showBanner(structuredError, "");
    showBanner(rawError, "");
    if (activeTab === "raw") {
      sendIpc({
        kind: "save_raw",
        raw: rawToml.value,
      });
      return;
    }
    sendIpc({
      kind: "save_structured",
      form: collectForm(),
    });
  });

  cancelButton.addEventListener("click", () => {
    sendIpc({ kind: "close_window" });
  });

  [
    "sttBackend",
    "parakeetModel",
    "parakeetQuantization",
    "onnxProviders",
    "threads",
    "modelTimeout",
  ].forEach((id) => {
    $(id).addEventListener("input", refreshModelStatusSoon);
    $(id).addEventListener("change", refreshModelStatusSoon);
  });

  $("overlayIndicator")?.addEventListener("change", syncOverlayIndicatorPreview);

  applyForm(state.form || {});
  rawToml.value = state.rawToml || "";
  modelStatus.textContent = state.modelStatus || "";
  showBanner(startupWarning, state.loadWarning || "");
  setActiveTab("general");
})();
