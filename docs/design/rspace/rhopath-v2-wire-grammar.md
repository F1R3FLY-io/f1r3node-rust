# RhoPathV2 outer wire grammar (proposal)

Status: proposed, non-activated. This document specifies the byte contract required
before any production RhoPathV2 adapter is implemented. RhoPathV1 remains the only
active path format.

## Goals and non-goals

RhoPathV2 gives channels an equality-faithful identity based on the validated RCI2
image. It must be disjoint from V1, prefix-free at every variable-length boundary,
independent of Rust allocation/layout, and compatible with a sidecar legacy-digest
bridge. A legacy digest is only a routing hint; it is never a semantic key.

This proposal does not change event, history, state-root, replay, or consensus bytes.
It also does not make RCI2 physical ACT bytes part of a key.

## Common framing

Every V2 key begins with the domain `RHP` followed by version byte `0x02` and a
one-byte kind:

```text
RhoPathV2 := `RHP` 02 kind payload
kind      := 01 datum | 02 continuation | 03 join
```

Variable byte strings use the existing order-preserving frame:

```text
frame(x) := zero_escape(x) 00 00
zero_escape(00) := 00 FF
```

The terminator cannot occur inside a frame. Every parser is a checked forward cursor;
it rejects malformed escapes, truncated frames, trailing bytes, and noncanonical
RCI2 payloads without recursive calls or an artificial depth limit.

## Datum keys

```text
datum_v2 := RHP 02 01
            frame(RCI2 channel_image)
            frame(RMP2 datum_structure)
            frame(canonical datum_identity)
```

The channel image is emitted once and is both the structural and semantic channel
identity. Consequently two `Par` channels compare equal exactly when their RCI2
frames compare equal, including the `connective_used` bit and excluding only the
top-level `locally_free` summary. The datum structure is the existing RMP2 structural
image; the final frame is the complete canonical datum identity required by replay.

## Continuation keys

```text
continuation_v2 := RHP 02 02
                   frame(RCI2 channel_image_1) ...
                   frame(RCI2 channel_image_n)
                   group_end
                   frame(canonical continuation_identity)
group_end        := 00 01
```

`n` is positive. The ordered image sequence is authoritative: duplicates and their
positions are retained. `group_end` is distinct from both an escaped zero (`00 FF`)
and a frame terminator (`00 00`). Continuation identity is source-free, as in V1;
source content remains the authoritative retained value.

## Join keys

```text
join_v2 := RHP 02 03
           frame(RCI2 indexed_channel_image)
           group_end
           canonical ordered join_identity bytes
```

The indexed channel is one complete RCI2 image. Join identity is not reordered or
hashed into a digest-only suffix.

## Legacy bridge and namespace policy

V2 keys live in a version-disjoint namespace from V1 (`RHP 02` versus `RHP 01`). A
store may contain both only when its manifest names the format version explicitly;
one PathMap authority must never silently interpret a V1 key as V2 or vice versa.

For each resident V2 channel, the bridge stores `{rci2_image, legacy_digest,
legacy_preimage_status}`. A legacy-digest bucket is allowed to contain multiple
images. Every hit compares complete RCI2 bytes; equal bytes reuse the first exact
legacy preimage, while different bytes remain distinct. Missing historical preimages
are a fail-closed migration condition, not an invitation to synthesize an image.

## Canonicality and proofs required before activation

The formal and executable gates must establish:

1. frame parsing and encoding are mutual inverses and prefix-free;
2. V2 datum/continuation/join keys are injective through RCI2/Par equality;
3. raw byte ordering is preserved wherever the caller relies on zipper order;
4. root `locally_free` changes do not change a channel frame, while
   `connective_used` changes do;
5. V1 and V2 domains are disjoint, and malformed/trailing input fails closed;
6. full-image comparison prevents legacy-digest collision aliasing;
7. ordered receive groups remain positional and unordered history remains separate;
8. play/replay/checkpoint/reset/export/import/state-sync differentials preserve all
   frozen V1 and legacy outputs.

The proposal is intentionally not an activation decision. Production use requires
the accepted formal model, generated parser/encoder, negative controls, complete
RSpace lifecycle differential, and an explicit consensus epoch.

