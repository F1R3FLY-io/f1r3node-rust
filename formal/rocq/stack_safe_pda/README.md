# Stack-safe PDA and EPathMap proof kernel

This Rocq development proves the algorithmic obligations shared by the generated Rust machines.

- `compile_run_equivalence` proves that an explicit post-order instruction/value-stack machine equals the mutually recursive fold for every finite typed tree and every algebra. Clone, equality, hashing, ordering, debug formatting, bincode/protobuf encoding, encoded length, clear, and teardown are instances of that theorem.
- `decoder_machine_equivalent_to_recursive_rebuild` instantiates the same machine with the tree-building algebra and proves exact reconstruction.
- `compiled_program_has_one_instruction_per_node` proves linear work and one completion instruction per node.
- `EPathMap.v` proves neutral-empty mode laws, set lattice laws, exact map-value conflict behavior, and the prefix-restriction subset law.
- `EPM1.v` models the actual envelope rather than an opaque suffix: canonical base-128 length fields, the length-framed ACTree03 arena, the ordered count-delimited generated-PDA value table, exact end-of-input checks, mode well-formedness, and ordinal-to-value association. It proves framing round trips, malformed/truncated/trailing rejection, ordinal uniqueness/range, and equality of generated-PDA versus recursive value bodies.

The theorem kernel is schema-parametric. The generated Rust equivalence manifest binds each concrete schema surface to a theorem and an independent recursive-oracle differential. The manifest test is the coverage proof: a new generated traversal cannot remain converted without a theorem/differential row. PathMap still owns the internal ACTree03 byte grammar and parser. `EPM1.v` preserves that arena byte-for-byte and proves the surrounding framing and the association law parametrically over the executable ordinal extractor; PathMap topology parsing remains an executable-library obligation checked by the Rust differential and malformed-input suites.

The development contains no axioms, admitted goals, or opaque assumptions.
