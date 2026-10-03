use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::error::{LensError, Result};

/// Cooperative cancellation shared between the overlay and recognition work.
/// Cancelled when the user presses Escape, runs an action, or starts a new
/// selection.
#[derive(Clone, Debug, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    /// Returns `Err(Cancelled)` if cancelled; sprinkle through long loops.
    pub fn check(&self) -> Result<()> {
        if self.is_cancelled() {
            Err(LensError::Cancelled)
        } else {
            Ok(())
        }
    }
}
