#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
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
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
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
    armed: bool,
}
#[derive(Clone, Serialize)]
struct AssetPayload {
    id: u64,
    label: String,
    angle: f32,
    preview: Option<String>,
    manual: bool,
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
    manual: bool,
}
struct Runtime {
    running: AtomicBool,
    ready: AtomicBool,
    busy: AtomicBool,
    pending: Mutex<Option<Pending>>,
    pairing_code: Mutex<String>,
    beacon: Mutex<Option<Child>>,
    paired_at: AtomicU64,
    telemetry_at: AtomicU64,
    started: Instant,
}

/// Only the trusted local webview can request this, after its asset animation ends.
/// The backend selects the received asset, not arbitrary frontend-supplied content.
#[tauri::command]
async fn complete_handoff(
    id: u64,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Arc<Runtime>>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    if window.label() != "main" {
        return Err("Only the overlay can finish a handoff".into());
    }
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
        let result = if pending.manual {Ok(())} else {pending.target.and_then(|target|native::paste(&pending.asset,target))};
        let message = match &result {
            Ok(_) if pending.manual => "Image/file received and saved in inbox; manual transfer does not paste into another app".into(),
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
fn frontend_ready(window: tauri::WebviewWindow, state: tauri::State<'_, Arc<Runtime>>) {
    if window.label() == "main" {
        state.ready.store(true, Ordering::Relaxed);
    }
}

#[derive(Serialize)]
struct DesktopStatus {
    code: String,
    address: Option<String>,
    beacon: bool,
    paired_age: Option<u64>,
    telemetry_age: Option<u64>,
    ready: bool,
}
#[tauri::command]
fn desktop_status(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Arc<Runtime>>,
) -> Result<DesktopStatus, String> {
    if window.label() != "control" {
        return Err("Control window required".into());
    }
    let address = UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| {
            s.connect("192.0.2.1:9")?;
            s.local_addr()
        })
        .ok()
        .map(|a| a.ip().to_string());
    let now = state.started.elapsed().as_millis() as u64 + 1;
    let age = |time: u64| {
        if time == 0 {
            None
        } else {
            Some(now.saturating_sub(time))
        }
    };
    let mut beacon = state
        .beacon
        .lock()
        .map_err(|_| "Beacon state unavailable")?;
    let live = beacon
        .as_mut()
        .is_some_and(|child| matches!(child.try_wait(), Ok(None)));
    if !live {
        *beacon = None;
    }
    Ok(DesktopStatus {
        code: state
            .pairing_code
            .lock()
            .map_err(|_| "Pairing state unavailable")?
            .clone(),
        address,
        beacon: live,
        paired_age: age(state.paired_at.load(Ordering::Relaxed)),
        telemetry_age: age(state.telemetry_at.load(Ordering::Relaxed)),
        ready: state.ready.load(Ordering::Relaxed),
    })
}
fn stop_child(slot: &mut Option<Child>) {
    if let Some(mut child) = slot.take() {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(b"\n");
        }
        let until = Instant::now() + Duration::from_secs(1);
        while Instant::now() < until {
            if !matches!(child.try_wait(), Ok(None)) {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = child.kill();
        let _ = child.wait();
    }
}
fn stop_beacon(state: &Runtime) {
    if let Ok(mut slot) = state.beacon.lock() {
        stop_child(&mut slot);
    }
}

#[tauri::command]
async fn set_beacon(
    enabled: bool,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Arc<Runtime>>,
) -> Result<(), String> {
    if window.label() != "control" {
        return Err("Control window required".into());
    }
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move||{
        let mut slot=state.beacon.lock().map_err(|_|"Beacon state unavailable")?;
        stop_child(&mut slot);
        if enabled {
            if !state.running.load(Ordering::Relaxed){return Err("Application is closing".into());}
            let path=std::env::current_exe().map_err(|e|e.to_string())?.parent().ok_or("No application directory")?.join(format!("emitter{}",std::env::consts::EXE_SUFFIX));
            let mut command=Command::new(path);
            #[cfg(windows)] { use std::os::windows::process::CommandExt;command.creation_flags(0x08000000); }
            let child=command.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()
                .map_err(|e|format!("Cannot start beacon: {e}. Keep emitter next to flick-overlay from the same desktop archive."))?;
            *slot=Some(child);
        }
        Ok(())
    }).await.map_err(|e|e.to_string())?
}

#[tauri::command]
fn open_inbox(window: tauri::WebviewWindow, app: tauri::AppHandle) -> Result<(), String> {
    if window.label() != "control" {
        return Err("Control window required".into());
    }
    let path = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("inbox");
    fs::create_dir_all(&path).map_err(|e| e.to_string())?;
    #[cfg(windows)]
    let program = "explorer.exe";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(target_os = "linux")]
    let program = "xdg-open";
    let mut child = Command::new(program)
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
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
    manual: bool,
    nonce: u64,
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
            let manual = capture.as_ref().is_some_and(|c| c.manual);
            capture = None;
            match result {
                Ok((asset, target)) => {
                    let label = asset.label();
                    let preview = match &asset {
                        Asset::File(path)
                            if path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                                matches!(
                                    e.to_ascii_lowercase().as_str(),
                                    "png" | "jpg" | "jpeg" | "webp" | "gif" | "avif"
                                )
                            }) =>
                        {
                            Some(path.to_string_lossy().into_owned())
                        }
                        _ => None,
                    };
                    if let Ok(mut pending) = state.pending.lock() {
                        *pending = Some(Pending {
                            id,
                            asset,
                            target,
                            ready: Instant::now(),
                            manual,
                        });
                    }
                    if let Err(e) = app.emit(
                        "flick://asset",
                        AssetPayload {
                            id,
                            label,
                            angle,
                            preview,
                            manual,
                        },
                    ) {
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
                    if let Some((nonce, authenticated)) =
                        handoff::manual_request(&bytes[..n], &token)
                    {
                        let mut status = *b"FLKPBUSY";
                        if !authenticated {
                            status = *b"FLKPBAD1";
                        } else if !state.ready.load(Ordering::Relaxed) {
                            status = *b"FLKPWAIT";
                        } else {
                            state.paired_at.store(
                                state.started.elapsed().as_millis() as u64 + 1,
                                Ordering::Relaxed,
                            );
                            if capture.is_none() && !state.busy.swap(true, Ordering::Relaxed) {
                                match TcpListener::bind("0.0.0.0:8090").and_then(|listener| {
                                    listener.set_nonblocking(true)?;
                                    Ok(listener)
                                }) {
                                    Ok(listener) => {
                                        capture = Some(Capture {
                                            id: next_id,
                                            peer,
                                            angle: std::f32::consts::PI,
                                            listener,
                                            expires: Instant::now() + Duration::from_secs(5),
                                            worker: false,
                                            manual: true,
                                            nonce,
                                        });
                                        next_id += 1;
                                        let _=app.emit("flick://status",StatusPayload{id:next_id-1,ok:true,message:"Manual send accepted; waiting for the file on TCP 8090".into()});
                                    }
                                    Err(error) => {
                                        state.busy.store(false, Ordering::Relaxed);
                                        status = *b"FLKPFAIL";
                                        let _ = app.emit(
                                            "flick://status",
                                            StatusPayload {
                                                id: 0,
                                                ok: false,
                                                message: format!("Cannot open TCP 8090: {error}"),
                                            },
                                        );
                                    }
                                }
                            }
                            if let Some(c) = &capture {
                                if c.manual && c.peer == peer && c.nonce == nonce && !c.worker {
                                    let mut reply = [0u8; 24];
                                    reply[..8].copy_from_slice(b"FLKSEND2");
                                    reply[8..16].copy_from_slice(&nonce.to_be_bytes());
                                    reply[16..].copy_from_slice(&c.id.to_be_bytes());
                                    let _ = udp.send_to(&reply, peer);
                                    continue;
                                }
                            }
                        }
                        let mut reply = [0u8; 16];
                        reply[..8].copy_from_slice(&status);
                        reply[8..].copy_from_slice(&nonce.to_be_bytes());
                        let _ = udp.send_to(&reply, peer);
                        continue;
                    }
                    if let Some(reply) = handoff::pairing_reply(
                        &bytes[..n],
                        &token,
                        state.ready.load(Ordering::Relaxed),
                    ) {
                        if &reply[..8] == b"FLKPOK01" {
                            state.paired_at.store(
                                state.started.elapsed().as_millis() as u64 + 1,
                                Ordering::Relaxed,
                            );
                        }
                        let _ = udp.send_to(&reply, peer);
                        continue;
                    }
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
                    state.telemetry_at.store(
                        state.started.elapsed().as_millis() as u64 + 1,
                        Ordering::Relaxed,
                    );
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
                                    manual: false,
                                    nonce: 0,
                                });
                                next_id += 1;
                                let _=app.emit("flick://status",StatusPayload{id:next_id-1,ok:true,message:"Flick captured; waiting for the queued file on TCP 8090".into()});
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
                            armed: gesture.armed,
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
                    armed: false,
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
                        let manual = c.manual;
                        std::thread::spawn(move || {
                            let target = if manual {
                                Err("Manual transfer".into())
                            } else {
                                native::foreground()
                            };
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
        pairing_code: Mutex::new(String::new()),
        beacon: Mutex::new(None),
        paired_at: AtomicU64::new(0),
        telemetry_at: AtomicU64::new(0),
        started: Instant::now(),
    });
    let setup_state = runtime.clone();
    let application = tauri::Builder::default()
        .manage(runtime.clone())
        .invoke_handler(tauri::generate_handler![
            frontend_ready,
            complete_handoff,
            desktop_status,
            set_beacon, open_inbox
        ])
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
            *setup_state
                .pairing_code
                .lock()
                .map_err(|_| "Pairing state unavailable")? =
                token.iter().map(|b| format!("{b:02x}")).collect();
            let udp = UdpSocket::bind("0.0.0.0:8089")?;
            udp.set_nonblocking(true)?;
            let handle = app.handle().clone();
            let state = setup_state.clone();
            std::thread::spawn(move || serve(handle, state, directory, token, udp));
            window.show()?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .unwrap_or_else(|error|{
            native::startup_error(&format!("Flick could not start: {error}\n\nClose any older flick-overlay instance before opening this version. On Windows, ensure WebView2 is installed."));
            std::process::exit(1)
        });
    application.run(move |handle, event| {
        if let tauri::RunEvent::WindowEvent {
            label,
            event: tauri::WindowEvent::CloseRequested { .. },
            ..
        } = &event
        {
            if label == "control" {
                handle.exit(0);
            }
        }
        if matches!(
            event,
            tauri::RunEvent::Exit | tauri::RunEvent::ExitRequested { .. }
        ) {
            runtime.running.store(false, Ordering::Relaxed);
            stop_beacon(&runtime);
        }
    });
}
