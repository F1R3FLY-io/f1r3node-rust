# Inline Binary Hashes for Runtime Caches

Status: draft proposal. Candidate counts and aggregate savings are not yet measured.
Date: 2026-11-25.

## Objective

Remove separate heap allocations for fixed 32-byte hashes in selected runtime caches.
Keep existing persisted keys, peer messages, hash inputs, and public API text unchanged.
Use a private cache-key type before considering a global hash-type change.

The [memory evaluation contract](README.md#memory-evaluation-contract) applies.
This proposal is independent of [sparse radix nodes](memory-sparse-radix-cache.md).
Evaluate each change separately before measuring their combined effect.

## Current Representations

| Owner | Representation | Candidate scope |
| --- | --- | --- |
| [RSpace hash wrapper](../../rspace++/src/rspace/hashing/blake2b256_hash.rs) | `Blake2b256Hash(Vec<u8>)` with derived serde and cloning | Later, only after a compatibility audit |
| [Radix caches](../../rspace++/src/rspace/history/radix_tree.rs) | `ByteVector` keys in read and write maps | First prototype at validated hash boundaries |
| [Queue block tracking](../../deploy-queue/src/engine.rs) | String keys in `created` and `learned` | Optional later prototype for validated block hashes |
| [Event payloads](../../shared/src/rust/shared/f1r3fly_event.rs) | Text hashes, signatures, and public keys | Deferred until ownership and encoding costs are measured |

The global RSpace wrapper does not enforce its nominal length in every constructor.
Its derived `Default` contains an empty vector.
Some callers and test generators can therefore represent values other than 32-byte hashes.
A direct replacement with `[u8; 32]` would change this accepted domain.

The [queue record](../../deploy-queue/src/record.rs) already uses binary signature vectors.
Do not report those records as hexadecimal strings or assume every signature has a 32-byte length.
This proposal does not change signature formats or token amount representations.

## Phase One: Private Cache Keys

Introduce a private fixed-length key around `[u8; 32]` at one cache boundary.
Construct it only from values whose length is already known or checked.
Keep the current byte-oriented store interface outside that boundary.
Convert to existing store keys only when an operation actually accesses storage.

Start with the radix read cache and its generated node hashes.
Measure the write cache separately because it also owns encoded node bytes.
Do not replace shared `ByteVector` aliases throughout the repository.

The cache wrapper should borrow input bytes for lookup without constructing a temporary vector.
Its equality and Rust hashing implementation must agree with the chosen borrowed lookup interface.
Tests must verify actual hits, misses, and removals, not only equality between key values.
Do not rely on a particular standard-library hash digest being stable between compiler versions.

Malformed or empty inputs must not silently become a zero hash.
If a legacy path accepts such input, preserve its behavior through an uncached path or an explicit compatibility representation.
Do not redefine missing-root tolerance, storage errors, or corruption handling as part of the optimization.
Keep constructor validation changes in a separately reviewed change if they are necessary.

The initial private type needs no serde implementation.
That restriction prevents it from becoming a different persisted representation accidentally.
The [existing canonical serializer](../../rspace++/src/rspace/serializers/serializers.rs) remains unchanged.

## Memory Model

The [documented 64-bit layout](../erd/memory/sizing.md) uses a 24-byte vector or string header.
Assume exact payload capacity for these illustrative calculations:

| Representation of one 32-byte hash | Header and payload bytes | Exclusions |
| --- | --- | --- |
| `Vec<u8>` | `24 + 32 = 56` | Allocator metadata, rounding, and owner padding |
| Hexadecimal `String` | `24 + 64 = 88` | Allocator metadata, rounding, and owner padding |
| `[u8; 32]` | `32` | Owner padding and map layout |

Replacing a separately owned exact-capacity vector gives a nominal 24-byte saving per hash.
Replacing exact-capacity hexadecimal text gives a nominal 56-byte saving.
These are layout comparisons, not measured process savings.

An inline key makes a map bucket larger than a vector header.
That increases spare-bucket cost even while it removes a payload allocation.
Measure map capacity, alignment, load factor, allocation size classes, and clone behavior before calculating net savings.

For a population of `N` independently allocated vectors, `24 * N` is only a preliminary estimate.
Shared byte buffers, retained spare capacity, and repeated conversion allocations change the result.
Existing reference-counted byte types can already share their payload and need a separate comparison.

## Phase Two: Optional Text Conversion

The queue retains block hashes in its `created` and `learned` maps.
A later prototype can decode validated block hashes once and retain their binary form.
Keep hexadecimal output at the existing JSON boundary.
Benchmark decoding cost against reduced retention and cloning.

Do not change signature IDs or public-key lengths by analogy with block hashes.
Keep unknown, malformed, and mixed-case textual input behavior explicit.
The current parser can accept arbitrary strings, so stricter validation is not automatically a compatible memory change.
Use a compatibility path or obtain approval for a validation change.

Keep decimal token amounts as required by the external contract.
Converting them into floating-point numbers would lose information and is outside this proposal.

## Deferred Global Migration

A later replacement of `Blake2b256Hash(Vec<u8>)` needs a full serialization and constructor audit.
Derived serde for a fixed array does not preserve the existing vector encoding automatically.
The current bincode vector format carries a length prefix, while an array representation can omit it.
That difference can change storage keys and consensus hashes even when the 32 payload bytes match.

If a global migration proceeds, retain an explicit legacy serialization adapter.
Prove byte equality with the current wrapper for every supported valid value.
Account separately for empty defaults, nonstandard lengths, arbitrary test generators, JSON representation, and embedded hash fields.
Do not declare those cases impossible without tracing their constructors and consumers.

Keep [stable hash inputs](../../rspace++/src/rspace/hashing/stable_hash_provider.rs), ordering, and equality unchanged.
Audit any use of Rust `Hash` that affects partitioning or collection behavior.
This larger migration is not required to deliver the private cache-key improvement.

## Validation

| Test group | Required evidence |
| --- | --- |
| Construction | Known 32-byte values round-trip exactly, and other lengths retain explicit legacy behavior |
| Lookup | Owned and borrowed keys agree for hits, misses, insertion, replacement, and removal |
| Cache behavior | Limits, eviction, clearing, and concurrent use preserve baseline outcomes |
| Storage boundary | Reads and writes use the same physical key bytes as before |
| Canonical identity | Leaf, channel, trace, and root hashes remain unchanged for identical inputs |
| Text boundary | Output encodings remain exact, and accepted textual forms retain reviewed behavior |
| Global adapter, if attempted | Legacy serialization fixtures remain byte-exact, including embedded values and constructor edge cases |
| End to end | Restart, state import, replay, merge recovery, counter updates, and finalization retain baseline results |

The global-adapter row applies only if a later change expands beyond private caches.
No such expansion should happen implicitly during the initial implementation.
Apply the existing [correctness evidence requirements](../cbc-verification-tiers.md) to the affected runtime code.

## Benchmark and Delivery

Measure key counts, unique allocations, map capacity, clone frequency, and conversion allocations for each candidate owner.
Compare lookup throughput, cache misses, allocator traffic, and retained heap under the same workload.
Keep node and queue process results separate.
Record the combined result only after measuring each representation change independently.

1. Inventory concrete hash owners and their construction paths.
2. Prototype one private cache-key conversion without serde or storage changes.
3. Run lookup, canonical-byte, and restart regressions.
4. Measure retained memory and lookup latency at representative capacities.
5. Expand only to owners with demonstrated net benefit.

Rollback requires a binary change, not a database migration.
Do not accept whole-node savings claims based only on `size_of` or historical disk row counts.
