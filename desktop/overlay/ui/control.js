"use strict";
const invoke = window.__TAURI__?.core.invoke;
const message = document.querySelector("#message"),
  beacon = document.querySelector("#beacon");
let enabled = false,
  polling = false;
function feedback(text, error = false) {
  message.textContent = text;
  message.className = error ? "error" : "";
}
async function refresh() {
  if (!invoke || polling) return;
  polling = true;
  try {
    const state = await invoke("desktop_status");
    document.querySelector("#address").value =
      state.address || "Check ipconfig for your LAN IPv4";
    const code = document.querySelector("#code");
    if (document.activeElement !== code) code.value = state.code;
    const service = document.querySelector("#service");
    service.textContent = state.ready
      ? "Visual layer ready"
      : "Visual layer loading";
    service.className = "pill" + (state.ready ? " ready" : "");
    document.querySelector("#phone").textContent =
      state.paired_age === null
        ? "No phone paired yet"
        : `Last code check ${Math.floor(state.paired_age / 1000)}s ago`;
    document.querySelector("#audio").textContent =
      state.telemetry_age !== null && state.telemetry_age < 1000
        ? "Sensor data live"
        : "Waiting for sensor data";
    enabled = state.beacon;
    beacon.textContent = enabled ? "Stop beacon" : "Start beacon";
  } catch (error) {
    feedback(String(error), true);
  } finally {
    polling = false;
  }
}
for (const input of document.querySelectorAll("input"))
  input.addEventListener("click", () => input.select());
beacon.addEventListener("click", async () => {
  beacon.disabled = true;
  try {
    const requested = !enabled;
    await invoke("set_beacon", { enabled: requested });
    await new Promise((resolve) => setTimeout(resolve, 250));
    await refresh();
    feedback(
      requested
        ? enabled
          ? "Beacon running. Pair and calibrate your phone."
          : "Beacon did not start. Check your audio output device."
        : "Beacon stopped.",
      requested && !enabled,
    );
  } catch (error) {
    feedback(String(error), true);
  } finally {
    beacon.disabled = false;
  }
});
document.querySelector("#open-inbox").addEventListener("click", async () => {
  try {
    await invoke("open_inbox");
  } catch (error) {
    feedback(String(error), true);
  }
});
async function start() {
  if (!invoke) {
    feedback("Open flick-overlay to use the desktop controls.", true);
    beacon.disabled = true;
    return;
  }
  await window.__TAURI__.event.listen("flick://telemetry", (event) => {
    const data = event.payload;
    if (Number.isFinite(data.velocity)) {
      const locked = data.locked !== false;
      document.querySelector("#velocity").textContent = locked
        ? `Radial velocity: ${data.velocity.toFixed(3)} m/s`
        : "Radial velocity: no 20 kHz lock · motion flick available";
    }
    const motion = data.motion === true;
    document.querySelector("#gesture").textContent =
      data.phase === "capture"
        ? "Flick captured · requesting file"
        : data.phase === "approach"
          ? motion
            ? "Push detected · stop to hand off"
            : "Approach detected · stop to hand off"
          : data.armed
            ? data.locked === false
              ? "Armed without a tone lock · push toward the screen, then stop"
              : "Armed · flick toward the screen and stop"
            : "Hold still to arm the flick detector";
  });
  await window.__TAURI__.event.listen("flick://status", (event) =>
    feedback(event.payload.message, !event.payload.ok),
  );
  await window.__TAURI__.event.listen("flick://asset", (event) =>
    (() => {
      const data = event.payload;
      feedback(`Received ${data.label}. Animating onto your workspace…`);
      document.querySelector("#received").hidden = false;
      document.querySelector("#received-name").textContent = data.label;
      const image = document.querySelector("#received-preview");
      const kinds = {
        pdf: "PDF document",
        doc: "Word document",
        mp3: "MP3 audio",
        wav: "WAV audio",
      };
      const kind = document.querySelector("#received-kind");
      image.removeAttribute("src");
      image.style.display = "none";
      kind.hidden = true;
      kind.textContent = "";
      if (typeof data.preview === "string") {
        image.src = window.__TAURI__.core.convertFileSrc(data.preview);
        image.style.display = "block";
      } else if (typeof data.kind === "string" && kinds[data.kind]) {
        kind.hidden = false;
        kind.textContent = kinds[data.kind];
      }
    })(),
  );
  await refresh();
  feedback("Ready. Start the beacon at low volume, then connect your phone.");
  setInterval(refresh, 1000);
}
start().catch((error) => feedback(String(error), true));
