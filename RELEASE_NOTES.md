# v0.2.0-alpha.3 — Direct inbox transfer and gesture diagnostics

This update makes image delivery testable independently of the acoustic detector.
Use matching desktop and Android builds, and close the old desktop app first.

## Changes

- **Send to inbox now** on Android sends the selected file/text without a beacon,
  microphone permission, calibration or gesture. It uses the same paired, bounded
  TCP receiver and stores the actual item in the desktop inbox.
- Manual transfers animate/preview received images but do not paste into another
  application. The automatic flick path retains its existing paste behavior.
- Separate transfer status shows requesting/sending/received or a network error;
  live accelerometer/gyro messages no longer overwrite transport feedback.
- Desktop displays radial velocity and hold-still/armed/approach/capture feedback.
  Calibration alone does not send a file. A flick still requires a queued payload,
  a key and an approach above 0.5 m/s toward the speaker followed by a stop.
- Ordinary Android rotation no longer recreates the recording activity. The image
  and key are retained in RAM across other configuration-driven recreations.
- Blank-key recording is explicitly labeled visuals-only. The last code check is
  distinguished from a current authenticated transfer.
- The shared Kotlin transport has a real UDP/TCP JPG test that verifies bytes in a
  disk inbox without using audio or IMU APIs.

## Verification and limits

Ten Rust tests, fourteen Kotlin regression groups, desktop compilation/lint,
Android APK/lint and browser UI tests are required by the release workflow.
Acoustic accuracy and native paste behavior still require physical-device testing.
A successful direct inbox send verifies file transport, not gesture accuracy.

Desktop archives contain flick-overlay, emitter, receiver, guides and GPL-3.0
LICENSE. The Android APK is debug-signed, API 26+. If signatures conflict with an
older debug APK, uninstall that APK first. The desktop pairing key and received
files are retained. SHA256SUMS covers the binary assets; source is available for
this tag. Desktop executables remain unsigned/unnotarized.

Use only a trusted LAN: pairing/control and TCP payloads are not encrypted. Allow
UDP 8089 and TCP 8090 on the desktop's private-network firewall. See OVERLAY.md
for protocol details, platform support, inbox recovery and gesture limitations.
