use std::any::Any;
use std::ffi::{c_char, c_void, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use super::{
    ffi, worker::CallbackScope, Backend, Error, Packet, Port, Protocol, ReceivedPacket, Result,
    TimestampMode,
};

type Handler = Box<dyn FnMut(ReceivedPacket) + Send>;

pub(super) struct Receiver {
    pub clock: super::Clock,
    handler: Mutex<Option<Handler>>,
    changed: Condvar,
    panic_payload: Mutex<Option<Box<dyn Any + Send>>>,
    pub retired: AtomicBool,
    pub panicked: AtomicBool,
    pub dropped: AtomicU64,
    limit: usize,
    used: AtomicUsize,
}

impl Receiver {
    pub fn new(
        clock: super::Clock,
        limit: usize,
        callback: impl FnMut(ReceivedPacket) + Send + 'static,
    ) -> Self {
        Self {
            clock,
            handler: Mutex::new(Some(Box::new(callback))),
            changed: Condvar::new(),
            panic_payload: Mutex::new(None),
            retired: AtomicBool::new(false),
            panicked: AtomicBool::new(false),
            dropped: AtomicU64::new(0),
            limit,
            used: AtomicUsize::new(0),
        }
    }

    fn admit(&self, size: usize) -> Option<Payload<'_>> {
        let mut used = self.used.load(Ordering::Relaxed);
        loop {
            if size > self.limit - used {
                return None;
            }
            match self.used.compare_exchange_weak(
                used,
                used + size,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    return Some(Payload {
                        used: &self.used,
                        size,
                    })
                }
                Err(current) => used = current,
            }
        }
    }

    fn deliver(&self, timestamp: i64, packet: Packet) {
        if self.retired.load(Ordering::SeqCst) {
            return;
        }
        let mut slot = self.handler.lock().unwrap_or_else(|p| p.into_inner());
        while slot.is_none() && !self.retired.load(Ordering::SeqCst) {
            slot = self.changed.wait(slot).unwrap_or_else(|p| p.into_inner());
        }
        if self.retired.load(Ordering::SeqCst) {
            return;
        }
        let Some(mut handler) = slot.take() else {
            return;
        };
        drop(slot);
        // User code runs with no internal lock held. Taking the callable out of
        // the slot still serializes FnMut deliveries from concurrent native calls.
        let _scope = CallbackScope::enter();
        let result = catch_unwind(AssertUnwindSafe(|| {
            handler(ReceivedPacket {
                timestamp_ns: (self.clock.mode == TimestampMode::Absolute).then_some(timestamp),
                clock: self.clock,
                packet,
            })
        }));
        if let Err(payload) = result {
            // A panic payload can itself have a panicking destructor. Retain it
            // until checked unregistration, then dispose outside the C callback.
            *self.panic_payload.lock().unwrap_or_else(|p| p.into_inner()) = Some(payload);
            self.panicked.store(true, Ordering::Release);
            self.retired.store(true, Ordering::SeqCst);
        }
        let mut slot = self.handler.lock().unwrap_or_else(|p| p.into_inner());
        *slot = Some(handler);
        self.changed.notify_all();
    }

    fn drain(&self) {
        self.retired.store(true, Ordering::SeqCst);
        let mut slot = self.handler.lock().unwrap_or_else(|p| p.into_inner());
        self.changed.notify_all();
        while slot.is_none() {
            slot = self.changed.wait(slot).unwrap_or_else(|p| p.into_inner());
        }
    }
}

impl Drop for Receiver {
    fn drop(&mut self) {
        drop_user_value(
            self.panic_payload
                .get_mut()
                .unwrap_or_else(|p| p.into_inner())
                .take(),
        );
        drop_user_value(
            self.handler
                .get_mut()
                .unwrap_or_else(|p| p.into_inner())
                .take(),
        );
    }
}

fn drop_user_value(value: impl Sized) {
    if let Err(payload) = catch_unwind(AssertUnwindSafe(|| drop(value))) {
        // A second malicious panic payload must not restart unwinding during
        // cleanup. Only this exceptional payload is intentionally retained.
        std::mem::forget(payload);
        eprintln!("meedee: callback data destructor panicked; native cleanup continued");
    }
}

struct Payload<'a> {
    used: &'a AtomicUsize,
    size: usize,
}
impl Drop for Payload<'_> {
    fn drop(&mut self) {
        self.used.fetch_sub(self.size, Ordering::AcqRel);
    }
}

#[derive(Default)]
struct Diagnostics(Mutex<Option<String>>);

pub(super) struct Native {
    input_direction: bool,
    backend: Backend,
    identity: Arc<()>,
    generation: u64,
    ports: Vec<NativePort>,
    observer: *mut c_void,
    input: *mut c_void,
    output: *mut c_void,
    // The allocation behind Arc has a stable callback context address.
    receiver: Option<Arc<Receiver>>,
    diagnostics: Box<Diagnostics>,
    runtime: bool,
    retained: bool,
}

impl Native {
    pub fn new(input_direction: bool, backend: Backend) -> Self {
        Self {
            input_direction,
            backend,
            identity: Arc::new(()),
            generation: 0,
            ports: Vec::new(),
            observer: ptr::null_mut(),
            input: ptr::null_mut(),
            output: ptr::null_mut(),
            receiver: None,
            diagnostics: Box::default(),
            runtime: false,
            retained: false,
        }
    }

    pub fn initialize(&mut self) -> Result<()> {
        // SAFETY: called only by the owning thread; shutdown is balanced there.
        status(unsafe { ffi::md_thread_init() })?;
        self.runtime = true;
        // SAFETY: diagnostics is boxed and lives until all native handles retire.
        let result = unsafe {
            ffi::md_observer_new(
                self.backend as i32,
                self.error_context(),
                on_error,
                &mut self.observer,
            )
        };
        self.check(result)
    }

    pub fn ports(&mut self) -> Result<Vec<Port>> {
        self.check(0)?;
        let mut collector = Collector {
            input: self.input_direction,
            ports: Vec::new(),
            error: None,
        };
        // SAFETY: enumeration calls visit synchronously. Its borrowed port is
        // cloned before visit returns; no borrowed native pointer escapes.
        let result = unsafe {
            ffi::md_enumerate(
                self.observer,
                u32::from(self.input_direction),
                (&mut collector as *mut Collector).cast(),
                visit,
            )
        };
        self.check(result)?;
        if let Some(error) = collector.error {
            return Err(error);
        }
        self.generation += 1;
        let mut ports = Vec::new();
        ports
            .try_reserve_exact(collector.ports.len())
            .map_err(|_| Error::Resource("port metadata allocation failed"))?;
        for (index, port) in collector.ports.iter().enumerate() {
            let (mut id, mut name, mut size) = (0, ptr::null(), 0);
            // SAFETY: the cloned port remains alive for the length of this call
            // and while its returned string is copied below.
            status(unsafe {
                ffi::md_port_info(
                    u32::from(self.input_direction),
                    port.raw,
                    &mut id,
                    &mut name,
                    &mut size,
                )
            })?;
            let bytes = if size == 0 {
                &[][..]
            } else {
                // SAFETY: native port_name.data()/size() remain valid in port.
                unsafe { std::slice::from_raw_parts(name.cast::<u8>(), size) }
            };
            let name = String::from_utf8(copy(bytes)?)
                .map_err(|_| Error::Invalid("native port name is not UTF-8"))?;
            ports.push(Port {
                owner: Arc::downgrade(&self.identity),
                generation: self.generation,
                index,
                backend: self.backend,
                backend_id: id,
                name,
            });
        }
        self.ports = collector.ports;
        Ok(ports)
    }

    fn port(&self, port: Option<&Port>) -> Result<*const c_void> {
        self.check(0)?;
        let Some(port) = port else {
            return Ok(ptr::null());
        };
        if !port.owner.ptr_eq(&Arc::downgrade(&self.identity)) || port.generation != self.generation
        {
            return Err(Error::Invalid(
                "port belongs to another client or an expired discovery snapshot",
            ));
        }
        self.ports
            .get(port.index)
            .map(|port| port.raw.cast_const())
            .ok_or(Error::Invalid("port is not in this discovery snapshot"))
    }

    pub fn open_input(
        &mut self,
        port: Option<&Port>,
        name: &CString,
        receiver: Arc<Receiver>,
    ) -> Result<()> {
        let port = self.port(port)?;
        self.receiver = Some(receiver);
        let receiver = self.receiver.as_mut().unwrap();
        let config = ffi::InputConfig {
            api: self.backend as i32,
            flags: u32::from(self.backend.protocol() == Protocol::Ump)
                | (u32::from(receiver.clock.mode == TimestampMode::Absolute) << 1),
            port,
            name: name.as_ptr(),
            context: Arc::as_ptr(receiver).cast_mut().cast(),
            bytes: on_bytes,
            words: on_words,
            error_context: self.error_context(),
            error: on_error,
        };
        // SAFETY: config/name/port live through the call; its stored callback
        // contexts are boxed and retained until checked free succeeds.
        let result = unsafe { ffi::md_input_new(&config, &mut self.input) };
        let result = self.check(result);
        if result.is_err() {
            self.receiver
                .as_ref()
                .unwrap()
                .retired
                .store(true, Ordering::SeqCst);
            if !self.input.is_null() {
                self.retained = true;
            }
        }
        result
    }

    pub fn open_output(&mut self, port: Option<&Port>, name: &CString) -> Result<()> {
        let port = self.port(port)?;
        // SAFETY: arguments are live for creation; diagnostics lives with self.
        let result = unsafe {
            ffi::md_output_new(
                self.backend as i32,
                port,
                name.as_ptr(),
                self.error_context(),
                on_error,
                &mut self.output,
            )
        };
        let result = self.check(result);
        if result.is_err() && !self.output.is_null() {
            self.retained = true;
        }
        result
    }

    pub fn close_input(&mut self) -> Result<()> {
        if let Some(receiver) = &self.receiver {
            receiver.drain();
        }
        if !self.input.is_null() {
            // SAFETY: only this thread owns this handle. On failure the handle,
            // context, COM runtime and owning thread remain alive, without retry.
            let result = unsafe { ffi::md_input_free(self.input) };
            if result != 0 {
                self.retained = true;
                return self.check(result);
            }
            self.input = ptr::null_mut();
        }
        self.receiver = None;
        Ok(())
    }

    pub fn close_output(&mut self) -> Result<()> {
        if !self.output.is_null() {
            // SAFETY: exclusive owner; bridge deletion does not retain arguments.
            let result = unsafe { ffi::md_output_free(self.output) };
            if result != 0 {
                self.retained = true;
                return self.check(result);
            }
            self.output = ptr::null_mut();
        }
        Ok(())
    }

    pub fn send(&mut self, packet: &Packet) -> Result<()> {
        self.check(0)?;
        if self.output.is_null() {
            return Err(Error::Closed);
        }
        match packet {
            Packet::Bytes(bytes) => {
                // SAFETY: the owned bytes live through this synchronous call.
                let result =
                    unsafe { ffi::md_send_bytes(self.output, bytes.as_ptr(), bytes.len()) };
                self.check(result)
            }
            Packet::Ump(words) => {
                let mut offset = 0;
                while offset < words.len() {
                    let size = super::UMP_WORDS[(words[offset] >> 28) as usize];
                    let packet = &words[offset..offset + size];
                    // SAFETY: framing was validated before queueing; this is
                    // exactly one complete UMP in its owned backing buffer.
                    // Per-packet calls avoid losing intermediate flush errors
                    // in libremidi's large-stream segmentation path.
                    let result =
                        unsafe { ffi::md_send_words(self.output, packet.as_ptr(), packet.len()) };
                    if let Err(error) = self.check(result) {
                        return Err(Error::Native(format!(
                            "UMP send failed after {offset} of {} words: {error}",
                            words.len()
                        )));
                    }
                    offset += size;
                }
                Ok(())
            }
        }
    }

    fn error_context(&self) -> *mut c_void {
        (&*self.diagnostics as *const Diagnostics).cast_mut().cast()
    }

    pub fn check_health(&self) -> Result<()> {
        self.check(0)
    }

    fn check(&self, code: i32) -> Result<()> {
        let mut diagnostic = self.diagnostics.0.lock().unwrap_or_else(|p| p.into_inner());
        if diagnostic.is_none() && code != 0 {
            *diagnostic = Some(format!("native MIDI operation failed ({code})"));
        }
        match &*diagnostic {
            Some(error) => Err(Error::Native(error.clone())),
            None => Ok(()),
        }
    }
}

impl Drop for Native {
    fn drop(&mut self) {
        if let Some(receiver) = &self.receiver {
            receiver.retired.store(true, Ordering::SeqCst);
        }
        if self.retained || self.close_input().is_err() || self.close_output().is_err() {
            retain_thread();
        }
        self.ports.clear();
        if !self.observer.is_null() {
            // SAFETY: no observation callbacks were registered; all use was on
            // this thread and disposal precedes diagnostics/runtime destruction.
            if unsafe { ffi::md_observer_free(self.observer) } != 0 {
                retain_thread();
            }
        }
        if self.runtime {
            // SAFETY: same native thread that successfully initialized it, after
            // all native handles have been released.
            unsafe { ffi::md_thread_shutdown() };
        }
    }
}

pub(super) fn status(code: i32) -> Result<()> {
    if code == 0 {
        Ok(())
    } else {
        Err(Error::Native(format!(
            "native MIDI operation failed ({code})"
        )))
    }
}

pub(super) fn port_name(name: &str) -> Result<CString> {
    CString::new(name).map_err(|_| Error::Invalid("port name must not contain NUL"))
}

pub(super) fn copy<T: Copy>(data: &[T]) -> Result<Vec<T>> {
    let mut owned = Vec::new();
    owned
        .try_reserve_exact(data.len())
        .map_err(|_| Error::Resource("MIDI payload allocation failed"))?;
    owned.extend_from_slice(data);
    Ok(owned)
}

struct NativePort {
    raw: *mut c_void,
    input: bool,
}
impl Drop for NativePort {
    fn drop(&mut self) {
        // SAFETY: port clones are owned by this thread and have no callbacks.
        unsafe { ffi::md_port_free(u32::from(self.input), self.raw) };
    }
}
struct Collector {
    input: bool,
    ports: Vec<NativePort>,
    error: Option<Error>,
}

unsafe extern "C" fn visit(ctx: *mut c_void, port: *const c_void) {
    // SAFETY: the synchronous enumeration owns Collector throughout this call.
    let collector = unsafe { &mut *ctx.cast::<Collector>() };
    if collector.error.is_some() {
        return;
    }
    let result = catch_unwind(AssertUnwindSafe(|| -> Result<()> {
        collector
            .ports
            .try_reserve(1)
            .map_err(|_| Error::Resource("port snapshot allocation failed"))?;
        let mut raw = ptr::null_mut();
        // SAFETY: port is valid only for this callback; the returned clone is owned.
        status(unsafe { ffi::md_port_clone(u32::from(collector.input), port, &mut raw) })?;
        collector.ports.push(NativePort {
            raw,
            input: collector.input,
        });
        Ok(())
    }));
    collector.error = match result {
        Ok(Ok(())) => None,
        Ok(Err(error)) => Some(error),
        Err(_) => Some(Error::Resource("port discovery panicked")),
    };
}

unsafe extern "C" fn on_error(
    ctx: *mut c_void,
    text: *const c_char,
    size: usize,
    _: *const c_void,
) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: native error callbacks borrow the boxed diagnostics and text.
        let diagnostics = unsafe { &*ctx.cast::<Diagnostics>() };
        let mut error = diagnostics.0.lock().unwrap_or_else(|p| p.into_inner());
        if error.is_none() {
            *error = Some(if size == 0 || text.is_null() {
                "native backend reported an error".into()
            } else {
                // SAFETY: native text is valid for size bytes during callback.
                String::from_utf8_lossy(unsafe {
                    std::slice::from_raw_parts(text.cast::<u8>(), size.min(4096))
                })
                .into_owned()
            });
        }
    }));
}

unsafe fn borrow_receiver(ctx: *mut c_void) -> Arc<Receiver> {
    let ptr = ctx.cast::<Receiver>().cast_const();
    // SAFETY: ctx came from Arc::as_ptr. Native retains its strong reference
    // until checked unregistration completes; each invocation owns this extra
    // reference until it has completely finished using the callback state.
    unsafe {
        Arc::increment_strong_count(ptr);
        Arc::from_raw(ptr)
    }
}

unsafe extern "C" fn on_bytes(ctx: *mut c_void, timestamp: i64, bytes: *const u8, size: usize) {
    // SAFETY: the boxed Arc context outlives all registered native callbacks.
    let receiver = unsafe { borrow_receiver(ctx) };
    if receiver.retired.load(Ordering::SeqCst) {
        return;
    }
    if size > receiver.limit || size > isize::MAX as usize || (size != 0 && bytes.is_null()) {
        receiver.dropped.fetch_add(1, Ordering::Relaxed);
        return;
    }
    // SAFETY: native callback buffer is borrowed for this call; empty buffers
    // need no slice construction and are forwarded as empty raw packets.
    let data = if size == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(bytes, size) }
    };
    let Some(_payload) = receiver.admit(size) else {
        receiver.dropped.fetch_add(1, Ordering::Relaxed);
        return;
    };
    match copy(data) {
        Ok(data) => receiver.deliver(timestamp, Packet::Bytes(data)),
        Err(_) => {
            receiver.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

unsafe extern "C" fn on_words(ctx: *mut c_void, timestamp: i64, words: *const u32, size: usize) {
    // SAFETY: same ownership contract as on_bytes; size is a word count.
    let receiver = unsafe { borrow_receiver(ctx) };
    if receiver.retired.load(Ordering::SeqCst) {
        return;
    }
    if size > receiver.limit / 4 || size > isize::MAX as usize / 4 || (size != 0 && words.is_null())
    {
        receiver.dropped.fetch_add(1, Ordering::Relaxed);
        return;
    }
    // SAFETY: a native UMP callback supplies aligned u32 words valid during call.
    let data = if size == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(words, size) }
    };
    let Some(_payload) = receiver.admit(size * 4) else {
        receiver.dropped.fetch_add(1, Ordering::Relaxed);
        return;
    };
    match copy(data) {
        Ok(data) => receiver.deliver(timestamp, Packet::Ump(data)),
        Err(_) => {
            receiver.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

fn retain_thread() -> ! {
    eprintln!("meedee: native teardown failed; retaining the client thread and resources until process exit");
    // Do not exit the COM apartment or drop callback-bearing state. The worker
    // still owns one of the bounded 32 client reservations. No automatic retry.
    loop {
        std::thread::park();
    }
}

#[cfg(test)]
mod tests {
    use super::super::Clock;
    use super::*;
    use std::sync::mpsc;

    fn receiver(
        mode: TimestampMode,
        limit: usize,
        handler: impl FnMut(ReceivedPacket) + Send + 'static,
    ) -> Arc<Receiver> {
        Arc::new(Receiver::new(
            Clock {
                id: 1,
                backend: Backend::CoreMidi,
                mode,
            },
            limit,
            handler,
        ))
    }

    #[test]
    fn callbacks_copy_borrowed_bytes_and_words_and_preserve_zero_time() {
        let (sender, receiver_channel) = mpsc::channel();
        let receiver = receiver(TimestampMode::Absolute, 32, move |packet| {
            sender.send(packet).unwrap();
        });
        let context = Arc::as_ptr(&receiver).cast_mut().cast();
        let mut bytes = [0x90, 60, 100];
        let mut words = [0x40903c00, 0xc0000000];
        // SAFETY: both buffers and the Arc allocation live through each call.
        unsafe {
            on_bytes(context, 0, bytes.as_ptr(), bytes.len());
            on_words(context, i64::MAX, words.as_ptr(), words.len());
        }
        bytes.fill(0);
        words.fill(0);
        let first = receiver_channel.recv().unwrap();
        assert_eq!(first.packet, Packet::Bytes(vec![0x90, 60, 100]));
        assert_eq!(first.timestamp_ns, Some(0));
        let second = receiver_channel.recv().unwrap();
        assert_eq!(second.packet, Packet::Ump(vec![0x40903c00, 0xc0000000]));
        assert_eq!(second.timestamp_ns, Some(i64::MAX));
        assert_eq!(first.clock, receiver.clock);
    }

    #[test]
    fn disabled_timestamps_are_absent_and_oversize_packets_are_counted() {
        let (sender, receiver_channel) = mpsc::channel();
        let receiver = receiver(TimestampMode::Off, 3, move |packet| {
            sender.send(packet).unwrap();
        });
        let context = Arc::as_ptr(&receiver).cast_mut().cast();
        let bytes = [0x90, 60, 100];
        let words = [0x20903c64];
        // SAFETY: synchronous borrowed buffers, with a live Arc callback context.
        unsafe {
            on_bytes(context, 123, bytes.as_ptr(), bytes.len());
            on_words(context, 456, words.as_ptr(), words.len());
        }
        assert_eq!(receiver_channel.recv().unwrap().timestamp_ns, None);
        assert_eq!(receiver.dropped.load(Ordering::Relaxed), 1);
        assert!(receiver_channel.try_recv().is_err());
    }

    #[test]
    fn callback_panic_does_not_cross_ffi_or_allow_another_delivery() {
        let receiver = receiver(TimestampMode::Absolute, 32, |_| {
            panic!("expected callback panic")
        });
        let context = Arc::as_ptr(&receiver).cast_mut().cast();
        let bytes = [0xf8];
        // SAFETY: context and byte buffer remain live, including after the panic.
        unsafe {
            on_bytes(context, 1, bytes.as_ptr(), bytes.len());
            on_bytes(context, 2, bytes.as_ptr(), bytes.len());
        }
        assert!(receiver.panicked.load(Ordering::Acquire));
        assert!(receiver.retired.load(Ordering::SeqCst));
        assert!(!receiver.handler.is_poisoned());
    }

    #[test]
    fn active_delivery_reserves_bytes_and_releases_them_after_return() {
        let (entered, started) = mpsc::channel();
        let (release, released) = mpsc::channel();
        let receiver = receiver(TimestampMode::Absolute, 3, move |_| {
            entered.send(()).unwrap();
            released.recv().unwrap();
        });
        let context = Arc::clone(&receiver);
        let delivery = std::thread::spawn(move || {
            let bytes = [0x90, 60, 100];
            // SAFETY: the context Arc and buffer outlive this invocation.
            unsafe {
                on_bytes(
                    Arc::as_ptr(&context).cast_mut().cast(),
                    0,
                    bytes.as_ptr(),
                    3,
                );
            }
        });
        started
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        let bytes = [0xf8];
        // Admission fails before waiting for the callable held by delivery.
        // SAFETY: receiver and buffer remain live through this invocation.
        unsafe {
            on_bytes(
                Arc::as_ptr(&receiver).cast_mut().cast(),
                1,
                bytes.as_ptr(),
                1,
            );
        }
        assert_eq!(receiver.dropped.load(Ordering::Relaxed), 1);
        release.send(()).unwrap();
        delivery.join().unwrap();
        assert_eq!(receiver.used.load(Ordering::Acquire), 0);
    }

    #[test]
    fn panicking_panic_payload_is_disposed_after_the_ffi_callback() {
        struct PanicOnDrop(Arc<AtomicBool>);
        impl Drop for PanicOnDrop {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Release);
                panic!("expected panic payload destructor failure");
            }
        }
        let dropped = Arc::new(AtomicBool::new(false));
        let payload_dropped = Arc::clone(&dropped);
        let receiver = receiver(TimestampMode::Absolute, 3, move |_| {
            std::panic::panic_any(PanicOnDrop(Arc::clone(&payload_dropped)));
        });
        let bytes = [0xf8];
        // SAFETY: receiver and buffer remain live through this invocation.
        unsafe {
            on_bytes(
                Arc::as_ptr(&receiver).cast_mut().cast(),
                0,
                bytes.as_ptr(),
                1,
            );
        }
        assert!(!dropped.load(Ordering::Acquire));
        drop(receiver);
        assert!(dropped.load(Ordering::Acquire));
    }
}
