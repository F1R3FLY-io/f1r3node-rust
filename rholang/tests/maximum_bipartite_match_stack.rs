use rholang::rust::interpreter::matcher::maximum_bipartite_match::MaximumBipartiteMatch;

#[test]
fn augmenting_paths_preserve_deterministic_reassignment() {
    let mut matcher = MaximumBipartiteMatch::new_with_cache_policy(
        Box::new(
            |pattern: &usize, target: &usize| match (*pattern, *target) {
                (0, 0) | (0, 1) | (1, 0) => Some((*pattern, *target)),
                _ => None,
            },
        ),
        Box::new(|_| true),
    );

    let matches = matcher
        .find_matches(vec![0, 1], vec![0, 1])
        .expect("the second pattern displaces the first onto its alternative");
    assert_eq!(matches, vec![(0, 1, (1, 0)), (1, 0, (0, 1))]);
}

#[test]
fn a_deep_augmenting_chain_uses_heap_frames() {
    const WIDTH: usize = 4_096;

    std::thread::Builder::new()
        .name("bipartite-pda-small-stack".to_string())
        .stack_size(64 * 1024)
        .spawn(|| {
            let mut matcher = MaximumBipartiteMatch::new_with_cache_policy(
                Box::new(|pattern: &usize, target: &usize| {
                    let matches = if *pattern + 1 == WIDTH {
                        *target == 0
                    } else {
                        target == pattern || *target == *pattern + 1
                    };
                    matches.then_some(())
                }),
                Box::new(|_| true),
            );

            let patterns = (0..WIDTH).collect::<Vec<_>>();
            let targets = (0..WIDTH).collect::<Vec<_>>();
            let matches = matcher
                .find_matches(patterns, targets)
                .expect("the final free target terminates the augmenting chain");
            assert_eq!(matches.len(), WIDTH);
        })
        .expect("spawn the deliberately small-stack witness")
        .join()
        .expect("iterative matching must not overflow the small native stack");
}
