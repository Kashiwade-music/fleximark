//! Role: Stream bounded browser subscriptions with change notifications.
use crate::delivery::PreviewPage;
use crate::http::{FORBIDDEN_RESPONSE, UNAVAILABLE_RESPONSE, write_empty_response};
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

pub(crate) struct StreamState {
    epoch: Mutex<u64>,
    changed: Condvar,
    sockets: Mutex<HashMap<u64, TcpStream>>,
    stopped: AtomicBool,
}
impl StreamState {
    pub(crate) fn new() -> Self {
        Self {
            epoch: Mutex::new(0),
            changed: Condvar::new(),
            sockets: Mutex::new(HashMap::new()),
            stopped: AtomicBool::new(false),
        }
    }
    pub(crate) fn is_shutdown(&self) -> bool {
        self.stopped.load(Ordering::Acquire)
    }
    pub(crate) fn notify(&self) {
        if let Ok(mut epoch) = self.epoch.lock() {
            *epoch = epoch.wrapping_add(1);
            self.changed.notify_all();
        }
    }
    pub(crate) fn shutdown(&self) {
        self.stopped.store(true, Ordering::Release);
        self.notify();
        if let Ok(sockets) = self.sockets.lock() {
            for stream in sockets.values() {
                let _ = stream.shutdown(Shutdown::Both);
            }
        }
    }
    fn reserve(self: &Arc<Self>, stream: &TcpStream) -> io::Result<Subscription> {
        let mut sockets = self
            .sockets
            .lock()
            .map_err(|_| io::Error::other("stream lock"))?;
        if self.is_shutdown() || sockets.len() >= 16 {
            return Err(io::Error::other("stream capacity exhausted"));
        }
        let id = (0..16)
            .find(|id| !sockets.contains_key(id))
            .ok_or_else(|| io::Error::other("stream capacity exhausted"))?;
        sockets.insert(id, stream.try_clone()?);
        Ok(Subscription {
            id,
            state: Arc::clone(self),
        })
    }
}
struct Subscription {
    id: u64,
    state: Arc<StreamState>,
}
impl Drop for Subscription {
    fn drop(&mut self) {
        if let Ok(mut sockets) = self.state.sockets.lock() {
            sockets.remove(&self.id);
        }
    }
}

pub(crate) struct DeadlineReader<'a> {
    stream: &'a TcpStream,
    deadline: Instant,
}
impl<'a> DeadlineReader<'a> {
    pub(crate) fn new(stream: &'a TcpStream, deadline: Instant) -> Self {
        Self { stream, deadline }
    }
}
impl Read for DeadlineReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let remaining = self
            .deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "request deadline"))?;
        self.stream.set_read_timeout(Some(remaining))?;
        self.stream.read(buffer)
    }
}

pub(crate) fn start(
    mut stream: TcpStream,
    token: &str,
    _last_event_id: Option<u64>,
    pages: &Arc<Mutex<HashMap<String, PreviewPage>>>,
    state: &Arc<StreamState>,
) {
    if !pages.lock().is_ok_and(|pages| pages.contains_key(token)) {
        write_empty_response(&mut stream, FORBIDDEN_RESPONSE);
        return;
    }
    let Ok(subscription) = state.reserve(&stream) else {
        write_empty_response(&mut stream, UNAVAILABLE_RESPONSE);
        return;
    };
    let token = token.to_owned();
    let pages = Arc::clone(pages);
    let state = Arc::clone(state);
    std::thread::spawn(move || {
        let _subscription = subscription;
        if stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: keep-alive\r\n\r\n").is_err() {
            return;
        }
        let mut delivered = 0;
        let mut first = true;
        loop {
            if state.is_shutdown() {
                return;
            }
            let Ok(epoch) = state.epoch.lock().map(|epoch| *epoch) else {
                return;
            };
            let Some(page) = pages
                .lock()
                .ok()
                .and_then(|pages| pages.get(&token).cloned())
            else {
                return;
            };
            let events = match publications(&page, delivered, first) {
                Ok(events) => events,
                Err(error) => {
                    fleximark_application::logging::log_operational_failure(
                        "preview-stream-failed",
                        &error,
                        None,
                        None,
                        None,
                    );
                    return;
                }
            };
            first = false;
            for (sequence, body) in events {
                if write!(stream, "id: {sequence}\ndata: {body}\n\n").is_err() {
                    return;
                }
                delivered = delivered.max(sequence);
            }
            let Ok(current) = state.epoch.lock() else {
                return;
            };
            if *current != epoch {
                continue;
            }
            let Ok((current, timeout)) =
                state
                    .changed
                    .wait_timeout_while(current, Duration::from_secs(15), |current| {
                        *current == epoch && !state.is_shutdown()
                    })
            else {
                return;
            };
            drop(current);
            if timeout.timed_out() && stream.write_all(b": keep-alive\n\n").is_err() {
                return;
            }
        }
    });
}

fn publications(
    page: &PreviewPage,
    delivered: u64,
    first: bool,
) -> Result<Vec<(u64, String)>, fleximark_application::ApplicationError> {
    let mut events = Vec::new();
    if first || page.frame_sequence > delivered {
        let frame = fleximark_wire_mapping::frame(&page.frame)?;
        events.push((page.frame_sequence, serde_json::to_string(&frame)?));
    }
    for navigation in [&page.durable_navigation, &page.navigation_event]
        .into_iter()
        .flatten()
    {
        if (first || navigation.sequence > delivered) && navigation.sequence > page.frame_sequence {
            events.push((navigation.sequence, navigation.encoded.clone()));
        }
    }
    events.sort_by_key(|(sequence, _)| *sequence);
    Ok(events)
}
