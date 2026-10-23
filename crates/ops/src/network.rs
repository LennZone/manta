//! Network reachability for the transfer manager.
//!
//! [`NetworkMonitor`] is a small trait so tests can simulate a network drop
//! ([`SimulatedNetwork`]). The app uses [`SystemNetwork`]: on macOS it wraps
//! `nw_path_monitor` of the Network framework (C API), which reports path
//! changes on a background dispatch queue; elsewhere it always reports
//! "online".

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Called with the new state (`true` = online) whenever it changes.
pub type NetworkListener = Box<dyn Fn(bool) + Send + Sync>;

/// Is the network reachable, and tell me when that changes.
pub trait NetworkMonitor: Send + Sync {
    /// Best guess whether a network path is available right now.
    fn is_online(&self) -> bool;
    /// Register a listener for changes. Listeners are called from a
    /// background thread and must return quickly.
    fn subscribe(&self, listener: NetworkListener);
}

/// Shared listener list used by the monitors below.
#[derive(Default)]
struct Listeners(Mutex<Vec<NetworkListener>>);

impl Listeners {
    fn add(&self, l: NetworkListener) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).push(l);
    }
    fn notify(&self, online: bool) {
        for l in self.0.lock().unwrap_or_else(|e| e.into_inner()).iter() {
            l(online);
        }
    }
}

/// A network that can be switched off and on by hand (tests, previews).
#[derive(Default)]
pub struct SimulatedNetwork {
    offline: AtomicBool,
    listeners: Listeners,
}

impl SimulatedNetwork {
    pub fn new(online: bool) -> Arc<SimulatedNetwork> {
        Arc::new(SimulatedNetwork {
            offline: AtomicBool::new(!online),
            listeners: Listeners::default(),
        })
    }

    /// Change the state; listeners are notified when it actually changes.
    pub fn set_online(&self, online: bool) {
        let was = !self.offline.swap(!online, Ordering::SeqCst);
        if was != online {
            self.listeners.notify(online);
        }
    }
}

impl NetworkMonitor for SimulatedNetwork {
    fn is_online(&self) -> bool {
        !self.offline.load(Ordering::SeqCst)
    }
    fn subscribe(&self, listener: NetworkListener) {
        self.listeners.add(listener);
    }
}

/// The system's network state (macOS: `NWPathMonitor`).
pub struct SystemNetwork {
    online: Arc<AtomicBool>,
    listeners: Arc<Listeners>,
}

impl SystemNetwork {
    /// Start monitoring (once per process is plenty; the monitor lives as
    /// long as the process).
    pub fn start() -> Arc<SystemNetwork> {
        let net = Arc::new(SystemNetwork {
            online: Arc::new(AtomicBool::new(true)),
            listeners: Arc::new(Listeners::default()),
        });
        #[cfg(target_os = "macos")]
        macos::start(net.online.clone(), net.listeners.clone());
        net
    }
}

impl NetworkMonitor for SystemNetwork {
    fn is_online(&self) -> bool {
        self.online.load(Ordering::SeqCst)
    }
    fn subscribe(&self, listener: NetworkListener) {
        self.listeners.add(listener);
    }
}

#[cfg(target_os = "macos")]
mod macos {
    //! `nw_path_monitor_*` from Network.framework through its C API. The
    //! update handler is an Objective-C block called on a private serial
    //! dispatch queue.

    use std::ffi::{c_char, c_void};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use block2::RcBlock;

    use super::Listeners;

    /// `nw_path_status_satisfied`.
    const NW_PATH_STATUS_SATISFIED: i32 = 1;

    #[link(name = "Network", kind = "framework")]
    extern "C" {
        fn nw_path_monitor_create() -> *mut c_void;
        fn nw_path_monitor_set_queue(monitor: *mut c_void, queue: *mut c_void);
        fn nw_path_monitor_set_update_handler(monitor: *mut c_void, handler: *mut c_void);
        fn nw_path_monitor_start(monitor: *mut c_void);
        fn nw_path_get_status(path: *mut c_void) -> i32;
    }

    extern "C" {
        fn dispatch_queue_create(label: *const c_char, attr: *mut c_void) -> *mut c_void;
    }

    pub(super) fn start(online: Arc<AtomicBool>, listeners: Arc<Listeners>) {
        // SAFETY: plain C calls; the monitor and queue are intentionally
        // never released (they live for the whole process), and the block
        // is copied by `nw_path_monitor_set_update_handler`.
        unsafe {
            let monitor = nw_path_monitor_create();
            if monitor.is_null() {
                log::warn!("network monitor unavailable; assuming online");
                return;
            }
            let queue =
                dispatch_queue_create(c"de.manta.network-monitor".as_ptr(), std::ptr::null_mut());
            nw_path_monitor_set_queue(monitor, queue);
            let block = RcBlock::new(move |path: *mut c_void| {
                let now = !path.is_null() && nw_path_get_status(path) == NW_PATH_STATUS_SATISFIED;
                let was = online.swap(now, Ordering::SeqCst);
                if was != now {
                    log::info!("network is {}", if now { "online" } else { "offline" });
                    listeners.notify(now);
                }
            });
            nw_path_monitor_set_update_handler(monitor, RcBlock::as_ptr(&block).cast());
            nw_path_monitor_start(monitor);
            // The handler holds its own copy of the block.
            drop(block);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn simulated_network_notifies_on_change_only() {
        let net = SimulatedNetwork::new(true);
        let seen = Arc::new(AtomicUsize::new(0));
        let s = seen.clone();
        net.subscribe(Box::new(move |_| {
            s.fetch_add(1, Ordering::SeqCst);
        }));
        net.set_online(true);
        assert_eq!(seen.load(Ordering::SeqCst), 0);
        net.set_online(false);
        assert!(!net.is_online());
        net.set_online(true);
        assert!(net.is_online());
        assert_eq!(seen.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn system_network_starts() {
        // Must not crash or block; on a build machine the network may be
        // either up or down.
        let net = SystemNetwork::start();
        let _ = net.is_online();
    }
}
