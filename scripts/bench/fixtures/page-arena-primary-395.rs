fn primary_mix(value: u64) -> u64 {
    let mut value = value.wrapping_add(0x9e3779b97f4a7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
    value ^ (value >> 31)
}

fn primary_trace_step(mut digest: u64, event: u64) -> u64 {
    for byte in event.to_le_bytes() {
        digest = (digest ^ u64::from(byte)).wrapping_mul(0x100000001b3);
    }
    digest
}

fn primary_located(seed: u64, ordinal: usize) -> CanonicalLocatedByte {
    let mixed = primary_mix(seed ^ ordinal as u64);
    CanonicalLocatedByte::new(
        (mixed % 251) as u8,
        CanonicalByteProvenance::new(395 ^ (seed as u32), ordinal),
    )
}

fn primary_export_drop(
    arena: &mut CanonicalBytesArena<'_>,
    handle: CanonicalBytesHandle,
    expected_last: CanonicalLocatedByte,
) {
    let view = arena
        .retained_source_window(handle)
        .expect("the exported handle has no retained source window");
    assert_eq!(view.len(), handle.len());
    assert_eq!(view.byte_at(handle.len() - 1), Some(expected_last.byte));
    assert_eq!(
        view.provenance_at(handle.len() - 1),
        Some(expected_last.provenance)
    );
    drop(view);
}

#[test]
fn page_arena_primary_395_seed_capacity() {
    let seed_text =
        std::env::var("PAGE_ARENA_PRIMARY_SEED").expect("reading PAGE_ARENA_PRIMARY_SEED failed");
    let seed = u64::from_str_radix(seed_text.strip_prefix("0x").expect("seed lacks 0x"), 16)
        .expect("parsing the seed as a hexadecimal u64 failed");
    let capacity = std::env::var("PAGE_ARENA_PRIMARY_CAPACITY")
        .expect("reading PAGE_ARENA_PRIMARY_CAPACITY failed")
        .parse::<usize>()
        .expect("parsing the capacity as a usize failed");
    assert_eq!(capacity, DEFAULT_PAGE_BYTES);
    assert!([32, 64, 128, 256].contains(&capacity));
    for workload in [
        "linear",
        "boundary_forked",
        "export_drop_append",
        "active_retained_view",
    ] {
        primary_run_cell(seed, &seed_text, capacity, workload);
    }
}

fn primary_run_cell(seed: u64, seed_text: &str, capacity: usize, workload: &str) {
    const PAYLOAD: usize = 65_536;
    let mut arena = CanonicalBytesArena::new();
    let mut trunk = arena.empty();
    let mut trunk_oracle = Vec::with_capacity(PAYLOAD);
    let mut branches = Vec::new();
    let mut active_views = Vec::new();
    let mut ordinal = 0usize;
    let mut exports_dropped = 0usize;
    let mut views_dropped = 0usize;
    let mut digest = primary_trace_step(0xcbf29ce484222325, seed);
    let fork_points = (1..=15).map(|point| point * 4096).collect::<Vec<_>>();

    while ordinal < PAYLOAD {
        if workload == "boundary_forked" && fork_points.contains(&ordinal) {
            let boundary = trunk_oracle.len() / capacity * capacity;
            let prefix_len = boundary - 1;
            let mut branch = arena
                .slice(trunk, 0..prefix_len)
                .expect("slicing the trunk prefix for a branch failed");
            let mut branch_oracle = trunk_oracle[..prefix_len].to_vec();
            let branch_len = 1 + primary_mix(seed ^ ordinal as u64) as usize % capacity;
            digest = primary_trace_step(digest, 0xf000000000000000 | prefix_len as u64);
            for _ in 0..branch_len.min(PAYLOAD - ordinal) {
                let value = primary_located(seed, ordinal);
                branch = arena
                    .append(branch, value)
                    .expect("appending to the branch failed");
                branch_oracle.push(value);
                digest = primary_trace_step(digest, 0xb000000000000000 | ordinal as u64);
                primary_export_drop(&mut arena, branch, value);
                exports_dropped += 1;
                ordinal += 1;
            }
            branches.push((branch, branch_oracle));
            continue;
        }

        let value = primary_located(seed, ordinal);
        trunk = arena
            .append(trunk, value)
            .expect("appending to the trunk failed");
        trunk_oracle.push(value);
        digest = primary_trace_step(digest, 0xa000000000000000 | ordinal as u64);
        let interval = match workload {
            "export_drop_append" => {
                1 + primary_mix(seed.rotate_left(7) ^ ordinal as u64) as usize % 4
            }
            "boundary_forked" => {
                1 + primary_mix(seed.rotate_left(13) ^ ordinal as u64) as usize % capacity
            }
            _ => 1 + primary_mix(seed.rotate_left(19) ^ ordinal as u64) as usize % (capacity / 2),
        };
        if ordinal % interval == 0 {
            primary_export_drop(&mut arena, trunk, value);
            exports_dropped += 1;
            digest = primary_trace_step(digest, 0xe000000000000000 | ordinal as u64);
        }
        if workload == "active_retained_view" && (ordinal + 1) % 8192 == 8191 {
            let view = arena
                .retained_source_window(trunk)
                .expect("the trunk has no retained source window");
            assert_eq!(view.len(), trunk_oracle.len());
            active_views.push((view, trunk_oracle.clone()));
            digest = primary_trace_step(digest, 0xc000000000000000 | ordinal as u64);
            if active_views.len() > 4 {
                drop(active_views.remove(0));
                views_dropped += 1;
            }
        }
        ordinal += 1;
    }

    assert_eq!(ordinal, PAYLOAD);
    assert_eq!(arena.located_bytes(trunk).collect::<Vec<_>>(), trunk_oracle);
    for (branch, oracle) in &branches {
        assert_eq!(arena.located_bytes(*branch).collect::<Vec<_>>(), *oracle);
    }
    for (view, oracle) in &active_views {
        assert_eq!(view.located_bytes().collect::<Vec<_>>(), *oracle);
    }
    let final_view = arena
        .retained_source_window(trunk)
        .expect("the final trunk has no retained source window");
    assert_eq!(final_view.located_bytes().collect::<Vec<_>>(), trunk_oracle);
    drop(final_view);
    exports_dropped += 1;
    if workload == "active_retained_view" {
        drop(active_views.remove(0));
        drop(active_views.remove(0));
        views_dropped += 2;
        assert_eq!(active_views.len(), 2);
    } else {
        assert!(active_views.is_empty());
    }
    let active_roots = active_views
        .iter()
        .map(|(view, _)| view.clone())
        .collect::<Vec<_>>();
    let cache_only_nodes = arena.primary_cache_only_nodes(&active_roots);
    assert_eq!(
        arena.backend.byte_pages.len(),
        arena.backend.provenance_pages.len()
    );
    let mut sealed_page_pairs = 0usize;
    for (bytes, provenance) in arena
        .backend
        .byte_pages
        .iter()
        .zip(&arena.backend.provenance_pages)
    {
        let byte_sealed = std::sync::Arc::strong_count(bytes) > 1;
        let provenance_sealed = std::sync::Arc::strong_count(provenance) > 1;
        assert_eq!(byte_sealed, provenance_sealed);
        sealed_page_pairs += usize::from(byte_sealed);
    }
    println!(
        "PAGE_ARENA_PRIMARY_395_V1\tseed={seed_text}\tcapacity={capacity}\tworkload={workload}\tpayload_bytes={PAYLOAD}\ttrace_digest={digest:016x}\tsealed_page_pairs={sealed_page_pairs}\tphysical_page_pairs={}\tcache_only_source_nodes={cache_only_nodes}\tbuilt_source_nodes={}\texports_dropped={exports_dropped}\tviews_dropped={views_dropped}\tactive_views={}\tbranches={}",
        arena.backend.byte_pages.len(),
        arena.stats().source_cache_nodes,
        active_views.len(),
        branches.len(),
    );
}
