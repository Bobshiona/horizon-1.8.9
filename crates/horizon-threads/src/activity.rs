//! Scoped process activity for offscreen benchmarks; no persistent OS preference.
trait Policy {
    type Guard;
    fn begin() -> Self::Guard;
}
struct Host;
/// Keeps a user-requested benchmark active until this guard leaves scope.
/// It allows idle system sleep and does not change display-sleep preferences.
pub struct BenchmarkActivity {
    _guard: <Host as Policy>::Guard,
}
pub fn benchmark_activity() -> BenchmarkActivity {
    BenchmarkActivity {
        _guard: Host::begin(),
    }
}
#[cfg(target_os = "macos")]
impl Policy for Host {
    type Guard = macos::Guard;
    fn begin() -> Self::Guard {
        macos::Guard::begin()
    }
}
#[cfg(not(target_os = "macos"))]
impl Policy for Host {
    type Guard = ();
    fn begin() {}
}
#[cfg(target_os = "macos")]
mod macos {
    use objc2::{
        rc::Retained,
        runtime::{NSObjectProtocol, ProtocolObject},
    };
    use objc2_foundation::{NSActivityOptions, NSProcessInfo, NSString};
    pub struct Guard {
        process: Retained<NSProcessInfo>,
        token: Retained<ProtocolObject<dyn NSObjectProtocol>>,
    }
    impl Guard {
        pub fn begin() -> Self {
            let process = NSProcessInfo::processInfo();
            let token = process.beginActivityWithOptions_reason(
                NSActivityOptions::UserInitiatedAllowingIdleSystemSleep
                    | NSActivityOptions::LatencyCritical,
                &NSString::from_str("Offscreen render benchmark"),
            );
            Self { process, token }
        }
    }
    impl Drop for Guard {
        fn drop(&mut self) {
            // SAFETY: this is the exact token returned by this process object's
            // matching begin call. Ownership ensures it is ended exactly once.
            unsafe {
                self.process.endActivity(&self.token);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn benchmark_activity_begins_and_ends_with_its_guard() {
        // Two guards in sequence: each ends its own token when dropped.
        drop(super::benchmark_activity());
        let guard = super::benchmark_activity();
        drop(guard);
    }
}
