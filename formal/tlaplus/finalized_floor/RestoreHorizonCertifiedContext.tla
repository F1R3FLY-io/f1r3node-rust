------------------ MODULE RestoreHorizonCertifiedContext ------------------
EXTENDS Integers, FiniteSets, Sequences, TLC

CONSTANT
  \* @type: Str;
  Defect

ASSUME Defect \in {
  "None",
  "DropMissingSlot",
  "DropMissingStake",
  "HeldnessClassification",
  "AbstainAllMissing"
}

Nodes == {"Full", "Restored"}
Validators == {"Live", "Silent"}
Blocks == {"G", "F", "L"}
CanonicalGenesis == "G"
AuthorityFloor == "F"

\* @type: Str => Int;
Stake(validator) == IF validator = "Live" THEN 7 ELSE 3
\* @type: Str => Str;
Latest(validator) == IF validator = "Live" THEN "L" ELSE CanonicalGenesis
\* @type: Str => Int;
Effect(block) == IF block = "L" THEN 11 ELSE 0
AuthorityStake == Stake("Live") + Stake("Silent")

VARIABLES
  \* @type: Str -> Set(Str);
  held,
  \* @type: Str -> Bool;
  inspected,
  \* @type: Str -> Bool;
  ready,
  \* @type: Str -> (Str -> Str);
  exact,
  \* @type: Str -> Set(Str);
  eligible,
  \* @type: Str -> Set(Str);
  excluded,
  \* @type: Str -> Int;
  denominator,
  \* @type: Str -> Int;
  charged,
  \* @type: Str -> Int;
  replayState,
  \* @type: Str -> Int;
  digest,
  \* @type: Str -> Str;
  error

vars == <<held, inspected, ready, exact, eligible, excluded, denominator,
          charged, replayState, digest, error>>

EmptyMap == [validator \in {} |-> Latest(validator)]

Init ==
  /\ held = [node \in Nodes |->
       IF node = "Full" THEN Blocks ELSE {AuthorityFloor, "L"}]
  /\ inspected = [node \in Nodes |-> FALSE]
  /\ ready = [node \in Nodes |-> FALSE]
  /\ exact = [node \in Nodes |-> EmptyMap]
  /\ eligible = [node \in Nodes |-> {}]
  /\ excluded = [node \in Nodes |-> {}]
  /\ denominator = [node \in Nodes |-> 0]
  /\ charged = [node \in Nodes |-> 0]
  /\ replayState = [node \in Nodes |-> 0]
  /\ digest = [node \in Nodes |-> 0]
  /\ error = [node \in Nodes |-> "None"]

\* @type: Str => (Str -> Str);
ExactFor(node) ==
  [validator \in
    {candidate \in Validators :
      ~(Defect = "DropMissingSlot" /\
        Latest(candidate) \notin held[node])}
   |-> Latest(validator)]

\* @type: (Str, Str -> Str) => Int;
DenominatorFor(node, exactMap) ==
  IF Defect = "DropMissingStake"
  THEN (IF "Live" \in DOMAIN exactMap THEN Stake("Live") ELSE 0) +
       (IF "Silent" \in DOMAIN exactMap /\ CanonicalGenesis \in held[node]
        THEN Stake("Silent")
        ELSE 0)
  ELSE AuthorityStake

\* @type: (Str, Str) => Bool;
PlaceholderFor(node, validator) ==
  IF Defect = "HeldnessClassification"
  THEN Latest(validator) = CanonicalGenesis /\ CanonicalGenesis \notin held[node]
  ELSE Latest(validator) = CanonicalGenesis

\* @type: Str => Bool;
MissingLive(node) ==
  \E validator \in Validators :
    Latest(validator) /= CanonicalGenesis /\ Latest(validator) \notin held[node]

\* @type: Str => Bool;
ReadyFor(node) ==
  ~MissingLive(node) \/ Defect = "AbstainAllMissing"

\* @type: (Str, Str -> Str) => Set(Str);
EligibleFor(node, exactMap) ==
  {validator \in DOMAIN exactMap :
    ~PlaceholderFor(node, validator) /\
    (Latest(validator) \in held[node] \/ Defect = "AbstainAllMissing")}

\* @type: (Str, Str -> Str) => Set(Str);
ExcludedFor(node, exactMap) == DOMAIN exactMap \ EligibleFor(node, exactMap)

\* @type: Set(Str) => Int;
ChargeFor(eligibleSet) ==
  (IF "Live" \in eligibleSet THEN Effect("L") ELSE 0) +
  (IF "Silent" \in eligibleSet THEN Effect(CanonicalGenesis) ELSE 0)

\* @type: Set(Str) => Int;
ValidatorSetCode(validators) ==
  (IF "Live" \in validators THEN 1 ELSE 0) +
  (IF "Silent" \in validators THEN 2 ELSE 0)

\* @type: (Str -> Str, Set(Str), Set(Str), Int, Int) => Int;
DigestFor(exactMap, eligibleSet, excludedSet, stakeValue, chargeValue) ==
  ((((ValidatorSetCode(DOMAIN exactMap) * 4 + ValidatorSetCode(eligibleSet)) * 4 +
      ValidatorSetCode(excludedSet)) * 16 + stakeValue) * 16 + chargeValue)

\* @type: Str => Bool;
Inspect(node) ==
  LET nextExact == ExactFor(node) IN
  LET nextEligible == EligibleFor(node, nextExact) IN
  LET nextExcluded == ExcludedFor(node, nextExact) IN
  LET nextDenominator == DenominatorFor(node, nextExact) IN
  LET nextCharge == ChargeFor(nextEligible) IN
  /\ inspected' = [inspected EXCEPT ![node] = TRUE]
  /\ ready' = [ready EXCEPT ![node] = ReadyFor(node)]
  /\ exact' = [exact EXCEPT ![node] = nextExact]
  /\ eligible' = [eligible EXCEPT ![node] = nextEligible]
  /\ excluded' = [excluded EXCEPT ![node] = nextExcluded]
  /\ denominator' = [denominator EXCEPT ![node] = nextDenominator]
  /\ charged' = [charged EXCEPT ![node] = nextCharge]
  /\ replayState' = [replayState EXCEPT ![node] = 100 + nextCharge]
  /\ digest' = [digest EXCEPT
       ![node] = DigestFor(
         nextExact, nextEligible, nextExcluded, nextDenominator, nextCharge)]
  /\ error' = [error EXCEPT
       ![node] = IF ReadyFor(node) THEN "None" ELSE "MissingLiveDependency"]
  /\ UNCHANGED held

\* @type: Str => Bool;
LoseLiveBody(node) ==
  /\ "L" \in held[node]
  /\ ~inspected[node]
  /\ held' = [held EXCEPT ![node] = @ \ {"L"}]
  /\ UNCHANGED <<inspected, ready, exact, eligible, excluded, denominator,
                  charged, replayState, digest, error>>

Next ==
  \/ \E node \in Nodes : Inspect(node)
  \/ \E node \in Nodes : LoseLiveBody(node)

Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ held \in [Nodes -> SUBSET Blocks]
  /\ inspected \in [Nodes -> BOOLEAN]
  /\ ready \in [Nodes -> BOOLEAN]
  /\ \A node \in Nodes :
       /\ DOMAIN exact[node] \subseteq Validators
       /\ \A validator \in DOMAIN exact[node] : exact[node][validator] \in Blocks
  /\ eligible \in [Nodes -> SUBSET Validators]
  /\ excluded \in [Nodes -> SUBSET Validators]
  /\ denominator \in [Nodes -> Int]
  /\ charged \in [Nodes -> Int]
  /\ replayState \in [Nodes -> Int]
  /\ digest \in [Nodes -> Int]
  /\ error \in [Nodes -> {"None", "MissingLiveDependency"}]

ExactSlotsComplete ==
  \A node \in Nodes : inspected[node] => DOMAIN exact[node] = Validators

AuthorityStakeRetained ==
  \A node \in Nodes : inspected[node] => denominator[node] = AuthorityStake

CanonicalPlaceholderExcluded ==
  \A node \in Nodes : inspected[node] => "Silent" \in excluded[node]

LiveEffectRetained ==
  \A node \in Nodes : inspected[node] /\ ready[node] /\ "L" \in held[node] =>
    /\ "Live" \in eligible[node]
    /\ charged[node] = Effect("L")
    /\ replayState[node] = 100 + Effect("L")

MissingLiveFailsClosed ==
  \A node \in Nodes : inspected[node] /\ MissingLive(node) =>
    /\ ~ready[node]
    /\ error[node] = "MissingLiveDependency"

ReadyContextsAgree ==
  inspected["Full"] /\ inspected["Restored"] /\
  ready["Full"] /\ ready["Restored"] =>
    /\ exact["Full"] = exact["Restored"]
    /\ eligible["Full"] = eligible["Restored"]
    /\ excluded["Full"] = excluded["Restored"]
    /\ denominator["Full"] = denominator["Restored"]
    /\ charged["Full"] = charged["Restored"]
    /\ replayState["Full"] = replayState["Restored"]
    /\ digest["Full"] = digest["Restored"]

Safety ==
  /\ TypeOK
  /\ ExactSlotsComplete
  /\ AuthorityStakeRetained
  /\ CanonicalPlaceholderExcluded
  /\ LiveEffectRetained
  /\ MissingLiveFailsClosed
  /\ ReadyContextsAgree

=============================================================================
