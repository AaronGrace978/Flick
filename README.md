# Flick acoustic Doppler

<img width="1408" height="768" alt="image" src="https://github.com/user-attachments/assets/18a2e436-4d4c-44a2-9a9d-fde243ae8e79" />

**New: [Flick Visual Layer](OVERLAY.md)** — a Tauri click-through cyan edge overlay,
relative-displacement gestures, styled native Android controls, and paired text/file
handoff with post-animation native paste. See that guide for GUI prerequisites,
pairing, platform support, and the trusted-LAN transfer protocol.

A software-only ultrasonic Doppler proof of concept: a Rust desktop emits a
20 kHz beacon, a native Android app estimates radial motion using Goertzel DSP,
and a Rust dashboard receives 60 Hz UDP telemetry.

The sole third-party audio dependency is the explicitly requested `cpal` crate.
Android uses platform hardware APIs and Kotlin/JDK standard libraries. No DSP,
audio-processing, UI, networking or sensor-fusion framework is bundled.

## Quick start

On the desktop, install stable Rust and the native audio prerequisites. Debian/
Ubuntu needs `libasound2-dev` and `pkg-config`; Windows/macOS use CPAL's native
backends. Run in separate terminals:

```sh
cd desktop
cargo run --release --locked --bin receiver
cargo run --release --locked --bin emitter
```

Allow inbound UDP 8089 on the trusted LAN. Start with **low speaker volume**.
20 kHz may be audible, especially to younger listeners. Do not increase volume
aggressively to compensate for poor ultrasonic response. The beacon has a fixed
0.1 peak amplitude and 100 ms startup/shutdown fades. Press Enter to stop.
Device loss or forced termination cannot guarantee a fade.

For Android, install JDK 17 or 21 and Android SDK Platform 35, Build-Tools 35.0.0
and Platform-Tools. Set `ANDROID_HOME` or configure `android/local.properties`.
The included Gradle 8.9 wrapper verifies its distribution with SHA-256.

```sh
cd android
./gradlew --no-daemon :core:check :app:assembleDebug :app:lintDebug
adb install -r app/build/outputs/apk/debug/app-debug.apk
```

Windows uses `gradlew.bat`. Android Studio can open `android/` directly. AGP
8.7.3 and Kotlin 2.0.21 are pinned. The debug APK is installable, but not an
app-store release; production signing keys are intentionally not stored here.

On Android 8.0/API 26 or newer, enter the desktop LAN IPv4 address and tap Start.
Grant microphone permission. Keep both devices stationary, turn the beacon on,
and tap **Calibrate stationary baseline**. Calibration collects 60 consecutive
valid windows (about one second), rejecting a spread above 3 Hz. It is session
local and must be repeated after a restart. Motion during calibration invalidates
its physical meaning. The phone shows signal state, signed velocity, acceleration
and gyro vectors. Stop releases all hardware asynchronously. Leaving the foreground
stops capture; the screen stays awake while capture is active. A failure also
releases resources and allows Start to retry.

## What it measures

A continuous single tone measures **radial velocity**, not absolute range,
proximity or full 3D position. Amplitude is not a reliable distance measurement.
The raw accelerometer and gyroscope samples are time-associated with acoustic
estimates; calibrated inertial positioning or world-coordinate fusion is not
claimed. Acceleration includes gravity in Android device axes (m/s²); gyro uses
rad/s. All three sensors are required.

With a stationary source and moving microphone:

`approach_velocity = 343 * (observed_frequency - reference_frequency) / reference_frequency`

Positive is approach; negative is recession. The reference is nominally 20 kHz,
or the observed stationary carrier after calibration. Calibration removes a fixed
clock/speaker bias, not subsequent drift or multipath. Sound speed is assumed to
be 343 m/s and varies with temperature. The desktop reconstructs an equivalent
frequency shift at the nominal 20 kHz carrier from the transmitted velocity;
the fixed packet has no raw frequency field.

Many speakers/microphones filter 20 kHz. Goertzel cannot recover frequencies the
hardware never captures. UNPROCESSED audio is tried when advertised, followed by
VOICE_RECOGNITION; both 48 kHz and 44.1 kHz configurations are tried through actual
recording startup. Processing or hardware may still alter the signal.

## DSP and latency

The pure Kotlin core uses N=4096 Hann-windowed samples and a 41-frequency Goertzel
bank with half-bin spacing centered on 20 kHz. Log-power interpolation estimates
signed shifts between bank frequencies. At 48 kHz, independent resolution is
11.72 Hz and the search spans about ±117 Hz (±108 Hz at 44.1 kHz). Silence,
clipping, weak peaks, competing strong peaks and search-edge peaks are rejected.
Thresholds are POC defaults and need real-device characterization.

Windows overlap at 60 Hz: 800 new samples at 48 kHz, 735 at 44.1 kHz. The window
spans 85.3/92.9 ms, so overlap improves update cadence rather than true resolution.
Measurement latency includes roughly half a window plus capture, computation and
network latency. Do not interpret 60 Hz telemetry as 16 ms physical latency.

Audio reads run on an audio-priority thread. Native BOOTTIME frame timestamps
identify the acquisition time; data over 100 ms old is suppressed, and persistent
capture overruns or unsupported timestamps stop capture with an actionable error.
IMU samples request 200 Hz and must be fresh within 100 ms. Sensors and UDP use
separate workers. A latest-value slot, non-blocking sends and a deadline-skipping
scheduler prevent replay after scheduling stalls. Invalid audio suppresses packets;
60 Hz scheduling does not imply 60 valid measurements each second.

## UDP protocol v1

Unicast to the selected desktop on port 8089. Exactly 24 bytes, big endian,
without padding:

| Offset | Type | Meaning |
| --- | --- | --- |
| 0 | unsigned 64-bit integer | Android boottime ns at the end of the audio window |
| 8 | IEEE754 float32 | signed radial approach velocity, m/s |
| 12 | IEEE754 float32 | raw acceleration X, m/s² |
| 16 | IEEE754 float32 | raw acceleration Y, m/s² |
| 20 | IEEE754 float32 | raw acceleration Z, m/s² |

Gyro stays in the Android estimate and display because the specified packet has
no gyro field. There is no range field. The alpha used estimate-availability
rather than acquisition-end timestamps; numeric ordering remains wire compatible.

The receiver binds `0.0.0.0:8089`; an optional argument overrides the bind address.
It validates lengths, finite/range-bounded values, sender ownership and timestamp
ordering. A sender owns the stream until two seconds of inactivity. Duplicates,
reordered packets and excess relative transit delay above 100 ms are dropped.
Arrival-minus-sender time is tracked against its minimum observed offset; clocks
do not need a common epoch. This cannot reveal the initial path delay or reject
every delayed packet. Guaranteed absolute age rejection requires synchronized
clocks. Display data becomes stale after 250 ms. The dashboard updates at 10 Hz,
independent of UDP ingestion, and produces newline-delimited output in pipes.

IP/UDP checksums plus strict parsing detect transport errors and malformed data;
there is no application CRC, encryption, authentication or retransmission. Use
only a trusted LAN. Other emitters, multipath and malicious/valid-looking data
can still produce incorrect estimates. This is experimental measurement software.

## Checks and automation

```sh
cd desktop
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
python3 ../tests/udp_smoke.py
cd ../android
./gradlew --no-daemon :core:check :app:assembleDebug :app:lintDebug
```

The receiver/library can be built without CPAL/native audio dependencies using
`cargo test --locked --no-default-features` or
`cargo build --release --locked --no-default-features --bin receiver`.

`core:check` executes ten standard-library regression groups (not a zero-test
JUnit run): both capture rates, Doppler sign/interpolation, noise, clipping,
competing tones, calibration, wire bytes, sensor freshness and sender freshness.
Rust tests cover packet validation, ordering, relative-delay rejection, reconnects,
and oscillator frequency/amplitude/fades. The UDP smoke test launches the actual
receiver and exercises real datagrams, stale indication and recovery. The GitHub
workflow configures Linux/macOS/Windows desktop builds and Android APK/lint checks;
remote CI results are not claimed until that workflow runs.

## Verified here and remaining acceptance

Verified in this Linux cloud instance:

- Both Rust release binaries compiled; eight Rust regression tests passed.
- Rust formatting and Clippy with warnings denied passed.
- The real loopback UDP smoke test passed.
- CPAL opened a 48 kHz ALSA null sink and exited cleanly on Enter.
- Ten Kotlin core regression groups passed on the JVM.
- Android debug APK built; Android lint passed (see the generated report).
- The checked-in Gradle wrapper downloaded and checksum-verified Gradle 8.9.

There is no physical audio device or attached Android device in this environment.
Acoustic response, Android lifecycle behavior on actual devices, sustained 60 Hz
throughput/thermal behavior, hardware timestamps and Windows/macOS runtime behavior
remain hardware acceptance checks. The null sink does not establish audible output
or acoustic tracking accuracy. This build is not a production-certified tracker.

For hardware acceptance, verify stationary calibration near zero, positive/negative
velocity while approaching/receding, suppression when the beacon is switched off,
clean resource release after permission revocation/backgrounding, and stable timing
under sustained load. Compare motion to an external reference before relying on
measurements. Absolute proximity would require a different waveform/protocol,
such as synchronized chirp/time-of-flight ranging; it cannot be engineered out of
a continuous unmodulated tone.
