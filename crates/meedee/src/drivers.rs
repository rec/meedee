//! Shared caller-thread workflow I/O; no separate scheduler or transport backend.
use crate::{events::Payload, live::Item, midi, playback::PlaybackClock};
use std::time::Duration;

pub trait Receiver {
    fn receive(&mut self, timeout: Duration) -> midi::Result<Option<Item>>;
    fn check(&self) -> midi::Result<()>;
    fn dropped_packets(&self) -> midi::Result<u64> {
        Ok(0)
    }
    fn check_monitor(&self) -> midi::Result<()> {
        self.check()
    }
}
impl Receiver for midi::Input {
    fn dropped_packets(&self) -> midi::Result<u64> {
        Ok(self.dropped_packets())
    }
    fn check_monitor(&self) -> midi::Result<()> {
        self.check_health()
    }
    fn receive(&mut self, timeout: Duration) -> midi::Result<Option<Item>> {
        self.receive(Some(timeout), None)
    }
    fn check(&self) -> midi::Result<()> {
        self.check_health()?;
        if self.dropped_packets() != 0 {
            return Err(midi::Error::Transport(crate::transport::Error::Resource(
                "input packet loss reported by transport counter".into(),
            )));
        }
        Ok(())
    }
}
pub trait Sender {
    fn send(&mut self, message: Payload) -> midi::Result<()>;
    fn send_management(&mut self, message: crate::ci::Management) -> midi::Result<()> {
        self.send(Payload::Message(crate::messages::AnyMessage::SysEx(
            message.sysex(),
        )))
    }
}
impl Sender for midi::Output {
    fn send(&mut self, message: Payload) -> midi::Result<()> {
        let report = self.send(message).map_err(|e| match e.cause {
            crate::playback::Cause::Send(e) => e,
            crate::playback::Cause::Input(e) => e.into(),
        })?;
        if report.cancelled {
            Err(midi::Error::Cancelled)
        } else {
            Ok(())
        }
    }
}
pub(crate) fn remaining(clock: &mut impl PlaybackClock, deadline: u64) -> Duration {
    Duration::from_nanos(deadline.saturating_sub(clock.now_ns())).min(Duration::from_millis(50))
}
pub(crate) fn invalid(reason: &str) -> midi::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, reason).into()
}
pub(crate) fn interrupted(error: &midi::Error) -> bool {
    matches!(error, midi::Error::Cancelled)
        || matches!(error, midi::Error::Io(e) if e.kind() == std::io::ErrorKind::Interrupted)
}
pub(crate) fn input_loss(error: &midi::Error) -> bool {
    matches!(
        error,
        midi::Error::Transport(crate::transport::Error::Resource(reason))
            if reason == "input packet loss reported by transport counter"
    )
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{
        messages::{AnyMessage, Message, SysEx},
        playback::Cancellation,
    };
    use std::{cell::Cell, collections::VecDeque, io, rc::Rc};
    #[derive(Clone, Default)]
    pub struct Clock(pub Rc<Cell<u64>>);
    impl PlaybackClock for Clock {
        fn now_ns(&mut self) -> u64 {
            self.0.get()
        }
        fn wait_until(&mut self, deadline: u64, cancel: &Cancellation) -> io::Result<bool> {
            if cancel.cancelled() {
                return Ok(false);
            }
            self.0.set(deadline.max(self.0.get()));
            Ok(true)
        }
    }
    pub struct Input {
        pub clock: Clock,
        pub items: VecDeque<Item>,
        pub lost: bool,
    }
    impl Receiver for Input {
        fn receive(&mut self, timeout: Duration) -> midi::Result<Option<Item>> {
            if let Some(item) = self.items.pop_front() {
                return Ok(Some(item));
            }
            self.clock
                .0
                .set(self.clock.0.get() + timeout.as_nanos() as u64);
            Ok(None)
        }
        fn check(&self) -> midi::Result<()> {
            if self.lost {
                Err(midi::Error::Transport(crate::transport::Error::Resource(
                    "known loss".into(),
                )))
            } else {
                Ok(())
            }
        }
    }
    pub struct Output {
        pub clock: Clock,
        pub sent: Vec<(u64, Payload)>,
        pub fail_at: Option<usize>,
    }
    impl Sender for Output {
        fn send(&mut self, message: Payload) -> midi::Result<()> {
            if self.fail_at == Some(self.sent.len()) {
                return Err(midi::Error::Send(crate::transport::Error::Native(
                    "send failed".into(),
                )));
            }
            self.sent.push((self.clock.0.get(), message));
            self.clock.0.set(self.clock.0.get() + 100);
            Ok(())
        }
    }
    pub fn sysex(data: &[u8]) -> Item {
        Item::Message(Box::new(
            Payload::Message(AnyMessage::SysEx(SysEx::new(data.to_vec()).unwrap()))
                .with_stream("s")
                .unwrap(),
        ))
    }
}
