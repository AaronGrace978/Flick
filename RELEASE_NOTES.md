# v0.1.0-alpha.2

First binary release of the Flick acoustic Doppler proof of concept.

## Downloads

- `flick-desktop-*-linux-x86_64.tar.gz`, `*-windows-x86_64.zip`,
  `*-macos-arm64.tar.gz`: the `emitter` (20 kHz beacon) and `receiver`
  (UDP dashboard) binaries.
- `flick-android-*.apk`: the Android receiver (API 26+). Debug-signed for
  sideloading; enable installs from unknown sources.

## Changes since v0.1.0-alpha.1

- Hardened acoustic tracking pipeline; shared desktop library with tests.
- Desktop and Android builds verified in CI, plus a UDP smoke test.
- Automated GitHub release workflow for desktop and Android artifacts.

## Limitations

This is an experimental alpha, not a production-ready positioning system, and
it has not been validated on audio/Android hardware. The continuous tone
estimates radial velocity, not absolute range. Ultrasonic response, clock
drift and multipath require device calibration. Telemetry has no
authentication; use only on a trusted LAN. Start the emitter at low volume.
See README.md for build steps, wire format and hardware acceptance checks.
