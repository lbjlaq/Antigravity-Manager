use std::sync::atomic::{AtomicBool, Ordering};

/// Armed only for this process when a login-item launch should skip the
/// frontend's automatic first `show_main_window`. Explicit opens clear it.
static QUIET_LAUNCH: AtomicBool = AtomicBool::new(false);

/// Lightweight entry is deferred to `RunEvent::Ready` so destroying the last
/// window cannot exit the process before the exit handler is installed.
static ENTER_LIGHTWEIGHT_ON_READY: AtomicBool = AtomicBool::new(false);

pub fn arm() {
    QUIET_LAUNCH.store(true, Ordering::SeqCst);
}

pub fn clear() {
    QUIET_LAUNCH.store(false, Ordering::SeqCst);
}

/// Returns true once, for the automatic first show of a quiet login launch.
pub fn take() -> bool {
    QUIET_LAUNCH.swap(false, Ordering::SeqCst)
}

pub fn request_lightweight_on_ready() {
    ENTER_LIGHTWEIGHT_ON_READY.store(true, Ordering::SeqCst);
}

pub fn take_lightweight_on_ready() -> bool {
    ENTER_LIGHTWEIGHT_ON_READY.swap(false, Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quiet_launch_latch_is_one_shot() {
        clear();
        ENTER_LIGHTWEIGHT_ON_READY.store(false, Ordering::SeqCst);

        assert!(!take());
        arm();
        assert!(take());
        assert!(!take());

        arm();
        clear();
        assert!(!take());

        assert!(!take_lightweight_on_ready());
        request_lightweight_on_ready();
        assert!(take_lightweight_on_ready());
        assert!(!take_lightweight_on_ready());
    }
}
