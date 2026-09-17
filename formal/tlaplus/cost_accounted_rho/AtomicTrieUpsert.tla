-------------------- MODULE AtomicTrieUpsert --------------------
EXTENDS Integers, Sequences, FiniteSets, TLC

CONSTANTS Workers, Keys, Paths, Depth, RecheckParent, CallbackUnderLock, ReturnNil, ReadConsumedBucket
ASSUME /\ Workers # {} /\ IsFiniteSet(Workers)
       /\ Keys # {} /\ IsFiniteSet(Keys)
       /\ Depth \in Nat
       /\ Paths \in [Keys -> Seq(Nat)]
       /\ \A k \in Keys : Len(Paths[k]) = Depth
       /\ RecheckParent \in BOOLEAN /\ CallbackUnderLock \in BOOLEAN
       /\ ReturnNil \in BOOLEAN /\ ReadConsumedBucket \in BOOLEAN
       /\ "free" \notin Workers

NoValue == -1
Free == "free"
Prefix(k, n) == SubSeq(Paths[k], 1, n)
Nodes == {Prefix(k, n) : k \in Keys, n \in 0..Depth}
Leaf(k) == Prefix(k, Depth)
Advance(v) == IF ReturnNil THEN NoValue ELSE IF v = NoValue THEN 1 ELSE v + 1

VARIABLES originalPresent, assigned, pc, offset, available, owner, children, queued, created,
          present, values, expected, observed, result, completed, acknowledgements, releases, bucket
vars == <<originalPresent, assigned, pc, offset, available, owner, children, queued, created,
          present, values, expected, observed, result, completed, acknowledgements, releases, bucket>>

Init ==
    /\ assigned \in [Workers -> Keys]
    /\ present \in SUBSET Keys
    /\ originalPresent = present
    /\ values \in [Keys -> {NoValue, 1}]
    /\ \A k \in Keys : k \notin present => values[k] = NoValue
    /\ LET existing == {<<>>} \cup {Prefix(k, n) : k \in present, n \in 0..Depth}
       IN /\ available = [node \in Nodes |-> IF node \in existing THEN 1 ELSE 0]
          /\ created = available
          /\ children = {node \in existing : node # <<>>}
    /\ owner = [node \in Nodes |-> Free]
    /\ queued = [node \in Nodes |-> 0]
    /\ pc = [w \in Workers |-> "walk"]
    /\ offset = [w \in Workers |-> 0]
    /\ observed = [w \in Workers |-> NoValue]
    /\ result = [w \in Workers |-> NoValue]
    /\ expected = values
    /\ completed = {}
    /\ acknowledgements = {}
    /\ releases = {}
    /\ bucket = [w \in Workers |-> [entries |-> values, members |-> present]]

Walk(w) ==
    /\ pc[w] = "walk"
    /\ available[Prefix(assigned[w], offset[w])] > 0
    /\ IF offset[w] = Depth
       THEN /\ pc' = [pc EXCEPT ![w] = IF CallbackUnderLock THEN "leaf" ELSE "outside"]
            /\ observed' = [observed EXCEPT ![w] = values[assigned[w]]]
            /\ bucket' = [bucket EXCEPT ![w] = [entries |-> values, members |-> present]]
            /\ UNCHANGED offset
       ELSE IF Prefix(assigned[w], offset[w] + 1) \in children
            THEN /\ offset' = [offset EXCEPT ![w] = @ + 1]
                 /\ UNCHANGED <<pc, observed, bucket>>
            ELSE /\ pc' = [pc EXCEPT ![w] = "parent"]
                 /\ UNCHANGED <<offset, observed, bucket>>
    /\ UNCHANGED <<originalPresent, assigned, available, owner, children, queued, created, present,
                    values, expected, result, completed, acknowledgements, releases>>

AcquireParent(w) ==
    /\ pc[w] = "parent"
    /\ LET node == Prefix(assigned[w], offset[w])
       IN /\ available[node] > 0
          /\ available' = [available EXCEPT ![node] = @ - 1]
          /\ owner' = [owner EXCEPT ![node] = w]
    /\ pc' = [pc EXCEPT ![w] = "recheck"]
    /\ UNCHANGED <<originalPresent, assigned, offset, children, queued, created, present, values,
                    expected, observed, result, completed, acknowledgements, releases, bucket>>

Recheck(w) ==
    /\ pc[w] = "recheck"
    /\ LET child == Prefix(assigned[w], offset[w] + 1)
       IN /\ children' = children \cup {child}
          /\ queued' = IF ~RecheckParent \/ child \notin children
                       THEN [queued EXCEPT ![child] = @ + 1] ELSE queued
    /\ pc' = [pc EXCEPT ![w] = "releaseParent"]
    /\ UNCHANGED <<originalPresent, assigned, offset, available, owner, created, present, values,
                    expected, observed, result, completed, acknowledgements, releases, bucket>>

ReleaseParent(w) ==
    /\ pc[w] = "releaseParent"
    /\ releases' = releases \cup {Prefix(assigned[w], offset[w])}
    /\ offset' = [offset EXCEPT ![w] = @ + 1]
    /\ pc' = [pc EXCEPT ![w] = "walk"]
    /\ UNCHANGED <<originalPresent, assigned, available, owner, children, queued, created, present, values,
                    expected, observed, result, completed, acknowledgements, bucket>>

PublishParent(node) ==
    /\ node \in releases
    /\ releases' = releases \ {node}
    /\ available' = [available EXCEPT ![node] = @ + 1]
    /\ owner' = [owner EXCEPT ![node] = Free]
    /\ UNCHANGED <<originalPresent, assigned, pc, offset, children, queued, created, present, values,
                    expected, observed, result, completed, acknowledgements, bucket>>

PublishChild(node) ==
    /\ queued[node] > 0
    /\ queued' = [queued EXCEPT ![node] = @ - 1]
    /\ available' = [available EXCEPT ![node] = @ + 1]
    /\ created' = [created EXCEPT ![node] = @ + 1]
    /\ UNCHANGED <<originalPresent, assigned, pc, offset, owner, children, present, values,
                    expected, observed, result, completed, acknowledgements, releases, bucket>>

OutsideCallback(w) ==
    /\ pc[w] = "outside"
    /\ result' = [result EXCEPT ![w] = Advance(observed[w])]
    /\ pc' = [pc EXCEPT ![w] = "leaf"]
    /\ UNCHANGED <<originalPresent, assigned, offset, available, owner, children, queued, created,
                    present, values, expected, observed, completed, acknowledgements, releases, bucket>>

AcquireLeaf(w) ==
    /\ pc[w] = "leaf"
    /\ LET leaf == Leaf(assigned[w])
       IN /\ available[leaf] > 0
          /\ available' = [available EXCEPT ![leaf] = @ - 1]
          /\ owner' = [owner EXCEPT ![leaf] = w]
    /\ observed' = [observed EXCEPT ![w] = values[assigned[w]]]
    /\ bucket' = IF ReadConsumedBucket THEN [bucket EXCEPT ![w] = [entries |-> values, members |-> present]] ELSE bucket
    /\ pc' = [pc EXCEPT ![w] = IF CallbackUnderLock THEN "callback" ELSE "reply"]
    /\ UNCHANGED <<originalPresent, assigned, offset, children, queued, created, present, values,
                    expected, result, completed, acknowledgements, releases>>

Callback(w) ==
    /\ pc[w] = "callback"
    /\ result' = [result EXCEPT ![w] = Advance(observed[w])]
    /\ pc' = [pc EXCEPT ![w] = "reply"]
    /\ UNCHANGED <<originalPresent, assigned, offset, available, owner, children, queued, created,
                    present, values, expected, observed, completed, acknowledgements, releases, bucket>>

ReceiveReply(w) ==
    /\ pc[w] = "reply"
    /\ pc' = [pc EXCEPT ![w] = "publish"]
    /\ UNCHANGED <<originalPresent, assigned, offset, available, owner, children, queued, created,
                    present, values, expected, observed, result, completed, acknowledgements, releases, bucket>>

PublishEntry(w) ==
    /\ pc[w] = "publish"
    /\ LET key == assigned[w]
           leaf == Leaf(key)
       IN /\ values' = [k \in Keys |-> IF k = key THEN result[w]
                        ELSE IF Leaf(k) = leaf THEN bucket[w].entries[k] ELSE values[k]]
          /\ expected' = [expected EXCEPT ![key] = Advance(@)]
          /\ present' = {k \in present : Leaf(k) # leaf}
                        \cup {k \in bucket[w].members : Leaf(k) = leaf} \cup {key}
          /\ available' = [available EXCEPT ![leaf] = @ + 1]
          /\ owner' = [owner EXCEPT ![leaf] = Free]
    /\ completed' = completed \cup {w}
    /\ pc' = [pc EXCEPT ![w] = "done"]
    /\ UNCHANGED <<originalPresent, assigned, offset, children, queued, created, observed, result, acknowledgements, releases, bucket>>

Acknowledge(w) ==
    /\ w \notin acknowledgements
    /\ pc[w] \in {"publish", "done"}
    /\ acknowledgements' = acknowledgements \cup {w}
    /\ UNCHANGED <<originalPresent, assigned, pc, offset, available, owner, children, queued, created,
                    present, values, expected, observed, result, completed, releases, bucket>>

Next == (\E w \in Workers : Walk(w) \/ AcquireParent(w) \/ Recheck(w)
             \/ ReleaseParent(w) \/ OutsideCallback(w) \/ AcquireLeaf(w)
             \/ Callback(w) \/ ReceiveReply(w) \/ PublishEntry(w) \/ Acknowledge(w))
        \/ (\E node \in Nodes : PublishChild(node) \/ PublishParent(node))
Spec == Init /\ [][Next]_vars

UniqueNodeCreation == \A node \in Nodes : created[node] + queued[node] <= 1
ExclusiveLeafOwnership == \A node \in Nodes : available[node] + (IF owner[node] = Free THEN 0 ELSE 1) <= 1
NoLostUpdates == values = expected
NoLostPresence == present = originalPresent \cup {assigned[w] : w \in completed}
CompletedKeysRemainPresent == \A w \in completed : assigned[w] \in present
CallbackHoldsLeaf == \A w \in Workers : pc[w] \in {"callback", "reply", "publish"} => owner[Leaf(assigned[w])] = w
AcknowledgementRequiresReply == \A w \in acknowledgements : pc[w] \in {"publish", "done"}

TestPaths == [k \in Keys |-> IF Depth = 0 THEN <<>> ELSE IF k = "other" THEN <<0, 1>> ELSE <<0, 0>>]
====================================================================
