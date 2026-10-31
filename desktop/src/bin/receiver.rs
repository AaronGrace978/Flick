//! Dependency-free receiver for the 24-byte, big-endian Flick v1 wire format.
//! Sender timestamps order packets only: Android and desktop clocks are unrelated.
use std::{io::{self, Write}, net::UdpSocket, time::{Duration, Instant}};

#[derive(Debug)]
struct Packet { timestamp: u64, velocity: f32, accel: [f32; 3] }
fn parse(bytes: &[u8]) -> Option<Packet> {
    if bytes.len() != 24 { return None; }
    let timestamp = u64::from_be_bytes(bytes[0..8].try_into().ok()?);
    let mut values = [0.0; 4];
    for (i, value) in values.iter_mut().enumerate() {
        *value = f32::from_be_bytes(bytes[8+i*4..12+i*4].try_into().ok()?);
    }
    if timestamp == 0 || values.iter().any(|v| !v.is_finite()) || values[0].abs() > 10.0
        || values[1..].iter().any(|v| v.abs() > 200.0) { return None; }
    Some(Packet { timestamp, velocity: values[0], accel: [values[1], values[2], values[3]] })
}
fn main() -> io::Result<()> {
    let socket = UdpSocket::bind("0.0.0.0:8089")?;
    socket.set_nonblocking(true)?;
    println!("Flick UDP :8089 | positive velocity = approach | Ctrl-C exits");
    let mut buffer = [0u8; 65536];
    let mut peer = None;
    let mut timestamp = 0;
    let mut last_seen = Instant::now();
    let mut last_print = Instant::now();
    let mut latest = None;
    loop {
        // Bounded drain keeps a busy sender from starving the dashboard.
        for _ in 0..256 {
            match socket.recv_from(&mut buffer) {
                Ok((n, address)) => {
                    let Some(packet) = parse(&buffer[..n]) else { continue };
                    if last_seen.elapsed() > Duration::from_secs(2) {
                        peer = None; timestamp = 0; latest = None;
                    }
                    if peer.is_some_and(|p| p != address) || packet.timestamp <= timestamp { continue; }
                    peer = Some(address);
                    timestamp = packet.timestamp;
                    last_seen = Instant::now();
                    latest = Some(packet);
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(e),
            }
        }
        if last_print.elapsed() >= Duration::from_millis(100) {
            if let Some(p) = &latest {
                if last_seen.elapsed() < Duration::from_millis(250) {
                    print!("\rΔf {:+7.2} Hz | |Δf| {:6.2} Hz | approach {:+6.3} m/s | accel [{:+7.3}, {:+7.3}, {:+7.3}] m/s²   ",
                        p.velocity * 20000.0 / 343.0, (p.velocity * 20000.0 / 343.0).abs(), p.velocity, p.accel[0], p.accel[1], p.accel[2]);
                } else { print!("\rTelemetry stale; waiting for fresh samples.                                                           "); }
                io::stdout().flush()?;
            }
            last_print = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wire_format_and_rejection() {
        let mut bytes = Vec::from(123_u64.to_be_bytes());
        for v in [0.5_f32, 1.0, 2.0, 9.81] { bytes.extend(v.to_be_bytes()); }
        let p = parse(&bytes).unwrap();
        assert_eq!(p.timestamp, 123); assert_eq!(p.velocity, 0.5); assert_eq!(p.accel[1], 2.0);
        assert!(parse(&bytes[..23]).is_none());
        bytes[8..12].copy_from_slice(&f32::NAN.to_be_bytes());
        assert!(parse(&bytes).is_none());
    }
}
