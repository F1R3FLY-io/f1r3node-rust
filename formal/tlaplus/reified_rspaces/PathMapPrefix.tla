---------------------------- MODULE PathMapPrefix ----------------------------
(***************************************************************************)
(* The delivery of a PathMap space (P5e: plan "PathMap delivery", decision *)
(* 9.3, F4, and decisions 37, 40, 45), with the merge check #2 of HEAD.     *)
(*                                                                         *)
(* In a PathMap space a channel is a path, and a send on the path p is a   *)
(* send on every prefix q of p. A datum d on p lives on D(p). Each prefix  *)
(* q of p holds one marker of d on I(q), and the waiters on q park on I(q).*)
(* The model runs the bag discipline (a scan shape) on non-persistent      *)
(* records:                                                                *)
(*   send p v      the datum meets the waiters of every prefix of p, the   *)
(*                 most specific first (PRODUCE-WAITERS). The first waiter *)
(*                 that fits the re-rooted datum and whose guard admits it *)
(*                 fires; a refusal moves on (decision 37). A peek waiter  *)
(*                 keeps the datum in flight (decision 30). With no fire   *)
(*                 the datum is stored with its markers.                   *)
(*   receive q     the waiter takes the first datum under q that fits its  *)
(*                 pattern (CONSUME-CANDIDATES, the path key first) and    *)
(*                 checks its guard once; a refusal parks it (decision     *)
(*                 37). The COMM on I(q) removes the marker of the datum   *)
(*                 on q. A delivery that does not peek retires the record  *)
(*                 and the other markers. A peek sends the datum again     *)
(*                 (decision 45): the second send meets the waiters of the *)
(*                 prefixes of its path; a fire spends the datum, and with *)
(*                 no fire the marker of the COMM is stored again.         *)
(* A pattern fits the re-rooted datum R(p\q, v): it names the length of    *)
(* the suffix (or any length) and the value (or any value).                *)
(*                                                                         *)
(* The model builds a base state with MaxOps operations, then forks two    *)
(* branches from it: branch A sends, and branch B receives. A merge check  *)
(* #2 of HEAD (merging_logic.rs) reports a conflict when one branch        *)
(* creates a produce and the other creates a consume on one channel.      *)
(*                                                                         *)
(* Invariants (TypeOK holds the types):                                    *)
(*   NoMatchRemains            no waiter without a guard fits a stored     *)
(*                             datum under its prefix (Rocq:               *)
(*                             no_match_remains)                           *)
(*   MarkerCount               the markers are exactly one marker for each *)
(*                             stored datum and each prefix of its path    *)
(*                             (Rocq: marker_invariant_preserved)          *)
(*   MergeDetectsPrefixContact a new datum on p in one branch and a new    *)
(*                             waiter on q, q a prefix of p, in the other  *)
(*                             share a channel that check #2 sees: the     *)
(*                             marker channel I(q) (Rocq:                  *)
(*                             prefix_contact_anchored)                    *)
(*                                                                         *)
(* Unsafe controls (each flag reverts one rule):                           *)
(*   SpaceWideIndex     the markers of every datum live on one space-wide  *)
(*                      index channel I(eps), as the node-local index of   *)
(*                      the legacy store kept them: refutes                *)
(*                      MergeDetectsPrefixContact                          *)
(*   ExactPathOnly      a send meets the waiters of its exact path only:   *)
(*                      refutes NoMatchRemains                             *)
(*   NoRetire           a delivery removes only the marker of its COMM:    *)
(*                      refutes MarkerCount                                *)
(*   MarkerNotRestored  the second send of a peek that meets no waiter     *)
(*                      does not store the marker of the COMM again:       *)
(*                      refutes MarkerCount                                *)
(*                                                                         *)
(* The token S(sid) and the orders of head-only disciplines are outside    *)
(* this model: CounterConflict.tla checks the token races, and             *)
(* PathMapPrefix.v proves heads_no_match and token_ord_reentry.            *)
(***************************************************************************)
EXTENDS Naturals, Sequences, FiniteSets, TLC

CONSTANTS
    Paths,              \* a prefix-closed set of paths
    Pats,               \* the patterns of the receives
    Vals,               \* the payload values
    MaxOps,             \* the operations of the base state
    Forks,              \* TRUE: fork two branches from the base state
    SpaceWideIndex,     \* Unsafe: every marker on I(eps)
    ExactPathOnly,      \* Unsafe: a send meets the waiters of its exact path only
    NoRetire,           \* Unsafe: a delivery removes only the marker of its COMM
    MarkerNotRestored   \* Unsafe: the second send does not restore the marker

ASSUME \A p \in Paths : \A k \in 0..Len(p) : SubSeq(p, 1, k) \in Paths
ASSUME "a" \in Vals
ASSUME Pats \subseteq [len : {"any", "one"}, val : {"a", "any"}]

(* The paths of the bounded configurations: the root, a path of one       *)
(* element with an extension, and a sibling; or the chain of the root, one *)
(* element and its extension.                                               *)
TwoLevelPaths == {<<>>, <<0>>, <<0, 1>>, <<1>>}
ChainPaths == {<<>>, <<0>>, <<0, 1>>}

(* The patterns: the length of the suffix ("any", or "one" for a suffix    *)
(* of one element) and the value ("a", or "any").                           *)
AllPats == [len : {"any", "one"}, val : {"a", "any"}]
ValuePats == [len : {"any"}, val : {"a", "any"}]
AnyPats == {[len |-> "any", val |-> "any"]}
Guards == {"none", "onlyA"}

Prefix(q, p) == Len(q) <= Len(p) /\ SubSeq(p, 1, Len(q)) = q
Suffix(q, p) == SubSeq(p, Len(q) + 1, Len(p))
PrefixesOf(p) == {SubSeq(p, 1, k) : k \in 0..Len(p)}

(* The order of the path keys (compare_keys): a proper prefix first, then *)
(* the elements in order.                                                   *)
RECURSIVE KeyLess(_, _)
KeyLess(a, b) ==
  IF Len(a) = 0 THEN Len(b) > 0
  ELSE IF Len(b) = 0 THEN FALSE
  ELSE IF Head(a) # Head(b) THEN Head(a) < Head(b)
  ELSE KeyLess(Tail(a), Tail(b))

Fits(pat, s, v) == (pat.len = "any" \/ Len(s) = 1) /\ (pat.val = "any" \/ pat.val = v)
Admits(g, v) == g = "none" \/ v = "a"

Datum == [id : Nat, path : Paths, val : Vals]
Waiter == [id : Nat, pre : Paths, pat : AllPats, guard : Guards, peek : BOOLEAN]
Marker == [pre : Paths, src : Nat, suf : Seq(Nat)]

MarkersOf(d) == {[pre |-> q, src |-> d.id, suf |-> Suffix(q, d.path)] : q \in PrefixesOf(d.path)}

FitsW(w, d) == Fits(w.pat, Suffix(w.pre, d.path), d.val)

(* The waiters that a send of d meets (PRODUCE-WAITERS). *)
SendSees(w, d) == IF ExactPathOnly THEN w.pre = d.path ELSE Prefix(w.pre, d.path)

(* The end of a send: a fresh datum is stored with its markers, and the     *)
(* second send of a peek stores the marker of its COMM again.               *)
Land(d, st, land) ==
  IF land.kind = "store"
  THEN [st EXCEPT !.data = @ \cup {d}, !.marks = @ \cup MarkersOf(d)]
  ELSE IF MarkerNotRestored THEN st
  ELSE [st EXCEPT !.marks = @ \cup {land.marker}]

(* A waiter that does not peek spends the datum: a fresh datum was never  *)
(* stored, and the second send retires the record and the other markers.  *)
Spend(d, st, land) ==
  IF land.kind = "store" THEN st
  ELSE [st EXCEPT !.data = @ \ {d}, !.marks = @ \ MarkersOf(d)]

(* OFFER: the most specific prefix first, then the least waiter id (the    *)
(* DR-75 order stands for any fixed order inside a prefix).                 *)
RECURSIVE Offer(_, _, _)
Offer(d, st, land) ==
  LET fire == {w \in st.wtrs : SendSees(w, d) /\ FitsW(w, d) /\ Admits(w.guard, d.val)}
  IN IF fire = {} THEN Land(d, st, land)
     ELSE LET deep == {w \in fire : \A x \in fire : Len(x.pre) <= Len(w.pre)}
              w == CHOOSE x \in deep : \A y \in deep : x.id <= y.id
              st1 == [st EXCEPT !.wtrs = @ \ {w}]
          IN IF w.peek THEN Offer(d, st1, land) ELSE Spend(d, st1, land)

(* RECEIVE: the first datum under the prefix that fits, the guard once.    *)
Receive(st, w) ==
  LET cands == {d \in st.data : Prefix(w.pre, d.path) /\ FitsW(w, d)}
  IN IF cands = {} THEN [st EXCEPT !.wtrs = @ \cup {w}]
     ELSE LET d == CHOOSE x \in cands :
                     \A y \in cands : x = y \/ KeyLess(x.path, y.path)
                                     \/ (x.path = y.path /\ x.id < y.id)
              m == [pre |-> w.pre, src |-> d.id, suf |-> Suffix(w.pre, d.path)]
              st1 == [st EXCEPT !.marks = @ \ {m}]
          IN IF ~Admits(w.guard, d.val) THEN [st EXCEPT !.wtrs = @ \cup {w}]
             ELSE IF w.peek THEN Offer(d, st1, [kind |-> "restore", marker |-> m])
             ELSE IF NoRetire THEN st1
             ELSE [st1 EXCEPT !.data = @ \ {d}, !.marks = @ \ MarkersOf(d)]

Send(st, p, v) == Offer([id |-> st.next, path |-> p, val |-> v], st, [kind |-> "store"])

Empty == [data |-> {}, marks |-> {}, wtrs |-> {}, next |-> 1]

Bump(st) == [st EXCEPT !.next = @ + 1]

VARIABLES phase, ops, base, brA, brB

vars == <<phase, ops, base, brA, brB>>

TypeOK ==
  /\ phase \in {"base", "done"}
  /\ ops \in 0..MaxOps
  /\ \A s \in {base, brA, brB} :
       /\ s.data \subseteq Datum
       /\ s.marks \subseteq Marker
       /\ s.wtrs \subseteq Waiter
       /\ s.next \in Nat

Init ==
  /\ phase = "base"
  /\ ops = 0
  /\ base = Empty
  /\ brA = Empty
  /\ brB = Empty

BaseSend ==
  /\ phase = "base" /\ ops < MaxOps
  /\ \E p \in Paths, v \in Vals : base' = Bump(Send(base, p, v))
  /\ ops' = ops + 1
  /\ UNCHANGED <<phase, brA, brB>>

BaseReceive ==
  /\ phase = "base" /\ ops < MaxOps
  /\ \E q \in Paths, pat \in Pats, g \in Guards, pk \in BOOLEAN :
       base' = Bump(Receive(base, [id |-> base.next, pre |-> q, pat |-> pat, guard |-> g, peek |-> pk]))
  /\ ops' = ops + 1
  /\ UNCHANGED <<phase, brA, brB>>

(* The fork: branch A sends and branch B receives from one base state. The *)
(* branches take distinct identities.                                       *)
Fork ==
  /\ Forks /\ phase = "base"
  /\ \E p \in Paths, v \in Vals : brA' = Send(base, p, v)
  /\ \E q \in Paths, pat \in Pats, g \in Guards, pk \in BOOLEAN :
       brB' = Receive(Bump(base), [id |-> base.next + 1, pre |-> q, pat |-> pat, guard |-> g, peek |-> pk])
  /\ phase' = "done"
  /\ UNCHANGED <<ops, base>>

Next == BaseSend \/ BaseReceive \/ Fork

Spec == Init /\ [][Next]_vars

States == {base, brA, brB}

NoMatchRemains ==
  \A s \in States : \A w \in s.wtrs, d \in s.data :
    (w.guard = "none" /\ Prefix(w.pre, d.path)) => ~FitsW(w, d)

MarkerCount ==
  \A s \in States : s.marks = UNION {MarkersOf(d) : d \in s.data}

DataChan(p) == <<"D", p>>
IndexChan(q) == <<"I", q>>
MarkChan(m) == IF SpaceWideIndex THEN IndexChan(<<>>) ELSE IndexChan(m.pre)

(* The channels of the produces that a branch created and did not destroy, *)
(* and of the consumes that it created and did not destroy.                *)
ProdChans(s) == {DataChan(d.path) : d \in s.data \ base.data}
                \cup {MarkChan(m) : m \in s.marks \ base.marks}
ConsChans(s) == {IndexChan(w.pre) : w \in s.wtrs \ base.wtrs}

Check2 == (ProdChans(brA) \cap ConsChans(brB)) \cup (ProdChans(brB) \cap ConsChans(brA)) # {}

Contact ==
  \/ \E d \in brA.data \ base.data, w \in brB.wtrs \ base.wtrs : Prefix(w.pre, d.path)
  \/ \E d \in brB.data \ base.data, w \in brA.wtrs \ base.wtrs : Prefix(w.pre, d.path)

MergeDetectsPrefixContact == phase = "done" => (Contact => Check2)
=============================================================================
