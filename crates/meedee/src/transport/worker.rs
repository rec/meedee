use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::thread::{self, ThreadId};

use super::{Error, Result};

type Command<T> = Box<dyn FnOnce(&mut T) + Send>;

pub(super) struct Worker<T> {
    sender: SyncSender<Command<T>>,
    owner: ThreadId,
}

impl<T: 'static> Worker<T> {
    pub fn spawn(
        make: impl FnOnce() -> T + Send + 'static,
        initialize: impl FnOnce(&mut T) -> Result<()> + Send + 'static,
    ) -> Result<Self> {
        if IN_CALLBACK.get() {
            return Err(Error::CallbackThread);
        }
        if CLIENTS.fetch_add(1, Ordering::AcqRel) >= CLIENT_LIMIT {
            CLIENTS.fetch_sub(1, Ordering::AcqRel);
            return Err(Error::Resource("native client capacity exhausted (32)"));
        }
        let permit = Permit;
        let (sender, receiver) = mpsc::sync_channel::<Command<T>>(1);
        let (ready, startup) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("meedee native client".into())
            .spawn(move || {
                let _permit = permit;
                // T need not be Send: construction, use and destruction stay here.
                let mut state = make();
                let result = initialize(&mut state);
                let initialized = result.is_ok();
                let _ = ready.send(result);
                if initialized {
                    while let Ok(command) = receiver.recv() {
                        command(&mut state);
                    }
                }
            })
            .map_err(|error| Error::Worker(error.to_string()))?;
        let owner = thread.thread().id();
        // Dropping the JoinHandle detaches. Dropping a client must not join a
        // native driver call that may never return. The last sender closes the
        // queue; native state is then destroyed on its owning thread.
        drop(thread);
        startup.recv().map_err(|_| Error::Closed)??;
        Ok(Self { sender, owner })
    }

    pub fn call<R: Send + 'static>(
        &self,
        operation: impl FnOnce(&mut T) -> Result<R> + Send + 'static,
    ) -> Result<R> {
        if IN_CALLBACK.get() || thread::current().id() == self.owner {
            return Err(Error::CallbackThread);
        }
        let (reply, result) = mpsc::sync_channel(1);
        let command = Box::new(move |state: &mut T| {
            let _ = reply.send(operation(state));
        });
        match self.sender.try_send(command) {
            Ok(()) => result.recv().map_err(|_| Error::Closed)?,
            Err(TrySendError::Full(_)) => Err(Error::Busy),
            Err(TrySendError::Disconnected(_)) => Err(Error::Closed),
        }
    }
}

pub(super) struct CallbackScope(bool);

impl CallbackScope {
    pub fn enter() -> Self {
        Self(IN_CALLBACK.replace(true))
    }
}

impl Drop for CallbackScope {
    fn drop(&mut self) {
        IN_CALLBACK.set(self.0);
    }
}

struct Permit;

impl Drop for Permit {
    fn drop(&mut self) {
        CLIENTS.fetch_sub(1, Ordering::AcqRel);
    }
}

thread_local! {
    static IN_CALLBACK: Cell<bool> = const { Cell::new(false) };
}

static CLIENTS: AtomicUsize = AtomicUsize::new(0);
const CLIENT_LIMIT: usize = 32;

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;
    use std::time::Duration;

    struct Local {
        _not_send: Rc<()>,
        owner: ThreadId,
        destroyed: mpsc::Sender<ThreadId>,
    }

    impl Drop for Local {
        fn drop(&mut self) {
            let _ = self.destroyed.send(thread::current().id());
        }
    }

    #[test]
    fn non_send_resources_are_created_used_and_destroyed_on_the_owner() {
        let (destroyed, dropped) = mpsc::channel();
        let worker = Worker::spawn(
            move || Local {
                _not_send: Rc::new(()),
                owner: thread::current().id(),
                destroyed,
            },
            |_| Ok(()),
        )
        .unwrap();
        let owner = worker
            .call(|local| {
                assert_eq!(local.owner, thread::current().id());
                Ok(local.owner)
            })
            .unwrap();
        assert_ne!(owner, thread::current().id());
        drop(worker);
        assert_eq!(dropped.recv_timeout(Duration::from_secs(2)).unwrap(), owner);
    }

    #[test]
    fn callback_control_operations_are_rejected_before_queueing() {
        let worker = Worker::spawn(|| (), |_| Ok(())).unwrap();
        let scope = CallbackScope::enter();
        assert_eq!(worker.call(|_| Ok(())), Err(Error::CallbackThread));
        drop(scope);
        worker.call(|_| Ok(())).unwrap();
    }

    #[test]
    fn native_initialization_failure_is_reported_and_destroys_on_owner() {
        let (destroyed, dropped) = mpsc::channel();
        let result = Worker::spawn(
            move || Local {
                _not_send: Rc::new(()),
                owner: thread::current().id(),
                destroyed,
            },
            |_| Err(Error::Capability("test initialization failed")),
        );
        assert!(matches!(
            result,
            Err(Error::Capability("test initialization failed"))
        ));
        assert_ne!(
            dropped.recv_timeout(Duration::from_secs(2)).unwrap(),
            thread::current().id()
        );
    }
}
