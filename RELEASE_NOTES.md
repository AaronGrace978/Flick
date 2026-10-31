# v0.4.0-alpha.1 — PDF, Word, MP3, and WAV

Flick a photo, a PDF, a Word document, an MP3, or a WAV from the phone to the
desktop. Use matching desktop and Android builds, and close the old desktop app
first.

## Changes

- PDF, Word (`.doc` and `.docx`), MP3, and WAV files are first-class handoffs.
  The desktop card names the type, then opens the file in the default app.
  Photos still preview and paste as before. A copy of every file stays in the inbox.
- The file limit is 64 MiB, so a normal document or song fits. Larger files are
  rejected before transfer.
- Anything else with a safe filename still transfers and can be pasted. Only the
  four document and audio types open automatically. Programs are not opened.

## Verification and limits

Rust tests, fourteen Kotlin regression groups, desktop compilation/lint, the
overlay browser check, and the release workflow cover this build. Opening a file
still depends on the desktop having a default app for that type. A WAV longer
than a few minutes can exceed 64 MiB because it is uncompressed.

Desktop archives contain flick-overlay, emitter, receiver, guides and GPL-3.0
LICENSE. The Android APK is debug-signed, API 26+. If signatures conflict with an
older debug APK, uninstall that APK first. The desktop pairing key and received
files are retained. SHA256SUMS covers the binary assets. Desktop executables
remain unsigned and unnotarized.

Use only a trusted LAN: pairing and file bytes are not encrypted. Allow UDP 8089
and TCP 8090 on the desktop's private-network firewall. See OVERLAY.md for the
transfer limits and what each file type does on arrival.
