-------------------- MODULE NumericMergeAuthority --------------------
EXTENDS Naturals, FiniteSets

CONSTANTS PreserveAuthority, IncludeConsumedBase, IncludeRejected, CollapseSignatures
VARIABLES outputs, signatures, accepted, ready, pending, collected, published, stage

Writers == {1, 2}
Validators == {1, 2}
Regions == {1, 2}
BaseRegion == 3
vars == <<outputs, signatures, accepted, ready, pending, collected, published, stage>>

Expected == UNION {outputs[w] : w \in accepted}
Demand(rs) == [s \in {1} |-> Cardinality({r \in rs : signatures[r] = s})]
Canonical(rs) == IF CollapseSignatures
                THEN {r \in rs : \A q \in rs : signatures[q] = signatures[r] => r <= q}
                ELSE rs

Init == /\ outputs \in [Writers -> SUBSET Regions]
        /\ signatures \in [Regions \cup {BaseRegion} -> 0..1]
        /\ accepted \in (SUBSET Writers) \ {{} }
        /\ ready = {}
        /\ pending = [v \in Validators |-> Writers]
        /\ collected = [v \in Validators |-> {}]
        /\ published = [v \in Validators |-> {}]
        /\ stage = [v \in Validators |-> "read"]

Prepare(w) == /\ w \notin ready
              /\ ready' = ready \cup {w}
              /\ UNCHANGED <<outputs, signatures, accepted, pending, collected, published, stage>>

Read(v, w) == /\ stage[v] = "read"
              /\ w \in ready \cap pending[v]
              /\ pending' = [pending EXCEPT ![v] = @ \ {w}]
              /\ collected' = [collected EXCEPT ![v] =
                    @ \cup (IF w \in accepted \/ IncludeRejected THEN outputs[w] ELSE {})]
              /\ UNCHANGED <<outputs, signatures, accepted, ready, published, stage>>

Publish(v) == /\ stage[v] = "read"
              /\ pending[v] = {}
              /\ published' = [published EXCEPT ![v] =
                    IF PreserveAuthority
                    THEN Canonical(collected[v] \cup (IF IncludeConsumedBase THEN {BaseRegion} ELSE {}))
                    ELSE {}]
              /\ stage' = [stage EXCEPT ![v] = "published"]
              /\ UNCHANGED <<outputs, signatures, accepted, ready, pending, collected>>

Next == (\E w \in Writers : Prepare(w))
        \/ (\E v \in Validators, w \in Writers : Read(v, w))
        \/ (\E v \in Validators : Publish(v))

Spec == Init /\ [][Next]_vars

RetainedProvenance == \A v \in Validators : stage[v] = "published" => published[v] = Expected
NoConsumedBaseResurrection == \A v \in Validators : BaseRegion \notin published[v]
ExactFundingMultiplicity == \A v \in Validators : stage[v] = "published" => Demand(published[v]) = Demand(Expected)
UnitProvenance == \A v \in Validators : stage[v] = "published" =>
                    {r \in published[v] : signatures[r] = 0} = {r \in Expected : signatures[r] = 0}
ParallelValidatorAgreement == (\A v \in Validators : stage[v] = "published") => published[1] = published[2]

=====================================================================
