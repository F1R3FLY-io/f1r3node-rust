-------------------- MODULE ProtectedParentFrontier --------------------
EXTENDS Naturals, FiniteSets, TLC

CONSTANTS
  \* @type: Set(Str);
  Validators,
  \* @type: Set(Int);
  Blocks,
  \* @type: Set(Int);
  Candidates,
  \* @type: Set(Int);
  CausalTips,
  \* @type: Int;
  GhostHead,
  \* @type: Int;
  FinalizedFloor,
  \* @type: Bool;
  HasVotes,
  \* @type: Bool;
  UseGenericCompaction

ASSUME /\ Validators # {}
       /\ Candidates \subseteq Blocks
       /\ CausalTips \subseteq Candidates
       /\ GhostHead \in Candidates
       /\ FinalizedFloor \in Candidates
       /\ HasVotes \in BOOLEAN
       /\ UseGenericCompaction \in BOOLEAN

\* @type: (Int, Int) => Bool;
Ancestor(ancestor, descendant) ==
  \/ ancestor = 0 /\ descendant \in {1, 2, 3, 4}
  \/ ancestor = 1 /\ descendant \in {2, 3, 4}
  \/ ancestor = 2 /\ descendant = 4

ProtectedAnchor == IF HasVotes THEN GhostHead ELSE FinalizedFloor

EligibleSecondary ==
  {candidate \in Candidates :
    /\ candidate # ProtectedAnchor
    /\ ~Ancestor(candidate, ProtectedAnchor)}

Maximal(candidateSet) ==
  {candidate \in candidateSet :
    ~\E other \in candidateSet :
      /\ other # candidate
      /\ Ancestor(candidate, other)}

ProtectedParents == {ProtectedAnchor} \union Maximal(EligibleSecondary)
GenericParents == Maximal(Candidates)

SelectedParents ==
  IF UseGenericCompaction THEN GenericParents ELSE ProtectedParents

Covers(parent, tip) == parent = tip \/ Ancestor(tip, parent)

VARIABLES
  \* @type: Set(Str);
  pending,
  \* @type: Str -> Int;
  recordedHead,
  \* @type: Str -> Set(Int);
  recordedParents

vars == <<pending, recordedHead, recordedParents>>

Init ==
  /\ pending = Validators
  /\ recordedHead = [validator \in Validators |-> ProtectedAnchor]
  /\ recordedParents = [validator \in Validators |-> {}]

Compute(validator) ==
  /\ validator \in pending
  /\ pending' = pending \ {validator}
  /\ recordedHead' = [recordedHead EXCEPT ![validator] = ProtectedAnchor]
  /\ recordedParents' = [recordedParents EXCEPT ![validator] = SelectedParents]

Idle ==
  /\ pending = {}
  /\ UNCHANGED vars

Next == (\E validator \in pending : Compute(validator)) \/ Idle

Spec ==
  /\ Init
  /\ [][Next]_vars
  /\ \A validator \in Validators : WF_vars(Compute(validator))

Processed == Validators \ pending

TypeOK ==
  /\ pending \subseteq Validators
  /\ recordedHead \in [Validators -> Blocks]
  /\ recordedParents \in [Validators -> SUBSET Blocks]

Inv_ProtectedAnchorIsMain ==
  \A validator \in Processed :
    /\ recordedHead[validator] = ProtectedAnchor
    /\ ProtectedAnchor \in recordedParents[validator]

Inv_NoVoteUsesFinalizedFloor ==
  ~HasVotes => \A validator \in Processed : recordedHead[validator] = FinalizedFloor

Inv_SecondaryProvenance ==
  \A validator \in Processed :
    \A parent \in recordedParents[validator] \ {ProtectedAnchor} :
      /\ parent \in Candidates
      /\ ~Ancestor(parent, ProtectedAnchor)

Inv_SecondaryAntichain ==
  \A validator \in Processed :
    \A left, right \in recordedParents[validator] \ {ProtectedAnchor} :
      left # right => ~Ancestor(left, right)

Inv_CausalCoverage ==
  \A validator \in Processed :
    \A tip \in CausalTips :
      \E parent \in recordedParents[validator] : Covers(parent, tip)

Inv_ValidatorAgreement ==
  \A left, right \in Processed :
    /\ recordedHead[left] = recordedHead[right]
    /\ recordedParents[left] = recordedParents[right]

Inv_ParentCountBound ==
  \A validator \in Processed :
    Cardinality(recordedParents[validator]) <= Cardinality(Candidates) + 1

Live_AllValidatorsCompute == <>(pending = {})

=======================================================================
