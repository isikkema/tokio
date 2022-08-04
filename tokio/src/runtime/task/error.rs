use std::any::Any;
use std::fmt;
use std::io;

use super::Id;
use crate::util::SyncWrapper;
cfg_rt! {
    /// Task failed to execute to completion.
    pub struct JoinError {
        repr: Repr,
        id: Id,
    }
}

enum Repr {
    Cancelled,
    Panic(SyncWrapper<Box<dyn Any + Send + 'static>>),
}

impl JoinError {
    pub(crate) fn cancelled(id: Id) -> JoinError {
        JoinError {
            repr: Repr::Cancelled,
            id,
        }
    }

    pub(crate) fn panic(id: Id, err: Box<dyn Any + Send + 'static>) -> JoinError {
        JoinError {
            repr: Repr::Panic(SyncWrapper::new(err)),
            id,
        }
    }

    /// Returns true if the error was caused by the task being cancelled.
    pub fn is_cancelled(&self) -> bool {
        matches!(&self.repr, Repr::Cancelled)
    }

    /// Returns true if the error was caused by the task panicking.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::panic;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let err = tokio::spawn(async {
    ///         panic!("boom");
    ///     }).await.unwrap_err();
    ///
    ///     assert!(err.is_panic());
    /// }
    /// ```
    pub fn is_panic(&self) -> bool {
        matches!(&self.repr, Repr::Panic(_))
    }

    /// Consumes the join error, returning the object with which the task panicked.
    ///
    /// # Panics
    ///
    /// `into_panic()` panics if the `Error` does not represent the underlying
    /// task terminating with a panic. Use `is_panic` to check the error reason
    /// or `try_into_panic` for a variant that does not panic.
    ///
    /// # Examples
    ///
    /// ```should_panic
    /// use std::panic;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let err = tokio::spawn(async {
    ///         panic!("boom");
    ///     }).await.unwrap_err();
    ///
    ///     if err.is_panic() {
    ///         // Resume the panic on the main task
    ///         panic::resume_unwind(err.into_panic());
    ///     }
    /// }
    /// ```
    #[track_caller]
    pub fn into_panic(self) -> Box<dyn Any + Send + 'static> {
        self.try_into_panic()
            .expect("`JoinError` reason is not a panic.")
    }

    /// Consumes the join error, returning the object with which the task
    /// panicked if the task terminated due to a panic. Otherwise, `self` is
    /// returned.
    ///
    /// # Examples
    ///
    /// ```should_panic
    /// use std::panic;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let err = tokio::spawn(async {
    ///         panic!("boom");
    ///     }).await.unwrap_err();
    ///
    ///     if let Ok(reason) = err.try_into_panic() {
    ///         // Resume the panic on the main task
    ///         panic::resume_unwind(reason);
    ///     }
    /// }
    /// ```
    pub fn try_into_panic(self) -> Result<Box<dyn Any + Send + 'static>, JoinError> {
        match self.repr {
            Repr::Panic(p) => Ok(p.into_inner()),
            _ => Err(self),
        }
    }

    /// Returns a [task ID] that identifies the task which errored relative to
    /// other currently spawned tasks.
    ///
    /// **Note**: This is an [unstable API][unstable]. The public API of this type
    /// may break in 1.x releases. See [the documentation on unstable
    /// features][unstable] for details.
    ///
    /// [task ID]: crate::task::Id
    /// [unstable]: crate#unstable-features
    #[cfg(tokio_unstable)]
    #[cfg_attr(docsrs, doc(cfg(tokio_unstable)))]
    pub fn id(&self) -> Id {
        self.id.clone()
    }
}

impl fmt::Display for JoinError {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.repr {
            Repr::Cancelled => write!(fmt, "task {} was cancelled", self.id),
            Repr::Panic(_) => write!(fmt, "task {} panicked", self.id),
        }
    }
}

impl fmt::Debug for JoinError {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.repr {
            Repr::Cancelled => write!(fmt, "JoinError::Cancelled({:?})", self.id),
            Repr::Panic(_) => write!(fmt, "JoinError::Panic({:?}, ...)", self.id),
        }
    }
}

impl std::error::Error for JoinError {}

impl From<JoinError> for io::Error {
    fn from(src: JoinError) -> io::Error {
        io::Error::new(
            io::ErrorKind::Other,
            match src.repr {
                Repr::Cancelled => "task was cancelled",
                Repr::Panic(_) => "task panicked",
            },
        )
    }
}

cfg_rt! {
    /// Failed to spawn a task
    #[derive(Debug)]
    pub struct SpawnError {
        pub(crate) kind: SpawnErrorKind,
    }
}

#[derive(Debug)]
pub(crate) enum SpawnErrorKind {
    /// Pool is shutting down and the task was not scheduled
    Shutdown,
    /// There are no worker threads available to take the task
    /// and the OS failed to spawn a new one
    NoBlockingThreads(io::Error),
}

impl SpawnError {
    pub(crate) fn shutdown() -> Self {
        Self {
            kind: SpawnErrorKind::Shutdown,
        }
    }

    pub(crate) fn no_blocking_threads(e: io::Error) -> Self {
        Self {
            kind: SpawnErrorKind::NoBlockingThreads(e),
        }
    }

    /// Returns `true` if the error was caused by the runtime being shutdown.
    pub fn is_shutdown(&self) -> bool {
        matches!(&self.kind, SpawnErrorKind::Shutdown)
    }

    /// Returns `true` if the error was caused by the blocking
    /// threadpool unable to spawn additional threads
    pub fn is_no_blocking_threads(&self) -> bool {
        matches!(&self.kind, SpawnErrorKind::NoBlockingThreads(_))
    }
}

impl fmt::Display for SpawnError {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            SpawnErrorKind::Shutdown => fmt.write_str("runtime shutting down"),
            SpawnErrorKind::NoBlockingThreads(_) => {
                fmt.write_str("unable to spawn blocking thread")
            }
        }
    }
}

impl std::error::Error for SpawnError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.kind {
            SpawnErrorKind::Shutdown => None,
            SpawnErrorKind::NoBlockingThreads(e) => Some(e),
        }
    }
}

impl From<SpawnError> for io::Error {
    fn from(src: SpawnError) -> io::Error {
        match src.kind {
            SpawnErrorKind::Shutdown => {
                io::Error::new(io::ErrorKind::Other, "runtime shutting down")
            }
            SpawnErrorKind::NoBlockingThreads(e) => e,
        }
    }
}
