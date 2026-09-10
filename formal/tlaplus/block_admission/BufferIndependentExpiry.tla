------------------------- MODULE BufferIndependentExpiry -------------------------
EXTENDS Naturals, Sequences, FiniteSets

CONSTANTS Identities, PageSize, SharedCursor, PrependArrival, MoveDuplicate,
          IncompleteRemoval, EarlyReturn, SkipBatch, OverlapPasses

VARIABLES recovery, expiry, members, expectedRecovery, expectedExpiry,
          running, remaining, obligations, returnedIncomplete, batch, overlapped

vars == <<recovery, expiry, members, expectedRecovery, expectedExpiry,
          running, remaining, obligations, returnedIncomplete, batch, overlapped>>
Elements(sequence) == {sequence[index] : index \in DOMAIN sequence}
Without(sequence, key) == SelectSeq(sequence, LAMBDA element : element # key)
Rotate(sequence) == IF sequence = <<>> THEN sequence
                    ELSE Tail(sequence) \o <<Head(sequence)>>

Init ==
    /\ recovery = <<>>
    /\ expiry = <<>>
    /\ members = {}
    /\ expectedRecovery = <<>>
    /\ expectedExpiry = <<>>
    /\ running = FALSE
    /\ remaining = 0
    /\ obligations = {}
    /\ returnedIncomplete = FALSE
    /\ batch = <<>>
    /\ overlapped = FALSE

Insert(key) ==
    /\ recovery' = IF key \in members THEN recovery ELSE Append(recovery, key)
    /\ expiry' = IF key \in members
                 THEN IF MoveDuplicate THEN Append(Without(expiry, key), key) ELSE expiry
                 ELSE IF PrependArrival THEN <<key>> \o expiry ELSE Append(expiry, key)
    /\ expectedRecovery' = IF key \in members THEN expectedRecovery
                           ELSE Append(expectedRecovery, key)
    /\ expectedExpiry' = IF key \in members THEN expectedExpiry
                         ELSE Append(expectedExpiry, key)
    /\ members' = members \cup {key}
    /\ UNCHANGED <<running, remaining, obligations, returnedIncomplete, batch, overlapped>>

Remove(key) ==
    /\ recovery' = Without(recovery, key)
    /\ expiry' = IF IncompleteRemoval THEN expiry ELSE Without(expiry, key)
    /\ expectedRecovery' = Without(expectedRecovery, key)
    /\ expectedExpiry' = Without(expectedExpiry, key)
    /\ members' = members \ {key}
    /\ obligations' = obligations \ {key}
    /\ UNCHANGED <<running, remaining, returnedIncomplete, batch, overlapped>>

Recover ==
    /\ recovery # <<>>
    /\ recovery' = Rotate(recovery)
    /\ expiry' = IF SharedCursor THEN Rotate(expiry) ELSE expiry
    /\ expectedRecovery' = Rotate(expectedRecovery)
    /\ UNCHANGED <<expectedExpiry, members, running, remaining,
                   obligations, returnedIncomplete, batch, overlapped>>

Begin ==
    /\ ~running \/ OverlapPasses
    /\ overlapped' = running
    /\ running' = TRUE
    /\ remaining' = Cardinality(members)
    /\ obligations' = members
    /\ returnedIncomplete' = FALSE
    /\ batch' = <<>>
    /\ UNCHANGED <<recovery, expiry, expectedRecovery, expectedExpiry, members>>

Examine ==
    /\ running /\ remaining > 0 /\ Len(batch) < PageSize
    /\ expiry' = Rotate(expiry)
    /\ recovery' = IF SharedCursor THEN Rotate(recovery) ELSE recovery
    /\ expectedExpiry' = Rotate(expectedExpiry)
    /\ batch' = IF expiry = <<>> THEN batch ELSE Append(batch, Head(expiry))
    /\ remaining' = remaining - 1
    /\ UNCHANGED <<expectedRecovery, members, running, returnedIncomplete, obligations, overlapped>>

Process ==
    /\ running /\ batch # <<>>
    /\ obligations' = IF SkipBatch THEN obligations ELSE obligations \ {Head(batch)}
    /\ batch' = Tail(batch)
    /\ UNCHANGED <<recovery, expiry, expectedRecovery, expectedExpiry, members,
                   remaining, running, returnedIncomplete, overlapped>>

Return ==
    /\ running /\ ((remaining = 0 /\ batch = <<>>) \/ EarlyReturn)
    /\ returnedIncomplete' = (obligations # {} \/ batch # <<>>)
    /\ running' = FALSE
    /\ UNCHANGED <<recovery, expiry, expectedRecovery, expectedExpiry,
                   members, remaining, obligations, batch, overlapped>>

Next == (\E key \in Identities : Insert(key) \/ Remove(key))
        \/ Recover \/ Begin \/ Examine \/ Process \/ Return

TypeOK ==
    /\ recovery \in Seq(Identities)
    /\ expiry \in Seq(Identities)
    /\ Len(recovery) <= Cardinality(Identities)
    /\ Len(expiry) <= Cardinality(Identities)
    /\ members \subseteq Identities
    /\ obligations \subseteq members
    /\ remaining \in 0..Cardinality(Identities)
    /\ running \in BOOLEAN
    /\ returnedIncomplete \in BOOLEAN
    /\ overlapped \in BOOLEAN
    /\ batch \in Seq(Identities)
    /\ Len(batch) <= PageSize

Inv_Membership ==
    /\ Elements(recovery) = members
    /\ Elements(expiry) = members
    /\ Len(recovery) = Cardinality(members)
    /\ Len(expiry) = Cardinality(members)
Inv_IndependentOrder ==
    /\ recovery = expectedRecovery
    /\ expiry = expectedExpiry
Inv_PassCoverage == running /\ remaining = 0 /\ batch = <<>> => obligations = {}
Inv_CompleteReturn == ~returnedIncomplete
Inv_SerializedPasses == ~overlapped
Safety == TypeOK /\ Inv_Membership /\ Inv_IndependentOrder
          /\ Inv_PassCoverage /\ Inv_CompleteReturn /\ Inv_SerializedPasses
Live_PassReturns == running ~> ~running
Spec == Init /\ [][Next]_vars /\ WF_vars(Examine) /\ WF_vars(Process) /\ WF_vars(Return)
=============================================================================
