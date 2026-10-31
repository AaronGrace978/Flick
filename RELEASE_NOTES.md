# v0.2.0-alpha.2 — Desktop UI and pairing fixes

This update fixes the missing desktop interface and silent pairing flow in alpha.1.
Install matching desktop and Android builds; the old desktop cannot answer the new
pairing check. **Close both old terminal windows before launching this version.**

## What changed

- A visible Flick desktop window with LAN IPv4, selectable pairing code, pairing
  age and live sensor-data indication. The transparent overlay remains separate.
- Start/stop the beacon from the desktop window. Windows release builds no longer
  open console windows; closing Flick stops its managed beacon.
- Real address/key verification before microphone capture. Wrong code, timeout,
  unready desktop and microphone failures produce visible feedback.
- Android disabled buttons now look disabled, feedback appears by the controls,
  and cancelled checks/permission prompts restore usable controls.
- Actual received-image previews in the floating card and desktop window, with
  an Open received folder button. Files are retained in the inbox.
- Scoped image access and control-window-only access to pairing/beacon commands.
- Nine Rust tests, twelve Kotlin check groups, browser UI/image checks, GUI build,
  Android build and lint checks. Native desktop/device acceptance remains pending.

## Downloads and upgrade

Desktop archives contain `flick-overlay`, `emitter`, `receiver`, guides and LICENSE.
Keep these executables together. The Android APK is debug-signed for API 26+;
if its signing key conflicts with your old APK, uninstall the old Flick APK first.
The existing desktop pairing.key is retained. SHA256SUMS covers binary downloads;
corresponding source is available from this tag's GitHub source archives.

## Limits

This is still an experimental alpha. A valid network pairing does not establish
microphone ultrasonic support or tracking accuracy. The tone measures radial
velocity and relative displacement, not absolute range. Native OS click-through,
focus/paste permissions and physical device behavior require acceptance testing.

Transport is paired but unencrypted: use a trusted LAN and allow UDP 8089/TCP 8090
on the private network. Desktop executables are unsigned/unnotarized. Linux needs
GTK3/WebKitGTK 4.1, Windows needs WebView2, and native paste support varies by OS
and target application. See OVERLAY.md for full instructions and platform limits.
