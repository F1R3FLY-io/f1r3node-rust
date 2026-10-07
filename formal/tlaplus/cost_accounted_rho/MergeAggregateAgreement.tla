-------------------------- MODULE MergeAggregateAgreement --------------------------
EXTENDS Integers, Sequences, TLC

CONSTANTS ContributionOrders, BaseValue, MachineMin, MachineMax,
          RejectInvalidPrefix

ASSUME /\ ContributionOrders \in SUBSET Seq(Int)
       /\ ContributionOrders # {}
       /\ BaseValue \in MachineMin..MachineMax
       /\ MachineMin < MachineMax
       /\ RejectInvalidPrefix \in BOOLEAN

RECURSIVE Sum(_)

Sum(values) ==
  IF values = <<>>
  THEN 0
  ELSE Head(values) + Sum(Tail(values))

Fits(value) == value \in MachineMin..MachineMax

RECURSIVE PrefixesFit(_, _)

PrefixesFit(origin, values) ==
  IF values = <<>>
  THEN TRUE
  ELSE /\ Fits(origin + Head(values))
       /\ PrefixesFit(origin + Head(values), Tail(values))

MathematicalAcceptance(values) == Fits(BaseValue + Sum(values))

SelectionAcceptance(values) ==
  IF RejectInvalidPrefix
  THEN PrefixesFit(BaseValue, values)
  ELSE MathematicalAcceptance(values)

ApplicationAcceptance(values) == MathematicalAcceptance(values)

VARIABLES contributionOrder, selected, applied

vars == <<contributionOrder, selected, applied>>

Init ==
  /\ contributionOrder \in ContributionOrders
  /\ selected = SelectionAcceptance(contributionOrder)
  /\ applied = ApplicationAcceptance(contributionOrder)

Next == UNCHANGED vars

Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ contributionOrder \in ContributionOrders
  /\ selected \in BOOLEAN
  /\ applied \in BOOLEAN

SelectionApplicationAgree == selected = applied

AcceptanceIsPermutationInvariant ==
  \A left, right \in ContributionOrders :
    Sum(left) = Sum(right) =>
      SelectionAcceptance(left) = SelectionAcceptance(right)

FinalResultIsMathematicalTotal ==
  selected => Fits(BaseValue + Sum(contributionOrder))

=============================================================================
