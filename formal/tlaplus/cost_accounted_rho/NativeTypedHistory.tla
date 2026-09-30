-------------------- MODULE NativeTypedHistory --------------------
EXTENDS Naturals

CONSTANTS
  \* @type: Set(Int);
  Workers,
  \* @type: Int;
  Records,
  \* @type: Int;
  Limit,
  \* @type: Int;
  RetryLimit,
  \* @type: Bool;
  SkipCredit,
  \* @type: Bool;
  EarlyCache
VARIABLES
  \* @type: Int -> Str;
  phase,
  \* @type: Int -> Int;
  used,
  \* @type: Int -> Int;
  paid,
  \* @type: Int -> Int;
  visited,
  \* @type: Int -> Int;
  decoded,
  \* @type: Int -> Int;
  cache,
  \* @type: Int -> Int;
  retries
vars == <<phase, used, paid, visited, decoded, cache, retries>>

Init ==
  /\ phase = [w \in Workers |-> "Reading"]
  /\ used = [w \in Workers |-> 0]
  /\ paid = [w \in Workers |-> 0]
  /\ visited = [w \in Workers |-> 0]
  /\ decoded = [w \in Workers |-> 0]
  /\ cache = [w \in Workers |-> 0]
  /\ retries = [w \in Workers |-> 0]

Reserve(w) ==
  /\ phase[w] = "Reading"
  /\ visited[w] < Records
  /\ paid[w] = visited[w]
  /\ used[w] < Limit
  /\ used' = [used EXCEPT ![w] = @ + 1]
  /\ paid' = [paid EXCEPT ![w] = @ + 1]
  /\ UNCHANGED <<phase, visited, decoded, cache, retries>>

Decode(w) ==
  /\ phase[w] = "Reading"
  /\ visited[w] < Records
  /\ paid[w] > visited[w] \/ SkipCredit
  /\ visited' = [visited EXCEPT ![w] = @ + 1]
  /\ decoded' = [decoded EXCEPT ![w] = @ + 1]
  /\ cache' = IF EarlyCache THEN [cache EXCEPT ![w] = visited[w] + 1] ELSE cache
  /\ UNCHANGED <<phase, used, paid, retries>>

Publish(w) ==
  /\ phase[w] = "Reading"
  /\ visited[w] = Records
  /\ phase' = [phase EXCEPT ![w] = "Complete"]
  /\ cache' = [cache EXCEPT ![w] = Records]
  /\ UNCHANGED <<used, paid, visited, decoded, retries>>

Reject(w) ==
  /\ phase[w] = "Reading"
  /\ phase' = [phase EXCEPT ![w] = "Rejected"]
  /\ UNCHANGED <<used, paid, visited, decoded, cache, retries>>

Retry(w) ==
  /\ phase[w] = "Rejected"
  /\ retries[w] < RetryLimit
  /\ phase' = [phase EXCEPT ![w] = "Reading"]
  /\ paid' = [paid EXCEPT ![w] = 0]
  /\ visited' = [visited EXCEPT ![w] = 0]
  /\ retries' = [retries EXCEPT ![w] = @ + 1]
  /\ UNCHANGED <<used, decoded, cache>>

Next == \E w \in Workers : Reserve(w) \/ Decode(w) \/ Publish(w) \/ Reject(w) \/ Retry(w)
Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ phase \in [Workers -> {"Reading", "Complete", "Rejected"}]
  /\ used \in [Workers -> 0..Limit]
  /\ paid \in [Workers -> 0..Records]
  /\ visited \in [Workers -> 0..Records]
  /\ decoded \in [Workers -> 0..(Records * (RetryLimit + 1))]
  /\ cache \in [Workers -> 0..Records]
  /\ retries \in [Workers -> 0..RetryLimit]

PaidDecoding == \A w \in Workers : decoded[w] <= used[w]
CompleteCache == \A w \in Workers : cache[w] \in {0, Records}
RejectedCacheEmpty == \A w \in Workers : phase[w] = "Rejected" => cache[w] = 0
CompletedCacheExact == \A w \in Workers : phase[w] = "Complete" => cache[w] = Records
=============================================================================
