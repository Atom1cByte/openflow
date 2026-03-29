const tauriInvoke =
  window.__TAURI__?.core?.invoke ||
  window.__TAURI__?.invoke ||
  window.__TAURI__?.tauri?.invoke ||
  null;

const LOCAL_KEYS = {
  history: "openflow.history",
  snippets: "openflow.snippets",
  dictionary: "openflow.dictionary",
  styles: "openflow.styles",
};

const FIELD_IDS = {
  groq_api_key: "groq-key-settings",
  hotkey_dictate: "hotkey-dictate-settings",
  hotkey_command: "hotkey-command-settings",
  hotkey_hands_free: "hotkey-hands-settings",
  whisper_model: "whisper-model-settings",
  chat_model: "chat-model-settings",
  enable_llm_enhancement: "llm-enhancement-settings",
  launch_on_startup: "launch-on-startup-settings",
  bar_visible: "bar-visible-settings",
};

const state = {
  barHidden: false,
};

function byId(id) {
  return document.getElementById(id);
}

function readLocal(key, fallback = []) {
  try {
    const raw = localStorage.getItem(key);
    return raw ? JSON.parse(raw) : fallback;
  } catch {
    return fallback;
  }
}

function writeLocal(key, value) {
  localStorage.setItem(key, JSON.stringify(value));
}

function pushHistory(message) {
  const history = readLocal(LOCAL_KEYS.history, []);
  history.unshift({
    id: crypto.randomUUID(),
    message,
    ts: new Date().toLocaleString(),
  });
  writeLocal(LOCAL_KEYS.history, history.slice(0, 100));
}

function setInputValue(id, value) {
  const element = byId(id);
  if (element) {
    element.value = value ?? "";
  }
}

function setCheckboxValue(id, value) {
  const element = byId(id);
  if (element instanceof HTMLInputElement) {
    element.checked = Boolean(value);
  }
}

function getInputValue(id) {
  const element = byId(id);
  return element instanceof HTMLInputElement || element instanceof HTMLTextAreaElement
    ? element.value.trim()
    : "";
}

function getCheckboxValue(id, fallback = false) {
  const element = byId(id);
  return element instanceof HTMLInputElement ? element.checked : fallback;
}

function buildSettingsPayload() {
  return {
    groq_api_key: getInputValue(FIELD_IDS.groq_api_key),
    hotkey_dictate: getInputValue(FIELD_IDS.hotkey_dictate) || "Ctrl+Shift+D",
    hotkey_command: getInputValue(FIELD_IDS.hotkey_command) || "Ctrl+Shift+E",
    hotkey_hands_free: getInputValue(FIELD_IDS.hotkey_hands_free) || "Ctrl+Shift+F",
    whisper_model: getInputValue(FIELD_IDS.whisper_model) || "whisper-large-v3-turbo",
    chat_model: getInputValue(FIELD_IDS.chat_model) || "llama-3.1-8b-instant",
    enable_llm_enhancement: getCheckboxValue(FIELD_IDS.enable_llm_enhancement, true),
    launch_on_startup: getCheckboxValue(FIELD_IDS.launch_on_startup),
    bar_hidden: !getCheckboxValue(FIELD_IDS.bar_visible, true),
    bar_x: null,
    bar_y: null,
  };
}

function activateView(view) {
  document.querySelectorAll(".nav-item").forEach((item) => {
    item.classList.toggle("active", item.dataset.view === view);
  });

  document.querySelectorAll(".section-view").forEach((section) => {
    section.classList.toggle("is-active", section.dataset.section === view);
  });
}

function updateBarButton() {
  const button = byId("toggle-bar");
  if (button) {
    button.textContent = state.barHidden ? "Show Dock Bar" : "Hide Dock Bar";
  }
}

function updateRuntimeStatus(status) {
  const dot = byId("status-dot");
  const label = byId("runtime-label");
  const hotkey = byId("runtime-hotkey");
  if (!dot || !label || !hotkey) {
    return;
  }

  const registered = Boolean(status?.hotkey_registered);
  state.barHidden = Boolean(status?.bar_hidden);
  updateBarButton();

  dot.style.background = registered ? "var(--accent-3)" : "#d7a86e";
  label.textContent = registered ? "Hotkey registered" : "Hotkey registration failed";

  if (registered && status?.active_hotkey) {
    hotkey.textContent = state.barHidden
      ? `Active shortcut: ${status.active_hotkey} | bar hidden`
      : `Active shortcut: ${status.active_hotkey}`;
    return;
  }

  hotkey.textContent = status?.last_hotkey_error || "No active shortcut";
}

function renderCollection(containerId, items, formatter, emptyMessage) {
  const container = byId(containerId);
  if (!container) {
    return;
  }

  container.innerHTML = items.length
    ? items.map((item) => formatter(item)).join("")
    : `<div class="collection-empty">${emptyMessage}</div>`;
}

function renderHistory() {
  renderCollection(
    "history-list",
    readLocal(LOCAL_KEYS.history, []),
    (item) => `
      <article class="collection-item">
        <div>
          <strong>${item.message}</strong>
          <p>${item.ts}</p>
        </div>
      </article>
    `,
    "No local history yet."
  );
}

function renderSnippets() {
  renderCollection(
    "snippets-list",
    readLocal(LOCAL_KEYS.snippets, []),
    (item) => `
      <article class="collection-item">
        <div>
          <strong>${item.trigger}</strong>
          <p>${item.content}</p>
        </div>
        <button class="mini-button" data-delete="snippet" data-id="${item.id}" type="button">Delete</button>
      </article>
    `,
    "No local snippets saved."
  );
}

function renderDictionary() {
  renderCollection(
    "dictionary-list",
    readLocal(LOCAL_KEYS.dictionary, []),
    (item) => `
      <article class="collection-item">
        <div>
          <strong>${item.term}</strong>
          <p>${item.value}</p>
        </div>
        <button class="mini-button" data-delete="dictionary" data-id="${item.id}" type="button">Delete</button>
      </article>
    `,
    "No local dictionary terms saved."
  );
}

function renderStyles() {
  renderCollection(
    "styles-list",
    readLocal(LOCAL_KEYS.styles, []),
    (item) => `
      <article class="collection-item">
        <div>
          <strong>${item.name}</strong>
          <p>${item.instruction}</p>
        </div>
        <button class="mini-button" data-delete="style" data-id="${item.id}" type="button">Delete</button>
      </article>
    `,
    "No local styles saved."
  );
}

function renderHomeSummary() {
  const snippets = readLocal(LOCAL_KEYS.snippets, []);
  const dictionary = readLocal(LOCAL_KEYS.dictionary, []);
  const styles = readLocal(LOCAL_KEYS.styles, []);
  const history = readLocal(LOCAL_KEYS.history, []);

  byId("stat-snippets").textContent = String(snippets.length);
  byId("stat-dictionary").textContent = String(dictionary.length);
  byId("stat-styles").textContent = String(styles.length);
  byId("stat-history").textContent = String(history.length);

  const summary = byId("home-summary");
  if (!summary) {
    return;
  }

  const recent = history.slice(0, 3);
  summary.innerHTML = recent.length
    ? recent
        .map(
          (item) => `
            <article class="summary-item">
              <strong>${item.message}</strong>
              <span>${item.ts}</span>
            </article>
          `
        )
        .join("")
    : `<div class="collection-empty">No local activity yet.</div>`;
}

function renderAllLocalSections() {
  renderHistory();
  renderSnippets();
  renderDictionary();
  renderStyles();
  renderHomeSummary();
}

async function refreshRuntimeStatus() {
  if (!tauriInvoke) {
    return;
  }
  try {
    updateRuntimeStatus(await tauriInvoke("get_runtime_status"));
  } catch (error) {
    console.error("Failed to load runtime status", error);
  }
}

async function loadSettings() {
  if (!tauriInvoke) {
    return;
  }

  try {
    const settings = await tauriInvoke("get_settings");
    setInputValue(FIELD_IDS.groq_api_key, settings.groq_api_key);
    setInputValue(FIELD_IDS.hotkey_dictate, settings.hotkey_dictate);
    setInputValue(FIELD_IDS.hotkey_command, settings.hotkey_command);
    setInputValue(FIELD_IDS.hotkey_hands_free, settings.hotkey_hands_free);
    setInputValue(FIELD_IDS.whisper_model, settings.whisper_model);
    setInputValue(FIELD_IDS.chat_model, settings.chat_model);
    setCheckboxValue(FIELD_IDS.enable_llm_enhancement, settings.enable_llm_enhancement ?? true);
    setCheckboxValue(FIELD_IDS.launch_on_startup, settings.launch_on_startup);
    setCheckboxValue(FIELD_IDS.bar_visible, !settings.bar_hidden);
    state.barHidden = Boolean(settings.bar_hidden);
    updateBarButton();
    await refreshRuntimeStatus();
  } catch (error) {
    console.error("Failed to load settings", error);
  }
}

async function saveSettings() {
  const existing = tauriInvoke ? await tauriInvoke("get_settings").catch(() => ({})) : {};
  const payload = {
    ...existing,
    ...buildSettingsPayload(),
    bar_x: existing?.bar_x ?? null,
    bar_y: existing?.bar_y ?? null,
  };

  if (!tauriInvoke) {
    return;
  }

  try {
    await tauriInvoke("save_settings", { settings: payload });
    pushHistory("Saved runtime settings");
    renderAllLocalSections();
    await refreshRuntimeStatus();
  } catch (error) {
    console.error("Failed to save settings", error);
  }
}

async function toggleDictation() {
  if (!tauriInvoke) {
    return;
  }

  try {
    await tauriInvoke("toggle_recording");
    pushHistory("Toggled dictation");
    renderAllLocalSections();
  } catch (error) {
    console.error("Failed to toggle recording", error);
  }
}

async function toggleBarVisibility() {
  if (!tauriInvoke) {
    return;
  }

  try {
    const hidden = !state.barHidden;
    const status = await tauriInvoke("set_bar_hidden", { hidden });
    setCheckboxValue(FIELD_IDS.bar_visible, !hidden);
    updateRuntimeStatus(status);
    pushHistory(hidden ? "Hidden dock bar" : "Showed dock bar");
    renderAllLocalSections();
  } catch (error) {
    console.error("Failed to toggle dock bar", error);
  }
}

function upsertLocalItem(key, matchFn, nextItem) {
  const items = readLocal(key, []);
  const existing = items.find(matchFn);
  if (existing) {
    Object.assign(existing, nextItem);
  } else {
    items.unshift(nextItem);
  }
  writeLocal(key, items);
}

function saveSnippet() {
  const trigger = getInputValue("snippet-trigger");
  const content = getInputValue("snippet-content");
  if (!trigger || !content) {
    return;
  }

  upsertLocalItem(
    LOCAL_KEYS.snippets,
    (item) => item.trigger.toLowerCase() === trigger.toLowerCase(),
    { id: crypto.randomUUID(), trigger, content }
  );

  setInputValue("snippet-trigger", "");
  setInputValue("snippet-content", "");
  pushHistory(`Updated snippet "${trigger}"`);
  renderAllLocalSections();
}

function saveDictionary() {
  const term = getInputValue("dictionary-term");
  const value = getInputValue("dictionary-value");
  if (!term || !value) {
    return;
  }

  upsertLocalItem(
    LOCAL_KEYS.dictionary,
    (item) => item.term.toLowerCase() === term.toLowerCase(),
    { id: crypto.randomUUID(), term, value }
  );

  setInputValue("dictionary-term", "");
  setInputValue("dictionary-value", "");
  pushHistory(`Updated dictionary term "${term}"`);
  renderAllLocalSections();
}

function saveStyle() {
  const name = getInputValue("style-name");
  const instruction = getInputValue("style-instruction");
  if (!name || !instruction) {
    return;
  }

  upsertLocalItem(
    LOCAL_KEYS.styles,
    (item) => item.name.toLowerCase() === name.toLowerCase(),
    { id: crypto.randomUUID(), name, instruction }
  );

  setInputValue("style-name", "");
  setInputValue("style-instruction", "");
  pushHistory(`Updated style "${name}"`);
  renderAllLocalSections();
}

function deleteLocalItem(type, id) {
  const key = LOCAL_KEYS[type];
  if (!key) {
    return;
  }

  writeLocal(key, readLocal(key, []).filter((item) => item.id !== id));
  pushHistory(`Deleted ${type}`);
  renderAllLocalSections();
}

function wireDeleteActions() {
  document.addEventListener("click", (event) => {
    const target = event.target;
    if (!(target instanceof HTMLElement)) {
      return;
    }

    const type = target.getAttribute("data-delete");
    const id = target.getAttribute("data-id");
    if (type && id) {
      deleteLocalItem(type, id);
    }
  });
}

function wireNav() {
  document.querySelectorAll(".nav-item").forEach((item) => {
    item.addEventListener("click", () => activateView(item.dataset.view));
  });
}

function wireButtons() {
  byId("toggle")?.addEventListener("click", toggleDictation);
  byId("toggle-bar")?.addEventListener("click", toggleBarVisibility);
  byId("save")?.addEventListener("click", saveSettings);
  byId("save-home")?.addEventListener("click", saveSettings);
  byId("save-snippet")?.addEventListener("click", saveSnippet);
  byId("save-dictionary")?.addEventListener("click", saveDictionary);
  byId("save-style")?.addEventListener("click", saveStyle);
  byId("clear-history")?.addEventListener("click", () => {
    writeLocal(LOCAL_KEYS.history, []);
    renderAllLocalSections();
  });
  byId(FIELD_IDS.bar_visible)?.addEventListener("change", () => {
    const shouldShow = getCheckboxValue(FIELD_IDS.bar_visible, true);
    if (shouldShow === state.barHidden) {
      toggleBarVisibility();
    }
  });
}

function init() {
  activateView("home");
  wireNav();
  wireButtons();
  wireDeleteActions();
  renderAllLocalSections();
  loadSettings();
}

if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", init);
} else {
  init();
}
