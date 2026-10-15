//! The network seam and its production implementation
//! (specsheet section 4, layer 7; IF-7, FR-13, FR-14).
//!
//! Everything that talks to the network is written against two
//! operations, so the whole updater family runs offline in tests and in
//! the differential harness while production dials the real thing.
//!
//! Port notes (specsheet section 4, phase 8): ureq 3.4.2 with its
//! default features gives rustls + ring + webpki-roots, so certificate
//! verification never reads the host trust store. `Proxy::try_from_env`
//! is python's `getproxies()` order - the environment first
//! (ALL_PROXY/HTTPS_PROXY/HTTP_PROXY, NO_PROXY honoured) and, on
//! windows when the environment is empty, the HKCU Internet Settings
//! fallback through the crate's `win-system-proxy` feature. A 4xx/5xx
//! answer is an error here too (ureq's default status-as-error against
//! urlopen's HTTPError). The timeout values are python's - 60 s forced,
//! 6 s non-forced, 1800 s for a binary download - and they are the
//! parity contract: ureq's `timeout_global` caps the whole request
//! while urlopen's timeout is per socket operation, a mechanism
//! difference recorded in the port notes, not a behaviour change.

use std::io::{Read, Write};
use std::time::Duration;

use crate::pyfs;
use ureq::ResponseExt;

/// python's `User-Agent` for every request.
pub const USER_AGENT: &str = "yt-dlp-cli";
/// `DL_TIMEOUT`: the binary-download ceiling, 30 minutes.
pub const DL_TIMEOUT: u64 = 1800;
/// `CHECK_TIMEOUT`: the non-forced auto-update check (audit F3, owner
/// pinned 2026-10-14 - a black-holed network must not stall the first
/// keystroke).
pub const CHECK_TIMEOUT: u64 = 6;
/// `http_text`'s own default: the forced text fetches.
pub const FORCED_TIMEOUT: u64 = 60;

/// The read block python's `http_file` streams with (`1 << 16`).
const CHUNK: usize = 1 << 16;

/// What python's `http_file` returns: `(got, final_url, total)`.
#[derive(Debug, PartialEq)]
pub struct FileOutcome {
    pub bytes: i64,
    pub final_url: String,
    pub content_length: i64,
}

/// The network as the rest of the port sees it: `http_text` and
/// `http_file`, python's two helpers, and nothing else. Errors are the
/// printed text python would have interpolated, so a caller can print
/// them the way python prints an exception.
pub trait HttpRt {
    /// `http_text(url, timeout)`: UTF-8 with replacement, like python's
    /// `decode("utf-8", "replace")`.
    fn text(&mut self, url: &str, timeout: u64) -> Result<String, String>;

    /// `http_file(url, dest, stage, timeout)`: stream through the
    /// `.part` sibling and rename when complete. `live` receives the
    /// python progress text while bytes arrive (python sets
    /// `stage.live`); it is None where python passed no stage.
    fn file(
        &mut self,
        url: &str,
        dest: &str,
        timeout: u64,
        live: Option<&mut dyn FnMut(&str)>,
    ) -> Result<FileOutcome, String>;
}

/// The production implementation: one pooled agent, python's request
/// shape.
pub struct UreqHttp {
    agent: ureq::Agent,
}

impl Default for UreqHttp {
    fn default() -> Self {
        Self::new()
    }
}

impl UreqHttp {
    pub fn new() -> Self {
        let config = ureq::Agent::config_builder()
            .user_agent(USER_AGENT)
            // python's getproxies(): the environment first, the windows
            // registry only when no proxy variable is set
            .proxy(ureq::Proxy::try_from_env())
            // urllib sends no Accept header and asks for identity
            // encoding; ureq would otherwise add its own two. Header
            // names are case-insensitive on the wire, but the set and
            // the values are pinned to python's here.
            .accept("")
            .accept_encoding("identity")
            .build();
        Self {
            agent: ureq::Agent::new_with_config(config),
        }
    }

    fn get(&self, url: &str, timeout: u64) -> Result<ureq::http::Response<ureq::Body>, String> {
        self.agent
            .get(url)
            .config()
            .timeout_global(Some(Duration::from_secs(timeout)))
            .build()
            .call()
            .map_err(error_text)
    }
}

impl HttpRt for UreqHttp {
    fn text(&mut self, url: &str, timeout: u64) -> Result<String, String> {
        let mut res = self.get(url, timeout)?;
        let bytes = res.body_mut().read_to_vec().map_err(error_text)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    fn file(
        &mut self,
        url: &str,
        dest: &str,
        timeout: u64,
        mut live: Option<&mut dyn FnMut(&str)>,
    ) -> Result<FileOutcome, String> {
        let mut res = self.get(url, timeout)?;
        let final_url = res.get_uri().to_string();
        // python reads the Content-Length header and keeps 0 unless it
        // is all digits, which is what a chunked answer gives
        let total = res.body().content_length().map(|n| n as i64).unwrap_or(0);
        let part = pyfs::with_suffix(dest, ".part");
        let mut fh = std::fs::File::create(&part).map_err(|e| e.to_string())?;
        let mut reader = res.body_mut().as_reader();
        let mut buf = vec![0u8; CHUNK];
        let mut got: i64 = 0;
        loop {
            let read = reader.read(&mut buf).map_err(|e| e.to_string())?;
            if read == 0 {
                break;
            }
            fh.write_all(&buf[..read]).map_err(|e| e.to_string())?;
            got += read as i64;
            if let Some(report) = live.as_deref_mut()
                && total > 0
            {
                report(&format!(
                    "downloading ffmpeg - {:.1} / {:.1} MB",
                    got as f64 / 1048576.0,
                    total as f64 / 1048576.0
                ));
            }
        }
        drop(fh);
        pyfs::replace(&part, dest).map_err(|e| e.to_string())?;
        Ok(FileOutcome {
            bytes: got,
            final_url,
            content_length: total,
        })
    }
}

/// ureq's failure text. python's urllib words differ (`HTTP Error
/// 404: Not Found`, `<urlopen error [Errno 11001] ...>`), so every line
/// that interpolates this text is shape-gated in the differential
/// harness, per the plan's rule for exception-text lines.
fn error_text(e: ureq::Error) -> String {
    match e {
        ureq::Error::StatusCode(code) => format!("http status {code}"),
        ureq::Error::HostNotFound => "host not found".to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::sync::Mutex;

    /// The environment proxy variables and `NO_PROXY`, the inputs
    /// `Proxy::try_from_env` reads. The tests below change them, so they
    /// take turns.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    const PROXY_VARS: [&str; 8] = [
        "ALL_PROXY",
        "all_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "HTTP_PROXY",
        "http_proxy",
        "NO_PROXY",
        "no_proxy",
    ];

    /// Set one variable while the env lock is held (edition 2024: the
    /// process environment is unsafe to touch while other threads may
    /// read it, so every write here is inside one locked region).
    fn set_var(key: &str, value: &str) {
        unsafe { std::env::set_var(key, value) };
    }

    /// Run `body` with no inherited proxy and `NO_PROXY` covering
    /// loopback, so a test reaches its own listener whatever this
    /// machine's environment says, then put the environment back.
    fn env_test<T>(body: impl FnOnce() -> T) -> T {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let saved: Vec<Option<String>> = PROXY_VARS.iter().map(|k| std::env::var(k).ok()).collect();
        for key in PROXY_VARS {
            unsafe { std::env::remove_var(key) };
        }
        set_var("NO_PROXY", "127.0.0.1,localhost");
        let out = body();
        for (key, value) in PROXY_VARS.iter().zip(saved) {
            unsafe {
                match value {
                    Some(v) => std::env::set_var(key, v),
                    None => std::env::remove_var(key),
                }
            }
        }
        out
    }

    /// Read one request head (up to the blank line) off a socket.
    fn read_head(sock: &mut std::net::TcpStream) -> String {
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            match sock.read(&mut byte) {
                Ok(0) | Err(_) => break,
                Ok(_) => head.push(byte[0]),
            }
        }
        String::from_utf8_lossy(&head).into_owned()
    }

    /// One loopback connection: answer with a canned response, hand
    /// back the request head so the test can assert what was dialed.
    fn one_shot(response: Vec<u8>) -> (u16, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            let head = read_head(&mut sock);
            sock.write_all(&response).unwrap();
            sock.flush().unwrap();
            head
        });
        (port, handle)
    }

    fn ok_response(body: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    /// The same answer with a raw body, so a test can put bytes on the
    /// wire that are not valid UTF-8.
    fn ok_response_bytes(body: &[u8]) -> Vec<u8> {
        let mut out = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        out.extend_from_slice(body);
        out
    }

    fn scratch(name: &str) -> String {
        let dir = std::env::temp_dir()
            .join(format!("ytdlp-net-test-{}-{name}", std::process::id()))
            .to_string_lossy()
            .into_owned();
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn text_reads_a_loopback_body_with_the_python_user_agent() {
        env_test(|| {
            let (port, server) = one_shot(ok_response("hello").into_bytes());
            let mut http = UreqHttp::new();
            let body = http
                .text(&format!("http://127.0.0.1:{port}/v"), FORCED_TIMEOUT)
                .unwrap();
            let head = server.join().unwrap();
            assert_eq!(body, "hello");
            // header names are case-insensitive on the wire; ureq spells
            // them lowercase, urllib capitalises - the value is the pin
            assert!(
                head.to_lowercase()
                    .contains(&format!("user-agent: {USER_AGENT}\r\n")),
                "{head}"
            );
            // python's request shape: no Accept header, identity
            // encoding
            assert!(!head.to_lowercase().contains("\naccept:"), "{head}");
            assert!(
                head.to_lowercase()
                    .contains("\naccept-encoding: identity\r\n"),
                "{head}"
            );
        });
    }

    #[test]
    fn text_replaces_an_undecodable_byte_like_python() {
        env_test(|| {
            // 0x80 is a lone continuation byte and 0xe9 a lone lead byte:
            // both invalid, both replaced, the ascii between them kept
            let (port, server) = one_shot(ok_response_bytes(&[0x80, b'x', 0xe9, b'y']));
            let mut http = UreqHttp::new();
            let body = http
                .text(&format!("http://127.0.0.1:{port}/v"), FORCED_TIMEOUT)
                .unwrap();
            server.join().unwrap();
            // python's errors="replace" semantics
            assert_eq!(body, "\u{fffd}x\u{fffd}y");
        });
    }

    #[test]
    fn a_status_error_is_an_error() {
        env_test(|| {
            let (port, server) = one_shot(
                "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    .to_string()
                    .into_bytes(),
            );
            let mut http = UreqHttp::new();
            let err = http
                .text(&format!("http://127.0.0.1:{port}/v"), FORCED_TIMEOUT)
                .unwrap_err();
            server.join().unwrap();
            assert_eq!(err, "http status 404");
        });
    }

    /// G8 proxy evidence, offline: with HTTP_PROXY pointing at a
    /// loopback listener, the request must reach that listener naming
    /// the target host - which is what "the proxy was dialed" looks
    /// like on the wire. ureq opens a CONNECT tunnel even for plain
    /// http (urllib sends an absolute-form GET); the dial is the
    /// parity, the framing is ureq's and recorded as such.
    #[test]
    fn the_environment_proxy_is_dialed() {
        env_test(|| {
            let (port, server) = one_shot(ok_response("ok").into_bytes());
            set_var("HTTP_PROXY", &format!("http://127.0.0.1:{port}"));
            {
                let dir = scratch("proxy");
                let dest = format!("{dir}/asset.zip");
                let mut http = UreqHttp::new();
                let result = http.file("http://example.invalid/asset.zip", &dest, DL_TIMEOUT, None);
                let head = server.join().unwrap();
                assert!(
                    head.starts_with("CONNECT example.invalid:80 HTTP/1.1\r\n"),
                    "the proxy listener must receive the dial: {head}"
                );
                // a CONNECT is not a 200-answerable request, so ureq
                // reports the tunnel refusal the fake proxy sent as a
                // protocol error - which is itself the proof that no
                // direct dial happened
                let err = result.unwrap_err();
                assert!(err.contains("invalid HTTP version"), "{err}");
                // nothing was staged: the .part name stays empty-free
                assert!(!std::path::Path::new(&dest).exists());
                assert!(!std::path::Path::new(&format!("{dest}.part")).exists());
                std::fs::remove_dir_all(&dir).unwrap();
            }
        });
    }

    /// And the same dial with a proxy that answers the CONNECT and then
    /// serves the tunnelled request, so the whole transfer through the
    /// proxy is proven, not just the dial.
    #[test]
    fn a_tunnelled_download_lands_on_disk() {
        env_test(|| {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let server = std::thread::spawn(move || {
                let (mut sock, _) = listener.accept().unwrap();
                let connect = read_head(&mut sock);
                sock.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                    .unwrap();
                sock.flush().unwrap();
                let tunnelled = read_head(&mut sock);
                sock.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello",
                )
                .unwrap();
                sock.flush().unwrap();
                (connect, tunnelled)
            });
            set_var("HTTP_PROXY", &format!("http://127.0.0.1:{port}"));
            {
                let dir = scratch("proxy-tunnel");
                let dest = format!("{dir}/asset.zip");
                let mut http = UreqHttp::new();
                let out = http
                    .file("http://example.invalid/asset.zip", &dest, DL_TIMEOUT, None)
                    .unwrap();
                let (connect, tunnelled) = server.join().unwrap();
                assert!(
                    connect.starts_with("CONNECT example.invalid:80"),
                    "{connect}"
                );
                // inside the tunnel the request is origin-form, as any
                // proxy-transported request would be
                assert!(
                    tunnelled.starts_with("GET /asset.zip HTTP/1.1\r\n"),
                    "{tunnelled}"
                );
                assert_eq!(out.bytes, 5);
                assert_eq!(std::fs::read(&dest).unwrap(), b"hello");
                assert!(!std::path::Path::new(&format!("{dest}.part")).exists());
                std::fs::remove_dir_all(&dir).unwrap();
            }
        });
    }

    #[test]
    fn file_reports_progress_and_the_final_url() {
        env_test(|| {
            // 3.5 MiB: exact halves in the python "%.1f MB" text
            let payload = "x".repeat(3_670_016);
            let (port, server) = one_shot(ok_response(&payload).into_bytes());
            let dir = scratch("file");
            let dest = format!("{dir}/blob");
            let mut http = UreqHttp::new();
            let mut seen: Vec<String> = Vec::new();
            let out = {
                let mut report = |text: &str| seen.push(text.to_string());
                http.file(
                    &format!("http://127.0.0.1:{port}/blob"),
                    &dest,
                    DL_TIMEOUT,
                    Some(&mut report),
                )
                .unwrap()
            };
            server.join().unwrap();
            assert_eq!(out.bytes, 3_670_016);
            assert_eq!(out.content_length, 3_670_016);
            assert_eq!(out.final_url, format!("http://127.0.0.1:{port}/blob"));
            // one live text per read of the stream: the read sizes are
            // the transport's business, so the count is not pinned - the
            // text shape and the closing value are
            assert!(
                seen.len() >= 56,
                "at least one text per 64 KiB: {}",
                seen.len()
            );
            assert!(
                seen.iter()
                    .all(|t| t.starts_with("downloading ffmpeg - ") && t.ends_with(" / 3.5 MB")),
                "{seen:?}"
            );
            assert_eq!(
                seen.last().map(String::as_str),
                Some("downloading ffmpeg - 3.5 / 3.5 MB")
            );
            assert_eq!(std::fs::read(&dest).unwrap().len(), 3_670_016);
            std::fs::remove_dir_all(&dir).unwrap();
        });
    }
}
