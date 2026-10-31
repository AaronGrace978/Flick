//! Transparent, non-activating overlay. UDP never performs blocking asset or UI work.
mod native;
use flick_radar::{
    gesture::{GestureEngine, Phase},
    handoff::{self, Asset},
    Packet, Tracker,
};
use serde::Serialize;
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    net::{SocketAddr, TcpListener, UdpSocket},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager};

#[derive(Clone, Serialize)]
struct TelemetryPayload {
    distance: f32,
    velocity: f32,
    angle: f32,
    phase: &'static str,
}
#[derive(Clone, Serialize)]
struct AssetPayload {
    id: u64,
    label: String,
    angle: f32,
}
#[derive(Clone, Serialize)]
struct StatusPayload {
    id: u64,
    ok: bool,
    message: String,
}
struct Pending {
    id: u64,
    asset: Asset,
    target: Result<u64, String>,
    ready: Instant,
}
struct Runtime {
    running: AtomicBool,
    ready: AtomicBool,
    busy: AtomicBool,
    pending: Mutex<Option<Pending>>,
}

/// Only the trusted local webview can request this, after its asset animation ends.
/// The backend selects the received asset, not arbitrary frontend-supplied content.
#[tauri::command]
async fn complete_handoff(
    id: u64,
    state: tauri::State<'_, Arc<Runtime>>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let pending = {
        let mut slot = state.pending.lock().map_err(|_| "Handoff state poisoned")?;
        let p = slot.as_ref().ok_or("No received asset")?;
        if p.id != id || p.ready.elapsed() < Duration::from_millis(700) {
            return Err("Wrong or premature handoff".into());
        }
        slot.take().unwrap()
    };
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let result = pending
            .target
            .and_then(|target| native::paste(&pending.asset, target));
        let message = match &result {
            Ok(_) => "Clipboard populated; paste sent to the active target".into(),
            Err(e) => format!("{e}. Received items remain in the inbox."),
        };
        let _ = app.emit(
            "flick://status",
            StatusPayload {
                id,
                ok: result.is_ok(),
                message,
            },
        );
        state.busy.store(false, Ordering::Relaxed);
        result
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
fn frontend_ready(state: tauri::State<'_, Arc<Runtime>>) {
    state.ready.store(true, Ordering::Relaxed);
}

fn pairing(directory: &std::path::Path) -> io::Result<[u8; 32]> {
    fs::create_dir_all(directory)?;
    let path = directory.join("pairing.key");
    let token = if path.exists() {
        let hex = fs::read_to_string(&path)?;
        if hex.trim().len() != 64 || !hex.trim().is_ascii() {
            return Err(io::Error::other(
                "Invalid pairing.key; remove it to re-pair",
            ));
        }
        let mut token = [0; 32];
        for (i, b) in token.iter_mut().enumerate() {
            *b = u8::from_str_radix(&hex.trim()[i * 2..i * 2 + 2], 16)
                .map_err(|_| io::Error::other("Invalid pairing code"))?;
        }
        token
    } else {
        let token = handoff::random_token()?;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&path)?;
        for b in token {
            write!(file, "{b:02x}")?;
        }
        file.flush()?;
        token
    };
    eprintln!(
        "Pair Android using the 64-character code in {} (never share it).",
        path.display()
    );
    Ok(token)
}
struct Capture {
    id: u64,
    peer: SocketAddr,
    angle: f32,
    listener: TcpListener,
    expires: Instant,
    worker: bool,
}
fn serve(
    app: tauri::AppHandle,
    state: Arc<Runtime>,
    directory: PathBuf,
    token: [u8; 32],
    udp: UdpSocket,
) {
    let epoch = Instant::now();
    let mut tracker = Tracker::default();
    let mut engine = GestureEngine::default();
    let mut last = Instant::now();
    let mut stale = false;
    let mut bytes = [0u8; 65536];
    let mut capture: Option<Capture> = None;
    let seed = handoff::random_token().unwrap_or([1; 32]);
    let mut next_id = u64::from_be_bytes(seed[..8].try_into().unwrap()) & ((1u64 << 48) - 1);
    next_id = next_id.max(1);
    type TransferResult = (u64, Result<(Asset, Result<u64, String>), String>);
    let (tx, rx) = mpsc::channel::<TransferResult>();
    while state.running.load(Ordering::Relaxed) {
        while let Ok((id, result)) = rx.try_recv() {
            if !capture.as_ref().is_some_and(|c| c.id == id) {
                continue;
            }
            let angle = capture.as_ref().map_or(std::f32::consts::PI, |c| c.angle);
            capture = None;
            match result {
                Ok((asset, target)) => {
                    let label = asset.label();
                    if let Ok(mut pending) = state.pending.lock() {
                        *pending = Some(Pending {
                            id,
                            asset,
                            target,
                            ready: Instant::now(),
                        });
                    }
                    if let Err(e) = app.emit("flick://asset", AssetPayload { id, label, angle }) {
                        eprintln!("Asset UI delivery: {e}");
                    }
                }
                Err(e) => {
                    state.busy.store(false, Ordering::Relaxed);
                    let _ = app.emit(
                        "flick://status",
                        StatusPayload {
                            id,
                            ok: false,
                            message: e,
                        },
                    );
                }
            }
        }
        if let Ok(mut pending) = state.pending.lock() {
            if pending
                .as_ref()
                .is_some_and(|p| p.ready.elapsed() > Duration::from_secs(30))
            {
                *pending = None;
                state.busy.store(false, Ordering::Relaxed);
                let _ = app.emit(
                    "flick://status",
                    StatusPayload {
                        id: 0,
                        ok: false,
                        message: "Animation acknowledgement expired; nothing pasted".into(),
                    },
                );
            }
        }
        for _ in 0..128 {
            match udp.recv_from(&mut bytes) {
                Ok((n, peer)) => {
                    let Some(packet) = Packet::parse(&bytes[..n]) else {
                        continue;
                    };
                    if !tracker.accept(peer, packet, epoch.elapsed())
                        || !state.ready.load(Ordering::Relaxed)
                    {
                        continue;
                    }
                    if last.elapsed() > Duration::from_millis(150) {
                        engine.reset();
                    }
                    last = Instant::now();
                    stale = false;
                    let gesture = engine.update(packet);
                    if gesture.phase == Phase::Capture && !state.busy.swap(true, Ordering::Relaxed)
                    {
                        match TcpListener::bind("0.0.0.0:8090").and_then(|listener| {
                            listener.set_nonblocking(true)?;
                            Ok(listener)
                        }) {
                            Ok(listener) => {
                                capture = Some(Capture {
                                    id: next_id,
                                    peer,
                                    angle: gesture.angle,
                                    listener,
                                    expires: Instant::now() + Duration::from_secs(5),
                                    worker: false,
                                });
                                next_id += 1;
                            }
                            Err(e) => {
                                state.busy.store(false, Ordering::Relaxed);
                                eprintln!("Cannot arm TCP handoff: {e}");
                            }
                        }
                    }
                    if let Some(c) = &capture {
                        if c.peer == peer && !c.worker {
                            let mut ack = [0u8; 12];
                            ack[..4].copy_from_slice(b"FLK1");
                            ack[4..].copy_from_slice(&c.id.to_be_bytes());
                            let _ = udp.send_to(&ack, peer);
                        }
                    }
                    let phase = match gesture.phase {
                        Phase::Idle => "idle",
                        Phase::Approach => "approach",
                        Phase::Capture => "capture",
                    };
                    let _ = app.emit(
                        "flick://telemetry",
                        TelemetryPayload {
                            distance: gesture.distance,
                            velocity: gesture.velocity,
                            angle: gesture.angle,
                            phase,
                        },
                    );
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => {
                    eprintln!("UDP receive failed: {e}");
                    state.running.store(false, Ordering::Relaxed);
                    break;
                }
            }
        }
        if last.elapsed() > Duration::from_millis(150) && !stale {
            engine.reset();
            stale = true;
            let _ = app.emit(
                "flick://telemetry",
                TelemetryPayload {
                    distance: 0.65,
                    velocity: 0.0,
                    angle: std::f32::consts::PI,
                    phase: "idle",
                },
            );
        }
        if let Some(c) = capture.as_mut() {
            if Instant::now() > c.expires {
                let _ = app.emit(
                    "flick://status",
                    StatusPayload {
                        id: c.id,
                        ok: false,
                        message: "No queued Android asset arrived; capture expired".into(),
                    },
                );
                capture = None;
                state.busy.store(false, Ordering::Relaxed);
            } else if !c.worker {
                match c.listener.accept() {
                    Ok((stream, peer)) if peer.ip() == c.peer.ip() => {
                        c.worker = true;
                        c.expires = Instant::now() + Duration::from_secs(12);
                        let id = c.id;
                        let directory = directory.join("inbox");
                        let tx = tx.clone();
                        let state = state.clone();
                        std::thread::spawn(move || {
                            let target = native::foreground();
                            let result =
                                handoff::receive(stream, &token, id, &directory, &state.running)
                                    .map(|asset| (asset, target))
                                    .map_err(|e| e.to_string());
                            let _ = tx.send((id, result));
                        });
                    }
                    Ok(_) => {}
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                    Err(e) => eprintln!("TCP accept: {e}"),
                }
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn main() {
    let runtime = Arc::new(Runtime {
        running: AtomicBool::new(true),
        ready: AtomicBool::new(false),
        busy: AtomicBool::new(false),
        pending: Mutex::new(None),
    });
    let setup_state = runtime.clone();
    let application = tauri::Builder::default()
        .manage(runtime.clone())
        .invoke_handler(tauri::generate_handler![frontend_ready, complete_handoff])
        .setup(move |app| {
            let window = app
                .get_webview_window("main")
                .ok_or("Missing overlay window")?;
            window.set_ignore_cursor_events(true)?;
            window.set_focusable(false)?;
            window.set_always_on_top(true)?;
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            window.set_visible_on_all_workspaces(true)?;
            #[cfg(target_os = "windows")]
            native::CLIPBOARD_OWNER.store(window.hwnd()?.0 as isize, Ordering::Relaxed);
            #[cfg(target_os = "macos")]
            {
                app.set_activation_policy(tauri::ActivationPolicy::Accessory);
                // Native fullscreen creates a separate macOS Space. Use a borderless
                // monitor-sized overlay instead, keeping the configured fullscreen intent.
                window.set_fullscreen(false)?;
                if let Some(monitor) = window.current_monitor()? {
                    window.set_position(*monitor.position())?;
                    window.set_size(*monitor.size())?;
                }
            }
            let directory = app.path().app_data_dir()?;
            let token = pairing(&directory)?;
            let udp = UdpSocket::bind("0.0.0.0:8089")?;
            udp.set_nonblocking(true)?;
            let handle = app.handle().clone();
            let state = setup_state.clone();
            std::thread::spawn(move || serve(handle, state, directory, token, udp));
            window.show()?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("Cannot initialize click-through overlay");
    application.run(move |_, event| {
        if matches!(
            event,
            tauri::RunEvent::Exit | tauri::RunEvent::ExitRequested { .. }
        ) {
            runtime.running.store(false, Ordering::Relaxed);
        }
    });
}
