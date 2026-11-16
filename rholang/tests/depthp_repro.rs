// Minimal reproducer for the genesis-bootstrap H1 misclassification.
// Builds the simplest Par shape that matches the Registry.rho pattern
// where H1 flags `depthP` as trapped despite its appearance in receive
// bodies via compound channels like @(*map, *depthP).

use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::garbage::is_scope_trapped;

fn dump_locally_free(label: &str, bitset: &[u8]) {
    let bits: Vec<usize> = (0..bitset.len() * 8)
        .filter(|&i| models::rust::utils::bitset_contains(bitset, i))
        .collect();
    println!(
        "  {}.locally_free bytes={:?} bits_set={:?}",
        label, bitset, bits
    );
}

#[test]
fn minimal_depthp_pattern() {
    // Mimic the shape of Registry.rho's inner contract pattern:
    //   new depthP in {
    //     contract foo(ret, @depth) = {
    //       new map in {
    //         @(*map, *depthP)!!(depth) |
    //         for (@depth <- @(*map, *depthP)) { ret!(*map) }
    //       }
    //     }
    //   }
    // The outer `foo` channel is forgeable (quoted), so it stands in
    // for the `TreeHashMap` binder in the real inner new which is a
    // sibling — this test isolates depthP alone.
    let rho = r#"
new depthP in {
  contract @"foo"(ret, @depth) = {
    new map in {
      @(*map, *depthP)!!(depth) |
      for (@d <- @(*map, *depthP)) { ret!(*map) }
    }
  }
}"#;

    let par = Compiler::source_to_adt(rho).expect("compile");
    println!("=== top-level par ===");
    dump_locally_free("par", &par.locally_free);
    println!("  news count: {}", par.news.len());

    // outer new: `new depthP in { ... }`
    let outer = &par.news[0];
    println!("\n=== outer new ===");
    println!("  bind_count={} uri={}", outer.bind_count, outer.uri.len());
    dump_locally_free("outer_new", &outer.locally_free);

    let outer_body = outer.p.as_ref().unwrap();
    println!("\n=== outer new body (depthP is at DB 0) ===");
    dump_locally_free("outer_body", &outer_body.locally_free);
    println!("  receives count: {}", outer_body.receives.len());

    // The `contract` desugars to a persistent receive on @"foo".
    let r = &outer_body.receives[0];
    println!("\n=== receive (contract foo) ===");
    println!("  bind_count={} persistent={}", r.bind_count, r.persistent);
    dump_locally_free("receive", &r.locally_free);

    for (bi, b) in r.binds.iter().enumerate() {
        println!("\n  --- bind[{}] source ---", bi);
        let src = b.source.as_ref().unwrap();
        dump_locally_free("source", &src.locally_free);
        println!(
            "    source: sends={} receives={} news={} exprs={} matches={} bundles={}",
            src.sends.len(),
            src.receives.len(),
            src.news.len(),
            src.exprs.len(),
            src.matches.len(),
            src.bundles.len()
        );
    }

    println!("\n=== receive body ===");
    let body = r.body.as_ref().unwrap();
    dump_locally_free("body", &body.locally_free);
    // depthP at body DB index = outer idx 0 shifted by r.bind_count
    let depth_in_body = r.bind_count as usize;
    println!(
        "  depthP expected at DB index {} in body; bit_set={}",
        depth_in_body,
        models::rust::utils::bitset_contains(&body.locally_free, depth_in_body)
    );

    // Also dump the structure of the body:
    println!(
        "  body: sends={} receives={} news={} exprs={} matches={}",
        body.sends.len(),
        body.receives.len(),
        body.news.len(),
        body.exprs.len(),
        body.matches.len()
    );

    // Final: run is_scope_trapped and report verdict.
    let trapped = is_scope_trapped(0, outer_body);
    println!("\n=== VERDICT ===");
    println!(
        "  is_scope_trapped(0, outer_body) = {}  (expected: false)",
        trapped
    );
    if trapped {
        println!("  *** H1 MISCLASSIFIES depthP AS TRAPPED ***");
    }
}

#[test]
fn minimal_depthp_without_contract_desugar() {
    // Same semantics but spelled as a persistent `for` directly.
    let rho = r#"
new depthP in {
  for (ret, @depth <= @"foo") {
    new map in {
      @(*map, *depthP)!!(depth) |
      for (@d <- @(*map, *depthP)) { ret!(*map) }
    }
  }
}"#;

    let par = Compiler::source_to_adt(rho).expect("compile");
    let outer = &par.news[0];
    let outer_body = outer.p.as_ref().unwrap();
    let r = &outer_body.receives[0];
    let body = r.body.as_ref().unwrap();

    println!("--- without contract desugar ---");
    println!(
        "  outer_new.bind_count={} uri={}",
        outer.bind_count,
        outer.uri.len()
    );
    println!(
        "  receive.bind_count={} persistent={}",
        r.bind_count, r.persistent
    );
    dump_locally_free("receive", &r.locally_free);
    dump_locally_free("receive.body", &body.locally_free);
    let depth_in_body = r.bind_count as usize;
    println!(
        "  depthP bit in body @ {} = {}",
        depth_in_body,
        models::rust::utils::bitset_contains(&body.locally_free, depth_in_body)
    );

    let trapped = is_scope_trapped(0, outer_body);
    println!("  is_scope_trapped = {}  (expected: false)", trapped);
}
