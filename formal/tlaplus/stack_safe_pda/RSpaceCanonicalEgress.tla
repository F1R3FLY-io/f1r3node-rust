---------------------- MODULE RSpaceCanonicalEgress ----------------------
EXTENDS Naturals, TLC

(***************************************************************************
Finite-state publication model for trie-native RSpace canonical egress.

The implementation first validates every resident identity and computes the
exact output size, then builds the complete final cold-store envelope in
private storage, and finally publishes it with one history/cold-store action. A malformed
identity rejects before any public byte changes. The model deliberately keeps
the private byte contents abstract: Verus and Rust differential tests prove
their exact sequence, while TLC owns the publication lifecycle.
***************************************************************************)

CONSTANT MaxRecords

ASSUME MaxRecords \in Nat

Phases == {"preflight", "ready", "building", "committed", "rejected"}
VisibleImages == {"old", "new", "partial"}

VARIABLES recordCount, invalidAt, cursor, emitted, headerBuilt, phase, visible, publications

vars == <<recordCount, invalidAt, cursor, emitted, headerBuilt, phase, visible, publications>>

RecordValid(index) == invalidAt = 0 \/ index # invalidAt

Init ==
    /\ recordCount \in 0..MaxRecords
    /\ invalidAt \in 0..recordCount
    /\ cursor = 0
    /\ emitted = 0
    /\ headerBuilt = FALSE
    /\ phase = "preflight"
    /\ visible = "old"
    /\ publications = 0

ValidateNext ==
    /\ phase = "preflight"
    /\ cursor < recordCount
    /\ RecordValid(cursor + 1)
    /\ cursor' = cursor + 1
    /\ UNCHANGED <<recordCount, invalidAt, emitted, headerBuilt, phase, visible, publications>>

RejectMalformed ==
    /\ phase = "preflight"
    /\ cursor < recordCount
    /\ ~RecordValid(cursor + 1)
    /\ phase' = "rejected"
    /\ UNCHANGED <<recordCount, invalidAt, cursor, emitted, headerBuilt, visible, publications>>

FinishPreflight ==
    /\ phase = "preflight"
    /\ cursor = recordCount
    /\ invalidAt = 0
    /\ phase' = "ready"
    /\ UNCHANGED <<recordCount, invalidAt, cursor, emitted, headerBuilt, visible, publications>>

BuildHeader ==
    /\ phase = "ready"
    /\ ~headerBuilt
    /\ headerBuilt' = TRUE
    /\ phase' = "building"
    /\ UNCHANGED <<recordCount, invalidAt, cursor, emitted, visible, publications>>

BuildPrivate ==
    /\ phase = "building"
    /\ headerBuilt
    /\ emitted < recordCount
    /\ emitted' = emitted + 1
    /\ UNCHANGED <<recordCount, invalidAt, cursor, headerBuilt, phase, visible, publications>>

PublishComplete ==
    /\ phase = "building"
    /\ headerBuilt
    /\ emitted = recordCount
    /\ phase' = "committed"
    /\ visible' = "new"
    /\ publications' = publications + 1
    /\ UNCHANGED <<recordCount, invalidAt, cursor, emitted, headerBuilt>>

Next == ValidateNext \/ RejectMalformed \/ FinishPreflight
        \/ BuildHeader \/ BuildPrivate \/ PublishComplete

Spec == Init /\ [][Next]_vars

TypeInvariant ==
    /\ recordCount \in 0..MaxRecords
    /\ invalidAt \in 0..recordCount
    /\ cursor \in 0..recordCount
    /\ emitted \in 0..recordCount
    /\ headerBuilt \in BOOLEAN
    /\ phase \in Phases
    /\ visible \in VisibleImages
    /\ publications \in 0..1

PreflightCompletesBeforeBuild ==
    phase \in {"ready", "building", "committed"} =>
      /\ cursor = recordCount
      /\ invalidAt = 0

PrivateImageCompletesBeforePublish ==
    phase = "committed" => /\ headerBuilt
                           /\ emitted = recordCount

VisibleImageIsAtomic ==
    /\ (visible = "new") <=> (phase = "committed")
    /\ visible # "partial"

MalformedInputNeverPublishes ==
    invalidAt # 0 => visible = "old"

RejectedInputIsNonMutating ==
    phase = "rejected" =>
      /\ visible = "old"
      /\ publications = 0

ExactlyOnePublication ==
    /\ publications = 1 <=> visible = "new"
    /\ publications = 0 <=> visible = "old"

(***************************************************************************
Negative control. Enabling this transition exposes a partially built image.
The proof harness checks that TLC reports a VisibleImageIsAtomic violation;
therefore the main model cannot pass vacuously because the invariant is weak.
***************************************************************************)
PublishPartialMutant ==
    /\ phase \in {"ready", "building"}
    /\ emitted < recordCount
    /\ visible' = "partial"
    /\ UNCHANGED <<recordCount, invalidAt, cursor, emitted, headerBuilt, phase, publications>>

MutantNext == Next \/ PublishPartialMutant
MutantSpec == Init /\ [][MutantNext]_vars

(***************************************************************************
Negative control. This transition publishes a payload-sized private image
without constructing the frozen enum/length envelope first.
***************************************************************************)
PublishWithoutHeaderMutant ==
    /\ phase = "ready"
    /\ ~headerBuilt
    /\ phase' = "committed"
    /\ emitted' = recordCount
    /\ visible' = "new"
    /\ publications' = publications + 1
    /\ UNCHANGED <<recordCount, invalidAt, cursor, headerBuilt>>

HeaderMutantNext == Next \/ PublishWithoutHeaderMutant
HeaderMutantSpec == Init /\ [][HeaderMutantNext]_vars

(***************************************************************************
Negative control. This transition treats a malformed current identity as if
preflight had succeeded. It must violate the preflight-before-build invariant
before any private construction or publication is possible.
***************************************************************************)
SkipValidationMutant ==
    /\ phase = "preflight"
    /\ cursor < recordCount
    /\ ~RecordValid(cursor + 1)
    /\ phase' = "ready"
    /\ UNCHANGED <<recordCount, invalidAt, cursor, emitted, headerBuilt, visible, publications>>

ValidationMutantNext == Next \/ SkipValidationMutant
ValidationMutantSpec == Init /\ [][ValidationMutantNext]_vars

=============================================================================
