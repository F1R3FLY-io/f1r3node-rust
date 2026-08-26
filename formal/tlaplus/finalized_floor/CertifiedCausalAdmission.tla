---------------------- MODULE CertifiedCausalAdmission ----------------------
EXTENDS Naturals, FiniteSets

CONSTANT
  \* @type: Str;
  DeliveryMode,
  \* @type: Bool;
  TraverseRejected,
  \* @type: Bool;
  ImportRejectedDelta,
  \* @type: Bool;
  ImportProofContext,
  \* @type: Bool;
  NormalizePerIncarnation,
  \* @type: Bool;
  IgnoreAmbientTracker,
  \* @type: Bool;
  RequireFullDependencies

ASSUME /\ DeliveryMode \in {"Individual", "Closure", "Partial", "All"}
       /\ TraverseRejected \in BOOLEAN
       /\ ImportRejectedDelta \in BOOLEAN
       /\ ImportProofContext \in BOOLEAN
       /\ NormalizePerIncarnation \in BOOLEAN
       /\ IgnoreAmbientTracker \in BOOLEAN
       /\ RequireFullDependencies \in BOOLEAN

Nodes == {1, 2}
Blocks == 1..15
Candidates == {6, 13}
Proofs == {"EA", "EB", "EC1", "EC2", "ED"}
Verdicts == {"Unevaluated", "Accepted", "Rejected"}

Parents(block) ==
  CASE block = 6 -> {5}
    [] block = 9 -> {7, 14}
    [] block = 12 -> {9}
    [] block = 13 -> {12}
    [] OTHER -> {}

Justifications(block) ==
  CASE block = 9 -> {8, 15}
    [] OTHER -> {}

StoredDecision(block) ==
  IF block \in {8, 9} THEN "Rejected" ELSE "Accepted"

BlockDelta(block) ==
  CASE block = 1 -> {"EB"}
    [] block = 5 -> {"EA"}
    [] block = 9 -> {"ED"}
    [] block = 13 -> {"EC1"}
    [] OTHER -> {}

BlockIncarnation(block) ==
  CASE block \in {1, 2} -> "A"
    [] block \in {3, 4} -> "B"
    [] block \in {7, 8, 14, 15} -> "C"
    [] block \in {10, 11} -> "D"
    [] OTHER -> "U"

BlockSequence(block) ==
  CASE block \in {1, 2, 3, 4, 7, 8, 10, 11} -> 1
    [] block \in {14, 15} -> 2
    [] OTHER -> block

ProofIncarnation(proof) ==
  CASE proof = "EA" -> "A"
    [] proof = "EB" -> "B"
    [] proof \in {"EC1", "EC2"} -> "C"
    [] proof = "ED" -> "D"

ProofSequence(proof) ==
  CASE proof \in {"EA", "EB", "EC1", "ED"} -> 1
    [] proof = "EC2" -> 2

ProofRank(proof) ==
  CASE proof = "EA" -> 1
    [] proof = "EB" -> 1
    [] proof = "EC1" -> 1
    [] proof = "EC2" -> 2
    [] proof = "ED" -> 1

ProofBlocks(proof) ==
  CASE proof = "EA" -> {1, 2}
    [] proof = "EB" -> {3, 4}
    [] proof = "EC1" -> {7, 8}
    [] proof = "EC2" -> {14, 15}
    [] proof = "ED" -> {10, 11}

ProofDependencies(proofs) == UNION {ProofBlocks(proof) : proof \in proofs}

ProofIsSound(proof) ==
  LET pair == ProofBlocks(proof) IN
    /\ Cardinality(pair) = 2
    /\ \A block \in pair : BlockIncarnation(block) = ProofIncarnation(proof)
    /\ \A block \in pair : BlockSequence(block) = ProofSequence(proof)

Canonical(proofs) ==
  IF NormalizePerIncarnation
  THEN {proof \in proofs :
          \A other \in proofs :
            ProofIncarnation(other) = ProofIncarnation(proof)
              => ProofRank(proof) <= ProofRank(other)}
  ELSE proofs

CausalClosure(candidate) ==
  CASE candidate = 6 -> {5}
    [] candidate = 13 ->
         IF TraverseRejected THEN {7, 8, 9, 12, 14, 15} ELSE {9, 12}

ImportedRaw(closure) ==
  LET direct ==
        UNION {IF StoredDecision(block) = "Accepted" \/ ImportRejectedDelta
               THEN BlockDelta(block)
               ELSE {} : block \in closure} IN
  LET proofRoots == ProofDependencies(direct) IN
    direct \union
      IF ImportProofContext
      THEN UNION {BlockDelta(block) : block \in proofRoots}
      ELSE {}

StructuralRaw(closure) ==
  {proof \in Proofs : ProofIsSound(proof) /\ ProofBlocks(proof) \subseteq closure}

InheritedContext(candidate) ==
  Canonical(ImportedRaw(CausalClosure(candidate)))

EffectiveContext(candidate) ==
  LET closure == CausalClosure(candidate) IN
    Canonical(ImportedRaw(closure) \union StructuralRaw(closure))

RequiredDelta(candidate) == EffectiveContext(candidate) \ InheritedContext(candidate)

ExpectedContext(candidate) ==
  CASE candidate = 6 -> {"EA"}
    [] candidate = 13 -> {"EC1"}

DependencyClosure(candidate) ==
  CASE candidate = 6 -> {1, 2, 3, 4, 5, 6}
    [] candidate = 13 -> {7, 8, 9, 10, 11, 12, 13, 14, 15}

Arrival(node, index) == IF node = 1 THEN index ELSE 16 - index

VARIABLE
  \* @type: Int -> Int;
  receiveIndex,
  \* @type: Int -> Set(Int);
  known,
  \* @type: Int -> Set(Str);
  tracker,
  \* @type: Int -> (Int -> Str);
  verdict,
  \* @type: Int -> (Int -> Set(Str));
  certifiedContext,
  \* @type: Int -> (Int -> Int);
  certifiedRuleset

vars == <<receiveIndex, known, tracker, verdict, certifiedContext, certifiedRuleset>>

Init ==
  /\ receiveIndex = [node \in Nodes |-> 0]
  /\ known = [node \in Nodes |-> {}]
  /\ tracker = [node \in Nodes |-> IF node = 1 THEN {"EB"} ELSE {}]
  /\ verdict = [node \in Nodes |-> [candidate \in Candidates |-> "Unevaluated"]]
  /\ certifiedContext = [node \in Nodes |-> [candidate \in Candidates |-> {}]]
  /\ certifiedRuleset = [node \in Nodes |-> [candidate \in Candidates |-> 0]]

ReceiveNext(node) ==
  /\ receiveIndex[node] < Cardinality(Blocks)
  /\ LET next == receiveIndex[node] + 1 IN
       /\ receiveIndex' = [receiveIndex EXCEPT ![node] = next]
       /\ known' = [known EXCEPT ![node] = @ \union {Arrival(node, next)}]
  /\ UNCHANGED <<tracker, verdict, certifiedContext, certifiedRuleset>>

DeliverDependencyClosure(node, candidate) ==
  /\ ~ (DependencyClosure(candidate) \subseteq known[node])
  /\ known' =
       [known EXCEPT ![node] = @ \union DependencyClosure(candidate)]
  /\ UNCHANGED
       <<receiveIndex, tracker, verdict, certifiedContext, certifiedRuleset>>

DeliverReadySubset(node, candidate) ==
  LET ready == {candidate} \union Parents(candidate) \union Justifications(candidate) IN
    /\ ~ (ready \subseteq known[node])
    /\ known' = [known EXCEPT ![node] = @ \union ready]
    /\ UNCHANGED
         <<receiveIndex, tracker, verdict, certifiedContext, certifiedRuleset>>

DeliverAllDependencies ==
  /\ \E node \in Nodes : known[node] # Blocks
  /\ known' = [node \in Nodes |-> Blocks]
  /\ UNCHANGED
       <<receiveIndex, tracker, verdict, certifiedContext, certifiedRuleset>>

ToggleAmbientTracker(node) ==
  /\ tracker' = [tracker EXCEPT
       ![node] = IF "EB" \in @ THEN @ \ {"EB"} ELSE @ \union {"EB"}]
  /\ UNCHANGED <<receiveIndex, known, verdict, certifiedContext, certifiedRuleset>>

Ready(node, candidate) ==
  /\ candidate \in known[node]
  /\ IF RequireFullDependencies
     THEN DependencyClosure(candidate) \subseteq known[node]
     ELSE Parents(candidate) \union Justifications(candidate) \subseteq known[node]

LocalContext(node, candidate) ==
  IF IgnoreAmbientTracker
  THEN EffectiveContext(candidate)
  ELSE Canonical(EffectiveContext(candidate) \union tracker[node])

LocalRequiredDelta(node, candidate) ==
  LocalContext(node, candidate) \ InheritedContext(candidate)

ContextAvailable(node, candidate) ==
  ProofDependencies(LocalContext(node, candidate)) \subseteq known[node]

ComputedVerdict(node, candidate) ==
  IF ContextAvailable(node, candidate)
       /\ BlockDelta(candidate) = LocalRequiredDelta(node, candidate)
  THEN "Accepted"
  ELSE "Rejected"

Validate(node, candidate) ==
  /\ verdict[node][candidate] = "Unevaluated"
  /\ Ready(node, candidate)
  /\ verdict' = [verdict EXCEPT ![node][candidate] = ComputedVerdict(node, candidate)]
  /\ certifiedContext' =
       [certifiedContext EXCEPT ![node][candidate] = LocalContext(node, candidate)]
  /\ certifiedRuleset' = [certifiedRuleset EXCEPT ![node][candidate] = 7]
  /\ UNCHANGED <<receiveIndex, known, tracker>>

Next ==
  \/ /\ DeliveryMode = "Individual"
     /\ \E node \in Nodes : ReceiveNext(node)
  \/ /\ DeliveryMode = "Closure"
     /\ \E node \in Nodes, candidate \in Candidates :
          DeliverDependencyClosure(node, candidate)
  \/ /\ DeliveryMode = "Partial"
     /\ \E node \in Nodes, candidate \in Candidates :
          DeliverReadySubset(node, candidate)
          \/ DeliverDependencyClosure(node, candidate)
  \/ /\ DeliveryMode = "All"
     /\ DeliverAllDependencies
  \/ \E node \in Nodes : ToggleAmbientTracker(node)
  \/ \E node \in Nodes, candidate \in Candidates : Validate(node, candidate)

TypeOK ==
  /\ receiveIndex \in [Nodes -> 0..Cardinality(Blocks)]
  /\ known \in [Nodes -> SUBSET Blocks]
  /\ tracker \in [Nodes -> SUBSET Proofs]
  /\ verdict \in [Nodes -> [Candidates -> Verdicts]]
  /\ certifiedContext \in [Nodes -> [Candidates -> SUBSET Proofs]]
  /\ certifiedRuleset \in [Nodes -> [Candidates -> 0..7]]

Evaluated(node, candidate) == verdict[node][candidate] # "Unevaluated"

Inv_CertifiedAdmissionAgreement ==
  \A first \in Nodes, second \in Nodes, candidate \in Candidates :
    Evaluated(first, candidate) /\ Evaluated(second, candidate)
      => /\ verdict[first][candidate] = verdict[second][candidate]
         /\ certifiedContext[first][candidate] = certifiedContext[second][candidate]

Inv_FullyKnownCandidatesAccepted ==
  \A node \in Nodes, candidate \in Candidates :
    Evaluated(node, candidate) /\ DependencyClosure(candidate) \subseteq known[node]
      => verdict[node][candidate] = "Accepted"

Inv_CertifiedContextExact ==
  \A node \in Nodes, candidate \in Candidates :
    Evaluated(node, candidate)
      => certifiedContext[node][candidate] = ExpectedContext(candidate)

Inv_OneCanonicalProofPerIncarnation ==
  \A node \in Nodes, candidate \in Candidates :
    \A first \in certifiedContext[node][candidate],
       second \in certifiedContext[node][candidate] :
      ProofIncarnation(first) = ProofIncarnation(second) => first = second

Inv_OutcomeBoundToRuleset ==
  \A node \in Nodes, candidate \in Candidates :
    Evaluated(node, candidate) => certifiedRuleset[node][candidate] = 7

Inv_RejectedWrapperTraversed == "EC1" \in EffectiveContext(13)

Inv_RejectedDeltaIgnored == "ED" \notin EffectiveContext(13)

Inv_ProofRootsAreLeafFacts == "EB" \notin EffectiveContext(6)

Inv_CanonicalIncarnationBound == Cardinality(EffectiveContext(13)) = 1

Safety ==
  /\ TypeOK
  /\ Inv_CertifiedAdmissionAgreement
  /\ Inv_FullyKnownCandidatesAccepted
  /\ Inv_CertifiedContextExact
  /\ Inv_OneCanonicalProofPerIncarnation
  /\ Inv_OutcomeBoundToRuleset
  /\ Inv_RejectedWrapperTraversed
  /\ Inv_RejectedDeltaIgnored
  /\ Inv_ProofRootsAreLeafFacts
  /\ Inv_CanonicalIncarnationBound

AllCandidatesCertified ==
  \A node \in Nodes, candidate \in Candidates : Evaluated(node, candidate)

Spec ==
  /\ Init
  /\ [][Next]_vars
  /\ \A node \in Nodes : WF_vars(ReceiveNext(node))
  /\ \A node \in Nodes, candidate \in Candidates : WF_vars(Validate(node, candidate))

Live_AllCandidatesCertified == <>AllCandidatesCertified

=============================================================================
