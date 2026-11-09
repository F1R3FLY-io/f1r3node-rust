# Finalized Bond Cache for Deploy Admission

## Change

Deploy admission rejects a local validator outside the stored finalized committee without building a Casper snapshot.

## Design

The Casper instance stores one cache entry with the last finalized block (LFB) hash and local membership in its metadata `weight_map`. Construction reads the stored DAG to initialize the entry. A successful finalization reads the stored LFB metadata and replaces the entry. A failed finalization does not publish a candidate entry.

Deploy admission already reads the DAG for deploy expiry. It uses the cached value when the cached hash equals that DAG's LFB hash. If the hashes differ, admission checks the LFB metadata in that same DAG. Missing metadata returns an error. This rule covers an LFB update that persists before a finalization error and an overlapping finalizer that publishes an older entry.

The proposer continues to use the selected parent's committee. Admission uses the LFB committee. A bond change can make those committees differ during the finality gap.

## Specification

- A node without a validator key rejects a deploy through the existing read-only or key check.
- A stored LFB metadata error fails construction.
- A cached entry is valid only for its LFB hash.
- A cache mismatch reads the stored LFB metadata and fails closed when that metadata is missing.
- Successful finalization publishes the bond status of the stored LFB, not the candidate block.
- Failed finalization does not publish a candidate cache entry.
- `bond_status` remains a PoS post-state query. Its answer can differ from the LFB metadata committee at an epoch boundary.

## Verification

Focused tests cover startup, deploy admission, the finality gap, stale entries in both directions, and missing metadata. A finalization test advances the LFB, fails a later effect, and checks that admission rejects an unbonded validator without refreshing the cache. The tests do not inject a storage write failure.
