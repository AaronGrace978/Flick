# Flick acoustic Doppler proof of concept

Desktop Rust beacon and UDP dashboard, with a native Kotlin Android receiver.
The only third-party runtime library is the explicitly requested `cpal` audio
backend. Android uses platform APIs and the Kotlin standard library. Gradle's
Android/Kotlin plugins are build tools, not bundled audio/DSP frameworks.

## Physical limits

This is a Doppler proof of concept, not a production positioning system. A
continuous 20 kHz tone cannot determine absolute range or proximity. Its amplitude
is strongly affected by speaker/microphone response, orientation, AGC and multipath.
The receiver measures radial velocity, not full 3D velocity. The IMU is sampled and
time-associated with the estimate; no unobservable absolute position or calibrated
inertial fusion is claimed. Raw accelerometer data includes gravity; gyro units
are rad/s and acceleration units are m/s² in Android's device coordinate frame.

For a stationary source and moving microphone, approach velocity is
`343 * (observed_hz - 20000) / 20000` m/s. Positive means approach. The dashboard
also shows the absolute frequency delta. Removing the sign before calculating
velocity would make approach and recession indistinguishable. The assumed sound
speed is 343 m/s; temperature and independent audio-clock drift cause bias.
A stationary calibration and a calibrated speed-of-sound model are needed for
accurate measurements. Many devices filter 20 kHz entirely.

The 4096-sample Hann window spans 85.3 ms at 48 kHz or 92.9 ms at 44.1 kHz.
Half-bin Goertzel sampling and interpolation improve peak localization, not true
resolution. Windows overlap at 60 Hz; this introduces roughly half-window
measurement latency plus capture, processing and network latency. Search span is
approximately ±117 Hz at 48 kHz. Weak, ambiguous and edge peaks suppress packets;
a 60 Hz transmit schedule does not imply 60 valid measurements per second.
Frequency estimation thresholds require real-device tuning.

## Desktop

Install a stable Rust toolchain. On Debian/Ubuntu, CPAL's native Linux backend
requires `pkg-config` and `libasound2-dev`; a usable audio device and sound server
must be accessible. Windows/macOS use CPAL's native platform backends.

```sh
cd desktop
cargo test --no-default-features --bin receiver
cargo build --release
cargo run --release --bin receiver
# In a second terminal:
cargo run --release --bin emitter
```

Allow inbound UDP port 8089 on the trusted LAN. Start with **low system volume**:
20 kHz may be audible, especially to younger people; do not compensate for weak
microphone response by increasing volume aggressively. Output amplitude is 0.1
with a 100 ms fade. Press Enter for a smooth stop. Forced process termination or
device removal cannot guarantee a fade. CPAL callback failures print to stderr.
The oscillator does no allocation, locking or networking inside its callback.

The receiver needs no dependencies when built with `--no-default-features`.
It accepts one sender until two seconds of inactivity, drains queued packets,
drops malformed/nonfinite/out-of-range and out-of-order packets, and marks data
stale after 250 ms. The dashboard refreshes at 10 Hz to limit terminal overhead;
UDP ingestion runs independently of that refresh rate. Ctrl-C stops the receiver.

## Android

Open `android/` in Android Studio with JDK 17, Android SDK 35 and Gradle 8.9,
or use an installed Gradle 8.9:

```sh
cd android
gradle --no-daemon :app:assembleDebug
adb install -r app/build/outputs/apk/debug/app-debug.apk
```

No Gradle wrapper binary is included. Android Gradle Plugin 8.7.3 and Kotlin
2.0.21 are pinned in the build files. No AndroidX dependency is required.
Use Android 8.0/API 26 or newer with microphone, accelerometer and gyroscope.
Enter the desktop LAN IPv4 address, tap Start and grant microphone permission.
The app stops when it leaves the foreground; it does not silently record in the
background. Recording requests UNPROCESSED when advertised, otherwise
VOICE_RECOGNITION to reduce voice processing. Hardware may still filter ultrasound.
IMU events request 200 Hz; actual delivery depends on hardware and Android policy.

The audio worker uses blocking native reads, with network sends on a separate
scheduled worker using a non-blocking DatagramChannel. There is no queued telemetry
backlog; only the newest valid estimate is retained. Sensors must be fresh within
100 ms. Audio acquisition/processing failures stop emission of telemetry; tap
Stop to release resources before restarting. Keep the screen awake during tests.

## Wire format (v1)

Exactly 24 bytes, network byte order (big endian); no padding:

| Offset | Type | Meaning |
| --- | --- | --- |
| 0 | unsigned 64-bit integer | Android elapsed-realtime timestamp, nanoseconds, estimate availability time |
| 8 | IEEE 754 float32 | signed radial approach velocity, m/s |
| 12 | IEEE 754 float32 | acceleration X, m/s² |
| 16 | IEEE 754 float32 | acceleration Y, m/s² |
| 20 | IEEE 754 float32 | acceleration Z, m/s² |

Gyro vectors are acquired and included in the local immutable Estimate, but the
specified wire format has no gyro field. Dashboard frequency shift is reconstructed
from velocity, using the same carrier and sound speed. No range field is invented.
UDP is unicast to the chosen desktop IP, port 8089 (not LAN-wide broadcast).
IP/UDP checksums and strict parsing reject transport corruption and malformed
payloads; this protocol has no cryptographic integrity or sender authentication.
Use it only on a trusted LAN. There is no retransmission.

Timestamp ordering rejects duplicates and reordering, but **cannot identify all
late packets**: host monotonic clocks are not synchronized. A first packet or an
in-order delayed packet has no provable transit age. Receiver arrival-time expiry
and sender freshness limits provide best-effort latency handling. Guaranteed age
rejection requires clock synchronization and an extended protocol. The latest
window is sent at most once; missed scheduler deadlines do not replay old data.

## Validation status and hardware acceptance

In the supplied cloud environment, Rust/Cargo, Gradle and the Android SDK were
unavailable. The configured network proxy refused connections, preventing tool
installation. Neither project has been compiled or run on hardware here. The
repository includes a Rust packet parser test; it is not claimed as passed.

On a development machine, run the commands above, then verify:

1. Emitter startup/shutdown is clean at low volume on the selected output device.
2. With the phone stationary, packets arrive and measured shift is near the
   device-specific calibrated baseline; turning the beacon off suppresses packets.
3. Motion toward/away from the speaker gives positive/negative radial velocity.
4. Stopping capture marks the desktop stale; malformed lengths and NaNs are rejected
   by the included parser test. Packet loss must never cause replay or blocking.
5. Measure sustained capture/processing timing and thermal load on the target
   phone; validate 60 Hz scheduling and audio overrun behavior before deployment.

Multipath, other 20 kHz emitters, audio-clock offsets and capture overruns can
produce plausible but wrong measurements. Device calibration, confidence telemetry,
a protocol version/checksum, authenticated networking, instrumentation and real
hardware testing are necessary before treating this as production infrastructure.
