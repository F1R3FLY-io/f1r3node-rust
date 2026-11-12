# Remedy study for the guarded PathMap final-teardown allocation count — 2026-11-12

## Summary

This study decides how bug 6441 (`guarded-pathmap-final-root-allocation-teardown`) is to be
fixed. The bug names two remedies. Remedy R2a applies when the released PathMap 0.3.4 public
interface, or a barrier stride larger than one edge inside the existing guard, cuts the
teardown's allocation count at least tenfold at both measured coordinates while keeping a
provable bound on the native stack that does not grow with the path depth. Remedy R2b applies
otherwise. The [measurement record of 2026-11-12](measurements/guarded-pathmap-final-teardown-pathmap-0.3.4-2026-11-12.md)
found 184,489 allocation calls under the final teardown at the realistic coordinate (threshold
18,170) and 32,489 at the value-free line coordinate (threshold 1,638).

**Decision: R2a.** The evidence, in order:

1. Almost every allocation of the teardown is one new trie node caused by the synthetic barrier
   that the guard writes at the parent of each edge before pruning it: to store the barrier,
   PathMap splits the compressed key that passes through the barrier's position (182,623 of
   184,489 calls and 31,984 of 32,489 calls).
2. PathMap 0.3.4 already offers a public removal that cuts one edge without any barrier:
   `remove_branches(false)` at a value-free parent with one child detaches only the edge to that
   child and allocates no node. Where the parent is the root or forks, the existing
   `prune_ascend` removes exactly one byte. A teardown built from these two operations writes no
   value at all.
3. A prototype of that teardown, measured with the record's own protocol, makes 1,866 calls at
   the realistic coordinate (98.9 times fewer) and 505 at the line coordinate (64.3 times
   fewer). Every other allocation site is unchanged to the call; only the split calls vanish.
4. Each cut detaches at most two nested trie nodes, so the destructor's stack per cut is bounded
   by a constant (PathMap's recursive destructor was measured at 80 bytes per nested node in
   the release build and 128 in the debug build). The prototype retires a 16,384-level dense
   spine on a thread with a 256 KiB stack (1 KiB is 1,024 bytes), while PathMap's own drop of
   the same trie overflows that stack.
5. Every existing test of the guard passes on the prototype except six that assert the barrier
   protocol itself; their observed events differ from the expected ones only by the barrier
   events.

The fix is three leaves: [L1](#l1-bound-and-model) states and proves the barrier-free contract in
the formal models, [L2](#l2-implementation-with-allocation-gate) implements it in the guard with
an allocation gate, and [L3](#l3-small-stack-gate) adds the fixed-stack gate. What this study
does not establish is listed in [Section 4.2](#42-what-was-not-established).

## Scope

This is leaf 14357 of bug 6441. It answers one question: does R2a hold, and if it does, which
fix leaves follow. It changes no source, test, manifest or formal file; the only repository
change is this study. The prototype and the measuring instruments exist only in the study's
scratch directory and are described by two diffs whose digests are listed in
[Evidence files](#evidence-files-not-committed). Every source statement refers to commit
`d0f38fddb` of this repository (the studied commit) and to crate `pathmap` 0.3.4 as published
in the cargo registry; the workspace checkout of PathMap was not read.

The constraints of the leaf are kept: the remedy must preserve PathMap's trie compression,
copy-on-write sharing, algebra and serialization, and the tuple space's multiplicities,
canonical order, bytes, hashes, COMM (the communication step that matches a send with a
receive), charges and recovery authority. [Preservation obligations](#preservation-obligations)
maps each of them to its evidence.

## Terms and symbols

| Term | Meaning |
|---|---|
| PathMap | The byte-keyed trie crate `pathmap`, version 0.3.4, pinned with `=0.3.4` by [`rspace++/Cargo.toml`](../../../rspace++/Cargo.toml). |
| `GuardedPathMap` | The stack-safe owner of one PathMap trie in [`guarded_pathmap.rs`](../../../rspace++/src/rspace/guarded_pathmap.rs). |
| final teardown | The function `<AnchorRoot<V, D, B> as Drop>::drop` for value type `V`, disposer type `D` and barrier type `B`; it runs when the last owner of a trie is dropped. |
| Phase A | The first teardown phase: it visits every value in byte preorder, removes it without removing any path byte, and hands it to the named disposer. |
| Phase B | The second teardown phase: on the now value-free trie, it removes the topology in reverse byte order, one logical edge at a time. |
| logical edge | One byte of one path of the trie, counted once however many paths pass through it. |
| run | One ascent of Phase B: from the rightmost childless leaf upwards to the first ancestor that stops it. |
| stopping parent | The ancestor that ends a run: the zipper root, an ancestor with a value, or an ancestor with more than one child (a fork). |
| synthetic barrier | A temporary value that the guard at the studied commit writes at a parent so that PathMap's upward pruning stops there; it is removed before the run ends. |
| node | A physical trie node of PathMap: a line-list node (two slots whose compressed keys share a 42-byte buffer, each slot holding either a value or a child pointer) or a dense node (one cell per byte, each cell holding an optional value and an optional child pointer). |
| node key | The bytes of the current path that lie inside the node the zipper is in. |
| split | The operation `LineListNode::split_0` (or `split_1`) that cuts a slot's compressed key in two and moves the tail into a newly allocated node. |
| copy-on-write | PathMap's sharing discipline: nodes are reference-counted and a node shared by two tries is cloned before it is changed ([Driscoll et al. 1989](#references)). |
| write zipper | PathMap's mutable cursor (`WriteZipperUntracked`); its focus is the path it currently points at, and its root is the path it was created at (the map's root in Phase B). |
| detach | To remove a payload (a value or a child pointer) from a node; the detached part is dropped by PathMap's destructor. |
| final owner | The `GuardedPathMap` that the measured test builds and then drops inside its timer. |
| donor | A value-free `GuardedPathMap` that `materialize_missing_suffix` builds when construction extends a path below a leaf: it holds the missing suffix, is grafted into the trie under construction (which then shares its nodes), and is dropped at once; its drop is also a final teardown. |
| folded stack | One line of `heaptrack_print -F`: the frames of one call stack, root first, followed by its number of allocation calls. |
| Heaptrack | The heap profiler, version 1.5.0, that counts every call to an allocation function; it was used only in record-only mode, and `heaptrack_print` rendered each recording once. |
| allocation call | One call to an allocation function as counted by Heaptrack. |
| release build | The release profile with debug information (`CARGO_PROFILE_RELEASE_DEBUG=true`). |
| debug build | The default `dev` profile, with debug assertions on. |
| small-stack test | A test that runs its work on a thread with a 256 KiB stack. |
| KiB, MiB | 1,024 bytes and 1,048,576 bytes. |
| SIGABRT | The abort signal, number 6; Rust's runtime raises it when a thread overflows its stack. |
| SHA-256 | Secure Hash Algorithm 2 with a 256-bit digest (SHA-256), used to fingerprint each evidence file. |

| Symbol | Meaning |
|---|---|
| $`x`$ | A run name: a shape and a size, for example $`x = (\mathrm{realistic}, 256)`$. |
| $`\Sigma(x)`$, $`\sigma`$, $`c_x(\sigma)`$ | The set of distinct folded stacks of run $`x`$, one such stack, and its allocation calls. |
| $`\mathcal{F}`$ | The set of final-teardown frames (the record writes $`D`$ for this set). |
| $`A_{\mathrm{drop}}(x)`$ | The allocation calls of run $`x`$ whose stack contains a final-teardown frame ([Section 1.2](#12-where-the-allocation-calls-come-from)). |
| $`N_{\mathrm{split}}(x)`$ | The part of $`A_{\mathrm{drop}}(x)`$ whose stack also contains `split_0`. |
| $`A_{\mathrm{other}}(x)`$ | $`A_{\mathrm{drop}}(x) - N_{\mathrm{split}}(x)`$: the allocation calls that are not splits. |
| $`T`$ | The number of logical edges that Phase B removes. |
| $`C`$ | The number of runs of Phase B. |
| $`\ell_c`$ | The number of edges of run $`c`$, so that $`\sum_{c=1}^{C} \ell_c = T`$. |
| $`k`$ | The barrier stride of candidate (b): one barrier per $`k`$ edges. |
| $`B(k)`$ | The number of barriers that Phase B installs with stride $`k`$. |
| $`s`$ | The measured fraction of barrier installs that split a node. |
| $`\alpha`$ | An upper bound on the allocation calls of one barrier install. |
| $`D`$ | The nesting depth of a drop: the number of node destructors active at the same time while a detached part is dropped. |
| $`f`$, $`c_0`$ | The native stack that PathMap's recursive destructor uses per nested node, and the constant part of a recursive drop. |
| $`S_{\mathrm{iter}}`$ | The native stack of PathMap's iterative destructor path (a line-list node with exactly one child pointer), which does not depend on the length of the line. |
| $`S_{\mathrm{nav}}`$ | The native stack used by the zipper navigation and mutation calls of Phase B. |
| $`L`$, $`b`$, $`P`$ | A childless value-free leaf, the last byte of its path, and its parent, so that $`L = P\,b`$ (the path of $`P`$ followed by $`b`$). |
| $`N`$, $`\kappa`$ | The node that holds the zipper's focus, and the node key of the focus. |
| $`X`$, $`Y`$ | An empty node, and a node below a child pointer. |

## Evidence and citation format

A source citation is a code span of the form `<root>/<relative-path>.rs::<function>`. The root
`rspace++` is the crate directory of this repository at the studied commit; the roots
`pathmap-0.3.4` and `local-or-heap-0.1.0` are the crate directories in the cargo registry. Each
citation names a file and a function defined in it; the study's check script resolved every
citation and printed the line numbers where it found each function.

Every measured number names the log it was read from. The logs, the recordings and the scripts
are in the study's scratch directory
`[WORKSPACE_ROOT]/target/scratch/session-401d0c58/c44/w14357`, where `[WORKSPACE_ROOT]` is the
repository root; they are not committed. A statement marked *inference* was derived from
reading sources or from other numbers and was not measured directly. A statement marked
*assumption* is used without proof and is named where it is used.

## 1. Mechanism

### 1.1 The teardown at the studied commit

The final teardown `rspace++/src/rspace/guarded_pathmap.rs::drop` calls
`rspace++/src/rspace/guarded_pathmap.rs::drain_pathmap`, which calls
`rspace++/src/rspace/guarded_pathmap.rs::drain_pathmap_observed` and then
`rspace++/src/rspace/guarded_pathmap.rs::drain_pathmap_with_provider`. Phase A walks the values
with `rspace++/src/rspace/guarded_pathmap.rs::advance_to_next_value` and removes each with
`rspace++/src/rspace/guarded_pathmap.rs::remove_value_preserving_path`, which removes only the
value. Phase B repeats runs on a write zipper rooted at the map's root.
`rspace++/src/rspace/guarded_pathmap.rs::descend_to_rightmost_leaf` forks a read zipper at the
write zipper's focus, finds the last path below it and moves the write zipper there; the guard
writes a barrier at that leaf and calls
`rspace++/src/rspace/guarded_pathmap.rs::remove_valued_leaf_and_bubble_with_provider`, which
climbs one edge at a time.

**Algorithm 1 (Phase B at the studied commit).**

```pseudocode
input:  a write zipper z, rooted at the map's root, over a value-free trie; a barrier provider
output: the empty trie; observer events Installed, Pruned(1) and Retired, reported after the loop

loop
    descend_to_rightmost_leaf(z)                    -- from z's focus to the last leaf below it
    if z is at the root and the root has no child then stop
    z.set_val(new barrier)                          -- install at the leaf
    loop                                            -- remove_valued_leaf_and_bubble_with_provider
        if z is at the root then
            remove and discard the root's barrier; end the run
        b <- last byte of z's path; z.ascend_byte()  -- z is now at the parent P
        stops <- P has a value or P has more than one child
        if P has no value then z.set_val(new barrier)  -- one install per edge
        z.descend_to_existing_byte(b)
        discard(z.remove_val(prune = false))       -- the child's barrier
        z.prune_ascend()                           -- asserted to remove exactly 1 byte
        if stops then remove and discard the barrier written at P, if any; end the run
```

The inner loop writes one barrier at the parent of every edge it removes, and the outer loop
writes one more at the leaf of every run. A run of $`\ell_c`$ edges therefore installs
$`1 + \ell_c`$ barriers, and the whole Phase B installs

```math
B(1) = \sum_{c=1}^{C} \left(1 + \ell_c\right) = T + C .
```

A barrier at the zipper root costs no node, because PathMap keeps the root's value in a field of
the map, outside the node tree (`pathmap-0.3.4/src/trie_map.rs::root_val` and the root branches
of `pathmap-0.3.4/src/write_zipper.rs::set_val` and `pathmap-0.3.4/src/write_zipper.rs::remove_val`).
The barriers exist only to stop `prune_ascend`, whose documented contract
(`pathmap-0.3.4/src/write_zipper.rs::prune_path`) is to prune the dangling path above the focus,
never above the zipper root, and to leave the zipper at the first path that is not dangling: an
ancestor with a value or with more than one branch. Without the barrier, a prune from a leaf
would detach the whole value-free chain above it at once, and dropping that chain can recurse
once per node ([Section 1.3](#13-what-the-destructor-recurses-over)).

```text
 ┌────────────────────────────────────────────────────────────────────┐
 │ <AnchorRoot<V, D, B> as Drop>::drop: the last owner is dropped     │
 └────────────────────────────────────────────────────────────────────┘
                                   │
                                   ▼
 ┌────────────────────────────────────────────────────────────────────┐
 │ drain_pathmap, drain_pathmap_observed,                             │
 │ drain_pathmap_with_provider                                        │
 └────────────────────────────────────────────────────────────────────┘
                                   │
                                   ▼
 ┌────────────────────────────────────────────────────────────────────┐
 │ Phase A: every value, in byte preorder                             │
 │   advance_to_next_value, remove_value_preserving_path,             │
 │   dispose(value): the named disposer takes it                      │
 └────────────────────────────────────────────────────────────────────┘
                                   │
                                   ▼
 ┌────────────────────────────────────────────────────────────────────┐
 │ Phase B: topology, reverse byte order, one run at a time           │
 │   descend_to_rightmost_leaf                         [R]            │
 │   set_val(barrier) at the leaf                      [S]            │
 │   for each edge of the run, from the leaf upwards:                 │
 │     set_val(barrier) at the parent                  [S]            │
 │     remove_val(false) of the child's barrier                       │
 │     prune_ascend: removes exactly one byte                         │
 │   at the stopping parent: remove its barrier                       │
 └────────────────────────────────────────────────────────────────────┘
```

Legend: each box is one stage of the final teardown, and the arrows give their order. The label
`[S]` marks the operations whose allocation is a node split, and `[R]` the operation whose
allocations are read-zipper buffers. Copy-on-write clones and the growth of the write zipper's
own buffers can occur wherever the write zipper enters or changes a node, in either phase; the
measurement does not separate the two phases, so those sites carry no label (inference from the
sources).

### 1.2 Where the allocation calls come from

The record attributes allocation calls to the final teardown by their folded stacks:

```math
A_{\mathrm{drop}}(x) = \sum_{\sigma \in \Sigma(x),\ \sigma \cap \mathcal{F} \neq \emptyset} c_x(\sigma),
\qquad
N_{\mathrm{split}}(x) = \sum_{\sigma \in \Sigma(x),\ \sigma \cap \mathcal{F} \neq \emptyset,\ \mathtt{split\_0} \in \sigma} c_x(\sigma).
```

The study re-ran the record's two coordinates on a release build of the unmodified studied
commit and obtained the record's numbers to the call (logs `40-analyse-before.log` and
`41-recount.log`; the second is an independent recount from the mangled symbols). It then
attributed every call of $`A_{\mathrm{drop}}`$ to one site: the frame nearest to the allocator,
below the teardown frame, whose mangled symbol names the `pathmap` crate; for a stack without
such a frame, the nearest frame of the guard's crate (script `42-classify-a-drop.py`, log
`42-classify-a-drop.log`; the mangled names of the two vector-growth sites are in log
`89j-mangled-growth-sites.log`):

| Site | Realistic 256 | Line 16384 | Why it allocates |
|---|---|---|---|
| `pathmap-0.3.4/src/line_list_node.rs::split_0` | 182,623 | 31,984 | a barrier key of two or more bytes ends inside a slot's key; the split moves the rest of that key into a new node (`pathmap-0.3.4/src/trie_node.rs::new_in`) |
| `pathmap-0.3.4/src/zipper.rs::descend_first` | 1,285 | 26 | buffers of the read zipper forked by `descend_to_rightmost_leaf` (inference) |
| `pathmap-0.3.4/src/zipper.rs::descend_indexed_byte` | 257 | 2 | the same read zipper (inference) |
| `pathmap-0.3.4/src/zipper.rs::reserve_buffers` | 257 | 2 | the same read zipper (inference) |
| `pathmap-0.3.4/src/line_list_node.rs::clone_self` | 23 | 391 | copy-on-write: a node shared with another trie is cloned before it is changed (`pathmap-0.3.4/src/trie_node.rs::make_mut`) |
| `SmallVec::reserve_one_unchecked` over PathMap's `TaggedNodePtr` | 20 | 36 | growth of a zipper's stack of node pointers (inference) |
| `RawVecInner::finish_grow`, the copy compiled for the `pathmap` crate | 12 | 28 | growth of a vector inside PathMap (inference) |
| `pathmap-0.3.4/src/write_zipper.rs::descend_to` | 10 | 18 | the write zipper's path buffer (inference) |
| no frame names `pathmap`; the nearest frame of the guard's crate is the test binary's counting global allocator | 2 | 2 | not attributed further; plausibly the guard's own path buffer in Phase B (inference) |
| total, equal to $`A_{\mathrm{drop}}`$ | 184,489 | 32,489 | |

No site is a dense conversion (`convert_to_dense`), so in both measured shapes every
barrier-induced allocation is a split. The split path is the following. A barrier is written
with `pathmap-0.3.4/src/write_zipper.rs::set_val`, which reaches
`pathmap-0.3.4/src/line_list_node.rs::node_set_val` and then
`pathmap-0.3.4/src/line_list_node.rs::set_payload_abstract`. When the barrier's key shares a
prefix with the key of slot 0, the function takes the length of the shared prefix, lowers it by
one byte if it equals the whole barrier key or the whole slot key, and, if at least one byte
remains, calls `split_0` at that length and stores the barrier in the new node; `split_1` is the
same for slot 1. If neither slot can take the value, the node is converted with
`pathmap-0.3.4/src/line_list_node.rs::convert_to_dense`. Without PathMap's `nightly` feature,
which this build does not enable (the manifests of `rspace++`, `models` and `rholang` request
only `arena_compact` beside the default features), `TrieNodeODRc::new_in` ignores its allocator
argument and calls `Box::new`: one allocation call per new node.

```text
 before: the leaf L at depth m of node N carries barrier_L
 ┌────────────────────────────────────────────────────┐
 │ N.slot0: key p1 p2 ... p(m-1) pm, value barrier_L  │
 └────────────────────────────────────────────────────┘

 set_val(barrier_P) at the parent P, node key p1 ... p(m-1):
 the key overlaps slot 0 by m - 1 bytes, so split_0(m - 2) runs
 ┌──────────────────────────────┐   ┌───────────────────────────────────────┐
 │ N.slot0: key p1 ... p(m-2),  │──►│ N' (new node: one allocation)         │
 │ child pointer to N'          │   │ slot0: key p(m-1), value barrier_P    │
 └──────────────────────────────┘   │ slot1: key p(m-1) pm, value barrier_L │
                                    └───────────────────────────────────────┘
```

Legend: a box is one node and each row one slot, with its compressed key and payload; the arrow
is a child pointer; $`m`$ is the length of the leaf's node key and `p1` to `pm` are its bytes.
The example is the last node of a value-free line with $`m \ge 3`$, drawn from
`set_payload_abstract` and `split_0` (inference: the slot contents were read, not printed).
Each further step up the line repeats the pattern one byte higher, so every barrier whose node
key has at least two bytes costs one new node.

The counts agree with this mechanism. Per teardown of the line, $`B(1) = 16{,}384 + 1`$ barriers
were installed and 15,992 of them split (the heaviest single stack of the record). The remaining
393 installs match one install at the first byte of each of the line's 391 nodes, where the node
key has one byte, plus the leaf's install and the root's (inference: 391 is also the number of
copy-on-write clones of the same line, and $`16{,}384 / 42`$ rounds up to 391). The measured split
fractions are

```math
s_{\mathrm{realistic}} = \frac{N_{\mathrm{split}}(\mathrm{realistic}, 256)}{B(1)} = \frac{182623}{187090} = 0.9761,
\qquad
s_{\mathrm{line}} = \frac{N_{\mathrm{split}}(\mathrm{line}, 16384)}{B(1)} = \frac{31984}{32770} = 0.9760,
```

where the realistic trie has $`T = 210 + 256 \cdot 729 = 186{,}834`$ edges in $`C = 256`$ runs:
each of its 256 keys of 939 bytes is a 203-byte constant prefix, an 8-byte big-endian ordinal
and 728 pseudo-random bytes
(`rspace++/tests/guarded_pathmap/two_phase_teardown_properties.rs::measurement_map`), so the keys
share their first 210 bytes and then fork; 255 runs of 729 edges stop at that fork and the last
run of 939 edges reaches the root. The line coordinate has two teardowns of one run of 16,384
edges each, the donor's and the final owner's, so there $`B(1) = 2 \cdot (16{,}384 + 1)`$. For the
realistic coordinate, the numerator also contains the donor's splits, about 915 calls
(inference: the donors' 967 calls before the remedy minus their 52 after), while $`B(1)`$ counts
only the final owner's barriers; the difference is half a percent.

### 1.3 What the destructor recurses over

PathMap's destructor is recursive over nodes, not over bytes:

- `pathmap-0.3.4/src/trie_node.rs::drop` (for `TrieNodeODRc`) returns at once for a pointer to
  the empty sentinel; otherwise it decrements the reference count and, when the count reaches
  zero, calls `pathmap-0.3.4/src/trie_node.rs::drop_inner_in`, which drops the boxed node.
- `pathmap-0.3.4/src/line_list_node.rs::drop` takes an iterative path,
  `pathmap-0.3.4/src/line_list_node.rs::list_node_iterative_drop`, only when exactly one slot
  holds a child pointer; otherwise it drops both slots in place, which recurses into each child.
  A dangling path is stored as a child pointer to the empty sentinel
  (`pathmap-0.3.4/src/line_list_node.rs::node_create_dangling`), so a node with one real child
  and one dangling slot takes the recursive branch.
- A dense node has no destructor of its own; dropping it drops its cells, and each cell drops
  its child pointer: recursive.

The native stack per nested node was measured with a scratch probe (`stack-probe.diff`). The
probe builds a spine of $`d`$ zero bytes with dangling teeth beside every spine byte (none for
the single-child line; the byte 1 for the two-child line; the bytes 1 and 2 for the dense spine,
whose three children per level do not fit the two slots of a line-list node), puts a value at
the end of the spine, and drops the trie on a thread with a 256 MiB stack. The value's destructor
records the lowest stack address at which it runs; its distance from a local variable of the
probe thread, taken just before the drop, is the stack depth at which the deepest value is
dropped. Depths 1, 1,000, 2,000, 4,000, 8,000 and 16,000 were measured (logs
`50-tests-proto-release-stack-probe.log` and `50-tests-proto-dev-stack-probe.log`), in bytes:

| Shape | Release, $`d = 1`$ | Release, $`d \ge 1000`$ | Debug, $`d = 1`$ | Debug, $`d \ge 1000`$ |
|---|---|---|---|---|
| single-child line (no teeth) | 112 | 144 at every depth | 135 | 263 at every depth |
| two-child line (tooth 1) | 151 | $`80\,d + 71`$ | 192 | $`128\,d + 64`$ |
| dense spine (teeth 1 and 2) | 144 | $`80\,d + 72`$ | 136 | $`128\,d + 8`$ |

From depth 1,000 on, every measured value lies exactly on its line, or equals its constant for
the single-child line. At depth 1 the two-child line lies on its line in both builds, the dense
spine lies on its line in the debug build and 8 bytes below it in the release build, and the
single-child line needs less than its constant. Each spine byte forks in the two recursive
shapes, so a spine of $`d`$ bytes nests one node destructor per byte (inference). Hence
$`f = 80`$ bytes in the release build and $`f = 128`$ bytes in the debug build, with
$`c_0 = 72`$ and $`c_0 = 64`$ (the larger constant of the two recursive shapes), and a recursive
drop of nesting depth $`D`$ needs

```math
S_{\mathrm{drop}}(D) \le f \cdot D + c_0 ,
```

while the iterative path needs the constant $`S_{\mathrm{iter}}`$, 144 bytes in the release
build and 263 in the debug build. The probe records the depth at which the deepest value's
destructor runs; the allocator's frames that free the deepest node lie below that point and add
a constant that was not measured.

A plain PathMap drop of the dense spine of 16,384 levels needs about
$`80 \cdot 16384 + 72 = 1{,}310{,}792`$ bytes in the release build, five times a 256 KiB stack.
The probe's positive control confirms it: the plain drop of that shape on a 256 KiB thread kills
its child process with SIGABRT, and the test asserts that the child's error output contains
Rust's stack-overflow message (log `50-tests-proto-release-probe.log`). The same shape is the
hardest case for any remedy.

## 2. Candidates and verdicts

The leaf asks for four candidate families; reading the sources added a fifth, which is the one
that survives.

### 2.1 Candidate (a): public PathMap items that remove or detach topology

| Public item | What it detaches | Recursion | Allocation | Verdict |
|---|---|---|---|---|
| dropping a `PathMap` or a subtrie (`pathmap-0.3.4/src/trie_node.rs::drop`, `pathmap-0.3.4/src/line_list_node.rs::drop`) | everything below | yes, once per dense or two-child node | none | the hazard itself |
| `remove_val(true)`, `remove_val_at(path, true)`, `remove(path)` (`pathmap-0.3.4/src/write_zipper.rs::remove_val`, `pathmap-0.3.4/src/trie_map.rs::remove_val_at`, `pathmap-0.3.4/src/trie_map.rs::remove`) | the value and the whole dangling chain above it, up to a value, fork or root | the detached chain is dropped as a whole | no split; copy-on-write clones | rejected: one call may detach an unbounded chain |
| `remove_val(false)` | only the value | none | none | not a topology remover; Phase A uses it |
| `prune_path`, `prune_ascend`, `PathMap::prune_path` (`pathmap-0.3.4/src/write_zipper.rs::prune_path`, `pathmap-0.3.4/src/write_zipper.rs::prune_ascend`, `pathmap-0.3.4/src/trie_map.rs::prune_path`) | the dangling chain up to a value, fork or root | as above | no split | rejected alone; exactly one byte when the parent is the root or a fork ([Lemma 3](#25-candidate-e-barrier-free-one-edge-cut)), and used so by candidate (e) |
| `remove_branches(true)`, `remove_branches_at(path, true)` (`pathmap-0.3.4/src/write_zipper.rs::remove_branches`, `pathmap-0.3.4/src/trie_map.rs::remove_branches_at`) | the whole subtrie below the focus, then the dangling chain above | the subtrie is dropped as a whole | no split | rejected: unbounded detachment |
| `remove_branches(false)` below the root | the payload of the focus's single child slot or cell | at most two nested nodes at a single-child parent above a childless child ([Lemma 2](#25-candidate-e-barrier-free-one-edge-cut)) | none beyond copy-on-write ([Lemma 2](#25-candidate-e-barrier-free-one-edge-cut)) | survives: the cut of candidate (e) |
| `remove_branches` at the zipper root | the whole trie; a new empty root node is made (`pathmap-0.3.4/src/trie_node.rs::new_allocated_in`) | the whole trie is dropped | one node | rejected |
| `remove_unmasked_branches` (`pathmap-0.3.4/src/write_zipper.rs::remove_unmasked_branches`) | the branches whose byte is not in the mask | as `remove_branches` | may clone a shared child node | equivalent to `remove_branches(false)` at a one-child parent; not needed |
| `take_map` (`pathmap-0.3.4/src/write_zipper.rs::take_map`) | moves the subtrie into a new `PathMap` | moves the drop obligation, it does not discharge it | a new empty root node at the zipper root (`pathmap-0.3.4/src/write_zipper.rs::take_focus`) | rejected |
| `graft`, `graft_map`, `graft_src_at`, `graft_child_maps`, `graft_masked_branches` (`pathmap-0.3.4/src/write_zipper.rs::graft`, `pathmap-0.3.4/src/write_zipper.rs::graft_internal`) | with an empty source: `remove_branches(false)` and `remove_val(false)`; otherwise the subtrie, which is replaced | as `remove_branches` | a non-empty source is written as a child pointer through `set_payload_abstract`, which can split (inference) | the empty-source form equals candidate (e); nothing gained |
| `join_into`, `meet_into`, `subtract_into`, `restrict` and their map forms (`pathmap-0.3.4/src/write_zipper.rs::meet_into`, `pathmap-0.3.4/src/trie_node.rs::pmeet_generic_internal`) | paths absent from the operand | recursive over both tries | builds result nodes | rejected |
| `join_k_path_into`, `meet_k_path_into`, `drop_head` (`pathmap-0.3.4/src/write_zipper.rs::join_k_path_into`, `pathmap-0.3.4/src/write_zipper.rs::drop_head`) | the leading bytes of every path, merging the rest | recursive merge | builds result nodes | rejected |
| `remove_prefix` (`pathmap-0.3.4/src/write_zipper.rs::remove_prefix`) | the bytes above the focus with every subtrie that hangs from them | the detached subtries are dropped as wholes | possible | rejected |
| owned iteration (`pathmap-0.3.4/src/trie_map.rs::into_iter`, `pathmap-0.3.4/src/write_zipper.rs::into_iter`) | each value with `remove_val(true)` | as `remove_val(true)` | as above | rejected; the guard's source-scan test `rspace++/src/rspace/guarded_pathmap.rs::implementation_contains_no_snapshot_count_or_owned_raw_escape` forbids the strings `OwnedZipperIter` and `into_write_zipper` in the guard's source, though not a call of `into_iter` as such |
| `merkleize` (`pathmap-0.3.4/src/trie_map.rs::merkleize`, `pathmap-0.3.4/src/merkleization.rs::merkleize_impl`) | nothing; it shares equal subtries | recursive | yes | not a removal |

Only `remove_branches(false)` below the root detaches a bounded part without a value at the
parent. The prune family is bounded by the stopping parent, which the guard can test before it
prunes. These two observations are candidate (e).

### 2.2 Candidate (b): one barrier per $`k`$ edges

With a barrier at the leaf of a run and then at every ancestor whose distance from that leaf is
a multiple of $`k`$, Phase B installs

```math
B(k) = \sum_{c=1}^{C} \left(1 + \left\lceil \frac{\ell_c}{k} \right\rceil\right),
\qquad
\frac{T}{k} + C \;\le\; B(k) \;<\; \frac{T}{k} + 2C ,
```

where the bounds follow from $`\ell_c / k \le \lceil \ell_c / k \rceil < \ell_c / k + 1`$ and
$`\sum_c \ell_c = T`$; $`B(1) = T + C`$ is the barrier count of the studied commit. Each prune
now detaches up to $`k`$ edges at once, and in the worst case every one of them is its own node
above an empty end node, so the nesting depth is at most $`k + 1`$ and the native stack of
candidate (b) per prune is

```math
S_{(b)}(k) \le S_{\mathrm{nav}} + \max\left((k + 1)\, f + c_0,\ S_{\mathrm{iter}}\right),
```

a bound in $`k`$ that does not grow with the path depth. For the allocation calls, under
assumption A1 (the split fraction $`s`$ does not depend on $`k`$) and assumption A2 (the
donors' barriers shrink with $`k`$ in the same proportion as the final owner's), the expected
count $`\hat{A}_{\mathrm{drop}}(k)`$ and a worst-case bound are

```math
\hat{A}_{\mathrm{drop}}(k) = A_{\mathrm{other}} + s \cdot B(k),
\qquad
A_{\mathrm{drop}}(k) \le A_{\mathrm{other}} + \alpha \cdot B(k), \quad \alpha = 5 .
```

The bound $`\alpha = 5`$ holds for a value of at most 8 bytes: an install either splits one slot
(one new node, after which the rest of the key fits the new node without a second split) or
converts the node to a dense node (the dense node, its cell vector, up to two intermediate nodes
for slot keys longer than one byte, and one new child node for a new key longer than one byte);
inference from `set_payload_abstract`, `split_0`, `split_1` and `convert_to_dense`. The
continuation nodes that `set_payload_abstract` creates for a key that shares no byte with either
slot are not reached, because a barrier key always shares a prefix with the slot that leads to
the leaf below it (inference). A larger value adds its own box. With the measured
$`A_{\mathrm{other}}`$ (1,866 and 505) and $`s`$ (log `80-stride-estimates-alpha5.log`):

| Stride $`k`$ | $`B(k)`$, realistic | Estimate, realistic | Bound, realistic | $`B(k)`$, line | Estimate, line | Bound, line |
|---|---|---|---|---|---|---|
| 1 | 187,090 | 184,489.0 | 937,316 | 32,770 | 32,489.0 | 164,355 |
| 8 | 23,834 | 25,130.9 | 121,036 | 4,098 | 4,504.7 | 20,995 |
| 11 | 17,427 | 18,876.9 | 89,001 | 2,982 | 3,415.5 | 15,415 |
| 12 | 15,890 | 17,376.6 | 81,316 | 2,734 | 3,173.4 | 14,175 |
| 28 | 7,175 | 8,869.7 | 37,741 | 1,174 | 1,650.8 | 6,375 |
| 29 | 6,919 | 8,619.8 | 36,461 | 1,132 | 1,609.8 | 6,165 |
| 64 | 3,331 | 5,117.5 | 18,521 | 514 | 1,006.7 | 3,075 |
| 128 | 1,794 | 3,617.2 | 10,836 | 258 | 756.8 | 1,795 |

The estimate meets the realistic threshold 18,170 from $`k = 12`$ and the line threshold 1,638
from $`k = 29`$; the worst-case bound meets neither threshold at $`k = 64`$ and not the line
threshold even at $`k = 128`$. Verdict: **feasible in principle but dominated by (e).** It was
not prototyped; it still allocates about one node per $`k`$ edges, it needs $`k \ge 29`$ by the
estimate alone, its stack bound grows with $`k`$, and it breaks the formal contract of
[`PathMapReversePruneTeardown.v`](../../../formal/rocq/stack_safe_pda/theories/PathMapReversePruneTeardown.v)
that every detachment spans one logical edge. Candidate (e) removes the per-edge term entirely
and keeps that contract.

### 2.3 Candidate (c): barriers only at existing node boundaries

The public interface reveals node boundaries through `shared_node_id`, declared at
`pathmap-0.3.4/src/zipper.rs::shared_node_id` and implemented in
`pathmap-0.3.4/src/trie_ref.rs::shared_node_id` and by the zippers. The zippers compute it with
`pathmap-0.3.4/src/zipper.rs::read_zipper_shared_node_id`, which returns an identifier only when
the focus is at the root of a node, holds no value, and the node is shared
(`pathmap-0.3.4/src/trie_ref.rs::is_shared`: reference count above one). PathMap's documentation
of the identifier says that it is based on the node's memory address, is not stable across runs,
and is subject to change when the content of the node is modified; the documentation of
`is_shared` warns that code must never rely on it for correctness. The nodes of a trie in its
final teardown are in general not shared, so their boundaries are not observable. Even where a
boundary is known, a barrier there does not avoid the allocation: a barrier key equal to a slot
key of $`m`$ bytes, with $`m \ge 2`$, is split at $`m - 1`$ bytes by `set_payload_abstract`. At
best the count would fall from one split per edge to about one per node. Verdict:
**rejected**: the interface does not expose the boundaries of unshared nodes, and a boundary
barrier still splits.

### 2.4 Candidate (d): reuse of one allocation across prunes

Reuse applies only if the allocation belongs to the barrier, not to the split. For the measured
value type `u64` the barrier is stored inline: with PathMap's default `slim_ptrs` feature a slot
stores its value as `LocalOrHeap<V, [u8; 8]>`, and `local-or-heap-0.1.0/src/lib.rs::new` boxes
a value only when `local-or-heap-0.1.0/src/lib.rs::is_heap` finds it larger than 8 bytes, so
every barrier-induced allocation is the split's new node
(`pathmap-0.3.4/src/line_list_node.rs::split_0`). No public item lets a caller provide or
recycle node storage, and the allocator parameter is ignored without the `nightly` feature,
which would change the dependency's configuration. The production barrier values `Multiplicity`
and `CandidatePosition` of `rspace++` wrap a `u64` and are stored inline too. The barrier
`ChannelSlot::SyntheticBarrier` is a variant of an enum whose other variant holds a `Bag`, which
has a `u128` field, and the barrier `Par::default()` of the `models` crate is a protocol message
with many fields; both are larger than 8 bytes and are boxed when stored (inference from the type
definitions; no size was printed). That box is created inside PathMap from the value passed in,
so the caller cannot reuse it either. Verdict: **inapplicable**: the allocation is the split's,
and no public item can recycle it.

### 2.5 Candidate (e): barrier-free one-edge cut

**Algorithm 2 (Barrier-free Phase B).**

```pseudocode
input:  a write zipper z, rooted at the map's root, over a value-free trie
output: the empty trie; one observer event Pruned(1) per logical edge, reported after the loop

cuts <- 0
loop
    descend_to_rightmost_leaf(z)
    if z is at the root and the root has no child then stop
    loop                                            -- cut_value_free_leaf_and_bubble
        if z is at the root then end the run
        b <- last byte of z's path; z.ascend_byte()  -- z is now at the parent P
        if P is the root or P has a value or P has more than one child then
            z.descend_to_existing_byte(b)
            z.prune_ascend()                        -- asserted to remove exactly 1 byte
            cuts <- cuts + 1; end the run
        z.remove_branches(prune = false)           -- detaches the edge to L only
        cuts <- cuts + 1                            -- P is now a childless leaf
emit Pruned(1) cuts times
```

The outer loop is that of Algorithm 1. The inner loop removes the run's edges from the leaf
upwards without writing any value. At a parent that does not stop the run,
`remove_branches(false)` detaches the one child edge and leaves the parent as the new childless
leaf; at the stopping parent the existing `prune_ascend` removes the last edge. In Phase B no node
holds a value, so the stopping parents are the root and the forks; the value test is kept so that
the function never cuts below a valued parent, a case that Phase B does not reach and that the
lemmas below do not cover. Phase B reports no event other than these prunes, so reporting them
after the loop gives the same event list as reporting each at its cut.

```text
         ┌──────────────────────────────────────────────────────┐
         │ the focus is the childless, value-free leaf L = P b; │
         │ ascend one byte to its parent P                      │
         └──────────────────────────────────────────────────────┘
                                    │
                                    ▼
         ┌──────────────────────────────────────────────────────┐
         │ is P the zipper root, valued, or a fork?             │
         └──────────────────────────────────────────────────────┘
                    │ no                          │ yes
                    ▼                             ▼
     ┌──────────────────────────────┐ ┌───────────────────────────────┐
     │ remove_branches(false) at P: │ │ descend back to L and         │
     │ detaches only L's payload,   │ │ prune_ascend: P bounds it to  │
     │ at most two node levels,     │ │ one byte; detaches L's empty  │
     │ allocates nothing; P is now  │ │ payload; the run ends and the │
     │ a childless value-free leaf: │ │ next run starts at the new    │
     │ repeat from the top with P   │ │ rightmost leaf                │
     └──────────────────────────────┘ └───────────────────────────────┘
```

Legend: the boxes are the steps of one iteration of the inner loop of Algorithm 2; the middle box
is its only decision, the left branch continues the run one edge higher and the right branch
ends it. "Allocates nothing" excludes the copy-on-write clone that any write to a shared node
makes.

The proof that the cut is bounded rests on premises read from the PathMap 0.3.4 sources. They
are premises, not theorems of this study; fix leaf [L1](#l1-bound-and-model) states them as
explicit hypotheses of the formal model.

| Premise | Statement | Source |
|---|---|---|
| P1 | A write zipper's focus node is the deepest node on its path: the zipper descends below every child pointer whose key is a proper prefix of the rest of its path. A focus that is not the zipper root has a non-empty node key. | `pathmap-0.3.4/src/write_zipper.rs::descend_step_internal`; `pathmap-0.3.4/src/write_zipper.rs::remove_branches`, `pathmap-0.3.4/src/write_zipper.rs::remove_val` and `pathmap-0.3.4/src/write_zipper.rs::set_val` assert, for an empty node key, that the zipper is at its root (the first through the depth of its node stack). |
| P2 | The node that holds a write zipper's focus is a line-list node, a dense node or an empty node. | The manifests request `arena_compact` and PathMap's default features (`graft_root_vals`, `slim_ptrs`, `serialization`), so the optional bridge nodes are not compiled; the mutating methods of the read-only tiny reference nodes are `unreachable!()` (`pathmap-0.3.4/src/tiny_node.rs::node_remove_all_branches`), and no debug run reached them (inference). |
| P3 | Every line-list node satisfies the invariants that `pathmap-0.3.4/src/line_list_node.rs::validate_node` checks in debug builds: a used slot has a non-empty key; slot 0 has no child pointer when slot 1's key strictly extends slot 0's; slot 1's key is never a proper prefix of slot 0's; two child pointers never share a key; two unequal keys that share a non-empty prefix are related by prefix; two keys never share more than one leading byte. | the function's checks |
| P4 | When the focus is an existing path inside a dense node, its node key is one byte. | `pathmap-0.3.4/src/dense_byte_node.rs::node_remove_all_branches` returns `false` for a longer key and asserts one byte otherwise; the prototype's debug assertion that the cut removed something never fired (inference). |
| P5 | A value at the end of a path is stored in the slot or cell whose key ends there, never inside the node below it. | the payload layout of `ValOrChildUnion` (a slot holds a value or a child pointer), and the comment in `pathmap-0.3.4/src/zipper.rs::read_zipper_shared_node_id` that values associated with nodes that can be shared are stored outside the node |
| P6 | At a focus with a non-empty node key $`\kappa`$, `prune_path` first calls `node_remove_dangling` on the focus node with $`\kappa`$ and a keep length, which is 0 for a zipper rooted at the map's root. On a line-list node that call takes the payload of a slot whose key equals $`\kappa`$ and points to an empty node, and returns the length of $`\kappa`$, or one less when the other slot's key starts with the same byte; on a dense node and a one-byte $`\kappa`$ it removes the cell of $`\kappa`$ when the cell has no value and no child pointer or a pointer to an empty node, and returns 1; otherwise it returns 0. Only after a positive count does `prune_path` call `prune_path_internal`, which returns 0 unless the focus node is now empty and otherwise climbs to the first ancestor that is the zipper root, holds a value or has more than one branch. `prune_path` returns the larger count. | `pathmap-0.3.4/src/write_zipper.rs::prune_path`, `pathmap-0.3.4/src/line_list_node.rs::node_remove_dangling`, `pathmap-0.3.4/src/dense_byte_node.rs::node_remove_dangling`, `pathmap-0.3.4/src/write_zipper.rs::prune_path_internal` |
| P7 | The navigation and mutation calls of Phase B are loops over heap-allocated stacks and use at most a constant native stack $`S_{\mathrm{nav}}`$, whatever the path depth. | read from the zipper sources (the branch count used by pruning, `pathmap-0.3.4/src/trie_node.rs::node_count_branches_recursive`, looks one node deep and does not recurse); the guard's small-stack tests at path depths up to 65,536 pass at the studied commit and on the prototype |

**Lemma 1 (one slot carries the cut).** Let the focus be a non-root parent $`P`$ that has no
value and exactly one child byte $`b`$, and let its child $`L = P\,b`$ have no value and no child.
If the focus node $`N`$ is a line-list node with node key $`\kappa`$, then exactly one slot of
$`N`$ has a key that starts with $`\kappa`$, and either (i) that key is $`\kappa\,b`$ and its
payload is a child pointer to an empty node $`X`$, or (ii) that key is $`\kappa`$ and its payload is
a child pointer to a node $`Y`$ whose only path is the dangling byte $`b`$.

*Proof.* By P1, $`\kappa`$ is not empty, and the paths below $`P`$ are stored in $`N`$, either in
a slot whose key extends $`\kappa`$ or below a child pointer stored at key exactly $`\kappa`$;
since $`P`$ has a child, at least one slot starts with $`\kappa`$. Take a slot whose key is
$`\kappa\,r`$ with $`r`$ non-empty. Its first byte is $`b`$, the only child byte of $`P`$. If
$`r`$ had a second byte, $`L`$ would have a child, so $`r = b`$. Its payload is not a value,
because $`L`$ has none, so it is a child pointer; the node it points to holds no path, since any
path in it would be a child of $`L`$, and by P5 it holds no value of $`L`$; so it is empty: case
(i). Take instead a slot whose key is exactly $`\kappa`$. Its payload is not a value, because
$`P`$ has none, so it is a child pointer to a node $`Y`$ that holds the paths below $`P`$; these
are exactly the dangling byte $`b`$: case (ii). Hence every key that starts with $`\kappa`$ is
$`\kappa`$ or $`\kappa\,b`$, and it carries a child pointer. Two slots cannot both start with
$`\kappa`$: two keys $`\kappa\,b`$, or two keys $`\kappa`$, would be two child pointers under one
key, excluded by P3; a child pointer at $`\kappa`$ in slot 0 with $`\kappa\,b`$ in slot 1 is
excluded by P3; and $`\kappa\,b`$ in slot 0 with $`\kappa`$ in slot 1 makes slot 1's key a proper
prefix of slot 0's, also excluded by P3. $`\square`$

**Lemma 2 (the cut is bounded).** Under the hypotheses of Lemma 1, `remove_branches(false)` at
$`P`$ (a) allocates no node beyond a copy-on-write clone, (b) leaves $`P`$ in the trie without a
value and without a child and changes no other path, and (c) drops a detached part whose nesting
depth is $`D \le 2`$.

*Proof.* By P1 the call takes the branch of `remove_branches` for a non-empty node key. With
`prune = false`, `pathmap-0.3.4/src/write_zipper.rs::node_prune_limit` returns `usize::MAX`, so
the call is `node_remove_all_branches` on $`N`$ with key $`\kappa`$ and limit `usize::MAX`,
followed by no prune. $`N`$ belongs to the trie being torn down alone: the write zipper made
every node it entered unique (`pathmap-0.3.4/src/write_zipper.rs::descend_step_internal`, and for
the root node `pathmap-0.3.4/src/write_zipper.rs::top_mut`, call `make_mut`, which clones a node
only when another trie shares it), exactly as for the barrier writes at the studied commit. By
P2, $`N`$ is a line-list node or a dense node; it is not empty, because it holds the path to
$`L`$.

- $`N`$ is a line-list node. By Lemma 1 exactly one slot $`i`$ starts with $`\kappa`$, and
  `pathmap-0.3.4/src/line_list_node.rs::node_remove_all_branches` selects it, because its key is
  longer than $`\kappa`$ in case (i) and it holds a child pointer in case (ii). With the limit
  `usize::MAX` that function passes `prune = false` to
  `pathmap-0.3.4/src/line_list_node.rs::remove_subtries` and does not call
  `pathmap-0.3.4/src/line_list_node.rs::preserve_prune_limit`, which requires a limit below the
  key length. Because the other slot does not start with $`\kappa`$ (Lemma 1), `remove_subtries`
  takes its non-pruning branch for slot $`i`$: it shortens the slot's key to $`\kappa`$ with
  `pathmap-0.3.4/src/line_list_node.rs::shorten_key_len` and swaps its payload for a pointer to
  the empty sentinel made by `pathmap-0.3.4/src/trie_node.rs::new_empty`, which allocates
  nothing. The other slot is untouched. Afterwards slot $`i`$ ends a dangling path at $`P`$, so
  $`P`$ exists without a value or a child and no other path changed: (a) and (b). The swapped-out
  payload is dropped. In case (i) it points to the empty node $`X`$: the sentinel is neither
  counted nor freed, and an allocated empty node has nothing below it, so $`D \le 1`$. In case
  (ii) it points to $`Y`$, a line-list node or a dense node that holds only the dangling byte
  $`b`$: if $`Y`$ is shared, only its count falls; otherwise its destructor frees at most one
  empty node below it, in place or through the iterative path, so $`D \le 2`$.
- $`N`$ is a dense node. By P4, $`\kappa`$ is a single byte, written $`a`$. The cell of $`a`$ has
  no value (the trie is value-free) and a child pointer to the node $`Y`$ that holds the paths
  below $`P`$, which are exactly the dangling byte $`b`$. With a non-zero limit,
  `pathmap-0.3.4/src/dense_byte_node.rs::node_remove_all_branches` clears only that child
  pointer and keeps the cell, so $`P`$ still exists without a value or a child; nothing is
  allocated; and $`Y`$ frees at most one empty node below it: $`D \le 2`$.

These cases are drawn below. $`\square`$

```text
 case (i): N is a line-list node and slot i has key κ b
 ┌─────────────────────────────────────────────────────┐
 │ N.slot i: key κ b, child pointer to an empty node X │
 └─────────────────────────────────────────────────────┘
 effect: the key is cut to κ and the payload is replaced by the empty
 sentinel; X is freed if it is an allocated node: nesting depth at most 1

 case (ii): N is a line-list node and slot i has key κ with a child pointer
 ┌────────────────────┐   ┌─────────────────────────────┐
 │ N.slot i: key κ,   │──►│ Y: one slot, key b,         │
 │ child pointer to Y │   │ child pointer to an empty X │
 └────────────────────┘   └─────────────────────────────┘
 effect: the payload is replaced by the empty sentinel; Y is dropped and
 frees X if X is an allocated node: nesting depth at most 2

 case (iii): N is a dense node and κ is the single byte a
 ┌─────────────────────┐   ┌──────────────────────┐
 │ N.cell a: no value, │──►│ Y: only the dangling │
 │ child pointer to Y  │   │ path b below it      │
 └─────────────────────┘   └──────────────────────┘
 effect: set_rec_option(None) clears the child pointer; Y is dropped and
 frees its empty child node, if it has one: nesting depth at most 2
```

Legend: a box is a node, a row inside it a slot or a cell, and an arrow a child pointer; the
text below each case states what `remove_branches(false)` at $`P`$ changes and what it drops. In
case (ii), $`Y`$ is drawn as a line-list node; a dense $`Y`$ whose only cell is $`b`$ behaves the
same way.

**Lemma 3 (the stopping cut is one byte).** Let the write zipper be rooted at the map's root, as
in Phase B, and let its focus be a childless value-free leaf $`L = P\,b`$ whose parent $`P`$ is the
zipper root or has more than one child. Then `prune_ascend` removes exactly the byte $`b`$,
detaches only the payload that represents $`L`$, and the detached part has nesting depth
$`D \le 1`$.

*Proof.* Write $`N_L`$ for the node that holds the focus and $`\kappa_L`$ for the node key of the
focus; $`\kappa_L`$ is not empty by P1.

First, $`\kappa_L`$ is the single byte $`b`$. If $`N_L`$ is a dense node this is P4. Let $`N_L`$ be
a line-list node. If $`P`$ is the zipper root, the path of $`L`$ is the single byte $`b`$, of which
$`\kappa_L`$ is a non-empty suffix. If $`P`$ has more than one child, suppose that $`\kappa_L`$ had
two or more bytes. Then $`P`$ lies inside $`N_L`$ with the non-empty node key $`\kappa_P`$,
which is $`\kappa_L`$ without its last byte, and by P1 no slot of $`N_L`$ holds a child pointer at
$`\kappa_P`$, since the zipper would then have descended below it. So a second child byte
$`c`$ of $`P`$, with $`c`$ different from $`b`$, is stored in a slot whose key starts with
$`\kappa_P\,c`$, while the path to $`L`$ is stored in a slot whose key starts with $`\kappa_P\,b`$.
These two keys share the non-empty prefix $`\kappa_P`$ and neither is a prefix of the other,
which P3 excludes.

Second, $`L`$ is represented in $`N_L`$ by exactly one payload. In a line-list node it is a slot
with key exactly $`b`$ and a child pointer to an empty node: a longer key would give $`L`$ a child,
a value would be a value of $`L`$, and a second slot whose key starts with $`b`$ is excluded as in
the proof of Lemma 1. In a dense node it is the cell of $`b`$, with no value and either no child
pointer or a pointer to an empty node.

Third, by P6 `prune_path` calls `node_remove_dangling` on $`N_L`$ with key $`b`$ and keep length 0.
On a line-list node it takes the payload of the slot with key $`b`$ and returns 1, since the other
slot's key does not start with $`b`$; on a dense node it removes the cell of $`b`$ and returns 1.
If $`P`$ has more than one child, $`P`$ is the root of $`N_L`$, because $`\kappa_L`$ is one byte,
and its other children remain in $`N_L`$; so $`N_L`$ is not empty and `prune_path_internal`
returns 0. If $`P`$ is the zipper root with the single child $`b`$, $`N_L`$ is the root node and is
now empty; `prune_path_internal` climbs from the one-byte path to the zipper root, where its loop
ends, finds nothing to remove in the empty root node, and returns 1. In both cases `prune_path`
returns 1 and `prune_ascend` moves the focus to $`P`$. The detached payload is a pointer to an
empty node or an empty cell, so $`D \le 1`$. The prototype also asserts in debug builds that the
prune removed exactly one byte; the assertion never fired. $`\square`$

**Theorem 1 (candidate (e) is bounded).** Algorithm 2 empties a value-free trie of $`T`$ logical
edges in exactly $`T`$ cuts; every cut detaches one logical edge, allocates no node beyond a
copy-on-write clone, and needs at most

```math
S_{(e)} \le S_{\mathrm{nav}} + \max\left(2f + c_0,\ S_{\mathrm{iter}}\right) = S_{\mathrm{nav}} + 2f + c_0
```

of native stack, independent of the path depth and of $`T`$; the equality holds for the measured
constants, since $`S_{\mathrm{iter}}`$ is 144 bytes against $`2 \cdot 80 + 72 = 232`$ in the
release build and 263 bytes against $`2 \cdot 128 + 64 = 320`$ in the debug build.

*Proof.* Each iteration of the inner loop starts at a childless value-free leaf $`L`$:
`descend_to_rightmost_leaf` ends at one, and after a cut at a non-stopping parent that parent is
one by Lemma 2(b). If $`L`$'s parent $`P`$ stops the run, Lemma 3 applies and the run ends with
the focus at $`P`$; in Phase B, $`P`$ is then the root or a fork that keeps at least one child,
so the next run finds a leaf below it unless the root has no child left, which ends the outer
loop. Otherwise $`P`$ is not the root, has no value, and has exactly one child (it has $`L`$, and
not more than one because it does not fork), so the hypotheses of Lemma 1 hold and Lemma 2
applies. Either way exactly one logical edge leaves the trie and none is added, so after $`T`$
cuts no edge remains. The stack of a cut is the navigation's $`S_{\mathrm{nav}}`$ (P7) plus the
destructor of the detached part: a recursive drop of nesting depth at most 2, bounded by
$`S_{\mathrm{drop}}(2) = 2f + c_0`$ ([Section 1.3](#13-what-the-destructor-recurses-over)), or the
iterative path, $`S_{\mathrm{iter}}`$. $`\square`$

Since no cut writes a value, Phase B no longer calls `set_val`, and with it the whole split path
of [Section 1.2](#12-where-the-allocation-calls-come-from) disappears. The remaining allocation
calls are those of the read-zipper forks, the write zipper's buffers and copy-on-write clones,
which the barrier protocol makes as well. Verdict: **survives**; it is the candidate measured in
[Section 3](#3-feasibility-measurement-of-candidate-e).

### 2.6 Verdicts

| Candidate | Verdict | Deciding evidence |
|---|---|---|
| (a) public removal items | only `remove_branches(false)` below the root and the parent-bounded prune survive, as parts of (e) | the item table of Section 2.1 |
| (b) one barrier per $`k`$ edges | feasible in principle for $`k \ge 29`$ by estimate; dominated by (e) | estimates under assumptions A1 and A2; the worst-case bound misses both thresholds at $`k = 64`$; the stack bound grows with $`k`$; it breaks the one-edge contract |
| (c) barriers at node boundaries | rejected | boundaries of unshared nodes are not exposed; a boundary barrier still splits |
| (d) reuse of one allocation | inapplicable | the allocation is the split's node; no public item recycles node storage |
| (e) barrier-free one-edge cut | survives | Theorem 1; measured in Section 3 |

## 3. Feasibility measurement of candidate (e)

### 3.1 The prototype

The prototype changes only Phase B of `drain_pathmap_with_provider` in the guard
[`guarded_pathmap.rs`](../../../rspace++/src/rspace/guarded_pathmap.rs) and adds one function;
the live removal path (`rspace++/src/rspace/guarded_pathmap.rs::remove_leaf_with_floor` and the
barrier functions it uses) is unchanged. The diff `prototype.diff` adds 62 lines and removes 32.
Its new function is

```rust
fn cut_value_free_leaf_and_bubble<V>(zipper: &mut WriteZipperUntracked<'_, '_, V>) -> usize
where V: Clone + Send + Sync + Unpin + 'static {
    debug_assert!(zipper.path_exists());
    debug_assert!(!zipper.is_val());
    debug_assert_eq!(zipper.child_count(), 0);
    let mut pruned_edges = 0usize;
    loop {
        if zipper.at_root() {
            break;
        }
        let child_byte = *zipper
            .path()
            .last()
            .expect("a non-root leaf has a final path byte");
        let child_depth = zipper.path().len();
        let ascended = zipper.ascend_byte();
        debug_assert!(ascended);
        let parent_depth = zipper.path().len();
        debug_assert_eq!(child_depth, parent_depth + 1);
        if zipper.at_root() || zipper.is_val() || zipper.child_count() > 1 {
            let descended = zipper.descend_to_existing_byte(child_byte);
            debug_assert!(descended);
            debug_assert_eq!(zipper.path().len(), child_depth);
            let pruned = zipper.prune_ascend();
            debug_assert_eq!(pruned, 1);
            debug_assert_eq!(zipper.path().len(), parent_depth);
            pruned_edges += pruned;
            break;
        }
        let removed = zipper.remove_branches(false);
        debug_assert!(removed);
        debug_assert!(zipper.path_exists());
        debug_assert_eq!(zipper.child_count(), 0);
        pruned_edges += 1;
    }
    pruned_edges
}
```

and Phase B calls it once per run in place of the barrier install and
`remove_valued_leaf_and_bubble_with_provider`, then reports one `pruned(1)` event per removed
edge to the observer and no barrier event. The barrier provider is no longer called by the final
teardown. A separate scratch module (`stack-probe.diff`, not part of the prototype) holds the
destructor probe of Section 1.3, the dense-spine tests and two adapted controls.

### 3.2 Method

Each step was registered in the study's ledger with its hypothesis and refutation criterion
before it ran. The builds used exports of the studied commit, `--locked --offline -j 8`, and the
campaign's resource wrapper; the release build carried debug information so that Heaptrack could
name inlined frames, exactly as in the record. The four Heaptrack recordings ran the record's
measurement test
`rspace++/tests/guarded_pathmap/two_phase_teardown_properties.rs::final_anchor_teardown_measurement`
with one trial, under a copy of the Heaptrack wrapper made exactly as the record describes, and
each was rendered by one `heaptrack_print` call; a copy of the record's extraction script
computed $`A_{\mathrm{drop}}`$, and an independent recount confirmed it. The registered
prediction for candidate (e) was that $`A_{\mathrm{drop}}`$ falls to the non-split remainder of
the record, 1,866 and 505, refuted if either result exceeded its threshold, if any small-stack
test failed, or if a debug assertion of the new code fired.

### 3.3 Allocation results

| Quantity | Realistic 256, studied commit | Realistic 256, prototype | Line 16384, studied commit | Line 16384, prototype | Log |
|---|---|---|---|---|---|
| allocation calls of the whole run | 190,832 | 8,209 | 33,602 | 1,618 | `40-analyse-*.log` |
| $`A_{\mathrm{drop}}`$ | 184,489 | 1,866 | 32,489 | 505 | `40-analyse-*.log`, `41-recount.log` |
| final owner's part | 183,522 | 1,814 | 16,049 | 57 | `40-analyse-*.log` |
| donors' part | 967 | 52 | 16,440 | 448 | `40-analyse-*.log` |
| $`N_{\mathrm{split}}`$ | 182,623 | 0 | 31,984 | 0 | `41-recount.log` |
| heaviest single stack under the teardown | 181,708 | 1,024 | 15,992 | 390 | `40-analyse-*.log` |
| the record's cross-check: calls of the measurement test outside `measurement_map` | 183,525 | 1,817 | 16,052 | 60 | `40-analyse-*.log` |
| temporary allocations, `heaptrack_print` counter | 178,833 | 1,103 | 31,283 | 82 | `heaptrack-*.report.txt` |
| peak heap, as printed | 403.73K | 403.71K | 253.56K | 254.47K | `heaptrack-*.report.txt` |
| threshold of R2a | 18,170 | 18,170 | 1,638 | 1,638 | the record |

The prediction holds to the call. The reductions are

```math
\frac{184489}{1866} = 98.9,
\qquad
\frac{32489}{505} = 64.3,
```

both above ten, and both results lie under their thresholds. The per-site attribution of
[Section 1.2](#12-where-the-allocation-calls-come-from) is identical before and after for every
site except `split_0`, which falls from 182,623 and 31,984 to zero (log
`42-classify-a-drop.log`). The studied commit's numbers equal the record's to the call, so the
baseline of this comparison is the record itself. Peak heap changes by less than 0.4 percent.

### 3.4 Tests on the prototype

| Test set | Release build | Debug build | Log |
|---|---|---|---|
| tests named `guarded_pathmap`, studied commit (control) | 39 passed, 0 failed, 3 ignored | not run | `50-tests-base-release-guarded.log` |
| tests named `guarded_pathmap`, prototype | 33 passed, 6 failed, 3 ignored | 33 passed, 6 failed, 3 ignored | `50-tests-proto-*-guarded.log` |
| scratch probe module (dense spine on 256 KiB, overflow control, adapted controls) | 4 passed, 2 ignored | 4 passed, 2 ignored | `50-tests-proto-*-probe.log` |
| destructor stack probe | 1 passed | 1 passed | `50-tests-proto-*-stack-probe.log` |

The six failures are exactly the six tests predicted before the run, each of which asserts the
barrier protocol itself:

1. `rspace++/src/rspace/guarded_pathmap.rs::barrier_install_prune_remove_order_is_observable_and_exact`
   observes `[Value, Prune(1), Prune(1)]` where it expects
   `[Value, Install, Install, Install, Prune(1), Prune(1), Remove, Remove, Remove]`.
2. `rspace++/tests/guarded_pathmap/two_phase_teardown_properties.rs::compressed_navigation_preserves_empty_root_and_long_dangling_line`,
3. `rspace++/tests/guarded_pathmap/two_phase_teardown_properties.rs::compressed_navigation_preserves_dense_comb_and_asymmetric_siblings` and
4. `rspace++/tests/guarded_pathmap/two_phase_teardown_properties.rs::compressed_navigation_matches_scalar_generated_topology_histories`
   compare the observer's events with a reference list of the barrier protocol (via
   `rspace++/tests/guarded_pathmap/two_phase_teardown_properties.rs::assert_navigation_drain`).
5. `rspace++/tests/guarded_pathmap/stateful_retirement.rs::stateful_retirement_preserves_cow_sibling_and_value_free_topology`
   asserts that the barrier provider was called.
6. `rspace++/tests/guarded_pathmap/stateful_retirement.rs::stateful_retirement_panics_fail_stop_without_raw_unwind`
   expects the injected panics of the barrier factory and discarder to abort the process; they
   are no longer called, so the factory mode's child process returns and reaches the test's own
   "must not return" assertion instead of aborting.

For tests 1 to 4 the script `55-compare-event-lists.py` removed the barrier events from each
printed reference list and compared the rest with the observed list: all 32 printed event-list
comparisons are equal after the removal, in both builds (the largest has 8,192 observed events
against a reference of 24,578, of which 16,386 are barrier events; logs
`55-compare-event-lists-proto-*.log`). For tests 5 and 6 the scratch module holds copies with
the barrier assertions replaced by their barrier-free counterparts (the provider is never
called; the factory and discarder modes return normally; the disposer mode still aborts); both
pass, and every copy-on-write assertion is kept: the sibling trie still holds its 257 values and
its dangling path after the original is retired. In the debug build no assertion of the new code
fired; every panic location in the logs is a test assertion.

All seven small-stack tests of the guard pass on the prototype in both builds:
`rspace++/src/rspace/guarded_pathmap.rs::root_and_dense_children_drain_in_exact_byte_preorder_on_256k_stack`,
`rspace++/src/rspace/guarded_pathmap.rs::deep_value_and_value_free_lines_drain_on_256k_stack`
(65,536-byte lines),
`rspace++/src/rspace/guarded_pathmap.rs::guarded_topology_only_line_uses_final_anchor_teardown_on_256k_stack`
(65,536 bytes),
`rspace++/src/rspace/guarded_pathmap.rs::deep_comb_drain_is_stack_bounded_and_byte_preorder`
(depth 4,096),
`rspace++/src/rspace/guarded_pathmap.rs::prefix_drain_of_comb_preserves_every_outside_sibling`
(depth 1,024),
`rspace++/tests/guarded_pathmap/stateful_retirement.rs::stateful_deep_branching_and_value_free_retirement_use_256k_stack`
(depth 8,192), and `rspace++/src/rspace/guarded_pathmap.rs::panicking_disposer_clone_child`,
which runs on a 256 KiB thread in the child process of
`rspace++/src/rspace/guarded_pathmap.rs::panicking_disposer_clone_precedes_raw_header_clone`.

### 3.5 Stack evidence

The scratch test that retires the dense spine of 16,384 levels (one value at the end of the
spine) through `rspace++/src/rspace/guarded_pathmap.rs::retire_pathmap` on a 256 KiB thread
passes in both builds, disposes of the one value, and reports that no barrier was made
(`depth=16384 barriers_made=0`). The positive control drops the same shape with PathMap's own
destructor on a 256 KiB thread in a child process, which dies by SIGABRT with Rust's
stack-overflow message (`code=None signal=Some(6)`). With the measured $`f = 80`$ bytes, that
plain drop would need about 1.25 MiB. Theorem 1 bounds the destructor's part of the prototype's
stack per cut by 232 bytes in the release build, plus the allocator's own frames.

### 3.6 Wall time (indicative only)

Five trials per coordinate ran in one process each, pinned to one processor, the studied commit
and the prototype back to back (log `70-time-teardown.log`). The load average was 98.65 on a
host with 32 logical processors throughout; the prototype's trials were bimodal, and the
studied commit's first realistic trial (126.88 ms) was an outlier.

| Coordinate | Studied commit, median of 5 | Prototype, median of 5 | Ratio |
|---|---|---|---|
| realistic 256 | 51.95 ms | 57.12 ms (trials 59.75, 60.25, 57.12, 22.73, 22.76) | 1.10 |
| line 16384 | 10.50 ms | 5.15 ms (trials 5.15, 6.76, 5.75, 1.74, 1.74) | 0.49 |

The registered criterion did not interpret ratios between one half and two; the realistic ratio
lies inside that band and the line ratio just outside it, in the direction of the prototype. With
this load and these spreads, the study draws no wall-time conclusion.

## 4. Decision

### 4.1 R2a holds

R2a has three conditions, and each is met by candidate (e):

1. *The released PathMap 0.3.4 public interface.* The cut uses only `remove_branches(false)`,
   `prune_ascend` and zipper navigation, all public methods of the pinned release; the guard
   needs no other dependency and no feature change.
2. *At least tenfold, under both thresholds.* Measured with the record's protocol, where
   $`A^{(e)}_{\mathrm{drop}}`$ is the count of the prototype:

   ```math
   A^{(e)}_{\mathrm{drop}}(\mathrm{realistic}, 256) = 1866 \le 18170,
   \qquad
   A^{(e)}_{\mathrm{drop}}(\mathrm{line}, 16384) = 505 \le 1638,
   ```

   factors of 98.9 and 64.3.
3. *A provable native-stack bound that does not grow with path depth.* Theorem 1 proves
   $`S_{(e)} \le S_{\mathrm{nav}} + \max(2f + c_0, S_{\mathrm{iter}})`$ from the premises P1 to
   P7; $`f`$, $`c_0`$ and $`S_{\mathrm{iter}}`$ were measured; the dense-spine test confirms the
   bound on the shape that defeats PathMap's own destructor.

Remedy R2b is therefore not needed, and no R2b options are listed.

### 4.2 What was not established

- The proof of Theorem 1 is a paper proof from premises P1 to P7, which were read from the
  PathMap sources and, for P2 and P4, partly inferred; it is not mechanized. Fix leaf L1 owns the
  mechanized version.
- Lemma 3 covers stopping parents that are the root or a fork. A valued stopping parent does not
  occur in Phase B, and the function's behaviour there is not covered.
- The probe measured the stack depth at which the deepest value is dropped; the allocator's
  frames below that point were not measured, so $`f`$, $`c_0`$ and $`S_{\mathrm{iter}}`$ describe
  PathMap's own frames.
- The guarded matching workload of the record (measurement M5: 20,000 binds, with
  48,461,906 allocation calls under the teardown at the measured commit) was not re-measured on
  the prototype; its reduction is an expectation, not a measurement.
- The tests of the `models` crate, whose trie codec retires tries through `retire_pathmap`
  ([`epathmap_trie_codec.rs`](../../../models/src/rust/epathmap_trie_codec.rs)), and the tests of
  every other crate were not run on the prototype.
- No production value type was measured. That the cut also removes the boxed barrier values of
  `ChannelSlot` and `Par` follows from the code (no value is written), but it was not observed.
- Candidate (b) was not prototyped; its figures are estimates under assumptions A1 and A2, and
  its bound $`\alpha = 5`$ is an inference from the sources.
- The destructor's per-node stack was measured for list nodes and ordinary dense nodes; the
  cell-based dense variant (`CellByteNode`) was not probed separately (inference: its cells drop
  their child pointers the same way).
- Wall time is not established (Section 3.6).
- Clippy was not run on the prototype; rustfmt was, and passed after one fix.

## 5. Fix leaves

The leaves follow the campaign's formal-first order: the contract is changed in the models
before the code changes.

### L1. Bound and model

- **Component:** the formal teardown contract: the Rocq model
  [`PathMapReversePruneTeardown.v`](../../../formal/rocq/stack_safe_pda/theories/PathMapReversePruneTeardown.v),
  its mutants file
  [`PathMapReversePruneTeardownMutants.v`](../../../formal/rocq/stack_safe_pda/theories/PathMapReversePruneTeardownMutants.v),
  the Verus projection
  [`pathmap_reverse_prune_teardown.rs`](../../../formal/verus/pathmap_reverse_prune_teardown.rs),
  and their registration in
  [`check-stack-safe-pda-proofs.sh`](../../../scripts/check-stack-safe-pda-proofs.sh) and in the
  manifest test
  [`formal_equivalence_manifest.rs`](../../../models/tests/formal_equivalence_manifest.rs).
- **Required outcome:** Phase B's segment of the model no longer installs and detaches barriers;
  a non-stopping step is a branch cut at a single-child value-free parent and a stopping step a
  one-byte prune at the root or a fork. The model proves that every step detaches exactly one
  logical edge, that Phase B writes no value, and that every real value is still disposed exactly
  once; a node-level refinement proves Lemmas 1 to 3, with premises P1 to P6 stated as an
  explicit well-formedness predicate on the modelled nodes rather than as axioms.
- **Acceptance:**
  1. `scripts/check-stack-safe-pda-proofs.sh` passes, with the closed-theorem counts updated
     in the runner and the manifest test, and every new theorem closed under the global context
     (no `Axiom`, `Admitted` or `admit`).
  2. The mutants file refutes at least three wrong segments: a cut at a forking parent, a cut at
     a valued parent, and an unbounded prune.
  3. The Verus projection describes the barrier-free step, verifies, and contains no `assume(`,
     `admit(` or `external_body`.
  4. `pathmap_reverse_prune_formal_first_artifacts_are_manifested` passes with the updated
     markers.
- **Dependencies:** none.

### L2. Implementation with allocation gate

- **Component:** `rspace++`: the guard
  [`guarded_pathmap.rs`](../../../rspace++/src/rspace/guarded_pathmap.rs) and its test files
  [`two_phase_teardown_properties.rs`](../../../rspace++/tests/guarded_pathmap/two_phase_teardown_properties.rs)
  and [`stateful_retirement.rs`](../../../rspace++/tests/guarded_pathmap/stateful_retirement.rs).
- **Required outcome:** Phase B of the final teardown implements Algorithm 2 (the prototype
  diff is one implementation of it); the live removal path and the signature of
  `retire_pathmap` stay as they are; the observer reports one `pruned(1)` per edge and no
  barrier events.
- **Acceptance:**
  1. The record's Heaptrack protocol, run on the implementation, gives
     $`A_{\mathrm{drop}}(\mathrm{realistic}, 256) \le 18170`$ and
     $`A_{\mathrm{drop}}(\mathrm{line}, 16384) \le 1638`$ with no `split_0` call under the
     teardown (the prototype gave 1,866 and 505).
  2. An allocation gate in the library tests counts the allocation calls of the final owner's
     drop at both coordinates and asserts both thresholds. The library test binary's global
     allocator already counts calls
     (`rspace++/tests/persistence/kind1_recovery_protocol_properties.rs::measure_allocations`,
     today visible only inside the persistence tests); a positive control shows that the gate
     fails for a drain that installs barriers.
  3. The six barrier-protocol tests of Section 3.4 assert the barrier-free protocol: the
     exact-order test expects `[Value, Prune(1), Prune(1)]`; the three compressed-navigation
     tests compare with a reference without barrier events; the copy-on-write test asserts that
     the provider is never called and keeps all its value and path assertions; the panic test
     expects the disposer's panic to abort and the provider's modes to complete.
  4. Every other test named `guarded_pathmap` passes in the release and the debug build,
     including `implementation_contains_no_snapshot_count_or_owned_raw_escape`.
  5. The tests of the `models` crate that retire tries through `retire_pathmap` pass.
  6. Measurement M5 is repeated under Heaptrack and its $`A_{\mathrm{drop}}`$ is reported against
     the record's 48,461,906.
  7. rustfmt passes and the build has no new warning.
- **Dependencies:** L1.

### L3. Small-stack gate

- **Component:** `rspace++`: the guard's fixed-stack tests in
  [`guarded_pathmap.rs`](../../../rspace++/src/rspace/guarded_pathmap.rs).
- **Required outcome:** a permanent test retires a dense spine of 16,384 levels (dangling teeth
  1 and 2 beside every spine byte, so every level is a dense node, and one value at the end of
  the spine) on a 256 KiB thread, both through `retire_pathmap` and through the final drop of a
  `GuardedPathMap`, beside a positive control in which PathMap's own drop of the same shape
  overflows a 256 KiB thread in a child process.
- **Acceptance:**
  1. Both retirements pass in the release and the debug build, dispose of the one value, and
     report that no barrier was made.
  2. The control's child process ends by SIGABRT with Rust's stack-overflow message.
  3. The seven existing small-stack tests of Section 3.4 pass.
- **Dependencies:** L2 (the gate tests the barrier-free implementation; whether the studied
  commit passes it was not measured).

### Preservation obligations

| Property of the leaf | Why the remedy leaves it unchanged | Evidence |
|---|---|---|
| trie compression | only the trie being torn down is changed, through public PathMap operations; live tries are never touched | adapted copy-on-write control: the sibling keeps every value and path |
| copy-on-write | a shared node is cloned before it is changed, by the write zipper's `make_mut`, exactly as at the studied commit | `clone_self` counts identical before and after (23 and 391) |
| algebra, serialization | Phase B calls no algebraic or serializing operation | the prototype's diff (inference) |
| multiplicity of values | Phase A, which removes and disposes every value once, is unchanged | the guard's disposal-count tests pass |
| canonical order | Phase A's byte preorder is unchanged; Phase B removes the same edges in the same order | the preorder tests pass; 32 of 32 event lists equal after removing barrier events |
| bytes, hashes | the teardown persists nothing and hashes nothing | the prototype's diff (inference) |
| COMM, charges, recovery authority | the teardown runs after the last owner is gone and calls no tuple-space, cost or persistence operation | the prototype's diff (inference; not tested at the tuple-space level) |

## Reproduction

The commands below are those of the study's scripts, with the paths written as placeholders:
`[WORKSPACE_ROOT]` is the repository root and `[ROOT]` the file-system root. `W` is the scratch
directory and `HEAVY` the campaign's resource wrapper. `W/base` and `W/src` are exports of the
studied commit; the study wrote the prototype into `W/src` and derived both diffs from it with
`diff -u` against `W/base`, so a reader reproduces the source state by applying them.

```bash
W='[WORKSPACE_ROOT]/target/scratch/session-401d0c58/c44/w14357'
HEAVY='[WORKSPACE_ROOT]/target/scratch/session-401d0c58/scripts/heavy.sh'
export TMPDIR="$W/tmp" TEMP="$W/tmp" TMP="$W/tmp"
export CARGO_TARGET_DIR="$W/target" CARGO_PROFILE_RELEASE_DEBUG=true
TEST='rspace::guarded_pathmap::two_phase_teardown_properties::final_anchor_teardown_measurement'
HT="$W/out/scripts/heaptrack-1.5.0-fifo-in-tmpdir.sh"
BASE="$W/bin/base-release/rspace_plus_plus-lib-test"
PROTO="$W/bin/proto-release/rspace_plus_plus-lib-test"
DEV="$W/bin/proto-dev/rspace_plus_plus-lib-test"

# Builds. After each one the unit-test executable is copied to the file named above.
cd "$W/src"
"$HEAVY" cargo test --release --locked --offline -p rspace_plus_plus --lib --no-run -j 8
patch -p1 < "$W/out/prototype.diff"
patch -p1 < "$W/out/stack-probe.diff"
"$HEAVY" cargo test --release --locked --offline -p rspace_plus_plus --lib --no-run -j 8
"$HEAVY" cargo test --locked --offline -p rspace_plus_plus --lib --no-run -j 8

# One recording per build and coordinate, rendered once (shown: prototype, realistic 256; the
# others use BASE or the line shape with RSPACE_TEARDOWN_SIZE=16384).
cd "$W/src/rspace++"
"$HEAVY" env -u DEBUGINFOD_URLS RSPACE_TEARDOWN_SHAPE=realistic RSPACE_TEARDOWN_SIZE=256 RSPACE_TEARDOWN_TRIALS=1 \
  "$HT" --record-only -o "$W/out/recordings/heaptrack-after-realistic-256" \
  "$PROTO" --exact "$TEST" --ignored --test-threads=1 --nocapture
"$HEAVY" '[ROOT]/usr/bin/heaptrack_print' -f "$W/out/recordings/heaptrack-after-realistic-256.zst" -t 0 -n 30 \
  -F "$W/out/logs/heaptrack-after-realistic-256.stacks.txt" --flamegraph-cost-type allocations \
  > "$W/out/logs/heaptrack-after-realistic-256.report.txt"

# Extraction, recount and site attribution.
python3 -I "$W/out/scripts/40-analyse-heaptrack.py" "$W/out/logs" rustfilt "$W/out/logs/40-after.json" \
  "$W/out/logs/40-after.tsv" after-realistic-256:lib after-line-16384:lib
python3 -I "$W/out/scripts/41-recount.py" "$W/out/logs" before-realistic-256 before-line-16384 \
  after-realistic-256 after-line-16384
python3 -I "$W/out/scripts/42-classify-a-drop.py" "$W/out/logs" rustfilt before-realistic-256 \
  after-realistic-256 before-line-16384 after-line-16384

# Tests, with core dumps off because several tests abort a child process on purpose; the same
# commands ran with BASE (control) and DEV.
"$HEAVY" bash -c 'ulimit -c 0 && exec "$0" "$@"' "$PROTO" guarded_pathmap --test-threads=8
"$HEAVY" bash -c 'ulimit -c 0 && exec "$0" "$@"' "$PROTO" pathmap_drop_stack_probe --test-threads=1 --nocapture
"$HEAVY" bash -c 'ulimit -c 0 && exec "$0" "$@"' "$PROTO" \
  --exact rspace::pathmap_drop_stack_probe::pathmap_destructor_stack_depth_probe --ignored --nocapture

# Indicative wall time: five trials per process, pinned to processor 27.
"$HEAVY" taskset -c 27 env RSPACE_TEARDOWN_SHAPE=realistic RSPACE_TEARDOWN_SIZE=256 RSPACE_TEARDOWN_TRIALS=5 \
  "$PROTO" --exact "$TEST" --ignored --test-threads=1 --nocapture
```

The copy of the Heaptrack 1.5.0 wrapper differs from the stock script in the two lines that the
record describes: its named pipe lives under `TMPDIR`, and it names the stock installation
directory.

## Evidence files (not committed)

| File | Bytes | SHA-256 |
|---|---|---|
| `recordings/heaptrack-before-realistic-256.zst` | 39,260 | `4fbc1b1f1ec9a7040bca9e13dfe8c9a1954fcfeecfa7ff6a07fb50e4eeb4a35b` |
| `recordings/heaptrack-before-line-16384.zst` | 37,174 | `59f0ebd2f323270aebe1e6747e6019fcaf91bc44c0fd7f1cd424bc07c55ca2a2` |
| `recordings/heaptrack-after-realistic-256.zst` | 37,169 | `d984555bc277317726d9738a381a5e8bbc0c1407313e297cca4390a190a9c185` |
| `recordings/heaptrack-after-line-16384.zst` | 36,433 | `88f50afbec66f4223db5b9de3727c13e629fb27e7935e9417908b9648088ab98` |
| `prototype.diff` | 5,373 | `51b7dfe94756dd02604efdb479abe02918f056d1458987b058189b61aee051f4` |
| `stack-probe.diff` | 12,271 | `2ebbfc4aee0fbcddcd2eabcad052ea2d9365246d66b2a06c740e5a7eb0c9d070` |
| library test executable, studied commit, release | 468,826,272 | `9bc4d771ddd7be4888c71dd406a04ebd6d7fbfc316832e8ea204c1b91cd64a31` |
| library test executable, prototype, release | 467,850,496 | `3f2c1efd282df8ecac07f3014d9cedade8735b90998fd6118a4ddbfae127e6c1` |
| library test executable, prototype, debug | 444,382,224 | `2d986990a4a54c8aed8a49816826c730276e86ec6dca91d75971891a03d147e8` |

The scratch directory's `results.md` lists every measurement and check with its exact command,
verdict and log, and `notes.md` is the ledger of hypotheses, results and errata in the order they
happened.

## References

- Donald R. Morrison. PATRICIA (Practical Algorithm To Retrieve Information Coded in
  Alphanumeric). *Journal of the Association for Computing Machinery*, 15(4):514–534, 1968.
  [doi:10.1145/321479.321481](https://doi.org/10.1145/321479.321481). The path-compressed trie
  whose compressed keys PathMap's line-list nodes store.
- James R. Driscoll, Neil Sarnak, Daniel D. Sleator and Robert E. Tarjan. Making data structures
  persistent. *Journal of Computer and System Sciences*, 38(1):86–124, 1989.
  [doi:10.1016/0022-0000(89)90034-2](https://doi.org/10.1016/0022-0000(89)90034-2). Path
  copying, the basis of copy-on-write sharing between tries.
- Donald E. Knuth. Literate programming. *The Computer Journal*, 27(2):97–111, 1984.
  [doi:10.1093/comjnl/27.2.97](https://doi.org/10.1093/comjnl/27.2.97). The presentation form of
  the algorithms above.
- [Guarded PathMap final-teardown allocations on PathMap 0.3.4 — 2026-11-12](measurements/guarded-pathmap-final-teardown-pathmap-0.3.4-2026-11-12.md):
  the measurement record whose protocol and numbers this study reuses.
- [Guarded PathMap final teardown and loaded-channel replacement — 2026-09-01](measurements/guarded-pathmap-final-teardown-and-loaded-replacement-2026-09-01.md):
  the PathMap 0.2.2 record that first described the two-phase teardown.
