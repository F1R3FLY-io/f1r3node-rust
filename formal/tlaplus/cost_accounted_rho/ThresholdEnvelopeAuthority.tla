---------------- MODULE ThresholdEnvelopeAuthority ----------------
EXTENDS FiniteSets, Sequences, TLC

CONSTANT
    \* @type: Str;
    Defect

ASSUME Defect \in {
    "None",
    "UnboundSubset",
    "PolicyMemberAuthority",
    "MemberZeroAuthority",
    "PolicyMemberDebit",
    "WitnessMismatch",
    "DuplicateGroundAccepted"
}

Validators == {"V1", "V2", "V3"}
Members == {"A", "B", "C"}
Envelopes == {"AB", "AC", "BC"}

Selected(envelope) ==
    CASE envelope = "AB" -> {"A", "B"}
      [] envelope = "AC" -> {"A", "C"}
      [] OTHER -> {"B", "C"}

Witnesses(envelope) ==
    IF Defect = "WitnessMismatch"
    THEN Members
    ELSE Selected(envelope)

GroundOwner(member) ==
    IF Defect = "DuplicateGroundAccepted" /\ member = "B"
    THEN "A"
    ELSE member

DeployIdentity(envelope) ==
    [intent |-> "intent",
     policy |-> "threshold-2-of-3",
     subset |-> IF Defect = "UnboundSubset" THEN {} ELSE Selected(envelope)]

FundingAuthority(envelope) ==
    IF Defect = "PolicyMemberDebit"
    THEN Members
    ELSE Selected(envelope)

RuntimeAuthority(envelope) ==
    CASE Defect = "PolicyMemberAuthority" -> Members
      [] Defect = "MemberZeroAuthority" -> Selected(envelope) \cup {"A"}
      [] OTHER -> Selected(envelope)

StateTransition(envelope) ==
    [selected |-> Selected(envelope),
     funding |-> FundingAuthority(envelope),
     authority |-> RuntimeAuthority(envelope)]

VARIABLE
    \* @type: (Str -> Set(Str));
    accepted

vars == accepted

Init == accepted = [validator \in Validators |-> {}]

Accept(validator, envelope) ==
    /\ validator \in Validators
    /\ envelope \in Envelopes
    /\ envelope \notin accepted[validator]
    /\ accepted' = [accepted EXCEPT ![validator] = @ \cup {envelope}]

Complete == \A validator \in Validators : accepted[validator] = Envelopes

TerminalStutter ==
    /\ Complete
    /\ UNCHANGED vars

Next ==
    \/ \E validator \in Validators, envelope \in Envelopes :
         Accept(validator, envelope)
    \/ TerminalStutter

Spec == Init /\ [][Next]_vars

TypeOK == accepted \in [Validators -> SUBSET Envelopes]

QuorumIsExact ==
    \A envelope \in Envelopes : Cardinality(Selected(envelope)) = 2

WitnessesSelectExactlyTheFunders ==
    \A envelope \in Envelopes :
        Witnesses(envelope) = Selected(envelope)

PolicyGroundOwnersAreUnique ==
    \A left, right \in Members :
        GroundOwner(left) = GroundOwner(right) => left = right

DeployIdentityBindsStateTransition ==
    \A left, right \in Envelopes :
        DeployIdentity(left) = DeployIdentity(right) =>
            StateTransition(left) = StateTransition(right)

UnsignedMembersHaveNoAuthority ==
    \A envelope \in Envelopes :
        RuntimeAuthority(envelope) = Selected(envelope)

UnsignedMembersAreNeverDebited ==
    \A envelope \in Envelopes :
        FundingAuthority(envelope) = Selected(envelope)

ValidatorsAgreeForEqualDeployIdentity ==
    \A first, second \in Validators,
       left, right \in Envelopes :
        /\ left \in accepted[first]
        /\ right \in accepted[second]
        /\ DeployIdentity(left) = DeployIdentity(right)
        => StateTransition(left) = StateTransition(right)

SelectedCompoundAuthorityIsOrderIndependent ==
    \A left, right \in Envelopes :
        Selected(left) = Selected(right) =>
            /\ FundingAuthority(left) = FundingAuthority(right)
            /\ RuntimeAuthority(left) = RuntimeAuthority(right)

=============================================================================
