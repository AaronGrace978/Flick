//! Relative-displacement gesture detector. No absolute ranging is inferred.
use crate::Packet;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Phase {
    Idle,
    Approach,
    Capture,
}
#[derive(Clone, Copy, Debug)]
pub struct Gesture {
    /// Normalized reference minus integrated approach, in nominal meters.
    pub distance: f32,
    pub velocity: f32,
    /// Screen-edge direction in radians. +X acceleration maps to the left edge.
    pub angle: f32,
    pub phase: Phase,
    pub armed: bool,
}

pub struct GestureEngine {
    previous: Option<Packet>,
    gravity: [f32; 3],
    stationary: f32,
    armed: bool,
    phase: Phase,
    distance: f32,
    angle: f32,
    peak: f32,
    cooldown: f32,
    approach_time: f32,
}
impl Default for GestureEngine {
    fn default() -> Self {
        Self {
            previous: None,
            gravity: [0.0; 3],
            stationary: 0.0,
            armed: false,
            phase: Phase::Idle,
            distance: 0.65,
            angle: std::f32::consts::PI,
            peak: 0.0,
            cooldown: 0.0,
            approach_time: 0.0,
        }
    }
}
impl GestureEngine {
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    pub fn update(&mut self, packet: Packet) -> Gesture {
        let dt = self
            .previous
            .and_then(|p| packet.timestamp.checked_sub(p.timestamp))
            .map(|ns| ns as f32 * 1e-9)
            .unwrap_or(0.0);
        if dt > 0.1 || (dt <= 0.0 && self.previous.is_some()) {
            self.reset();
        }
        let previous_velocity = self.previous.map_or(packet.velocity, |p| p.velocity);
        if self.previous.is_none() {
            self.gravity = packet.accel;
        }
        let dt = if self.previous.is_some() { dt } else { 0.0 };
        let residual = std::array::from_fn::<_, 3, _>(|i| packet.accel[i] - self.gravity[i]);
        let acceleration = residual.iter().map(|x| x * x).sum::<f32>().sqrt();
        self.cooldown = (self.cooldown - dt).max(0.0);
        if self.phase == Phase::Capture {
            self.phase = Phase::Idle;
        }
        if packet.velocity.abs() < 0.08 && self.phase == Phase::Idle {
            for (g, a) in self.gravity.iter_mut().zip(packet.accel) {
                *g += (a - *g) * (dt * 4.0).min(1.0);
            }
        }
        if packet.velocity.abs() < 0.08 && acceleration < 1.0 && self.phase == Phase::Idle {
            self.stationary += dt;
            if self.stationary >= 0.3 && self.cooldown == 0.0 {
                self.armed = true;
                self.distance = 0.65;
            }
        } else {
            self.stationary = 0.0;
        }
        if self.phase == Phase::Idle
            && self.armed
            && self.cooldown == 0.0
            && packet.velocity > 0.5
            && packet.velocity > previous_velocity
        {
            self.phase = Phase::Approach;
            self.armed = false;
            self.peak = packet.velocity;
            self.approach_time = 0.0;
            // Device axes are a heuristic. Hold the phone upright in a fixed orientation.
            if residual[0].hypot(residual[1]) > 0.5 {
                self.angle = (-residual[1]).atan2(-residual[0]);
            }
        }
        if self.phase == Phase::Approach {
            self.approach_time += dt;
            self.distance =
                (self.distance - 0.5 * (packet.velocity + previous_velocity) * dt).clamp(0.0, 1.0);
            self.peak = self.peak.max(packet.velocity);
            let slowing = dt > 0.0 && (packet.velocity - previous_velocity) / dt < -3.0;
            let stopped = packet.velocity <= 0.1;
            let deceleration_peak =
                slowing && acceleration > 1.5 && packet.velocity < self.peak * 0.4;
            if self.approach_time > 0.03 && (stopped || deceleration_peak) {
                self.phase = Phase::Capture;
                self.cooldown = 1.2;
            } else if self.approach_time > 1.5 {
                self.phase = Phase::Idle;
                self.armed = false;
                self.cooldown = 0.5;
            }
        }
        self.previous = Some(packet);
        Gesture {
            distance: self.distance,
            velocity: packet.velocity,
            angle: self.angle,
            phase: self.phase,
            armed: self.armed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn packet(frame: u64, v: f32, ax: f32) -> Packet {
        Packet {
            timestamp: 1 + frame * 16_666_667,
            velocity: v,
            accel: [ax, 0.0, 9.81],
        }
    }
    #[test]
    fn flick_integrates_and_captures_once() {
        let mut e = GestureEngine::default();
        for i in 0..30 {
            assert_eq!(e.update(packet(i, 0.0, 0.0)).phase, Phase::Idle);
        }
        let start = e.update(packet(30, 0.8, 3.0));
        assert_eq!(start.phase, Phase::Approach);
        assert!(start.angle.abs() > 3.0);
        let end = e.update(packet(31, 1.0, 0.0));
        assert!(end.distance < start.distance);
        e.update(packet(32, 0.9, 0.0));
        assert_eq!(e.update(packet(33, 0.0, -3.0)).phase, Phase::Capture);
        assert_eq!(e.update(packet(34, 0.8, 3.0)).phase, Phase::Idle);
    }
    #[test]
    fn gaps_and_unarmed_motion_never_capture() {
        let mut e = GestureEngine::default();
        assert_eq!(e.update(packet(1, 1.0, 0.0)).phase, Phase::Idle);
        for i in 2..32 {
            e.update(packet(i, 0.0, 0.0));
        }
        assert_eq!(e.update(packet(32, 0.8, -3.0)).phase, Phase::Approach);
        assert_eq!(e.update(packet(60, 0.0, 0.0)).phase, Phase::Idle);
    }
}
