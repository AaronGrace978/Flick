# Flick Visual Layer

The Tauri 2 workspace member adds a non-activating, click-through desktop overlay,
relative-displacement gesture detection and paired clipboard/file handoff. The
frontend is plain HTML/CSS/WebGL/JavaScript; no npm install or frontend bundler is
needed to build the app. Tauri/Serde are intentional new dependencies of the GUI
package; the terminal receiver remains usable without them.

## Run

Install Rust stable and Tauri's native prerequisites. On Debian/Ubuntu:

```sh
sudo apt-get install libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev pkg-config xclip xdotool
cd desktop
cargo run --locked -p flick-overlay
```

Windows requires the MSVC C++ build tools and WebView2 runtime. macOS requires
Xcode command line tools. Use the same Cargo command there. Build an executable
with `cargo build --release --locked -p flick-overlay`. Packaging/signing/notarization
are separate distribution steps; bundling is disabled in this source configuration.

Run the emitter in a second terminal (`cargo run --release --locked --bin emitter`).
Use the overlay **instead of** the terminal receiver: both listen on UDP 8089.
Allow UDP 8089 and TCP 8090 only on the trusted LAN. The TCP listener opens only
for a capture, for five seconds; a connected transfer has a ten-second deadline.
Stop the overlay with Ctrl-C in its launch terminal or the operating system's
process manager. It has no focus-stealing control panel or global keyboard hook.

The window config contains the requested fullscreen, transparent, undecorated,
always-on-top, skip-taskbar, fixed-size, non-focused flags. Setup additionally sets
click-through and non-focusable native styles **before** showing the window. On
macOS, native fullscreen would create a different Space; setup replaces it with a
borderless monitor-sized window and Accessory activation policy. The effect covers
one monitor. Unsupported native styling fails initialization rather than leaving
an input-blocking fullscreen window behind. WebView rendering and compositor
behavior must still be validated on each OS/compositor used for distribution.

## Pair and hand off

On first launch, the desktop creates an OS-random 256-bit pairing key in its app
data directory and prints the **file path**, not its contents. Open `pairing.key`
yourself and paste its 64 hex characters into Android's pairing field. Unix file
permissions are 0600. The key persists until removed; remove it while the overlay
is stopped to re-pair. Never publish the key. Android holds it only for the session
and masks the input. Leaving the field empty enables visuals without file transfer.

1. Enter the desktop LAN IPv4 address in the Android app.
2. Tap **Use clipboard** to explicitly queue text, or **Choose file** for the native
   document picker. Text is limited to 256 KiB, files to 16 MiB. Selection stages a
   bounded in-memory snapshot. Nothing scans storage or silently reads the clipboard.
3. Tap **Connect and listen**, hold the phone stationary with the beacon on, then
   calibrate. Both the app and transfer worker stop when leaving the foreground.
4. Focus the desired target application on the desktop. Hold the phone in a fixed,
   upright orientation and approach, then stop/reverse the flick. Positive X motion
   chooses the left edge; negative chooses the right, using gravity-subtracted
   device axes. This mapping is a heuristic, not a world-coordinate measurement.
5. Capture triggers a splash and a gesture-bound TCP request to that Android UDP
   sender. After receipt, the asset card slides inward for 850 ms. Only its
   completion acknowledgement allows the backend to populate the clipboard and
   issue paste. Missed animation acknowledgement expires without injecting anything.

The target is identified when the TCP connection is accepted and checked again
before paste. Do not switch target applications during the animation. Focus checks
reduce accidental pastes but cannot atomically lock an arbitrary application's
text field. OS input acceptance also cannot prove an application consumed the data.
The UI reports that paste was sent, not that arbitrary applications accepted it.

Queued Android content is cleared only after a desktop receipt acknowledgement.
Every received file **and text snapshot** is retained in the desktop app-data
`inbox/` for recovery if paste fails. Files get generated capture-prefixed names
and cannot escape that directory. Remove retained items yourself when no longer
needed. They are not automatically opened or executed.

## Native paste behavior

| Platform | Mechanism and limits |
| --- | --- |
| Windows | Win32 clipboard: CF_UNICODETEXT or CF_HDROP; SendInput Ctrl-V into the still-active window. Windows may reject injection into elevated targets. |
| macOS | Native pbcopy/AppleScript file clipboard, System Events Cmd-V. Grant macOS Automation/Accessibility access when prompted; denied access leaves the received item in the inbox. Transparency uses Tauri's private-API feature. |
| Linux X11 | xclip selection ownership and xdotool Ctrl-V. Install both. File paste uses text/uri-list and depends on target file-manager support. |
| Wayland | There is no universal unrestricted focus/injection API. Automatic paste is explicitly unsupported. Overlay support also depends on the compositor; use an X11 session for the full workflow. |

This uses the target's native clipboard paste semantics, not fabricated drag/drop
DOM events. File explorers and text fields must support their respective clipboard
formats. Unsupported/changed targets report a failure without activating a window.

## Gesture model and effect

The detector must first see 300 ms of low radial velocity and a settled acceleration
baseline. A rising approach above 0.5 m/s arms integration. Trapezoidal integration
updates `distance -= velocity * dt`, starting at a **normalized 0.65 m reference**.
It is relative displacement, never an absolute range measurement. Audio-clock bias
and movement during calibration affect this estimate. Gaps above 100 ms reset the
model rather than bridging stale samples. Reversal/dead stop, or a strong
acceleration-associated velocity drop, captures once and starts a 1.2 s cooldown.
An approach lasting over 1.5 s cancels.

Below the normalized 0.5 threshold, a cubic-Bezier Gaussian edge ridge increases
amplitude toward the center. Its cyan premultiplied-alpha mesh is capped at 0.4 and
has zero alpha at screen center. Capture adds an expanding edge splash, particles,
and a glass-styled asset card. The app respects reduced-motion preferences for the
card trajectory and particles. Idle pixels are transparent, and both the native
window and frontend ignore pointer input.

The canvas draws a visual light/distortion illusion. It **does not sample or warp
pixels belonging to other applications**. Real refraction of desktop content would
require a separate screen-capture pipeline and OS capture permissions, which this
implementation does not request.

## Transfer framing

The original 24-byte telemetry packet is unchanged. Desktop replies from UDP 8089
to that sender's source endpoint with 12 bytes: ASCII `FLK1`, then a big-endian u64
capture ID. Android accepts control packets only from the configured endpoint.

The phone connects to TCP 8090 and sends:

| Field | Size |
| --- | --- |
| ASCII FLICK001 | 8 |
| Pairing key, raw bytes | 32 |
| Expected capture ID, big-endian | 8 |
| Kind: 0 UTF-8 text, 1 file | 1 |
| UTF-8 filename length, big-endian | 2 |
| Content byte length, big-endian | 8 |
| Filename (empty for text, safe ASCII for files) | 0–100 |
| Content | bounded by kind |

Only the same telemetry source IP is accepted, pairing and capture IDs must match,
and only one transfer is active. Partial files are removed. Desktop replies `OK`
only after complete receipt and recovery storage; this means receipt, not successful
paste. UDP, TCP and animation workers do not block one another. There are no
retransmission queues. A failed transfer remains queued on Android for a new gesture.

This is a **trusted-LAN protocol, not TLS**. The pairing key and content cross TCP
in cleartext. The key prevents unauthenticated injection but not a LAN observer or
active man-in-the-middle. Do not expose either port to the Internet. UDP gestures
can still be spoofed to produce visual effects; a valid pairing key is required to
supply an injectable payload. Deployment on untrusted networks needs authenticated
encryption and key exchange before claiming secure transport.

## Validation

Run core/GUI checks with:

```sh
cd desktop
cargo test --locked --no-default-features
cargo clippy --locked -p flick-overlay -- -D warnings
cargo build --locked -p flick-overlay
cd ../android
./gradlew :core:check :app:assembleDebug :app:lintDebug
```

`tests/overlay_browser.cjs` additionally runs a real browser against a local static
server and mocked IPC. Install Playwright as an external test-only tool, set
NODE_PATH to that installation, and CHROMIUM to a local Chromium/Chrome executable:

```sh
npm install --prefix /tmp/flick-browser-tests --no-save playwright@1.56.1
NODE_PATH=/tmp/flick-browser-tests/node_modules CHROMIUM=/usr/bin/chromium node tests/overlay_browser.cjs
```

The test checks WebGL compilation, idle transparency, CSS click-through, visible
approach rendering, safe filename text, and exactly one post-animation IPC call.
Rust tests exercise gesture ordering/integration/gaps/capture, pairing, bounded
frames, partial-file cleanup and real TCP receipt/cancellation. Kotlin tests verify
matching frame/control layout. Browser tests do not prove native OS transparency,
focus, paste permissions or acoustic accuracy. Those require desktop/device
acceptance tests; the platform matrix in CI checks compilation, not physical use.

### Current-instance results

Linux Tauri compilation/linking and workspace Clippy passed. All eight Rust tests,
ten Kotlin check groups, the real UDP smoke test, and the Chromium shader/animation
regression passed. The Android debug APK built and Android lint reports no issues.
The Win32 clipboard/input module also type-checks against Rust's Windows target.
A native X11 end-to-end attempt was blocked by this non-root environment's relocated
WebKit installation: its helper executables require standard system paths. Thus
native click-through, active-window paste, macOS permissions and real phone motion
remain acceptance tests, not claimed successes. CI covers GUI compilation on all
three operating systems; local browser tests use mocked Tauri IPC.
