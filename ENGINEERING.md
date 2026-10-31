# Engineering changes after v0.1.0-alpha.1

The released alpha remains immutable. These changes are a follow-up working-tree
revision; no new release or environment publication is implied.

- Native capture initialization, stop/join/release and network shutdown are serialized
  on a lifecycle executor. Failure cleanup is automatic, close is idempotent, and
  activity callbacks are scoped to the current capture session.
- Permission-result handling cannot restart capture after leaving the foreground.
  Destination input is validated; capture keeps the screen awake and persists only
  the non-secret desktop address. Buttons reflect initialization/cleanup state.
- Recording startup is tested for each supported source/rate. Hardware audio frame
  timestamps gate freshness instead of stamping old buffered samples as new.
  Unsupported timestamps or persistent overruns stop the session explicitly.
- Calibration uses 60 valid observations with a median and spread check. Invalid
  audio cancels calibration. Gyro and acceleration are displayed locally; the
  original 24-byte telemetry layout remains intact.
- DSP code is separated into an Android-independent JVM module. Confidence checks
  reject clipping and competing strong peaks. Nine executable regression groups
  use no test framework dependency.
- UDP scheduling skips missed deadlines rather than replaying scheduled work.
  Desktop stream ordering also rejects excess delay relative to the best observed
  offset and marks data stale when delivery stops.
- Desktop audio callbacks no longer evaluate sin() per sample or write to stderr.
  An oscillator recurrence is periodically normalized; stream failures propagate to
  the main thread. Oscillator behavior is covered at 44.1/48/96 kHz.
- Cargo.lock and a checksum-pinned Gradle wrapper support repeatable builds. Native
  Android resources, an icon, backup exclusions and cross-platform CI are included.

See README.md for commands, protocol semantics, validation evidence and physical
limits. No absolute position/range or calibrated inertial fusion is claimed.
