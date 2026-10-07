-------------------- MODULE NativeCacheBacking --------------------
EXTENDS Naturals

CONSTANTS
  \* @type: Set(Int);
  Workers,
  \* @type: Set(Int);
  Warm,
  \* @type: Int;
  Stages,
  \* @type: Int;
  Records,
  \* @type: Int;
  Limit,
  \* @type: Int;
  RetryLimit,
  \* @type: Bool;
  SkipCredit,
  \* @type: Bool;
  EarlyPublish
VARIABLES
  \* @type: Int -> Str;
  phase,
  \* @type: Int -> Int;
  used,
  \* @type: Int -> Int;
  paid,
  \* @type: Int -> Int;
  prepared,
  \* @type: Int -> Int;
  allocated,
  \* @type: Int -> Int;
  cache,
  \* @type: Int -> Int;
  retries
vars == <<phase, used, paid, prepared, allocated, cache, retries>>
Original(w) == IF w \in Warm THEN Records ELSE 0

Init ==
  /\ phase = [w \in Workers |-> "Preparing"]
  /\ used = [w \in Workers |-> 0]
  /\ paid = [w \in Workers |-> 0]
  /\ prepared = [w \in Workers |-> 0]
  /\ allocated = [w \in Workers |-> 0]
  /\ cache = [w \in Workers |-> Original(w)]
  /\ retries = [w \in Workers |-> 0]

Reserve(w) ==
  /\ phase[w] = "Preparing"
  /\ prepared[w] < Stages
  /\ paid[w] = prepared[w]
  /\ used[w] < Limit
  /\ used' = [used EXCEPT ![w] = @ + 1]
  /\ paid' = [paid EXCEPT ![w] = @ + 1]
  /\ UNCHANGED <<phase, prepared, allocated, cache, retries>>

Copy(w) ==
  /\ phase[w] = "Preparing"
  /\ prepared[w] < Stages
  /\ paid[w] > prepared[w] \/ SkipCredit
  /\ prepared' = [prepared EXCEPT ![w] = @ + 1]
  /\ allocated' = [allocated EXCEPT ![w] = @ + 1]
  /\ cache' = IF EarlyPublish /\ w \notin Warm
               THEN [cache EXCEPT ![w] = 1] ELSE cache
  /\ UNCHANGED <<phase, used, paid, retries>>

Publish(w) ==
  /\ phase[w] = "Preparing"
  /\ prepared[w] = Stages
  /\ phase' = [phase EXCEPT ![w] = "Complete"]
  /\ cache' = [cache EXCEPT ![w] = Records]
  /\ UNCHANGED <<used, paid, prepared, allocated, retries>>

Cancel(w) ==
  /\ phase[w] = "Preparing"
  /\ phase' = [phase EXCEPT ![w] = "Cancelled"]
  /\ UNCHANGED <<used, paid, prepared, allocated, cache, retries>>

Retry(w) ==
  /\ phase[w] = "Cancelled"
  /\ retries[w] < RetryLimit
  /\ phase' = [phase EXCEPT ![w] = "Preparing"]
  /\ paid' = [paid EXCEPT ![w] = 0]
  /\ prepared' = [prepared EXCEPT ![w] = 0]
  /\ retries' = [retries EXCEPT ![w] = @ + 1]
  /\ UNCHANGED <<used, allocated, cache>>

Restore(w) ==
  /\ phase[w] = "Complete"
  /\ phase' = [phase EXCEPT ![w] = "Restored"]
  /\ cache' = [cache EXCEPT ![w] = Original(w)]
  /\ UNCHANGED <<used, paid, prepared, allocated, retries>>

Next == \E w \in Workers : Reserve(w) \/ Copy(w) \/ Publish(w) \/ Cancel(w) \/ Retry(w) \/ Restore(w)
Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ phase \in [Workers -> {"Preparing", "Complete", "Cancelled", "Restored"}]
  /\ used \in [Workers -> 0..Limit]
  /\ paid \in [Workers -> 0..Stages]
  /\ prepared \in [Workers -> 0..Stages]
  /\ allocated \in [Workers -> 0..(Stages * (RetryLimit + 1))]
  /\ cache \in [Workers -> 0..Records]
  /\ retries \in [Workers -> 0..RetryLimit]
PaidCopies == \A w \in Workers : allocated[w] <= used[w]
NoPartialCache == \A w \in Workers : cache[w] \in {0, Records}
UnpublishedCachePreserved == \A w \in Workers :
  phase[w] \in {"Preparing", "Cancelled", "Restored"} => cache[w] = Original(w)
CompleteResultPrepared == \A w \in Workers :
  phase[w] = "Complete" => prepared[w] = Stages /\ cache[w] = Records
=============================================================================
