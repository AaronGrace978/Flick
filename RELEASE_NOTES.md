# v0.2.0-alpha.1 — Flick Visual Layer

The first visual-overlay and paired-handoff prerelease. Experimental software;
physical tracking accuracy and native OS handoff behavior still need device testing.

## Downloads

- `flick-desktop-v0.2.0-alpha.1-<platform>-<architecture>.tar.gz` or `.zip`:
  `flick-overlay` (transparent Tauri UI), `emitter` (20 kHz beacon), `receiver`
  (terminal dashboard), setup guides, and GPL-3.0 license. Architecture is detected
  from the build host rather than assumed.
- `flick-android-v0.2.0-alpha.1.apk`: debug-signed Android app, API 26+. This is a
  sideloadable development APK, not a production-signed app-store package.
- `SHA256SUMS`: checksums for all downloadable binary assets.
- Corresponding source is available through GitHub's source archives for this tag.

Desktop executables are unsigned/unnotarized. Linux needs GTK3/WebKitGTK 4.1 and
ALSA runtimes; Windows needs WebView2; macOS may request native automation/accessibility
permissions. See OVERLAY.md before running the automatic paste workflow.

## Changes since v0.1.0-alpha.2

- Tauri workspace with transparent, always-on-top, click-through, non-focusable UI.
- Cyan Gaussian/Bezier edge warp, capture splash, particles and a floating asset
  card. The canvas is transparent at idle and avoids unnecessary idle GPU draws.
- Gesture arming, relative-displacement integration, direction heuristic, reversal/
  stop capture, stale-sample resets and cooldown.
- Dark native Android console with explicit clipboard/file selection and pairing.
- Bounded, non-blocking paired TCP handoff, receipt acknowledgement, inbox recovery
  copies and native paste only after the asset animation completes.
- Target-focus checks and platform-specific clipboard handling; failures preserve
  received data instead of silently discarding it.
- Rust gesture/framing/TCP tests, Kotlin matching-frame checks, real-browser visual
  and animation checks, and GUI compilation jobs for Linux, Windows and macOS.

## Validation and limits

Before tagging, Linux GUI linking, workspace Clippy, eight Rust tests, ten Kotlin
check groups, Android APK build/lint, real UDP/TCP tests and Chromium visual tests
passed. Win32 clipboard/input bindings type-checked. Release publication additionally
waits for desktop builds/tests, Android checks and browser checks on GitHub Actions.

Native X11 end-to-end testing was blocked by the cloud environment's relocated
WebKit helper paths. Native click-through, OS paste permissions, physical phone
tracking and Windows/macOS runtime behavior are not claimed as verified.

A continuous tone measures radial velocity; integrated distance is a normalized
relative displacement, not absolute range. The shader draws an illusion and does
not capture or distort other applications' pixels. Raw device-axis acceleration
provides a direction heuristic, not calibrated world-space position.

Transfers are paired but unencrypted and intended only for a trusted LAN. UDP
telemetry is unauthenticated. Received files and text recovery copies remain in
app-data/inbox until manually removed. Wayland automatic paste is unsupported;
X11 file paste depends on the target application's clipboard support. A successful
paste command cannot prove every target application consumed the asset.

Use low speaker volume. See README.md and OVERLAY.md for setup, protocol details,
platform requirements and hardware acceptance checks.
