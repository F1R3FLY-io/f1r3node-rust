-------------------- MODULE LatestMessageMaterialization --------------------
EXTENDS FiniteSets, Integers

CONSTANT
  \* @type: Bool;
  PlaceholderIsBottom

ASSUME PlaceholderIsBottom \in BOOLEAN

Replicas == {"Full", "Restored"}
Messages == {"A", "B", "C"}
Blocks == {"G"} \union Messages
Genesis == "G"

\* @type: Str => Int;
Sequence(block) ==
  CASE block = "C" -> 1
    [] OTHER -> 0

\* @type: Str => Int;
HashRank(block) ==
  CASE block = "G" -> 0
    [] block = "B" -> 1
    [] block = "A" -> 2
    [] OTHER -> 3

\* @type: (Str, Str) => Bool;
CandidateWins(candidate, current) ==
  IF current = Genesis /\ PlaceholderIsBottom
  THEN TRUE
  ELSE \/ Sequence(candidate) > Sequence(current)
       \/ /\ Sequence(candidate) = Sequence(current)
          /\ HashRank(candidate) < HashRank(current)

\* @type: Set(Str) => Str;
Canonical(messages) ==
  IF messages = {}
  THEN Genesis
  ELSE IF "C" \in messages
       THEN "C"
       ELSE IF "B" \in messages THEN "B" ELSE "A"

VARIABLES
  \* @type: Str -> Set(Str);
  seen,
  \* @type: Str -> Str;
  latest,
  \* @type: Str -> Str;
  phase

vars == <<seen, latest, phase>>

Init ==
  /\ seen = [replica \in Replicas |-> {}]
  /\ latest = [replica \in Replicas |-> Genesis]
  /\ phase = [replica \in Replicas |-> "Running"]

\* @type: (Str, Str) => Bool;
Insert(replica, message) ==
  /\ phase[replica] = "Running"
  /\ message \in Messages
  /\ seen' = [seen EXCEPT ![replica] = @ \union {message}]
  /\ latest' = [latest EXCEPT
       ![replica] = IF CandidateWins(message, @) THEN message ELSE @]
  /\ UNCHANGED phase

\* @type: Str => Bool;
Crash(replica) ==
  /\ phase[replica] = "Running"
  /\ latest' = [latest EXCEPT ![replica] = Genesis]
  /\ phase' = [phase EXCEPT ![replica] = "Restoring"]
  /\ UNCHANGED seen

\* @type: Str => Bool;
Reconcile(replica) ==
  /\ phase[replica] = "Restoring"
  /\ latest' = [latest EXCEPT ![replica] = Canonical(seen[replica])]
  /\ phase' = [phase EXCEPT ![replica] = "Running"]
  /\ UNCHANGED seen

Next ==
  \/ \E replica \in Replicas, message \in Messages : Insert(replica, message)
  \/ \E replica \in Replicas : Crash(replica)
  \/ \E replica \in Replicas : Reconcile(replica)

Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ seen \in [Replicas -> SUBSET Messages]
  /\ latest \in [Replicas -> Blocks]
  /\ phase \in [Replicas -> {"Running", "Restoring"}]

MaterializationCorrect ==
  \A replica \in Replicas :
    phase[replica] = "Running" => latest[replica] = Canonical(seen[replica])

PlaceholderReplaced ==
  \A replica \in Replicas :
    phase[replica] = "Running" /\ seen[replica] /= {} => latest[replica] /= Genesis

EqualViewsAgree ==
  \A left \in Replicas, right \in Replicas :
    /\ phase[left] = "Running"
    /\ phase[right] = "Running"
    /\ seen[left] = seen[right]
    => latest[left] = latest[right]

=============================================================================
