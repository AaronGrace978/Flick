# v0.3.0-alpha.1 — Flicks without holding the phone on the speaker

A flick no longer depends on a strong 20 kHz lock. Hold the phone still until the
desktop says armed, then flick it toward the screen and stop. Use matching desktop
and Android builds, and close the old desktop app first.

## Changes

- The phone keeps sending motion when it cannot hear the beacon. Those packets use
  the same 24-byte layout with a NaN velocity. A sharp push followed by a stop
  hands the queued file off. A tap, a bump, or walking around does not.
- The acoustic flick still works when the speaker is audible. The search is wider
  (about ±5 m/s), loud room noise no longer hides a weak tone, and a single noisy
  velocity spike cannot fire a capture. A short dropout during an approach no
  longer cancels the flick.
- The desktop shows whether the tone is locked and whether it is waiting for a
  push or an approach. Arming takes about a quarter second of stillness.
- Direct inbox send is unchanged and still needs no gesture.

## Verification and limits

Sixteen Rust tests, fourteen Kotlin regression groups, desktop compilation/lint,
Android APK/lint and browser UI tests are required by the release workflow.
Which physical motion feels like a flick, and whether a given speaker reaches the
phone, still need a real device. A successful direct inbox send verifies file
transport only.

Desktop archives contain flick-overlay, emitter, receiver, guides and GPL-3.0
LICENSE. The Android APK is debug-signed, API 26+. If signatures conflict with an
older debug APK, uninstall that APK first. The desktop pairing key and received
files are retained. SHA256SUMS covers the binary assets; source is available for
this tag. Desktop executables remain unsigned/unnotarized.

Use only a trusted LAN: pairing/control and TCP payloads are not encrypted. Allow
UDP 8089 and TCP 8090 on the desktop's private-network firewall. See OVERLAY.md
for protocol details, platform support, inbox recovery and gesture limitations.
