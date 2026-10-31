# v0.1.0-alpha.1

Initial source-only release of the Flick acoustic Doppler proof of concept.

- Rust 20 kHz desktop emitter using CPAL, with startup and shutdown fades.
- Native Android PCM capture and overlapping Hann-windowed Goertzel estimation.
- Native accelerometer/gyroscope acquisition and non-blocking UDP telemetry.
- Rust telemetry dashboard with strict packet parsing and stale-data indication.

## Validation and limitations

This is an experimental alpha, not a production-ready positioning system.
Compilation, automated Rust tests, and audio/Android hardware testing have not
been performed: the cloud machine lacks the build tools and its network proxy
refuses connections. Android manifest XML parsing and whitespace checks passed.
No binaries or APKs are provided.

The continuous tone estimates radial velocity, not absolute range. IMU readings
are associated with acoustic estimates; calibrated inertial positioning is not
implemented. Ultrasonic response, clock drift and multipath require device
calibration. Telemetry has no authentication and cannot guarantee rejection of
all late packets without synchronized clocks. See README.md for build steps,
wire format, safety guidance and hardware acceptance checks.
