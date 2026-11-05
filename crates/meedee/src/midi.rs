//! Native MIDI discovery, input, output and scoped connections.

pub use crate::input::{Input, InputErrors, Recording, RecordingOptions};
#[doc(hidden)]
pub use crate::output::{select_port_index, OutputState};
pub use crate::output::{Output, OutputValue, PlaybackOptions};
pub use crate::transport::{compiled_backends, Backend, Port, Protocol, TimestampMode};

/// A high-level operation failure, retaining the native category and cause.
#[derive(Debug)]
pub enum Error {
    Init(crate::transport::Error),
    Connect(crate::transport::Error),
    Send(crate::transport::Error),
    Transport(crate::transport::Error),
    Io(std::io::Error),
    Closed,
    Cancelled,
    /// A retained failure also available through Input::failure().
    Shared(std::sync::Arc<Error>),
    Value(crate::messages::ValueError),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Init(e) | Self::Connect(e) | Self::Send(e) | Self::Transport(e) => e.fmt(f),
            Self::Io(e) => e.fmt(f),
            Self::Closed => f.write_str("input is closed"),
            Self::Cancelled => f.write_str("MIDI receive cancelled"),
            Self::Shared(e) => e.fmt(f),
            Self::Value(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(match self {
            Self::Init(e) | Self::Connect(e) | Self::Send(e) | Self::Transport(e) => e,
            Self::Io(e) => e,
            Self::Closed | Self::Cancelled => return None,
            Self::Shared(e) => e.as_ref(),
            Self::Value(e) => e,
        })
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        if error.get_ref().is_some_and(|e| e.is::<Self>()) {
            *error
                .into_inner()
                .expect("checked MIDI cause")
                .downcast::<Self>()
                .expect("checked MIDI type")
        } else if error
            .get_ref()
            .is_some_and(|e| e.is::<crate::transport::Error>())
        {
            let cause = error.into_inner().expect("checked native cause");
            Self::Transport(
                *cause
                    .downcast::<crate::transport::Error>()
                    .expect("checked native type"),
            )
        } else if error
            .get_ref()
            .is_some_and(|e| e.is::<crate::messages::ValueError>())
        {
            Self::Value(
                *error
                    .into_inner()
                    .expect("checked value cause")
                    .downcast::<crate::messages::ValueError>()
                    .expect("checked value type"),
            )
        } else {
            Self::Io(error)
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

impl From<Error> for std::io::Error {
    fn from(error: Error) -> Self {
        match error {
            Error::Io(e) => e,
            Error::Value(e) => std::io::Error::new(std::io::ErrorKind::InvalidInput, e),
            other => std::io::Error::other(other),
        }
    }
}

impl From<crate::messages::ValueError> for Error {
    fn from(error: crate::messages::ValueError) -> Self {
        Self::Value(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error as StdError;

    #[test]
    fn conversion_retains_operation_category_and_native_cause() {
        let error = Error::Send(crate::transport::Error::CompoundSend {
            completed_messages: Some(1),
            total_messages: 3,
            cause: Box::new(crate::transport::Error::Resource("queue full")),
        });
        let restored = Error::from(std::io::Error::from(error));
        assert!(matches!(
            restored,
            Error::Send(crate::transport::Error::CompoundSend {
                completed_messages: Some(1),
                total_messages: 3,
                ..
            })
        ));
        assert_eq!(
            restored.source().unwrap().source().unwrap().to_string(),
            "queue full"
        );
        let error = Error::from(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "denied",
        ));
        assert!(
            matches!(error, Error::Io(ref e) if e.kind() == std::io::ErrorKind::PermissionDenied)
        );
    }
}
