//! Bounded receive forwarding, including the packet currently being delivered.
use super::*;
use std::collections::VecDeque;
use std::sync::{
    atomic::{AtomicBool, AtomicU64},
    Condvar,
};

struct Event {
    sequence: u64,
    timestamp: Option<i64>,
    packet: Packet,
}
struct State {
    pending: VecDeque<Event>,
    bytes: usize,
    count: usize,
}
pub(super) struct Delivery {
    pub generation: u64,
    state: Mutex<State>,
    changed: Condvar,
    limit: usize,
    retired: AtomicBool,
    local: AtomicU64,
    remote: AtomicU64,
    report: Box<dyn Fn(u64) + Send + Sync>,
    thread: Mutex<Option<thread::JoinHandle<()>>>,
}
impl Delivery {
    pub fn parent(receiver: Arc<Receiver>, limit: usize, generation: u64) -> Result<Arc<Self>> {
        let reported = Arc::clone(&receiver);
        Self::start(
            limit,
            generation,
            move |dropped| {
                reported.transport_dropped.store(dropped, Ordering::Relaxed);
            },
            move |event, _| {
                if let Some(event) = event {
                    receiver.deliver(event.sequence, event.timestamp, event.packet);
                }
                Ok(())
            },
        )
    }
    pub fn child(
        sender: Arc<Mutex<Box<dyn Write + Send>>>,
        limit: usize,
        generation: u64,
        diagnostics: Arc<Mutex<Option<super::super::InputDiagnostics>>>,
    ) -> Result<Arc<Self>> {
        let weak: Arc<Mutex<Weak<Self>>> = Arc::new(Mutex::new(Weak::new()));
        let own = Arc::clone(&weak);
        let mut reported = None;
        let delivery = Self::start(
            limit,
            generation,
            |_| {},
            move |event, dropped| {
                let native = diagnostics
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .as_ref()
                    .map_or(0, super::super::InputDiagnostics::dropped_packets);
                let dropped = dropped.saturating_add(native);
                if event.is_none() && reported == Some(dropped) {
                    return Ok(());
                }
                reported = Some(dropped);
                let value = if let Some(event) = event {
                    Value::Tuple(vec![
                        text("message"),
                        Value::Tuple(vec![
                            integer(generation),
                            integer(event.sequence),
                            event.timestamp.map_or(Value::Null, integer),
                            packet_value(&event.packet),
                            integer(dropped),
                        ]),
                    ])
                } else {
                    Value::Tuple(vec![
                        text("dropped"),
                        Value::Tuple(vec![integer(generation), integer(dropped)]),
                    ])
                };
                let mut stream = sender.lock().unwrap_or_else(|p| p.into_inner());
                let result = wire::write(&mut *stream, &value)
                    .and_then(|_| stream.flush())
                    .map_err(worker_error);
                if result.is_err() {
                    if let Some(delivery) = own.lock().unwrap_or_else(|p| p.into_inner()).upgrade()
                    {
                        delivery.retire();
                    }
                }
                result
            },
        )?;
        *weak.lock().unwrap_or_else(|p| p.into_inner()) = Arc::downgrade(&delivery);
        Ok(delivery)
    }
    fn start(
        limit: usize,
        generation: u64,
        report: impl Fn(u64) + Send + Sync + 'static,
        mut send: impl FnMut(Option<Event>, u64) -> Result<()> + Send + 'static,
    ) -> Result<Arc<Self>> {
        let delivery = Arc::new(Self {
            generation,
            state: Mutex::new(State {
                pending: VecDeque::new(),
                bytes: 0,
                count: 0,
            }),
            changed: Condvar::new(),
            limit,
            retired: AtomicBool::new(false),
            local: AtomicU64::new(0),
            remote: AtomicU64::new(0),
            report: Box::new(report),
            thread: Mutex::new(None),
        });
        let own = Arc::clone(&delivery);
        let thread = thread::Builder::new()
            .name("middi worker delivery".into())
            .spawn(move || loop {
                let mut state = own.state.lock().unwrap_or_else(|p| p.into_inner());
                if state.pending.is_empty() && !own.retired.load(Ordering::Acquire) {
                    state = own
                        .changed
                        .wait_timeout(state, Duration::from_millis(100))
                        .unwrap_or_else(|p| p.into_inner())
                        .0;
                }
                if own.retired.load(Ordering::Acquire) {
                    return;
                }
                let event = state.pending.pop_front();
                drop(state);
                let size = event.as_ref().map(|e| packet_size(&e.packet));
                if send(event, own.dropped()).is_err() {
                    own.retire();
                    return;
                }
                if let Some(size) = size {
                    let mut state = own.state.lock().unwrap_or_else(|p| p.into_inner());
                    state.bytes -= size;
                    state.count -= 1;
                }
            })
            .map_err(worker_error)?;
        *delivery.thread.lock().unwrap_or_else(|p| p.into_inner()) = Some(thread);
        Ok(delivery)
    }
    pub fn push(&self, sequence: u64, timestamp: Option<i64>, packet: Packet) {
        let size = packet_size(&packet);
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if self.retired.load(Ordering::Acquire)
            || state.count >= 128
            || size > self.limit.saturating_sub(state.bytes)
        {
            self.local.fetch_add(1, Ordering::Relaxed);
            drop(state);
            (self.report)(self.dropped());
            return;
        }
        state.bytes += size;
        state.count += 1;
        state.pending.push_back(Event {
            sequence,
            timestamp,
            packet,
        });
        self.changed.notify_one();
    }
    pub fn update(&self, dropped: u64) {
        self.remote.fetch_max(dropped, Ordering::Relaxed);
        (self.report)(self.dropped());
    }
    pub fn dropped(&self) -> u64 {
        self.local
            .load(Ordering::Relaxed)
            .saturating_add(self.remote.load(Ordering::Relaxed))
    }
    pub fn retire(&self) {
        self.retired.store(true, Ordering::Release);
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        self.local
            .fetch_add(state.pending.len() as u64, Ordering::Relaxed);
        while let Some(event) = state.pending.pop_front() {
            state.bytes -= packet_size(&event.packet);
            state.count -= 1;
        }
        self.changed.notify_all();
        drop(state);
        (self.report)(self.dropped());
    }
    pub fn drain(&self) {
        if let Some(thread) = self.thread.lock().unwrap_or_else(|p| p.into_inner()).take() {
            let _ = thread.join();
        }
    }
}
