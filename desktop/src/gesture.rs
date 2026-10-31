//! Relative-displacement gesture detector. No absolute ranging is inferred.
//!
//! Two independent triggers share one arming state:
//! - Doppler: a consistent approach toward the speaker followed by a stop/reversal.
//! - Motion: a sharp push followed by a braking deceleration, from the phone's
//!   accelerometer alone. This works without a 20 kHz lock, e.g. across a room.
use crate::Packet;

/// Samples further apart than this are not bridged; the model restarts.
const GAP_RESET: f32 = 0.25;
const STILL_VELOCITY: f32 = 0.15;
const STILL_ACCEL: f32 = 1.2;
const ARM_TIME: f32 = 0.25;
/// Continuous movement this long without a flick disarms (walking, picking the phone up).
const DISARM_MOVING: f32 = 0.6;
const APPROACH_VELOCITY: f32 = 0.35;
const APPROACH_TIMEOUT: f32 = 1.5;
const PUSH_ACCEL: f32 = 9.0;
const BRAKE_ACCEL: f32 = 6.0;
/// Push acceleration must continue for at least one more sample, which rejects taps.
const PUSH_MIN: f32 = 0.015;
const PUSH_TIMEOUT: f32 = 0.5;
const COOLDOWN: f32 = 1.2;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Phase {
    Idle,
    Approach,
    Capture,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Trigger {
    Doppler,
    Motion,
}

#[derive(Clone, Copy, Debug)]
pub struct Gesture {
    /// Normalized reference minus integrated approach, in nominal meters.
    pub distance: f32,
    /// Latest acoustic radial velocity; zero while there is no 20 kHz lock.
    pub velocity: f32,
    pub locked: bool,
    /// Screen-edge direction in radians. +X acceleration maps to the left edge.
    pub angle: f32,
    pub phase: Phase,
    pub armed: bool,
    /// Which detector owns the current approach/capture.
    pub trigger: Option<Trigger>,
}

#[derive(Clone, Copy)]
struct Push {
    direction: [f32; 3],
    elapsed: f32,
    sustained: f32,
}

pub struct GestureEngine {
    previous: Option<u64>,
    last_velocity: Option<f32>,
    gravity: [f32; 3],
    stationary: f32,
    moving: f32,
    armed: bool,
    phase: Phase,
    trigger: Option<Trigger>,
    distance: f32,
    angle: f32,
    peak: f32,
    cooldown: f32,
    approach_time: f32,
    push: Option<Push>,
}
impl Default for GestureEngine {
    fn default() -> Self {
        Self {
            previous: None,
            last_velocity: None,
            gravity: [0.0; 3],
            stationary: 0.0,
            moving: 0.0,
            armed: false,
            phase: Phase::Idle,
            trigger: None,
            distance: 0.65,
            angle: std::f32::consts::PI,
            peak: 0.0,
            cooldown: 0.0,
            approach_time: 0.0,
            push: None,
        }
    }
}

fn norm(v: [f32; 3]) -> f32 {
    v.iter().map(|x| x * x).sum::<f32>().sqrt()
}

impl GestureEngine {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    fn capture(&mut self, trigger: Trigger) {
        self.phase = Phase::Capture;
        self.trigger = Some(trigger);
        self.armed = false;
        self.push = None;
        self.cooldown = COOLDOWN;
        self.distance = 0.0;
    }

    fn cancel(&mut self) {
        self.phase = Phase::Idle;
        self.trigger = None;
        self.armed = false;
        self.push = None;
        self.cooldown = self.cooldown.max(0.3);
        self.distance = 0.65;
    }

    /// Device axes are a heuristic. Hold the phone upright in a fixed orientation.
    fn aim(&mut self, residual: [f32; 3]) {
        if residual[0].hypot(residual[1]) > 0.5 {
            self.angle = (-residual[1]).atan2(-residual[0]);
        }
    }

    pub fn update(&mut self, packet: Packet) -> Gesture {
        let dt = self
            .previous
            .and_then(|p| packet.timestamp.checked_sub(p))
            .map(|ns| ns as f32 * 1e-9)
            .unwrap_or(0.0);
        if dt > GAP_RESET || (dt <= 0.0 && self.previous.is_some()) {
            self.reset();
        }
        let first = self.previous.is_none();
        let dt = if first { 0.0 } else { dt };
        if first {
            self.gravity = packet.accel;
        }
        let velocity = packet.velocity;
        let previous_velocity = self.last_velocity;
        let residual = std::array::from_fn::<_, 3, _>(|i| packet.accel[i] - self.gravity[i]);
        let acceleration = norm(residual);
        self.cooldown = (self.cooldown - dt).max(0.0);
        if self.phase == Phase::Capture {
            self.phase = Phase::Idle;
            self.trigger = None;
            self.distance = 0.65;
        }

        // A pure rotation keeps |accel| near |gravity|; a translation does not. That lets
        // gravity follow orientation changes even without an acoustic velocity.
        let idle = self.phase == Phase::Idle && self.push.is_none();
        let acoustically_still = velocity.is_none_or(|v| v.abs() < STILL_VELOCITY);
        let rotating = (norm(packet.accel) - norm(self.gravity)).abs() < 1.0;
        if idle && acoustically_still && (acceleration < 2.5 || rotating) {
            for (g, a) in self.gravity.iter_mut().zip(packet.accel) {
                *g += (a - *g) * (dt * 4.0).min(1.0);
            }
        }

        if idle && acoustically_still && acceleration < STILL_ACCEL {
            self.stationary += dt;
            self.moving = 0.0;
            if self.stationary >= ARM_TIME && self.cooldown == 0.0 {
                self.armed = true;
                self.distance = 0.65;
            }
        } else {
            self.stationary = 0.0;
            if idle && acceleration > 2.5 {
                self.moving += dt;
                if self.moving > DISARM_MOVING {
                    self.armed = false;
                }
            }
        }

        // Doppler trigger: two consecutive rising locked samples reject one-off outliers.
        if self.phase == Phase::Idle && self.armed && self.cooldown == 0.0 {
            if let (Some(v), Some(p)) = (velocity, previous_velocity) {
                if v > APPROACH_VELOCITY && p > STILL_VELOCITY && v > p {
                    self.phase = Phase::Approach;
                    self.trigger = Some(Trigger::Doppler);
                    self.peak = v;
                    self.approach_time = 0.0;
                    self.aim(residual);
                }
            }
        }
        if self.phase == Phase::Approach && self.trigger == Some(Trigger::Doppler) {
            self.approach_time += dt;
            if let Some(v) = velocity {
                let p = previous_velocity.unwrap_or(v);
                self.distance = (self.distance - 0.5 * (v + p) * dt).clamp(0.0, 1.0);
                self.peak = self.peak.max(v);
                let slowing = dt > 0.0 && (v - p) / dt < -3.0;
                // A stop only counts after a real approach, so a noisy blip cannot capture.
                let stopped = v <= 0.1 && self.peak >= 0.5;
                let deceleration_peak = slowing && acceleration > 1.5 && v < self.peak * 0.4;
                if self.approach_time > 0.03 && (stopped || deceleration_peak) {
                    self.capture(Trigger::Doppler);
                }
            }
            if self.phase == Phase::Approach && self.approach_time > APPROACH_TIMEOUT {
                self.cancel();
            }
        }

        // Motion trigger, also active during a Doppler approach whose lock drops out.
        if self.phase != Phase::Capture && self.cooldown == 0.0 {
            match self.push.as_mut() {
                None if (self.armed || self.phase == Phase::Approach)
                    && acceleration > PUSH_ACCEL =>
                {
                    self.push = Some(Push {
                        direction: residual.map(|x| x / acceleration),
                        elapsed: 0.0,
                        sustained: 0.0,
                    });
                    if self.phase == Phase::Idle {
                        self.phase = Phase::Approach;
                        self.trigger = Some(Trigger::Motion);
                        self.armed = false;
                        self.aim(residual);
                    }
                }
                Some(push) => {
                    push.elapsed += dt;
                    let along: f32 = (0..3).map(|i| residual[i] * push.direction[i]).sum();
                    if along > PUSH_ACCEL * 0.5 {
                        push.sustained += dt;
                    }
                    let (sustained, elapsed) = (push.sustained, push.elapsed);
                    if self.trigger == Some(Trigger::Motion) {
                        self.distance = (0.65 - elapsed * 3.0).max(0.1);
                    }
                    if along < -BRAKE_ACCEL && sustained >= PUSH_MIN {
                        self.capture(Trigger::Motion);
                    } else if along < -BRAKE_ACCEL || elapsed > PUSH_TIMEOUT {
                        // Braking without a real push is a tap or bump.
                        self.push = None;
                        if self.trigger == Some(Trigger::Motion) {
                            self.cancel();
                        }
                    }
                }
                None => {}
            }
        }

        self.previous = Some(packet.timestamp);
        self.last_velocity = velocity;
        Gesture {
            distance: self.distance,
            velocity: velocity.unwrap_or(0.0),
            locked: velocity.is_some(),
            angle: self.angle,
            phase: self.phase,
            armed: self.armed,
            trigger: self.trigger,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const FRAME: u64 = 16_666_667;
    fn packet(frame: u64, v: f32, ax: f32) -> Packet {
        Packet {
            timestamp: 1 + frame * FRAME,
            velocity: Some(v),
            accel: [ax, 0.0, 9.81],
        }
    }
    fn motion(frame: u64, accel: [f32; 3]) -> Packet {
        Packet {
            timestamp: 1 + frame * FRAME,
            velocity: None,
            accel,
        }
    }
    fn settle(e: &mut GestureEngine, from: u64, to: u64, locked: bool) {
        for i in from..to {
            let p = if locked {
                packet(i, 0.0, 0.0)
            } else {
                motion(i, [0.0, 0.0, 9.81])
            };
            assert_eq!(e.update(p).phase, Phase::Idle);
        }
    }

    #[test]
    fn flick_integrates_and_captures_once() {
        let mut e = GestureEngine::default();
        settle(&mut e, 0, 30, true);
        assert!(e.update(packet(30, 0.3, 0.0)).armed);
        let start = e.update(packet(31, 0.8, 3.0));
        assert_eq!(start.phase, Phase::Approach);
        assert_eq!(start.trigger, Some(Trigger::Doppler));
        assert!(start.angle.abs() > 3.0);
        let end = e.update(packet(32, 1.0, 0.0));
        assert!(end.distance < start.distance);
        e.update(packet(33, 0.9, 0.0));
        let captured = e.update(packet(34, 0.0, -3.0));
        assert_eq!(captured.phase, Phase::Capture);
        assert_eq!(captured.trigger, Some(Trigger::Doppler));
        assert_eq!(e.update(packet(35, 0.3, 3.0)).phase, Phase::Idle);
        assert_eq!(e.update(packet(36, 0.8, 3.0)).phase, Phase::Idle);
    }

    #[test]
    fn gaps_and_unarmed_motion_never_capture() {
        let mut e = GestureEngine::default();
        assert_eq!(e.update(packet(1, 1.0, 0.0)).phase, Phase::Idle);
        settle(&mut e, 2, 32, true);
        e.update(packet(32, 0.3, 0.0));
        assert_eq!(e.update(packet(33, 0.8, -3.0)).phase, Phase::Approach);
        assert_eq!(e.update(packet(60, 0.0, 0.0)).phase, Phase::Idle);
        assert!(!e.update(packet(61, 0.0, 0.0)).armed);
    }

    #[test]
    fn single_outlier_does_not_start_an_approach() {
        let mut e = GestureEngine::default();
        settle(&mut e, 0, 30, true);
        assert_eq!(e.update(packet(30, 0.9, 0.0)).phase, Phase::Idle);
        assert_eq!(e.update(packet(31, 0.0, 0.0)).phase, Phase::Idle);
    }

    #[test]
    fn fast_flick_survives_brief_lock_dropout() {
        let mut e = GestureEngine::default();
        settle(&mut e, 0, 30, true);
        e.update(packet(30, 0.4, 0.0));
        assert_eq!(e.update(packet(31, 1.5, 0.0)).phase, Phase::Approach);
        for i in 32..38 {
            let g = e.update(motion(i, [0.0, 0.0, 9.81]));
            assert!(!g.locked && g.phase == Phase::Approach);
        }
        assert_eq!(e.update(packet(38, 0.05, 0.0)).phase, Phase::Capture);
    }

    #[test]
    fn motion_flick_captures_without_acoustic_lock() {
        let mut e = GestureEngine::default();
        settle(&mut e, 0, 30, false);
        assert!(e.update(motion(30, [0.0, 0.0, 9.81])).armed);
        let start = e.update(motion(31, [-12.0, 0.0, 9.81]));
        assert_eq!(
            (start.phase, start.trigger),
            (Phase::Approach, Some(Trigger::Motion))
        );
        assert!(start.angle.abs() < 0.1, "-X push aims at the right edge");
        for i in 32..36 {
            let g = e.update(motion(i, [-11.0, 0.0, 9.81]));
            assert_eq!(g.phase, Phase::Approach);
        }
        let captured = e.update(motion(36, [9.0, 0.0, 9.81]));
        assert_eq!(
            (captured.phase, captured.trigger),
            (Phase::Capture, Some(Trigger::Motion))
        );
        // The rebound and the cooldown cannot produce a second capture.
        for i in 37..80 {
            let a = if i % 2 == 0 { 12.0 } else { -12.0 };
            assert_ne!(e.update(motion(i, [a, 0.0, 9.81])).phase, Phase::Capture);
        }
    }

    #[test]
    fn taps_bumps_and_wandering_do_not_capture() {
        let mut e = GestureEngine::default();
        settle(&mut e, 0, 30, false);
        // A one-sample spike followed by an opposite rebound is a tap, not a flick.
        e.update(motion(30, [0.0, 15.0, 9.81]));
        assert_ne!(
            e.update(motion(31, [0.0, -15.0, 9.81])).phase,
            Phase::Capture
        );
        settle(&mut e, 32, 80, false);
        // Moderate movement for longer than the disarm window, then a sharp jab.
        for i in 80..130 {
            let g = e.update(motion(i, [4.0, 0.0, 9.81 + 3.0]));
            assert_ne!(g.phase, Phase::Capture);
        }
        assert!(!e.update(motion(130, [4.0, 0.0, 12.8])).armed);
        for i in 131..135 {
            e.update(motion(i, [20.0, 0.0, 9.81]));
        }
        assert_ne!(
            e.update(motion(135, [-15.0, 0.0, 9.81])).phase,
            Phase::Capture
        );
    }

    #[test]
    fn short_push_captures_and_a_noisy_blip_does_not() {
        let mut motion_engine = GestureEngine::default();
        settle(&mut motion_engine, 0, 30, false);
        motion_engine.update(motion(30, [-12.0, 0.0, 9.81]));
        motion_engine.update(motion(31, [-12.0, 0.0, 9.81]));
        assert_eq!(
            motion_engine.update(motion(32, [9.0, 0.0, 9.81])).phase,
            Phase::Capture
        );
        let mut acoustic = GestureEngine::default();
        settle(&mut acoustic, 0, 30, true);
        acoustic.update(packet(30, 0.2, 0.0));
        assert_eq!(acoustic.update(packet(31, 0.4, 0.0)).phase, Phase::Approach);
        assert_eq!(acoustic.update(packet(32, 0.0, 0.0)).phase, Phase::Approach);
    }

    #[test]
    fn gravity_follows_slow_rotation_and_rearms() {
        let mut e = GestureEngine::default();
        settle(&mut e, 0, 30, false);
        e.update(motion(30, [0.0, 15.0, 9.81]));
        assert!(!e.update(motion(31, [0.0, -15.0, 9.81])).armed);
        let tilted = [0.0, 9.81 * 0.5_f32.sin(), 9.81 * 0.5_f32.cos()];
        let mut armed = false;
        for i in 32..120 {
            armed = e.update(motion(i, tilted)).armed;
        }
        assert!(armed, "a held tilt must re-arm after gravity adapts");
    }
}
