//! One owner per data directory, with an authenticated, acknowledged launch queue.
//! The file lock is released by the OS on crash; a stale endpoint never owns it.
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashSet, VecDeque},
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

pub type Queue = Arc<Mutex<VecDeque<Vec<String>>>>;
pub struct Incoming(pub SingleInstance);
impl gpui::Global for Incoming {}

pub fn http_urls(urls: impl IntoIterator<Item = String>) -> Vec<String> {
    urls.into_iter()
        .filter_map(|value| {
            // Do not interpret arbitrary CLI flags, files, or search terms as links.
            let url = url::Url::parse(&value).ok()?;
            (matches!(url.scheme(), "http" | "https") && url.host_str().is_some())
                .then(|| url.to_string())
        })
        .collect()
}

pub fn enqueue(queue: &Queue, urls: Vec<String>) {
    let urls = http_urls(urls);
    if !urls.is_empty() {
        queue.lock().unwrap().push_back(urls);
    }
}

#[derive(Serialize, Deserialize)]
struct Endpoint {
    port: u16,
    token: String,
    pid: u32,
}
#[derive(Serialize, Deserialize)]
struct Request {
    token: String,
    id: String,
    urls: Vec<String>,
}

pub struct SingleInstance {
    pub incoming: Queue,
    _lock: File,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl SingleInstance {
    pub fn start() -> Result<Option<Self>> {
        let urls = http_urls(std::env::args().skip(1));
        // Smoke runs have their own profile and must never contact a real app.
        let dir = crate::persistence::data_dir().unwrap_or_else(|| {
            std::env::temp_dir().join(format!("axolotl-smoke-{}", std::process::id()))
        });
        #[cfg(target_os = "macos")]
        let awaiting_apple_event = urls.is_empty();
        let instance = Self::at(&dir, urls)?;
        #[cfg(target_os = "macos")]
        if instance.is_none() && awaiting_apple_event {
            drain_secondary_apple_events(dir)?;
        }
        Ok(instance)
    }

    fn at(dir: &Path, urls: Vec<String>) -> Result<Option<Self>> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join("browser-instance.json");
        let mut options = OpenOptions::new();
        options.create(true).truncate(false).read(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        // Windows byte-range locks deny reads too. Keep the locked file
        // separate from the rendezvous data that secondary processes read.
        let lock = options.open(dir.join("browser-instance.lock"))?;
        let id = uuid::Uuid::new_v4().to_string();
        let deadline = Instant::now() + Duration::from_secs(12);
        loop {
            match lock.try_lock() {
                Ok(()) => break,
                Err(std::fs::TryLockError::WouldBlock) => {
                    // The owner may still be publishing its endpoint. Retry with
                    // the same ID, so a lost ACK cannot open the URLs twice.
                    if let Ok(endpoint) = std::fs::read(&path).and_then(|data| {
                        serde_json::from_slice::<Endpoint>(&data).map_err(Into::into)
                    }) && forward(&endpoint, &id, &urls).is_ok()
                    {
                        return Ok(None);
                    }
                    if Instant::now() >= deadline {
                        bail!(
                            "The running Axolotl did not accept this launch. Quit it and try again."
                        );
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(error) => return Err(error.into()),
            }
        }
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        listener.set_nonblocking(true)?;
        let token = uuid::Uuid::new_v4().to_string();
        let endpoint = Endpoint {
            port: listener.local_addr()?.port(),
            token: token.clone(),
            pid: std::process::id(),
        };
        options.truncate(true);
        let mut endpoint_file = options.open(&path)?;
        serde_json::to_writer(&mut endpoint_file, &endpoint)?;
        endpoint_file.flush()?;
        let incoming = Queue::default();
        enqueue(&incoming, urls);
        let queue = incoming.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let worker = std::thread::spawn(move || {
            let mut seen = HashSet::new();
            while !stopping.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
                        let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
                        let mut line = String::new();
                        let read = BufReader::new((&stream).take(65537)).read_line(&mut line);
                        if read.is_ok()
                            && line.len() <= 65536
                            && let Ok(request) = serde_json::from_str::<Request>(&line)
                            && request.token == token
                        {
                            if seen.insert(request.id) {
                                // Empty launch requests also foreground the owner.
                                queue.lock().unwrap().push_back(http_urls(request.urls));
                            }
                            let _ = stream.write_all(b"OK\n");
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(20))
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Some(Self {
            incoming,
            _lock: lock,
            stop,
            worker: Some(worker),
        }))
    }
}

/// A forced second bundle launch receives its URLs as Apple events, not argv.
/// Keep a windowless event loop alive long enough to receive the startup events
/// and acknowledge their handoff before exiting. Never restore a second session.
#[cfg(target_os = "macos")]
fn drain_secondary_apple_events(dir: std::path::PathBuf) -> Result<()> {
    let queue = Queue::default();
    let incoming = queue.clone();
    let outcome = Arc::new(Mutex::new(Ok(())));
    let result = outcome.clone();
    let application = gpui::Application::new();
    application.on_open_urls(move |urls| enqueue(&incoming, urls));
    application.run(move |cx| {
        cx.spawn(async move |cx| {
            let started = Instant::now();
            let mut last_delivery = None;
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(50))
                    .await;
                let batches: Vec<_> = queue.lock().unwrap().drain(..).collect();
                for urls in batches {
                    let path = dir.join("browser-instance.json");
                    let delivery = cx
                        .background_executor()
                        .spawn(async move {
                            let id = uuid::Uuid::new_v4().to_string();
                            let deadline = Instant::now() + Duration::from_secs(12);
                            loop {
                                let attempt = (|| -> Result<()> {
                                    let endpoint =
                                        serde_json::from_slice::<Endpoint>(&std::fs::read(&path)?)?;
                                    forward(&endpoint, &id, &urls)
                                })();
                                if attempt.is_ok() || Instant::now() >= deadline {
                                    return attempt;
                                }
                                std::thread::sleep(Duration::from_millis(50));
                            }
                        })
                        .await;
                    if let Err(error) = delivery {
                        *result.lock().unwrap() = Err(error);
                        let _ = cx.update(|cx| cx.quit());
                        return;
                    }
                    last_delivery = Some(Instant::now());
                }
                // A normal empty executable launch has no Apple event. Allow
                // startup events to drain, then exit without leaving a helper.
                if last_delivery.is_some_and(|last| last.elapsed() > Duration::from_millis(500))
                    || (last_delivery.is_none() && started.elapsed() > Duration::from_secs(3))
                {
                    let _ = cx.update(|cx| cx.quit());
                    return;
                }
            }
        })
        .detach();
    });
    std::mem::replace(&mut *outcome.lock().unwrap(), Ok(()))
}

fn forward(endpoint: &Endpoint, id: &str, urls: &[String]) -> Result<()> {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, endpoint.port));
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_millis(500))?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    #[cfg(target_os = "windows")]
    unsafe {
        // Pass foreground permission from the OS-launched process to the owner.
        let _ = windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(endpoint.pid);
    }
    let request = Request {
        token: endpoint.token.clone(),
        id: id.into(),
        urls: urls.into(),
    };
    let mut data = serde_json::to_vec(&request)?;
    data.push(b'\n');
    anyhow::ensure!(data.len() <= 65536, "Too many URL arguments in one launch");
    stream.write_all(&data)?;
    let mut ack = [0; 3];
    stream
        .read_exact(&mut ack)
        .context("launch acknowledgement")?;
    anyhow::ensure!(&ack == b"OK\n", "Invalid launch acknowledgement");
    Ok(())
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn external_urls_are_only_absolute_web_urls() {
        let urls = http_urls(
            [
                "--smoke-test",
                "file:///tmp/a",
                "javascript:alert(1)",
                "https://example.com/a?q=1&b=2#x",
                "HTTP://example.org",
                "example.com",
            ]
            .map(String::from),
        );
        assert_eq!(
            urls,
            ["https://example.com/a?q=1&b=2#x", "http://example.org/"]
        );
    }
    #[test]
    fn singleton_handoff_retries_and_crash_recovery() -> Result<()> {
        let dir =
            std::env::temp_dir().join(format!("axolotl-instance-test-{}", uuid::Uuid::new_v4()));
        let first = SingleInstance::at(&dir, vec!["https://cold.test/".into()])?.unwrap();
        let endpoint: Endpoint =
            serde_json::from_slice(&std::fs::read(dir.join("browser-instance.json"))?)?;
        let urls = vec!["https://one.test/".into(), "https://two.test/".into()];
        let unauthorized = Endpoint {
            port: endpoint.port,
            pid: endpoint.pid,
            token: "wrong-token".into(),
        };
        assert!(forward(&unauthorized, "rejected", &urls).is_err());
        forward(&endpoint, "same-delivery", &urls)?;
        forward(&endpoint, "same-delivery", &urls)?;
        assert!(SingleInstance::at(&dir, vec!["https://next.test/".into()])?.is_none());
        let batches: Vec<_> = first.incoming.lock().unwrap().drain(..).collect();
        assert_eq!(
            batches,
            vec![
                vec!["https://cold.test/".to_string()],
                urls,
                vec!["https://next.test/".into()]
            ]
        );
        drop(first);
        let replacement = SingleInstance::at(&dir, vec![])?.unwrap();
        let current: Endpoint =
            serde_json::from_slice(&std::fs::read(dir.join("browser-instance.json"))?)?;
        assert_ne!(endpoint.token, current.token);
        drop(replacement);
        std::fs::remove_dir_all(dir)?;
        Ok(())
    }
}
