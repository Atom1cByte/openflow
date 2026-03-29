const tauriEvent = window.__TAURI__?.event || null;
const tauriInvoke =
  window.__TAURI__?.core?.invoke ||
  window.__TAURI__?.invoke ||
  window.__TAURI__?.tauri?.invoke ||
  null;

const bar = document.getElementById("bar");
const waveBars = Array.from(document.querySelectorAll(".wave span"));
const dot = document.getElementById("dot");
const openHub = document.getElementById("open-hub");
const hideBar = document.getElementById("hide-bar");

let phase = 0;
let intensity = 0.28;

function animateWave() {
  waveBars.forEach((bar, index) => {
    const value = Math.abs(Math.sin(phase + index * 0.55));
    bar.style.height = `${6 + value * 9 * intensity}px`;
    bar.style.opacity = `${0.22 + value * 0.48}`;
  });
  phase += 0.08;
}

function setStatus(status) {
  if (!dot) {
    return;
  }

  if (status === "listening") {
    intensity = 1;
    dot.style.background = "#22c55e";
    return;
  }

  if (status === "transcribing") {
    intensity = 0.6;
    dot.style.background = "#f59e0b";
    return;
  }

  intensity = 0.28;
  dot.style.background = "#6b7280";
}

function wireActions() {
  bar?.addEventListener("mousedown", async (event) => {
    if (!tauriInvoke || event.button !== 0) {
      return;
    }
    if (event.target instanceof HTMLElement && event.target.closest("button")) {
      return;
    }
    try {
      await tauriInvoke("start_bar_drag");
    } catch (error) {
      console.error("Failed to drag dock bar", error);
    }
  });

  openHub?.addEventListener("click", async () => {
    if (!tauriInvoke) {
      return;
    }
    try {
      await tauriInvoke("show_hub");
    } catch (error) {
      console.error("Failed to show hub", error);
    }
  });

  hideBar?.addEventListener("click", async () => {
    if (!tauriInvoke) {
      return;
    }
    try {
      await tauriInvoke("hide_bar_and_show_hub");
    } catch (error) {
      console.error("Failed to hide dock bar", error);
    }
  });
}

function wireStatusEvents() {
  if (!tauriEvent?.listen) {
    return;
  }

  tauriEvent.listen("bar_status", (event) => {
    setStatus(event.payload?.status || "idle");
  });
}

wireActions();
wireStatusEvents();
setStatus("idle");
setInterval(animateWave, 160);
