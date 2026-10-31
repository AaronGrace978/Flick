//! Bounded paired LAN transfer. This protocol is not encrypted: use a trusted LAN.
#[cfg(unix)]
use std::fs::File;
use std::{
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
pub const MAGIC: &[u8; 8] = b"FLICK001";
pub const MAX_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_TEXT: u64 = 256 * 1024;
#[derive(Debug)]
pub enum Asset {
    Text(String),
    File(PathBuf),
}
impl Asset {
    pub fn label(&self) -> String {
        match self {
            Self::Text(_) => "Clipboard text".into(),
            Self::File(p) => {
                let name = p.file_name().unwrap_or_default().to_string_lossy();
                name.splitn(3, '-').nth(2).unwrap_or(&name).to_owned()
            }
        }
    }
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
/// Decode only after constant-work token comparison and expected gesture binding.
/// No sender-controlled path is ever joined into the output directory.
pub fn decode<R: Read>(
    reader: &mut R,
    token: &[u8; 32],
    id: u64,
    directory: &Path,
) -> io::Result<Asset> {
    let mut header = [0u8; 59];
    reader.read_exact(&mut header)?;
    if &header[..8] != MAGIC {
        return Err(invalid("Wrong handoff version"));
    }
    let difference = header[8..40]
        .iter()
        .zip(token)
        .fold(0u8, |diff, (a, b)| diff | (a ^ b));
    if difference != 0 || u64::from_be_bytes(header[40..48].try_into().unwrap()) != id {
        return Err(invalid("Pairing or gesture mismatch"));
    }
    let kind = header[48];
    let name_length = u16::from_be_bytes(header[49..51].try_into().unwrap()) as usize;
    let length = u64::from_be_bytes(header[51..59].try_into().unwrap());
    if length > MAX_BYTES
        || name_length > 100
        || kind > 1
        || (kind == 0 && (length > MAX_TEXT || name_length != 0))
        || (kind == 1 && name_length == 0)
    {
        return Err(invalid("Invalid asset bounds"));
    }
    let mut name = vec![0u8; name_length];
    reader.read_exact(&mut name)?;
    if kind == 0 {
        let mut bytes = vec![0; length as usize];
        reader.read_exact(&mut bytes)?;
        let text = String::from_utf8(bytes).map_err(|_| invalid("Clipboard must be UTF-8"))?;
        if text.contains('\0') {
            return Err(invalid("Clipboard contains NUL"));
        }
        return Ok(Asset::Text(text));
    }
    if name
        .iter()
        .any(|b| !b.is_ascii_alphanumeric() && !b"._-".contains(b))
        || name.last() == Some(&b'.')
    {
        return Err(invalid("Unsafe filename"));
    }
    let name = std::str::from_utf8(&name).map_err(|_| invalid("Invalid filename"))?;
    fs::create_dir_all(directory)?;
    let path = directory.join(format!("capture-{id}-{name}"));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&path)?;
    let result = (|| {
        let copied = io::copy(&mut reader.take(length), &mut file)?;
        if copied != length {
            return Err(invalid("Truncated file"));
        }
        file.flush()?;
        Ok(())
    })();
    if let Err(error) = result {
        drop(file);
        let _ = fs::remove_file(&path);
        return Err(error);
    }
    Ok(Asset::File(path))
}
struct DeadlineReader<'a> {
    stream: TcpStream,
    deadline: Instant,
    running: &'a AtomicBool,
}
impl Read for DeadlineReader<'_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        loop {
            if !self.running.load(Ordering::Relaxed) || Instant::now() >= self.deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Handoff cancelled or timed out",
                ));
            }
            match self.stream.read(out) {
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(2))
                }
                result => return result,
            }
        }
    }
}
/// All TCP I/O is non-blocking on its own worker and has a whole-transfer deadline.
pub fn receive(
    stream: TcpStream,
    token: &[u8; 32],
    id: u64,
    directory: &Path,
    running: &AtomicBool,
) -> io::Result<Asset> {
    stream.set_nonblocking(true)?;
    stream.set_nodelay(true)?;
    let mut reader = DeadlineReader {
        stream,
        deadline: Instant::now() + Duration::from_secs(10),
        running,
    };
    let asset = decode(&mut reader, token, id, directory)?;
    // Preserve text as well as files before acknowledging receipt. A clipboard
    // permission error must never discard the user's only remaining copy.
    if let Asset::Text(text) = &asset {
        fs::create_dir_all(directory)?;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(directory.join(format!("capture-{id}-clipboard.txt")))?;
        file.write_all(text.as_bytes())?;
        file.flush()?;
    }
    let mut written = 0;
    while written < 2 {
        if !running.load(Ordering::Relaxed) || Instant::now() >= reader.deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Receipt acknowledgement timed out",
            ));
        }
        match reader.stream.write(&b"OK"[written..]) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "Cannot acknowledge asset",
                ))
            }
            Ok(n) => written += n,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(2))
            }
            Err(e) => return Err(e),
        }
    }
    Ok(asset)
}
/// Uses the OS cryptographic generator; pairing material is never logged.
pub fn random_token() -> io::Result<[u8; 32]> {
    let mut bytes = [0; 32];
    #[cfg(unix)]
    File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    #[cfg(windows)]
    {
        #[link(name = "bcrypt")]
        unsafe extern "system" {
            fn BCryptGenRandom(
                handle: *mut std::ffi::c_void,
                buffer: *mut u8,
                length: u32,
                flags: u32,
            ) -> i32;
        }
        if unsafe { BCryptGenRandom(std::ptr::null_mut(), bytes.as_mut_ptr(), 32, 2) } != 0 {
            return Err(io::Error::other("OS random generator failed"));
        }
    }
    Ok(bytes)
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    fn frame(kind: u8, name: &str, bytes: &[u8]) -> Vec<u8> {
        let mut b = Vec::from(*MAGIC);
        b.extend([7; 32]);
        b.extend(42u64.to_be_bytes());
        b.push(kind);
        b.extend((name.len() as u16).to_be_bytes());
        b.extend((bytes.len() as u64).to_be_bytes());
        b.extend(name.as_bytes());
        b.extend(bytes);
        b
    }
    #[test]
    fn authentic_text_and_invalid_frames() {
        let dir = Path::new("unused");
        assert!(
            matches!(decode(&mut Cursor::new(frame(0,"",b"hello")),&[7;32],42,dir).unwrap(),Asset::Text(t) if t=="hello")
        );
        for f in [
            frame(0, "", b"a\0b"),
            frame(0, "", &[255]),
            frame(1, "../evil", b"x"),
            frame(2, "", b"x"),
        ] {
            assert!(decode(&mut Cursor::new(f), &[7; 32], 42, dir).is_err());
        }
        assert!(decode(&mut Cursor::new(frame(0, "", b"a")), &[8; 32], 42, dir).is_err());
        assert!(decode(&mut Cursor::new(frame(0, "", b"a")), &[7; 32], 41, dir).is_err());
        let mut oversized = frame(0, "", b"x");
        oversized[51..59].copy_from_slice(&(MAX_BYTES + 1).to_be_bytes());
        assert!(decode(&mut Cursor::new(oversized), &[7; 32], 42, dir).is_err());
    }
    #[test]
    fn files_are_bounded_and_partial_files_are_removed() {
        let dir = std::env::temp_dir().join(format!("flick-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mut truncated = frame(1, "test.txt", b"abc");
        truncated.pop();
        assert!(decode(&mut Cursor::new(truncated), &[7; 32], 42, &dir).is_err());
        assert!(!dir.join("capture-42-test.txt").exists());
        let asset = decode(
            &mut Cursor::new(frame(1, "test.txt", b"abc")),
            &[7; 32],
            42,
            &dir,
        )
        .unwrap();
        let Asset::File(path) = asset else {
            panic!("Expected file")
        };
        assert_eq!(fs::read(path).unwrap(), b"abc");
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn real_tcp_acknowledges_and_cancellation_is_bounded() {
        use std::{net::TcpListener, sync::Arc};
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let running = Arc::new(AtomicBool::new(true));
        let flag = running.clone();
        let directory = std::env::temp_dir().join(format!("flick-tcp-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        let server_dir = directory.clone();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            receive(stream, &[7; 32], 42, &server_dir, &flag)
        });
        let mut client = TcpStream::connect(addr).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        client.write_all(&frame(0, "", b"paired text")).unwrap();
        let mut ack = [0; 2];
        client.read_exact(&mut ack).unwrap();
        assert_eq!(&ack, b"OK");
        assert!(matches!(server.join().unwrap().unwrap(),Asset::Text(t) if t=="paired text"));
        assert_eq!(
            fs::read_to_string(directory.join("capture-42-clipboard.txt")).unwrap(),
            "paired text"
        );
        fs::remove_dir_all(directory).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let _client = TcpStream::connect(addr).unwrap();
        let (stream, _) = listener.accept().unwrap();
        running.store(false, Ordering::Relaxed);
        assert_eq!(
            receive(stream, &[7; 32], 42, Path::new("unused"), &running)
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
    }
}
