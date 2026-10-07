-------------------- MODULE InitialFundingDiscovery --------------------
EXTENDS Naturals, FiniteSets, Apalache

CONSTANTS Principals, Custody, CustodyOf, PreBalance, Fee,
          SeedSelected, IncludeAbsent, CountAliasesTwice

ASSUME /\ IsFiniteSet(Principals)
       /\ Principals # {}
       /\ IsFiniteSet(Custody)
       /\ CustodyOf \in [Principals -> Custody]
       /\ PreBalance \in [Custody -> Nat]
       /\ Fee \in Nat
       /\ SeedSelected \in BOOLEAN
       /\ IncludeAbsent \in BOOLEAN
       /\ CountAliasesTwice \in BOOLEAN

VARIABLES selected, discovered, phase, capacity, started
vars == <<selected, discovered, phase, capacity, started>>

Sum(function, domain) ==
    LET Add(total, key) == total + function[key]
    IN ApaFoldSet(Add, 0, domain)

Physical(keys) == {CustodyOf[key] : key \in keys}
Supply(keys) == Sum(PreBalance, Physical(keys))
AfterFee(amount) == IF amount >= Fee THEN amount - Fee ELSE 0

Init ==
    /\ selected \in SUBSET Principals \ {{}}
    /\ discovered = {}
    /\ phase = "Discover"
    /\ capacity = 0
    /\ started = FALSE

Discover ==
    /\ phase = "Discover"
    /\ discovered' =
         (IF SeedSelected THEN selected ELSE {})
           \cup (IF IncludeAbsent THEN Principals \ selected ELSE {})
    /\ phase' = "Start"
    /\ UNCHANGED <<selected, capacity, started>>

Start ==
    /\ phase = "Start"
    /\ LET amount ==
             IF CountAliasesTwice
             THEN Sum([key \in discovered |-> PreBalance[CustodyOf[key]]], discovered)
             ELSE Supply(discovered)
       IN /\ capacity' = AfterFee(amount)
          /\ started' = (capacity' > 0)
    /\ phase' = "Done"
    /\ UNCHANGED <<selected, discovered>>

Spec == Init /\ [][Discover \/ Start]_vars

SelectedLeavesArePresent == phase # "Discover" => selected \subseteq discovered
AbsentLeavesAreExcluded == discovered \subseteq selected
CapacityUsesDistinctPrestateCustody ==
    phase = "Done" => capacity = AfterFee(Supply(selected))
FundedSetupCanStart ==
    phase = "Done" /\ Supply(selected) > Fee => started

ExamplePrincipals == {"alice", "bob", "carol", "dana"}
ExampleCustody == {"ab", "c", "d"}
ExampleCustodyOf ==
    [key \in ExamplePrincipals |->
      CASE key \in {"alice", "bob"} -> "ab"
        [] key = "carol" -> "c"
        [] OTHER -> "d"]
ExampleBalance == [key \in ExampleCustody |->
    CASE key = "ab" -> 3 [] key = "c" -> 2 [] OTHER -> 0]

=============================================================================
