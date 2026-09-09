--------------------------- MODULE HostWorkBudget ---------------------------
EXTENDS Naturals, Integers, Sequences, FiniteSets, TLC

CONSTANT Defect

ASSUME Defect \in {
  "none",
  "arrival-order-result",
  "wrapping-arithmetic",
  "saturating-arithmetic",
  "mutation-before-reservation",
  "schedule-mismatch",
  "replay-without-enforcement",
  "noncumulative-replay",
  "cross-shard-shared-budget",
  "economic-debit",
  "decode-after-allocation"
}

Validators == {"validator-0", "validator-1"}
Shards == {"shard-0", "shard-1"}
Branches == {"branch-0", "branch-1"}
Phases == {
  "structural-admission",
  "pure-reduction",
  "primitive-evaluation",
  "substitution",
  "authority-discovery",
  "physical-search",
  "witness-decoding",
  "witness-verification"
}
Dimensions == {
  "structural-items",
  "structural-bytes",
  "reduction-steps",
  "reduction-term-bytes",
  "primitive-calls",
  "primitive-input-bytes",
  "substitution-bindings",
  "substitution-bytes",
  "authority-nodes",
  "authority-depth",
  "search-candidates",
  "search-state-bytes",
  "witness-fields",
  "witness-bytes",
  "verification-operations",
  "verification-bytes"
}
Schedules == {"schedule-0", "schedule-1"}
Jobs == 0..19

NoBranch == "no-branch"
NoSchedule == "no-schedule"
NoVerdict == "no-verdict"
CanonicalSchedule == "schedule-0"
AlternativeSchedule == "schedule-1"
CanonicalBranch == "branch-0"
MaxCounter == 7
InitialBalance == 100
JobCount == Cardinality(Jobs)
TotalWork == 64

DimensionPhase == [dimension \in Dimensions |->
  CASE dimension \in {"structural-items", "structural-bytes"} ->
         "structural-admission"
    [] dimension \in {"reduction-steps", "reduction-term-bytes"} ->
         "pure-reduction"
    [] dimension \in {"primitive-calls", "primitive-input-bytes"} ->
         "primitive-evaluation"
    [] dimension \in {"substitution-bindings", "substitution-bytes"} ->
         "substitution"
    [] dimension \in {"authority-nodes", "authority-depth"} ->
         "authority-discovery"
    [] dimension \in {"search-candidates", "search-state-bytes"} ->
         "physical-search"
    [] dimension \in {"witness-fields", "witness-bytes"} ->
         "witness-decoding"
    [] OTHER -> "witness-verification"]

JobDimension == [job \in Jobs |->
  CASE job \in {0, 1} -> "structural-items"
    [] job = 2 -> "structural-bytes"
    [] job \in {3, 19} -> "reduction-steps"
    [] job = 4 -> "reduction-term-bytes"
    [] job = 5 -> "primitive-calls"
    [] job = 6 -> "primitive-input-bytes"
    [] job = 7 -> "substitution-bindings"
    [] job = 8 -> "substitution-bytes"
    [] job \in {9, 16} -> "authority-nodes"
    [] job = 10 -> "authority-depth"
    [] job = 11 -> "search-candidates"
    [] job = 12 -> "search-state-bytes"
    [] job = 13 -> "witness-fields"
    [] job \in {14, 17} -> "witness-bytes"
    [] job = 15 -> "verification-operations"
    [] OTHER -> "verification-bytes"]

JobPhase == [job \in Jobs |-> DimensionPhase[JobDimension[job]]]

JobShard == [job \in Jobs |->
  CASE job \in {0, 2, 3, 5, 7, 9, 11, 13, 14, 15, 16, 17} -> "shard-0"
    [] OTHER -> "shard-1"]

JobBranch == [job \in Jobs |->
  IF job % 2 = 0 THEN "branch-0" ELSE "branch-1"]

JobWork == [job \in Jobs |->
  CASE job \in {0, 1, 2} -> 2
    [] job \in {4, 6, 8, 12, 18} -> 3
    [] job \in {9, 16} -> 4
    [] job \in {14, 17} -> 2
    [] job = 19 -> 0
    [] OTHER -> 1]

ScheduleLimit(schedule, dimension) ==
  CASE dimension = "structural-items" -> 3
    [] dimension = "structural-bytes" -> 3
    [] dimension = "reduction-steps" -> 2
    [] dimension = "reduction-term-bytes" -> 4
    [] dimension = "primitive-calls" -> 2
    [] dimension = "primitive-input-bytes" -> 4
    [] dimension = "substitution-bindings" -> 2
    [] dimension = "substitution-bytes" -> 4
    [] dimension = "authority-nodes" -> 7
    [] dimension = "authority-depth" -> 2
    [] dimension = "search-candidates" ->
         IF schedule = CanonicalSchedule THEN 2 ELSE 1
    [] dimension = "search-state-bytes" -> 4
    [] dimension = "witness-fields" -> 2
    [] dimension = "witness-bytes" -> 3
    [] dimension = "verification-operations" -> 2
    [] OTHER -> 4

SeqMembers(sequence) == {sequence[index] : index \in 1..Len(sequence)}

CheckedFits(current, work, limit) ==
  /\ current \in 0..MaxCounter
  /\ current <= limit
  /\ work <= MaxCounter - current
  /\ work <= limit - current

ArithmeticMode ==
  CASE Defect = "wrapping-arithmetic" -> "wrapping"
    [] Defect = "saturating-arithmetic" -> "saturating"
    [] OTHER -> "checked"

Minimum(left, right) == IF left <= right THEN left ELSE right

MachineNext(current, work, limit) ==
  CASE ArithmeticMode = "wrapping" -> (current + work) % (MaxCounter + 1)
    [] ArithmeticMode = "saturating" -> Minimum(limit, current + work)
    [] OTHER -> current + work

MachineFits(current, work, limit) ==
  CASE ArithmeticMode = "checked" -> CheckedFits(current, work, limit)
    [] OTHER -> MachineNext(current, work, limit) <= limit

UsesSharedBudget == Defect = "cross-shard-shared-budget"

RECURSIVE IntendedUsage(_, _, _)
IntendedUsage(count, shard, dimension) ==
  IF count = 0
  THEN 0
  ELSE LET job == count - 1
       IN IntendedUsage(job, shard, dimension) +
          IF JobShard[job] = shard /\ JobDimension[job] = dimension
          THEN JobWork[job]
          ELSE 0

CompleteEventVerdict(schedule) ==
  IF \E shard \in Shards, dimension \in Dimensions :
       LET requested == IntendedUsage(JobCount, shard, dimension)
       IN requested > MaxCounter \/ requested > ScheduleLimit(schedule, dimension)
  THEN "rejected"
  ELSE "accepted"

VARIABLES
  arrival,
  selected,
  completed,
  usage,
  logicalUsage,
  sharedUsage,
  accepted,
  rejected,
  rejectionSeen,
  mutations,
  failedMutation,
  balances,
  decodedAllocated,
  falseShardRejection,
  finished,
  verdict,
  transactionUsage,
  transactionMutations,
  checkpointed,
  checkpointPublished,
  checkpointEvents,
  checkpointUsage,
  checkpointAccepted,
  checkpointResult,
  checkpointSchedule,
  checkpointVerdict,
  replayStarted,
  replayCursor,
  replayRejected,
  replayed,
  replayEvents,
  replayUsage,
  replayResultUsage,
  replayAccepted,
  replaySchedule,
  replayEnforced,
  replayVerdict

vars == <<
  arrival,
  selected,
  completed,
  usage,
  logicalUsage,
  sharedUsage,
  accepted,
  rejected,
  rejectionSeen,
  mutations,
  failedMutation,
  balances,
  decodedAllocated,
  falseShardRejection,
  finished,
  verdict,
  transactionUsage,
  transactionMutations,
  checkpointed,
  checkpointPublished,
  checkpointEvents,
  checkpointUsage,
  checkpointAccepted,
  checkpointResult,
  checkpointSchedule,
  checkpointVerdict,
  replayStarted,
  replayCursor,
  replayRejected,
  replayed,
  replayEvents,
  replayUsage,
  replayResultUsage,
  replayAccepted,
  replaySchedule,
  replayEnforced,
  replayVerdict
>>

ZeroUsage == [validator \in Validators |->
  [shard \in Shards |-> [dimension \in Dimensions |-> 0]]]

ZeroValidatorUsage ==
  [shard \in Shards |-> [dimension \in Dimensions |-> 0]]

ZeroSharedUsage == [validator \in Validators |->
  [dimension \in Dimensions |-> 0]]

Init ==
  /\ arrival = [validator \in Validators |-> <<>>]
  /\ selected = [validator \in Validators |-> NoBranch]
  /\ completed = [validator \in Validators |-> {}]
  /\ usage = ZeroUsage
  /\ logicalUsage = ZeroUsage
  /\ sharedUsage = ZeroSharedUsage
  /\ accepted = [validator \in Validators |-> {}]
  /\ rejected = [validator \in Validators |-> {}]
  /\ rejectionSeen = [validator \in Validators |-> FALSE]
  /\ mutations = [validator \in Validators |-> {}]
  /\ failedMutation = [validator \in Validators |-> FALSE]
  /\ balances = [validator \in Validators |->
       [shard \in Shards |-> InitialBalance]]
  /\ decodedAllocated = [validator \in Validators |->
       [shard \in Shards |-> 0]]
  /\ falseShardRejection = [validator \in Validators |-> FALSE]
  /\ finished = {}
  /\ verdict = [validator \in Validators |-> NoVerdict]
  /\ transactionUsage = ZeroUsage
  /\ transactionMutations = [validator \in Validators |-> {}]
  /\ checkpointed = {}
  /\ checkpointPublished = {}
  /\ checkpointEvents = [validator \in Validators |-> {}]
  /\ checkpointUsage = ZeroUsage
  /\ checkpointAccepted = [validator \in Validators |-> {}]
  /\ checkpointResult = [validator \in Validators |-> NoBranch]
  /\ checkpointSchedule = [validator \in Validators |-> NoSchedule]
  /\ checkpointVerdict = [validator \in Validators |-> NoVerdict]
  /\ replayStarted = {}
  /\ replayCursor = [validator \in Validators |-> 0]
  /\ replayRejected = [validator \in Validators |-> FALSE]
  /\ replayed = {}
  /\ replayEvents = [validator \in Validators |-> {}]
  /\ replayUsage = ZeroUsage
  /\ replayResultUsage = ZeroUsage
  /\ replayAccepted = [validator \in Validators |-> {}]
  /\ replaySchedule = [validator \in Validators |-> NoSchedule]
  /\ replayEnforced = [validator \in Validators |-> FALSE]
  /\ replayVerdict = [validator \in Validators |-> NoVerdict]

Receive(validator, branch) ==
  /\ branch \notin SeqMembers(arrival[validator])
  /\ arrival' = [arrival EXCEPT ![validator] = Append(@, branch)]
  /\ UNCHANGED <<selected, completed, usage, logicalUsage, sharedUsage,
       accepted, rejected, rejectionSeen, mutations, failedMutation, balances,
       decodedAllocated, falseShardRejection, finished, verdict,
       transactionUsage, transactionMutations, checkpointed,
       checkpointPublished, checkpointEvents, checkpointUsage,
       checkpointAccepted, checkpointResult, checkpointSchedule,
       checkpointVerdict, replayStarted, replayCursor, replayRejected,
       replayed, replayEvents, replayUsage, replayResultUsage, replayAccepted,
       replaySchedule, replayEnforced, replayVerdict>>

Select(validator) ==
  /\ selected[validator] = NoBranch
  /\ SeqMembers(arrival[validator]) = Branches
  /\ selected' = [selected EXCEPT ![validator] =
       IF Defect = "arrival-order-result"
       THEN Head(arrival[validator])
       ELSE CanonicalBranch]
  /\ UNCHANGED <<arrival, completed, usage, logicalUsage, sharedUsage,
       accepted, rejected, rejectionSeen, mutations, failedMutation, balances,
       decodedAllocated, falseShardRejection, finished, verdict,
       transactionUsage, transactionMutations, checkpointed,
       checkpointPublished, checkpointEvents, checkpointUsage,
       checkpointAccepted, checkpointResult, checkpointSchedule,
       checkpointVerdict, replayStarted, replayCursor, replayRejected,
       replayed, replayEvents, replayUsage, replayResultUsage, replayAccepted,
       replaySchedule, replayEnforced, replayVerdict>>

BranchJobs(branch) == {job \in Jobs : JobBranch[job] = branch}

BranchComplete(validator, branch) ==
  BranchJobs(branch) \subseteq completed[validator]

NextBranchJob(validator, branch) ==
  CHOOSE job \in BranchJobs(branch) \ completed[validator] :
    \A other \in BranchJobs(branch) \ completed[validator] : job <= other

EffectiveCounter(validator, job) ==
  IF UsesSharedBudget
  THEN sharedUsage[validator][JobDimension[job]]
  ELSE usage[validator][JobShard[job]][JobDimension[job]]

JobFits(validator, job) ==
  MachineFits(
    EffectiveCounter(validator, job),
    JobWork[job],
    ScheduleLimit(CanonicalSchedule, JobDimension[job]))

LocalJobFits(validator, job) ==
  CheckedFits(
    usage[validator][JobShard[job]][JobDimension[job]],
    JobWork[job],
    ScheduleLimit(CanonicalSchedule, JobDimension[job]))

Process(validator, branch) ==
  /\ selected[validator] \in Branches
  /\ branch \in Branches
  /\ ~BranchComplete(validator, branch)
  /\ LET job == NextBranchJob(validator, branch)
         shard == JobShard[job]
         phase == JobPhase[job]
         dimension == JobDimension[job]
         work == JobWork[job]
         limit == ScheduleLimit(CanonicalSchedule, dimension)
         current == EffectiveCounter(validator, job)
         fits == JobFits(validator, job)
         nextCounter == MachineNext(current, work, limit)
     IN
       /\ phase = DimensionPhase[dimension]
       /\ completed' = [completed EXCEPT ![validator] = @ \cup {job}]
       /\ usage' =
            IF fits
            THEN IF UsesSharedBudget
                 THEN [usage EXCEPT ![validator][shard][dimension] = @ + work]
                 ELSE [usage EXCEPT ![validator][shard][dimension] = nextCounter]
            ELSE usage
       /\ logicalUsage' =
            IF fits
            THEN [logicalUsage EXCEPT ![validator][shard][dimension] = @ + work]
            ELSE logicalUsage
       /\ sharedUsage' =
            IF fits /\ UsesSharedBudget
            THEN [sharedUsage EXCEPT ![validator][dimension] = nextCounter]
            ELSE sharedUsage
       /\ accepted' = [accepted EXCEPT ![validator] =
            IF fits THEN @ \cup {job} ELSE @]
       /\ rejected' = [rejected EXCEPT ![validator] =
            IF fits THEN @ ELSE @ \cup {job}]
       /\ rejectionSeen' = [rejectionSeen EXCEPT ![validator] = @ \/ ~fits]
       /\ mutations' = [mutations EXCEPT ![validator] =
            IF fits \/ Defect = "mutation-before-reservation"
            THEN @ \cup {job}
            ELSE @]
       /\ failedMutation' = [failedMutation EXCEPT ![validator] =
            @ \/ (~fits /\ Defect = "mutation-before-reservation")]
       /\ balances' =
            IF fits /\ Defect = "economic-debit"
            THEN [balances EXCEPT ![validator][shard] = @ - work]
            ELSE balances
       /\ decodedAllocated' =
            IF dimension = "witness-bytes" /\
                 (fits \/ Defect = "decode-after-allocation")
            THEN [decodedAllocated EXCEPT ![validator][shard] = @ + work]
            ELSE decodedAllocated
       /\ falseShardRejection' = [falseShardRejection EXCEPT ![validator] =
            @ \/ (UsesSharedBudget /\ ~fits /\ LocalJobFits(validator, job))]
  /\ UNCHANGED <<arrival, selected, finished, verdict, transactionUsage,
       transactionMutations, checkpointed, checkpointPublished,
       checkpointEvents, checkpointUsage, checkpointAccepted,
       checkpointResult, checkpointSchedule, checkpointVerdict,
       replayStarted, replayCursor, replayRejected, replayed, replayEvents,
       replayUsage, replayResultUsage, replayAccepted, replaySchedule,
       replayEnforced, replayVerdict>>

Finish(validator) ==
  /\ completed[validator] = Jobs
  /\ validator \notin finished
  /\ LET failed == rejectionSeen[validator]
     IN
       /\ finished' = finished \cup {validator}
       /\ verdict' = [verdict EXCEPT ![validator] =
            IF failed THEN "rejected" ELSE "accepted"]
       /\ transactionUsage' = [transactionUsage EXCEPT ![validator] =
            IF failed THEN ZeroValidatorUsage ELSE usage[validator]]
       /\ transactionMutations' = [transactionMutations EXCEPT ![validator] =
            IF failed THEN {} ELSE mutations[validator]]
  /\ UNCHANGED <<arrival, selected, completed, usage, logicalUsage,
       sharedUsage, accepted, rejected, rejectionSeen, mutations,
       failedMutation, balances, decodedAllocated, falseShardRejection,
       checkpointed, checkpointPublished, checkpointEvents, checkpointUsage,
       checkpointAccepted, checkpointResult, checkpointSchedule,
       checkpointVerdict, replayStarted, replayCursor, replayRejected,
       replayed, replayEvents, replayUsage, replayResultUsage, replayAccepted,
       replaySchedule, replayEnforced, replayVerdict>>

Checkpoint(validator) ==
  /\ validator \in finished
  /\ validator \notin checkpointed
  /\ checkpointed' = checkpointed \cup {validator}
  /\ checkpointPublished' =
       IF verdict[validator] = "accepted"
       THEN checkpointPublished \cup {validator}
       ELSE checkpointPublished
  /\ checkpointEvents' = [checkpointEvents EXCEPT ![validator] = Jobs]
  /\ checkpointUsage' = [checkpointUsage EXCEPT ![validator] = usage[validator]]
  /\ checkpointAccepted' = [checkpointAccepted EXCEPT ![validator] = accepted[validator]]
  /\ checkpointResult' = [checkpointResult EXCEPT ![validator] = selected[validator]]
  /\ checkpointSchedule' = [checkpointSchedule EXCEPT ![validator] = CanonicalSchedule]
  /\ checkpointVerdict' = [checkpointVerdict EXCEPT ![validator] = verdict[validator]]
  /\ UNCHANGED <<arrival, selected, completed, usage, logicalUsage,
       sharedUsage, accepted, rejected, rejectionSeen, mutations,
       failedMutation, balances, decodedAllocated, falseShardRejection,
       finished, verdict, transactionUsage, transactionMutations,
       replayStarted, replayCursor, replayRejected, replayed, replayEvents,
       replayUsage, replayResultUsage, replayAccepted, replaySchedule,
       replayEnforced, replayVerdict>>

ReplayScheduleFor(validator) ==
  IF Defect = "schedule-mismatch"
  THEN AlternativeSchedule
  ELSE checkpointSchedule[validator]

ReplayStart(validator) ==
  /\ validator \in checkpointed
  /\ validator \notin replayStarted
  /\ replayStarted' = replayStarted \cup {validator}
  /\ replaySchedule' = [replaySchedule EXCEPT
       ![validator] = ReplayScheduleFor(validator)]
  /\ replayEnforced' = [replayEnforced EXCEPT
       ![validator] = Defect # "replay-without-enforcement"]
  /\ UNCHANGED <<arrival, selected, completed, usage, logicalUsage,
       sharedUsage, accepted, rejected, rejectionSeen, mutations,
       failedMutation, balances, decodedAllocated, falseShardRejection,
       finished, verdict, transactionUsage, transactionMutations,
       checkpointed, checkpointPublished, checkpointEvents, checkpointUsage,
       checkpointAccepted, checkpointResult, checkpointSchedule,
       checkpointVerdict, replayCursor, replayRejected, replayed, replayEvents,
       replayUsage, replayResultUsage, replayAccepted, replayVerdict>>

ReplayFits(validator, job) ==
  IF Defect = "replay-without-enforcement"
  THEN TRUE
  ELSE LET shard == JobShard[job]
           dimension == JobDimension[job]
           current == IF Defect = "noncumulative-replay"
                      THEN 0
                      ELSE replayUsage[validator][shard][dimension]
       IN CheckedFits(
            current,
            JobWork[job],
            ScheduleLimit(replaySchedule[validator], dimension))

ReplayStep(validator) ==
  /\ validator \in replayStarted
  /\ validator \notin replayed
  /\ replayCursor[validator] < JobCount
  /\ LET job == replayCursor[validator]
         shard == JobShard[job]
         dimension == JobDimension[job]
         work == JobWork[job]
         fits == ReplayFits(validator, job)
     IN
       /\ replayCursor' = [replayCursor EXCEPT ![validator] = @ + 1]
       /\ replayEvents' = [replayEvents EXCEPT ![validator] = @ \cup {job}]
       /\ replayUsage' =
            IF fits /\ Defect # "replay-without-enforcement"
            THEN [replayUsage EXCEPT ![validator][shard][dimension] = @ + work]
            ELSE replayUsage
       /\ replayAccepted' = [replayAccepted EXCEPT ![validator] =
            IF fits THEN @ \cup {job} ELSE @]
       /\ replayRejected' = [replayRejected EXCEPT ![validator] = @ \/ ~fits]
  /\ UNCHANGED <<arrival, selected, completed, usage, logicalUsage,
       sharedUsage, accepted, rejected, rejectionSeen, mutations,
       failedMutation, balances, decodedAllocated, falseShardRejection,
       finished, verdict, transactionUsage, transactionMutations,
       checkpointed, checkpointPublished, checkpointEvents, checkpointUsage,
       checkpointAccepted, checkpointResult, checkpointSchedule,
       checkpointVerdict, replayStarted, replayed, replayResultUsage,
       replaySchedule, replayEnforced, replayVerdict>>

ReplayFinish(validator) ==
  /\ validator \in replayStarted
  /\ validator \notin replayed
  /\ replayCursor[validator] = JobCount
  /\ LET failed == replayRejected[validator]
     IN
       /\ replayed' = replayed \cup {validator}
       /\ replayVerdict' = [replayVerdict EXCEPT ![validator] =
            IF failed THEN "rejected" ELSE "accepted"]
       /\ replayResultUsage' = [replayResultUsage EXCEPT ![validator] =
            IF failed THEN ZeroValidatorUsage ELSE replayUsage[validator]]
  /\ UNCHANGED <<arrival, selected, completed, usage, logicalUsage,
       sharedUsage, accepted, rejected, rejectionSeen, mutations,
       failedMutation, balances, decodedAllocated, falseShardRejection,
       finished, verdict, transactionUsage, transactionMutations,
       checkpointed, checkpointPublished, checkpointEvents, checkpointUsage,
       checkpointAccepted, checkpointResult, checkpointSchedule,
       checkpointVerdict, replayStarted, replayCursor, replayRejected,
       replayEvents, replayUsage, replayAccepted, replaySchedule,
       replayEnforced>>

Next ==
  \/ \E validator \in Validators, branch \in Branches : Receive(validator, branch)
  \/ \E validator \in Validators : Select(validator)
  \/ \E validator \in Validators, branch \in Branches : Process(validator, branch)
  \/ \E validator \in Validators : Finish(validator)
  \/ \E validator \in Validators : Checkpoint(validator)
  \/ \E validator \in Validators : ReplayStart(validator)
  \/ \E validator \in Validators : ReplayStep(validator)
  \/ \E validator \in Validators : ReplayFinish(validator)

Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ DimensionPhase \in [Dimensions -> Phases]
  /\ JobDimension \in [Jobs -> Dimensions]
  /\ JobPhase \in [Jobs -> Phases]
  /\ JobShard \in [Jobs -> Shards]
  /\ JobBranch \in [Jobs -> Branches]
  /\ arrival \in [Validators -> Seq(Branches)]
  /\ selected \in [Validators -> Branches \cup {NoBranch}]
  /\ completed \in [Validators -> SUBSET Jobs]
  /\ usage \in [Validators -> [Shards -> [Dimensions -> 0..MaxCounter]]]
  /\ logicalUsage \in [Validators -> [Shards -> [Dimensions -> 0..TotalWork]]]
  /\ sharedUsage \in [Validators -> [Dimensions -> 0..MaxCounter]]
  /\ accepted \in [Validators -> SUBSET Jobs]
  /\ rejected \in [Validators -> SUBSET Jobs]
  /\ rejectionSeen \in [Validators -> BOOLEAN]
  /\ mutations \in [Validators -> SUBSET Jobs]
  /\ failedMutation \in [Validators -> BOOLEAN]
  /\ balances \in [Validators -> [Shards -> 0..InitialBalance]]
  /\ decodedAllocated \in [Validators -> [Shards -> 0..TotalWork]]
  /\ falseShardRejection \in [Validators -> BOOLEAN]
  /\ finished \subseteq Validators
  /\ verdict \in [Validators -> {"accepted", "rejected", NoVerdict}]
  /\ transactionUsage \in [Validators -> [Shards -> [Dimensions -> 0..MaxCounter]]]
  /\ transactionMutations \in [Validators -> SUBSET Jobs]
  /\ checkpointed \subseteq Validators
  /\ checkpointPublished \subseteq Validators
  /\ checkpointEvents \in [Validators -> SUBSET Jobs]
  /\ checkpointUsage \in [Validators -> [Shards -> [Dimensions -> 0..MaxCounter]]]
  /\ checkpointAccepted \in [Validators -> SUBSET Jobs]
  /\ checkpointResult \in [Validators -> Branches \cup {NoBranch}]
  /\ checkpointSchedule \in [Validators -> Schedules \cup {NoSchedule}]
  /\ checkpointVerdict \in [Validators -> {"accepted", "rejected", NoVerdict}]
  /\ replayStarted \subseteq Validators
  /\ replayCursor \in [Validators -> 0..JobCount]
  /\ replayRejected \in [Validators -> BOOLEAN]
  /\ replayed \subseteq Validators
  /\ replayEvents \in [Validators -> SUBSET Jobs]
  /\ replayUsage \in [Validators -> [Shards -> [Dimensions -> 0..TotalWork]]]
  /\ replayResultUsage \in [Validators -> [Shards -> [Dimensions -> 0..TotalWork]]]
  /\ replayAccepted \in [Validators -> SUBSET Jobs]
  /\ replaySchedule \in [Validators -> Schedules \cup {NoSchedule}]
  /\ replayEnforced \in [Validators -> BOOLEAN]
  /\ replayVerdict \in [Validators -> {"accepted", "rejected", NoVerdict}]

Inv_ArrivalOrderResult ==
  \A left \in Validators, right \in Validators :
    selected[left] \in Branches /\ selected[right] \in Branches
      => selected[left] = selected[right]

Inv_CheckedReservation ==
  \A validator \in Validators, shard \in Shards, dimension \in Dimensions :
    /\ usage[validator][shard][dimension] = logicalUsage[validator][shard][dimension]
    /\ logicalUsage[validator][shard][dimension]
         <= ScheduleLimit(CanonicalSchedule, dimension)

Inv_FailureNonMutation ==
  \A validator \in Validators : ~failedMutation[validator]

Inv_RejectionSticky ==
  \A validator \in Validators :
    rejectionSeen[validator] = (rejected[validator] # {})

Inv_ShardLocalAdmission ==
  \A validator \in Validators : ~falseShardRejection[validator]

Inv_EconomicSeparation ==
  balances = [validator \in Validators |->
    [shard \in Shards |-> InitialBalance]]

Inv_DecodeAllocationBound ==
  \A validator \in Validators, shard \in Shards :
    decodedAllocated[validator][shard]
      <= ScheduleLimit(CanonicalSchedule, "witness-bytes")

Inv_TransactionalFailureRollback ==
  \A validator \in finished :
    rejectionSeen[validator] =>
      /\ verdict[validator] = "rejected"
      /\ transactionUsage[validator] = ZeroValidatorUsage
      /\ transactionMutations[validator] = {}

Inv_RejectedDeploymentPublishesNothing ==
  \A validator \in checkpointed :
    checkpointVerdict[validator] = "rejected" =>
      /\ validator \notin checkpointPublished
      /\ transactionMutations[validator] = {}

Inv_EndVerdictScheduleIndependent ==
  \A left \in finished, right \in finished :
    verdict[left] = verdict[right]

Inv_CompleteEventVerdictScheduleIndependent ==
  \A left \in Schedules, right \in Schedules :
    CompleteEventVerdict(left) = CompleteEventVerdict(right)

Inv_CompleteEventVerdictAgreement ==
  \A validator \in finished :
    verdict[validator] = CompleteEventVerdict(CanonicalSchedule)

Inv_CheckpointCoherent ==
  \A validator \in checkpointed :
    /\ checkpointUsage[validator] = usage[validator]
    /\ checkpointEvents[validator] = Jobs
    /\ checkpointAccepted[validator] = accepted[validator]
    /\ checkpointResult[validator] = selected[validator]
    /\ checkpointSchedule[validator] = CanonicalSchedule
    /\ checkpointVerdict[validator] = verdict[validator]

Inv_ReplayScheduleAgreement ==
  \A validator \in replayed :
    replaySchedule[validator] = checkpointSchedule[validator]

Inv_ReplayEnforced ==
  \A validator \in replayed : replayEnforced[validator]

Inv_ReplayRollback ==
  \A validator \in replayed :
    replayVerdict[validator] = "rejected" =>
      replayResultUsage[validator] = ZeroValidatorUsage

Inv_ReplayCumulativeVerdictAgreement ==
  \A validator \in replayed :
    replayVerdict[validator] = checkpointVerdict[validator]

Inv_ReplayResultAgreement ==
  \A validator \in replayed :
    /\ replayEvents[validator] = checkpointEvents[validator]
    /\ (checkpointVerdict[validator] = "accepted" =>
          /\ replayAccepted[validator] = checkpointAccepted[validator]
          /\ replayResultUsage[validator] = checkpointUsage[validator])

Safety ==
  /\ TypeOK
  /\ Inv_ArrivalOrderResult
  /\ Inv_CheckedReservation
  /\ Inv_FailureNonMutation
  /\ Inv_RejectionSticky
  /\ Inv_ShardLocalAdmission
  /\ Inv_EconomicSeparation
  /\ Inv_DecodeAllocationBound
  /\ Inv_TransactionalFailureRollback
  /\ Inv_RejectedDeploymentPublishesNothing
  /\ Inv_EndVerdictScheduleIndependent
  /\ Inv_CompleteEventVerdictScheduleIndependent
  /\ Inv_CompleteEventVerdictAgreement
  /\ Inv_CheckpointCoherent
  /\ Inv_ReplayScheduleAgreement
  /\ Inv_ReplayEnforced
  /\ Inv_ReplayRollback
  /\ Inv_ReplayCumulativeVerdictAgreement
  /\ Inv_ReplayResultAgreement

=============================================================================
