use std::cell::Cell;

thread_local! {
    static STABLE_MATCHING: Cell<bool> = const { Cell::new(false) };
}

pub fn stable_matching() -> bool { STABLE_MATCHING.with(Cell::get) }

pub fn enable_stable_matching_for_current_thread() { STABLE_MATCHING.with(|flag| flag.set(true)); }
