//! Portable thread roles with macOS scheduling requests behind a policy seam.
//! QoS communicates urgency; physical core placement remains the OS's decision.
#[derive(Clone, Copy, Debug)]
pub enum Class {
    Interactive,
    Utility,
}
trait Policy {
    fn set_current(class: Class) -> bool;
}
struct Host;
#[cfg(target_os = "macos")]
impl Policy for Host {
    fn set_current(class: Class) -> bool {
        match class {
            Class::Interactive => horizon_threads::favour_current(),
            Class::Utility => horizon_threads::far_terrain_current(),
        }
    }
}
#[cfg(not(target_os = "macos"))]
impl Policy for Host {
    fn set_current(_class: Class) -> bool {
        false
    }
}

pub fn apply(role: &str, class: Class) {
    let accepted = Host::set_current(class);
    eprintln!("THREAD_QOS role={role} class={class:?} accepted={accepted}");
}
