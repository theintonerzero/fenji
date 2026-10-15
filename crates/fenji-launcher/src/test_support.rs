//! What the crate's tests share.

/// Held by every test that writes an executable or starts a process. A child forked while another
/// test still has its fresh script open for writing would make that script "Text file busy"
/// (ETXTBSY) to exec, so the two never overlap.
static SPAWN: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(crate) fn spawning() -> std::sync::MutexGuard<'static, ()> {
    SPAWN.lock().unwrap_or_else(|e| e.into_inner())
}
