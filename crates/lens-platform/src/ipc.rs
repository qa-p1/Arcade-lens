//! Single-instance control channel.
//!
//! The running Lens process listens on a loopback TCP port; the port and a
//! random token are written to a file only the user can read. `arcade-lens
//! capture` (e.g. bound to a compositor shortcut on Wayland) uses it to
//! trigger the running instance. Plain std, identical on every platform.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::time::Duration;

fn token() -> std::io::Result<String> {
    // 32 bytes from the OS random generator.
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| std::io::Error::other(e.to_string()))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Starts listening and calls `handler` for each command on a background
/// thread. Returns an error if another instance is already serving.
pub fn serve(endpoint_file: &Path, handler: impl Fn(&str) -> String + Send + 'static) -> std::io::Result<()> {
    if send(endpoint_file, "ping").is_ok() {
        return Err(std::io::Error::new(std::io::ErrorKind::AddrInUse, "Arcade Lens is already running"));
    }
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    let tok = token()?;
    if let Some(dir) = endpoint_file.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    opts.open(endpoint_file)?.write_all(format!("{port} {tok}\n").as_bytes())?;
    std::thread::Builder::new().name("lens-ipc".into()).spawn(move || {
        for stream in listener.incoming().flatten() {
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let mut reader = BufReader::new(&stream);
            let mut line = String::new();
            if reader.read_line(&mut line).is_err() {
                continue;
            }
            let reply = match line.trim().split_once(' ') {
                Some((t, cmd)) if t == tok => handler(cmd),
                _ => "denied".into(),
            };
            let _ = (&stream).write_all(format!("{reply}\n").as_bytes());
        }
    })?;
    Ok(())
}

/// Sends a command to the running instance and returns its reply.
pub fn send(endpoint_file: &Path, command: &str) -> std::io::Result<String> {
    let content = fs::read_to_string(endpoint_file)?;
    let (port, tok) = content.trim().split_once(' ').ok_or_else(|| std::io::Error::other("bad endpoint file"))?;
    let addr = format!("127.0.0.1:{port}").parse().map_err(std::io::Error::other)?;
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_millis(300))?;
    s.set_read_timeout(Some(Duration::from_secs(5)))?;
    s.write_all(format!("{tok} {command}\n").as_bytes())?;
    let mut reply = String::new();
    BufReader::new(s).read_line(&mut reply)?;
    Ok(reply.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_token_check() {
        let file = std::env::temp_dir().join(format!("lens-ipc-test-{}", std::process::id()));
        serve(&file, |cmd| format!("got {cmd}")).unwrap();
        assert_eq!(send(&file, "capture").unwrap(), "got capture");
        // A second instance refuses to start.
        assert!(serve(&file, |_| String::new()).is_err());
        // Wrong token is denied.
        let content = fs::read_to_string(&file).unwrap();
        let port = content.split(' ').next().unwrap();
        let mut s = TcpStream::connect(format!("127.0.0.1:{port}")).unwrap();
        s.write_all(b"nope capture\n").unwrap();
        let mut r = String::new();
        BufReader::new(s).read_line(&mut r).unwrap();
        assert_eq!(r.trim(), "denied");
        fs::remove_file(file).ok();
    }
}
