# catalog casper 034

[Inventory](inventory.md) | [PlantUML](catalog-casper-034.puml)

![catalog-casper-034](catalog-casper-034.svg)

## Cardinality and Size

[Method, units, limits, and runtime evidence](sizing.md). These are declaration-level assessments, not measured live counts.

### BundleEntry

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `logical_name` | 0..n UTF-8 bytes | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `canon_path` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `kind` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `mode` | 0..n UTF-8 bytes | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `consensus_mode` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### ProofOfStake

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `minimum_bond` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `maximum_bond` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `validators` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: Follow the referenced declaration or external implementation. No allocation bound established. |
| `epoch_length` | 1 scalar | 4 B | None |
| `quarantine_length` | 1 scalar | 4 B | None |
| `number_of_active_validators` | 1 scalar | 4 B | None |
| `fault_tolerance_threshold_ppm` | 1 scalar | 8 B | None |
| `max_parent_depth` | 1 scalar | 4 B | None |
| `deploy_lifespan` | 1 scalar | 8 B | None |
| `min_phlo_price` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `max_phlo_limit` | 1 scalar | 8 B | None |
| `pos_multi_sig_public_keys` | 0..n elements | 24 B. Vec header on the generator target | capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `pos_multi_sig_quorum` | 1 scalar | 4 B | None |

### REGISTRY_PUB_KEY

Cardinality: One slot per linked process instance. Lazy payload can be uninitialized.

Size: Declared slot or lazy wrapper layout plus initialized owned allocations.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `value` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### VERSIONED_REGISTRY_PUB_KEY

Cardinality: One slot per linked process instance. Lazy payload can be uninitialized.

Size: Declared slot or lazy wrapper layout plus initialized owned allocations.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `value` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### LIST_OPS_PUB_KEY

Cardinality: One slot per linked process instance. Lazy payload can be uninitialized.

Size: Declared slot or lazy wrapper layout plus initialized owned allocations.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `value` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### EITHER_PUB_KEY

Cardinality: One slot per linked process instance. Lazy payload can be uninitialized.

Size: Declared slot or lazy wrapper layout plus initialized owned allocations.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `value` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### NON_NEGATIVE_NUMBER_PUB_KEY

Cardinality: One slot per linked process instance. Lazy payload can be uninitialized.

Size: Declared slot or lazy wrapper layout plus initialized owned allocations.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `value` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### MAKE_MINT_PUB_KEY

Cardinality: One slot per linked process instance. Lazy payload can be uninitialized.

Size: Declared slot or lazy wrapper layout plus initialized owned allocations.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `value` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

## Cross-Page References

- `BundleEntry: consensus_mode -> BundleConsensusMode [catalog-casper-033.svg]`
- `BundleEntry: kind -> BundleEntryKind [catalog-casper-033.svg]`
- `ProofOfStake: maximum_bond -> TokenAmount [catalog-shared-009.svg]`
- `ProofOfStake: min_phlo_price -> TokenAmount [catalog-shared-009.svg]`
- `ProofOfStake: minimum_bond -> TokenAmount [catalog-shared-009.svg]`
