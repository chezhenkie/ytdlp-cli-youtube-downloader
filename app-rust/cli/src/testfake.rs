//! Test doubles for the network and console seams (phase 8).
//!
//! Only the crate's own tests compile this: every answer here is a
//! script, never a dial. `FakeHttp` records the calls with the timeout
//! each one was given, so a test can pin the 60 / 6 / 1800 values and
//! can assert that no contact happened at all.

use crate::net::{FileOutcome, HttpRt};
use crate::pyfs;
use crate::tty::Out;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// A scripted network: per url text and file answers, each either a
/// body or a failure text.
#[derive(Default)]
pub struct FakeHttp {
    texts: HashMap<String, Result<String, String>>,
    files: HashMap<String, Result<Vec<u8>, String>>,
    /// every call, in order, spelled `text <url> <timeout>` /
    /// `file <url> <dest> <timeout>`
    pub calls: Arc<Mutex<Vec<String>>>,
    /// the live texts a file transfer reported
    pub live: Arc<Mutex<Vec<String>>>,
}

impl FakeHttp {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_text(mut self, url: &str, body: &str) -> Self {
        self.texts.insert(url.to_string(), Ok(body.to_string()));
        self
    }

    pub fn with_text_err(mut self, url: &str, err: &str) -> Self {
        self.texts.insert(url.to_string(), Err(err.to_string()));
        self
    }

    pub fn with_file(mut self, url: &str, body: &[u8]) -> Self {
        self.files.insert(url.to_string(), Ok(body.to_vec()));
        self
    }

    pub fn with_file_err(mut self, url: &str, err: &str) -> Self {
        self.files.insert(url.to_string(), Err(err.to_string()));
        self
    }

    /// Replace one answer after the harness was built, so a test can
    /// start from a working script and break exactly one thing.
    pub fn set_text(&mut self, url: &str, body: &str) -> &mut Self {
        self.texts.insert(url.to_string(), Ok(body.to_string()));
        self
    }

    pub fn set_text_err(&mut self, url: &str, err: &str) -> &mut Self {
        self.texts.insert(url.to_string(), Err(err.to_string()));
        self
    }

    pub fn set_file(&mut self, url: &str, body: &[u8]) -> &mut Self {
        self.files.insert(url.to_string(), Ok(body.to_vec()));
        self
    }

    pub fn set_file_err(&mut self, url: &str, err: &str) -> &mut Self {
        self.files.insert(url.to_string(), Err(err.to_string()));
        self
    }

    /// The same script with the call log emptied, so a test can measure
    /// only what the call under test did.
    pub fn with_fresh_log(&self) -> Self {
        Self {
            texts: self.texts.clone(),
            files: self.files.clone(),
            calls: Arc::new(Mutex::new(Vec::new())),
            live: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn live(&self) -> Vec<String> {
        self.live.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn record(&self, call: String) {
        self.calls
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(call);
    }
}

impl HttpRt for FakeHttp {
    fn text(&mut self, url: &str, timeout: u64) -> Result<String, String> {
        self.record(format!("text {url} {timeout}"));
        match self.texts.get(url) {
            Some(Ok(body)) => Ok(body.clone()),
            Some(Err(e)) => Err(e.clone()),
            None => Err(format!("no fake text for {url}")),
        }
    }

    fn file(
        &mut self,
        url: &str,
        dest: &str,
        timeout: u64,
        live: Option<&mut dyn FnMut(&str)>,
    ) -> Result<FileOutcome, String> {
        self.record(format!("file {url} {dest} {timeout}"));
        let body = match self.files.get(url) {
            Some(Ok(body)) => body.clone(),
            Some(Err(e)) => return Err(e.clone()),
            None => return Err(format!("no fake file for {url}")),
        };
        // python's staging: write the .part sibling, then rename
        let part = pyfs::with_suffix(dest, ".part");
        std::fs::write(&part, &body).unwrap();
        if let Some(report) = live {
            let text = format!(
                "downloading ffmpeg - {:.1} / {:.1} MB",
                body.len() as f64 / 1048576.0,
                body.len() as f64 / 1048576.0
            );
            self.live
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(text.clone());
            report(&text);
        }
        pyfs::replace(&part, dest).unwrap();
        Ok(FileOutcome {
            bytes: body.len() as i64,
            final_url: url.to_string(),
            content_length: body.len() as i64,
        })
    }
}

/// One printed line per `emit_line`: the recorder drops the newline so a
/// test can compare lines instead of raw writes.
pub struct LinesOut {
    pub lines: Arc<Mutex<Vec<String>>>,
}

impl Out for LinesOut {
    fn write(&mut self, text: &str) {
        if text == "\n" {
            return;
        }
        self.lines
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(text.to_string());
    }

    fn flush(&mut self) {}
}

/// A scratch directory under the system temp dir, named after the
/// process so parallel test binaries do not collide.
pub fn scratch(tag: &str) -> String {
    let dir = std::env::temp_dir()
        .join(format!("ytdlp-p8-{}-{tag}", std::process::id()))
        .to_string_lossy()
        .into_owned();
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
