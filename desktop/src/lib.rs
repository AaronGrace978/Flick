//! Shared wire validation, stream ordering and allocation-free beacon oscillator.
use std::{net::SocketAddr, time::Duration};
pub const CARRIER_HZ: f64 = 20_000.0;
pub const SOUND_SPEED: f32 = 343.0;
pub const PACKET_BYTES: usize = 24;

/// v1 telemetry uses big-endian IEEE754 floats and Android boottime nanoseconds.
/// A NaN velocity is a motion-only sample: the phone has no reliable 20 kHz lock.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Packet {
    pub timestamp: u64,
    pub velocity: Option<f32>,
    pub accel: [f32; 3],
}
impl Packet {
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != PACKET_BYTES {
            return None;
        }
        let timestamp = u64::from_be_bytes(bytes[0..8].try_into().ok()?);
        let mut values = [0.0; 4];
        for (i, value) in values.iter_mut().enumerate() {
            *value = f32::from_be_bytes(bytes[8 + i * 4..12 + i * 4].try_into().ok()?);
        }
        let velocity = (!values[0].is_nan()).then_some(values[0]);
        if timestamp == 0
            || velocity.is_some_and(|v| !v.is_finite() || v.abs() > 10.0)
            || values[1..]
                .iter()
                .any(|v| !v.is_finite() || v.abs() > 200.0)
        {
            return None;
        }
        Some(Self {
            timestamp,
            velocity,
            accel: [values[1], values[2], values[3]],
        })
    }
}

/// Single-sender ownership, monotonic ordering, and relative-delay rejection.
/// Arrival is a duration since a receiver-local epoch. Clock epochs need not match.
/// An initial path delay remains unknowable; only excess delay above the minimum
/// observed offset is rejected. Restart ownership expires after two idle seconds.
#[derive(Default)]
pub struct Tracker {
    peer: Option<SocketAddr>,
    timestamp: u64,
    last_arrival: Duration,
    min_offset: i128,
}
impl Tracker {
    pub fn accept(&mut self, peer: SocketAddr, packet: Packet, arrival: Duration) -> bool {
        if self.peer.is_some() && arrival.saturating_sub(self.last_arrival) > Duration::from_secs(2)
        {
            *self = Self::default();
        }
        if self.peer.is_some_and(|p| p != peer) || packet.timestamp <= self.timestamp {
            return false;
        }
        let offset = arrival.as_nanos() as i128 - packet.timestamp as i128;
        if self.peer.is_some() && offset - self.min_offset > 100_000_000 {
            return false;
        }
        self.min_offset = if self.peer.is_none() {
            offset
        } else {
            self.min_offset.min(offset)
        };
        self.peer = Some(peer);
        self.timestamp = packet.timestamp;
        self.last_arrival = arrival;
        true
    }
}

/// Complex unit-circle recurrence: no trigonometric call per audio sample.
/// A periodic normalization bounds accumulated floating-point roundoff.
pub struct Oscillator {
    sin: f64,
    cos: f64,
    step_sin: f64,
    step_cos: f64,
    gain: f64,
    ramp: f64,
    frames: u32,
}
impl Oscillator {
    pub fn new(rate: u32) -> Self {
        assert!(rate >= 44_100);
        let (step_sin, step_cos) = (std::f64::consts::TAU * CARRIER_HZ / rate as f64).sin_cos();
        Self {
            sin: 0.0,
            cos: 1.0,
            step_sin,
            step_cos,
            gain: 0.0,
            ramp: 1.0 / (rate as f64 * 0.1),
            frames: 0,
        }
    }
    /// Fixed -20 dBFS peak amplitude and 100 ms linear fades.
    pub fn sample(&mut self, stopping: bool) -> f32 {
        let target = if stopping { 0.0 } else { 1.0 };
        self.gain += (target - self.gain).clamp(-self.ramp, self.ramp);
        let sample = (self.sin * 0.1 * self.gain) as f32;
        let sin = self.sin * self.step_cos + self.cos * self.step_sin;
        self.cos = self.cos * self.step_cos - self.sin * self.step_sin;
        self.sin = sin;
        self.frames += 1;
        if self.frames == 4096 {
            let scale = self.sin.hypot(self.cos);
            self.sin /= scale;
            self.cos /= scale;
            self.frames = 0;
        }
        sample
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn bytes() -> Vec<u8> {
        let mut b = Vec::from(123_u64.to_be_bytes());
        for v in [0.5_f32, 1.0, -2.0, 9.81] {
            b.extend(v.to_be_bytes());
        }
        b
    }
    #[test]
    fn parsing() {
        let b = bytes();
        let p = Packet::parse(&b).unwrap();
        assert_eq!(p.timestamp, 123);
        assert_eq!(p.velocity, Some(0.5));
        assert_eq!(p.accel, [1.0, -2.0, 9.81]);
        for n in 0..24 {
            assert!(Packet::parse(&b[..n]).is_none());
        }
        let mut big = b.clone();
        big.push(0);
        assert!(Packet::parse(&big).is_none());
        for index in 0..4 {
            for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 201.0] {
                if index == 0 && value.is_nan() {
                    continue;
                }
                let mut bad = b.clone();
                bad[8 + index * 4..12 + index * 4].copy_from_slice(&value.to_be_bytes());
                assert!(Packet::parse(&bad).is_none());
            }
        }
        let mut motion = b.clone();
        motion[8..12].copy_from_slice(&f32::NAN.to_be_bytes());
        let m = Packet::parse(&motion).unwrap();
        assert_eq!((m.velocity, m.accel), (None, [1.0, -2.0, 9.81]));
        let mut fast = b.clone();
        fast[8..12].copy_from_slice(&10.5_f32.to_be_bytes());
        assert!(Packet::parse(&fast).is_none());
        let mut zero = b;
        zero[..8].fill(0);
        assert!(Packet::parse(&zero).is_none());
    }
    #[test]
    fn ordering_delay_and_reconnect() {
        let a = "127.0.0.1:1000".parse().unwrap();
        let b = "127.0.0.1:1001".parse().unwrap();
        let mut p = Packet::parse(&bytes()).unwrap();
        let mut t = Tracker::default();
        assert!(t.accept(a, p, Duration::ZERO));
        assert!(!t.accept(a, p, Duration::from_millis(1)));
        p.timestamp += 16_000_000;
        assert!(!t.accept(b, p, Duration::from_millis(16)));
        assert!(t.accept(a, p, Duration::from_millis(16)));
        p.timestamp += 16_000_000;
        assert!(!t.accept(a, p, Duration::from_millis(200)));
        p.timestamp += 200_000_000;
        assert!(t.accept(a, p, Duration::from_millis(232)));
        p.timestamp = 1;
        assert!(t.accept(b, p, Duration::from_secs(3)));
    }
    #[test]
    fn oscillator_frequency_gain_and_fade() {
        for rate in [44100, 48000, 96000] {
            let mut o = Oscillator::new(rate);
            for i in 0..rate * 2 {
                let sample = o.sample(false);
                assert!(sample.is_finite() && sample.abs() <= 0.100001);
                if i > rate / 10 {
                    let expected =
                        (std::f64::consts::TAU * CARRIER_HZ * i as f64 / rate as f64).sin() * 0.1;
                    assert!((sample as f64 - expected).abs() < 1e-6);
                }
            }
            for _ in 0..rate / 10 + 1 {
                o.sample(true);
            }
            for _ in 0..1000 {
                assert!(o.sample(true).abs() < 1e-8);
            }
        }
    }
}

pub mod gesture;
pub mod handoff;
