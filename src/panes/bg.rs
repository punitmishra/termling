//! A tiny background refresher shared by the local panes.
//!
//! Local panes collect their data by shelling out (`git …`, `uptime`/`sysctl`/
//! `vm_stat`) or by reading a file (the buddy model). Doing that inline on the
//! render loop can visibly stall on a large repo, so [`Bg<T>`] runs the
//! collection on a short-lived background thread and hands the result back
//! through a shared slot the pane reads on a later tick. Only one collection is
//! ever in flight at a time. It uses only std `thread`/`sync` — no new deps and
//! no network — so the zero-dependency-by-default guarantee holds.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// A single-slot, single-in-flight background collector for a pane's data.
pub struct Bg<T> {
    /// The most recent completed value, waiting to be [`take`](Bg::take)n.
    slot: Arc<Mutex<Option<T>>>,
    /// True while a collection thread is running — guards against a second one.
    in_flight: Arc<AtomicBool>,
}

impl<T: Send + 'static> Bg<T> {
    pub fn new() -> Bg<T> {
        Bg {
            slot: Arc::new(Mutex::new(None)),
            in_flight: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Start a collection on a background thread, unless one is already running.
    /// Returns `true` when a fresh thread was spawned.
    pub fn trigger<F>(&self, f: F) -> bool
    where
        F: FnOnce() -> T + Send + 'static,
    {
        // Claim the in-flight slot; bail if a collection is already running.
        if self.in_flight.swap(true, Ordering::AcqRel) {
            return false;
        }
        let slot = Arc::clone(&self.slot);
        let in_flight = Arc::clone(&self.in_flight);
        let spawned = std::thread::Builder::new()
            .name("termling-pane".into())
            .spawn(move || {
                let value = f();
                if let Ok(mut g) = slot.lock() {
                    *g = Some(value);
                }
                in_flight.store(false, Ordering::Release);
            })
            .is_ok();
        if !spawned {
            // Couldn't spawn — release the claim so a later tick can retry.
            self.in_flight.store(false, Ordering::Release);
        }
        spawned
    }

    /// Take the most recent completed value, leaving the slot empty.
    pub fn take(&self) -> Option<T> {
        self.slot.lock().ok().and_then(|mut g| g.take())
    }
}

impl<T: Send + 'static> Default for Bg<T> {
    fn default() -> Self {
        Bg::new()
    }
}
