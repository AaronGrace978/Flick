//! Non-blocking single-sender telemetry dashboard. Optional argument: bind address.
use flick_radar::{Packet, Tracker, CARRIER_HZ, SOUND_SPEED};
use std::{
    io::{self, IsTerminal, Write},
    net::UdpSocket,
    time::{Duration, Instant},
};
fn main() -> io::Result<()> {
    let bind = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "0.0.0.0:8089".into());
    let socket = UdpSocket::bind(&bind)?;
    socket.set_nonblocking(true)?;
    println!(
        "Flick UDP {} | positive velocity = approach | Ctrl-C exits",
        socket.local_addr()?
    );
    io::stdout().flush()?;
    let mut buffer = [0u8; 65536];
    let epoch = Instant::now();
    let mut tracker = Tracker::default();
    let mut last_seen = Instant::now();
    let mut last_print = Instant::now();
    let mut latest: Option<Packet> = None;
    let terminal = io::stdout().is_terminal();
    let mut stale = false;
    loop {
        for _ in 0..256 {
            match socket.recv_from(&mut buffer) {
                Ok((n, peer)) => {
                    let Some(packet) = Packet::parse(&buffer[..n]) else {
                        continue;
                    };
                    if !tracker.accept(peer, packet, epoch.elapsed()) {
                        continue;
                    }
                    last_seen = Instant::now();
                    latest = Some(packet);
                    stale = false;
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
        if last_print.elapsed() >= Duration::from_millis(100) {
            if let Some(p) = &latest {
                if last_seen.elapsed() < Duration::from_millis(250) {
                    let acoustic = match p.velocity {
                        Some(v) => {
                            let shift = v * CARRIER_HZ as f32 / SOUND_SPEED;
                            format!(
                                "Δf {shift:+7.2} Hz | |Δf| {:6.2} Hz | approach {v:+6.3} m/s",
                                shift.abs()
                            )
                        }
                        None => "no 20 kHz lock (motion only)".into(),
                    };
                    let text = format!(
                        "{acoustic} | accel [{:+7.3}, {:+7.3}, {:+7.3}] m/s²",
                        p.accel[0], p.accel[1], p.accel[2]
                    );
                    if terminal {
                        print!("\r{text}          ");
                    } else {
                        println!("{text}");
                    }
                } else if !stale {
                    let text = "Telemetry stale; waiting for fresh samples.";
                    if terminal {
                        print!("\r{text:150}");
                    } else {
                        println!("{text}");
                    }
                    stale = true;
                }
                io::stdout().flush()?;
            }
            last_print = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}
