pub fn reopen_stale_receipt(
    received: &mut bool,
    in_casper_buffer: bool,
    timestamp: &mut u64,
    initial_timestamp: u64,
    current_time: u64,
    stale_lifetime: u64,
) -> bool {
    if !*received
        || in_casper_buffer
        || current_time.saturating_sub(initial_timestamp) <= stale_lifetime
    {
        return false;
    }
    *received = false;
    *timestamp = current_time;
    true
}
