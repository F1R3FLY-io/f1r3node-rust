const MIN_VALIDATION_WIDTH: usize = 3;

fn derive_validation_width(cpu_cores: usize) -> usize {
    cpu_cores
        .saturating_sub(2)
        .clamp(MIN_VALIDATION_WIDTH, tokio::sync::Semaphore::MAX_PERMITS - 2)
}

/// How many blocks block ingest validates at once: every core but two, at
/// least three. Replays use this many permits plus two.
pub(crate) fn resolve_validation_width() -> usize {
    let cpu_cores = std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or_else(|error| {
            tracing::warn!(%error, "Could not determine CPU capacity; using the validation floor");
            1
        });
    let width = derive_validation_width(cpu_cores);
    tracing::info!(
        validation_width = width,
        replay_permits = width + 2,
        cpu_cores,
        "Block validation concurrency resolved"
    );
    width
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_width_uses_the_cpu_capacity() {
        for (cores, width) in [(6, 4), (8, 6), (16, 14), (32, 30)] {
            assert_eq!(derive_validation_width(cores), width);
        }
    }

    #[test]
    fn small_cpu_limits_keep_the_validation_floor() {
        for cores in 0..=5 {
            assert_eq!(derive_validation_width(cores), 3);
        }
    }

    #[test]
    fn the_replay_reserve_fits_the_semaphore_at_extreme_inputs() {
        let width = derive_validation_width(usize::MAX);
        assert_eq!(width, tokio::sync::Semaphore::MAX_PERMITS - 2);
        assert!(width.checked_add(2).is_some());
    }
}
