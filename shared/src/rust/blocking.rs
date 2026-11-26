/// Runs `f` with the current tokio worker handed off to another thread, so a
/// long synchronous pass does not stall the tasks queued on that worker. Outside
/// a multi-thread runtime it runs `f` inline.
pub fn in_blocking<T>(f: impl FnOnce() -> T) -> T {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => {
            tokio::task::block_in_place(f)
        }
        _ => f(),
    }
}
