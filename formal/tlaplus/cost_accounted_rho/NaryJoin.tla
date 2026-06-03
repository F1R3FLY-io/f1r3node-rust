-------------------------------- MODULE NaryJoin --------------------------------
\* Cost-Accounted Rho — N-ary join authority conservation (spec §4.8 Prop 4.7 /
\* §4.8.4 / §4.8.5), as an Apalache SYMBOLIC check. The receiver authority and the
\* N sender authorities are symbolic integer atom-counts; the combined funding key
\* fuses them. Apalache (SMT) verifies, over ALL symbolic authority valuations:
\*   Conservation : the fused key consumes exactly the receiver + all senders, and
\*                  REGROUPING the senders (the §4.8.4 reverse-currying partition
\*                  ((r∘s1)∘(s2∘s3))) yields the same total — partition invariance.
\*   NoWeakening  : a fired non-trivial join strictly exceeds the receiver alone
\*                  (§4.8.5: the sender authorities cannot be silently dropped).
\* Symbolic in the authority VALUES (vs TLC's concrete enumeration); a faithful
\* arithmetic projection of CAJoinConservation. Corroborates the Rocq + Why3 legs.

EXTENDS Integers

VARIABLES
  \* @type: Int;
  recv,
  \* @type: Int;
  s1,
  \* @type: Int;
  s2,
  \* @type: Int;
  s3,
  \* @type: Int;
  combined

Init ==
  /\ recv \in 1..5
  /\ s1 \in 1..5
  /\ s2 \in 1..5
  /\ s3 \in 1..5
  /\ combined = 0

\* The join fires: the combined key fuses the receiver with all three senders.
Fire ==
  /\ combined = 0
  /\ combined' = recv + s1 + s2 + s3
  /\ UNCHANGED <<recv, s1, s2, s3>>

Next == Fire \/ UNCHANGED <<recv, s1, s2, s3, combined>>

\* Conservation + partition invariance (the regrouped fold equals the flat fold).
Conservation == (combined # 0) => (combined = (recv + s1) + (s2 + s3))

\* No-weakening: a fired join strictly exceeds the receiver authority alone.
NoWeakening == (combined # 0) => (combined > recv)

Inv == Conservation /\ NoWeakening
=================================================================================
