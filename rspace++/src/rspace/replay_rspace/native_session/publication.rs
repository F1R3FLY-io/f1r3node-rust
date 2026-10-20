use super::{AtomicBool, Ordering};

pub(super) struct PublicationGuard<'a, F: FnOnce() = fn()> {
    closed: &'a AtomicBool,
    armed: bool,
    invalidate: Option<F>,
}

impl<'a> PublicationGuard<'a> {
    #[cfg(test)]
    pub(super) fn new(closed: &'a AtomicBool) -> Self {
        Self {
            closed,
            armed: true,
            invalidate: None,
        }
    }
}

impl<'a, F: FnOnce()> PublicationGuard<'a, F> {
    pub(super) fn with_invalidation(closed: &'a AtomicBool, invalidate: F) -> Self {
        Self {
            closed,
            armed: true,
            invalidate: Some(invalidate),
        }
    }

    pub(super) fn complete(mut self) { self.armed = false; }
}

impl<F: FnOnce()> Drop for PublicationGuard<'_, F> {
    fn drop(&mut self) {
        if self.armed {
            self.closed.store(true, Ordering::Release);
            if let Some(invalidate) = self.invalidate.take() {
                invalidate();
            }
        }
    }
}
