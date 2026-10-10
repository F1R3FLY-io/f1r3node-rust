------------------------------ MODULE AgentSpaces ------------------------------
(***************************************************************************)
(* The spaces that user agents implement (P8, design v2 W9).               *)
(*                                                                         *)
(* Routing. A channel has one of the classes of route.rs: a system         *)
(* channel (sys), a default object id (obj), an unforgeable name (u), a    *)
(* name of the native space N (nN), an agent-minted name of the user agent *)
(* A1 or A2 (t1, t2), or a forged channel (f). A use-context is none, the  *)
(* native space N, or one of the agents. Route gives the space that runs   *)
(* an operation on a channel under a context (R1 to R5): the default space *)
(* D, N, A1 or A2. A receive whose binds route to two spaces aborts with   *)
(* CrossSpaceJoin (TR-1). A use block or an annotation of new names a      *)
(* space by a reference: the handle H of N or the agent A1, bare or under  *)
(* one bundle. A bundle without the write flag aborts with                 *)
(* HandleNotWritable (AG-4).                                               *)
(*                                                                         *)
(* The agent. The operations that route to A1 become calls on it (TS-1,    *)
(* TR-7): produce(raw, v) for a send, and consume(pairs, arrow, body) for  *)
(* a receive, with one pair (pattern, raw) for each bind. The agent is the *)
(* reference agent of examples/reified_rspaces/agent_hashmap_bag.rho. Its  *)
(* state holds the stored messages and the stored consumes in insertion    *)
(* order. produce stores the message and offers it to the stored consumes  *)
(* in order (offer), and a peek keeps it for the next consume. consume     *)
(* fires at once on the stored messages or stores the consume (drain), and *)
(* a persistent consume fires again until no stored messages satisfy it.   *)
(* find takes the first choice, in insertion order, of distinct stored     *)
(* messages that the matchers accept. Each fire sends to the body          *)
(* capability of its consume. The node runs a body receive that is linear *)
(* for "<-" and "<<-" and persistent for "<=" (TR-6), so a second send to  *)
(* a linear body parks. A misbehaving agent can send to a body again       *)
(* (Resend).                                                               *)
(*                                                                         *)
(* Replay. Each call and each send of the agent is an ordinary event of    *)
(* the default space, so replay runs the calls of the log again, in their  *)
(* order, from the initial state of the agent.                             *)
(*                                                                         *)
(* Invariants:                                                             *)
(*   RoutingIsolation      a name of a space routes to that space under    *)
(*                         every context, and no call of the agent carries *)
(*                         a name of another space                         *)
(*   SystemChannelsDefault system channels, default objects and           *)
(*                         unforgeable names route to the default space    *)
(*                         under every context, so no call of the agent    *)
(*                         carries one                                     *)
(*   JoinSameSpace         a receive aborts with CrossSpaceJoin exactly     *)
(*                         when its binds route to two spaces, and a join  *)
(*                         on one space goes to that space                 *)
(*   HandleUnwrapExact     a reference names a space exactly when it is    *)
(*                         bare or under one bundle with the write flag,   *)
(*                         and the space is the unwrapped reference        *)
(*   ReplayEquivalence     the replay of the call log of the agent gives    *)
(*                         its state, its sends and the runs of each body  *)
(*   BodyAtMostOnceLinear  the body of a linear receive runs at most once, *)
(*                         also when the agent sends to it again           *)
(*   NoMatchRemains        no stored consume of the agent has matchers     *)
(*                         that accept distinct stored messages (ASO-1)    *)
(*                                                                         *)
(* Unsafe controls (each refutes exactly one invariant):                   *)
(*   SystemRoutesDefault = FALSE  SystemRouteUnsafe: a system channel     *)
(*                         routes as a forged channel does, to the space   *)
(*                         of the context -> SystemChannelsDefault         *)
(*   AgentTriesStored = FALSE     FaultyAgentUnsafe: consume stores the    *)
(*                         receive without a try on the stored messages    *)
(*                         -> NoMatchRemains, while ReplayEquivalence and  *)
(*                         the other invariants hold                       *)
(*                                                                         *)
(* Code (paths under rholang/src/rust/interpreter):                        *)
(*   Route            reified/route.rs:126-160 (classify_channel),         *)
(*                    173-227 (route)                                      *)
(*   JoinOutcome      reified/reducer_hooks.rs:1406-1446 (join_route),     *)
(*                    1452-1471 (one_agent)                                *)
(*   Classify         reified/scope.rs:141-166 (classify_space_ref),       *)
(*                    170-181 (call_channel)                                *)
(*   the calls        reified/user_agent.rs:1088-1206 (translate_receive:  *)
(*                    the pairs, the arrow, and the body receive, which is *)
(*                    persistent only for "<="),                           *)
(*                    reified/reducer_hooks.rs:797-891 (agent_consume)     *)
(*   Offer, Drain,    examples/reified_rspaces/agent_hashmap_bag.rho, the  *)
(*   Find, Apply      contracts offer, drain and find and the methods      *)
(*                    produce and consume of the agent agentHashmapBag     *)
(*                                                                         *)
(* Abstractions:                                                           *)
(*   - One native space N and two agents. The dynamic part runs the        *)
(*     operations that route to A1. A2 is symmetric, and the operations of *)
(*     D and N belong to other models.                                     *)
(*   - A message is one value of {1, 2}. A matcher is its pattern, which   *)
(*     accepts every value ("any") or only 1 ("one"), guard included.      *)
(*   - The raw channel of t1 is r1. The raw channel of a forged channel    *)
(*     under the context A1 is the channel itself.                         *)
(*   - One step runs one operation and the whole method of the agent that  *)
(*     it calls. The methods of one instance run one at a time, because    *)
(*     its state is one cell.                                              *)
(***************************************************************************)
EXTENDS Integers, Sequences, FiniteSets

CONSTANTS
    \* TRUE: R1 routes a system channel to the default space. FALSE: the
    \* Unsafe route to the space of the context.
    SystemRoutesDefault,
    \* TRUE: consume tries the stored messages (the reference agent). FALSE:
    \* the faulty agent stores every consume.
    AgentTriesStored,
    \* The number of steps of one run.
    MaxSteps

ASSUME SystemRoutesDefault \in BOOLEAN
ASSUME AgentTriesStored \in BOOLEAN
ASSUME MaxSteps \in 1..4

Agents == {"A1", "A2"}
Spaces == {"D", "N"} \cup Agents
Channels == {"sys", "obj", "u", "nN", "t1", "t2", "f"}
Ctxs == {"none", "N"} \cup Agents
Values == {1, 2}
Patterns == {"any", "one"}
Arrows == {"<-", "<<-", "<="}
Raws == {"r1", "r2", "sys", "obj", "u", "nN", "f"}
Refs == {"H", "W(H)", "R(H)", "A1", "W(A1)", "R(A1)"}
BodyIds == 1..MaxSteps

-----------------------------------------------------------------------------
(* Routing, joins and references                                           *)

\* The space of a use-context.
CtxSpace(ctx) == IF ctx = "none" THEN "D" ELSE ctx

\* route (R1 to R5): system channels, default objects and unforgeable names
\* to D (R1), the names of N to N (R3), agent-minted names to their agent
\* (R4), and forged channels to the space of the context (R5).
Route(ctx, ch) ==
    CASE ch = "sys" -> IF SystemRoutesDefault THEN "D" ELSE CtxSpace(ctx)
      [] ch \in {"obj", "u"} -> "D"
      [] ch = "nN" -> "N"
      [] ch = "t1" -> "A1"
      [] ch = "t2" -> "A2"
      [] ch = "f" -> CtxSpace(ctx)

\* The raw channel that an agent sees: the raw part of an agent-minted name,
\* and the channel itself otherwise.
Raw(ch) == CASE ch = "t1" -> "r1" [] ch = "t2" -> "r2" [] OTHER -> ch

\* join_route: a receive whose binds all route to D, to N or to one agent
\* runs on that space, and every other receive aborts.
JoinOutcome(ctx, chs) ==
    LET routes == {Route(ctx, chs[i]) : i \in DOMAIN chs}
    IN IF \E s \in Spaces : routes = {s}
       THEN CHOOSE s \in Spaces : routes = {s}
       ELSE "CrossSpaceJoin"

\* classify_space_ref with the write rule of AG-4: one bundle is removed,
\* and it needs the write flag.
Classify(ref) ==
    CASE ref \in {"H", "W(H)"} -> "N"
      [] ref \in {"A1", "W(A1)"} -> "A1"
      [] ref \in {"R(H)", "R(A1)"} -> "HandleNotWritable"

\* The reference without its bundle, or "none" for a bare reference.
Bundle(ref) ==
    CASE ref \in {"W(H)", "W(A1)"} -> "write"
      [] ref \in {"R(H)", "R(A1)"} -> "read"
      [] OTHER -> "none"

Unwrapped(ref) == IF ref \in {"H", "W(H)", "R(H)"} THEN "H" ELSE "A1"

-----------------------------------------------------------------------------
(* The reference agent                                                     *)

NoPairs == << >>
NoConsume == [pairs |-> NoPairs, arrow |-> "<-", body |-> 0]

\* A matcher accepts a value when its pattern does.
Accepts(p, v) == p = "any" \/ v = 1

\* The stored messages without the positions in used, in order.
RECURSIVE KeepFrom(_, _, _)
KeepFrom(msgs, used, j) ==
    IF j > Len(msgs) THEN << >>
    ELSE (IF j \in used THEN << >> ELSE << msgs[j] >>) \o KeepFrom(msgs, used, j + 1)

Without(msgs, used) == KeepFrom(msgs, used, 1)

\* The sequence s without its element at position k.
Remove(s, k) == SubSeq(s, 1, k - 1) \o SubSeq(s, k + 1, Len(s))

\* find: the first choice, in insertion order, of distinct stored messages
\* for the binds of pairs from bind i on. Bind i tries the stored messages
\* from position j on.
RECURSIVE FindFrom(_, _, _, _), TryFrom(_, _, _, _, _)
FindFrom(pairs, msgs, i, used) ==
    IF i > Len(pairs) THEN [ok |-> TRUE, used |-> used]
    ELSE TryFrom(pairs, msgs, i, 1, used)
TryFrom(pairs, msgs, i, j, used) ==
    IF j > Len(msgs) THEN [ok |-> FALSE, used |-> {}]
    ELSE IF j \notin used /\ msgs[j][1] = pairs[i][2] /\ Accepts(pairs[i][1], msgs[j][2])
         THEN LET deeper == FindFrom(pairs, msgs, i + 1, used \cup {j})
              IN IF deeper.ok THEN deeper ELSE TryFrom(pairs, msgs, i, j + 1, used)
         ELSE TryFrom(pairs, msgs, i, j + 1, used)

Find(pairs, msgs) == FindFrom(pairs, msgs, 1, {})

\* offer: from position k on, fire the first stored consume that the stored
\* messages satisfy. A peek keeps the message and the next consumes get it.
RECURSIVE Offer(_, _)
Offer(st, k) ==
    IF k > Len(st.cons) THEN st
    ELSE LET c == st.cons[k]
             r == Find(c.pairs, st.msgs)
         IN IF ~r.ok THEN Offer(st, k + 1)
            ELSE LET fired == [st EXCEPT !.sends = Append(@, c.body)]
                 IN CASE c.arrow = "<<-" -> Offer([fired EXCEPT !.cons = Remove(@, k)], k)
                      [] c.arrow = "<-" ->
                            [fired EXCEPT !.msgs = Without(@, r.used), !.cons = Remove(@, k)]
                      [] OTHER -> [fired EXCEPT !.msgs = Without(@, r.used)]

\* drain: fire a new consume on the stored messages, or store it. A
\* persistent consume fires again until the stored messages no longer
\* satisfy it.
RECURSIVE Drain(_, _)
Drain(st, c) ==
    LET r == Find(c.pairs, st.msgs)
    IN IF ~r.ok THEN [st EXCEPT !.cons = Append(@, c)]
       ELSE LET fired == [st EXCEPT !.sends = Append(@, c.body)]
            IN CASE c.arrow = "<<-" -> fired
                 [] c.arrow = "<-" -> [fired EXCEPT !.msgs = Without(@, r.used)]
                 [] OTHER -> Drain([fired EXCEPT !.msgs = Without(@, r.used)], c)

\* One call of the agent: a method, or a send of a misbehaving agent.
Apply(st, call) ==
    CASE call.m = "produce" -> Offer([st EXCEPT !.msgs = Append(@, <<call.raw, call.v>>)], 1)
      [] call.m = "consume" ->
            IF AgentTriesStored THEN Drain(st, call.c) ELSE [st EXCEPT !.cons = Append(@, call.c)]
      [] call.m = "resend" -> [st EXCEPT !.sends = Append(@, call.b)]

InitAgent == [msgs |-> << >>, cons |-> << >>, sends |-> << >>]

NoBody == [linear |-> FALSE, open |-> FALSE, runs |-> 0]

\* The node delivers each send to its body: an open body receive runs the
\* body, and a linear one closes; a send to a closed body parks.
RECURSIVE DeliverFrom(_, _, _)
DeliverFrom(bs, sends, i) ==
    IF i > Len(sends) THEN bs
    ELSE LET b == sends[i]
         IN DeliverFrom(IF bs[b].open
                        THEN [bs EXCEPT ![b] = [linear |-> bs[b].linear,
                                                open |-> ~bs[b].linear,
                                                runs |-> bs[b].runs + 1]]
                        ELSE bs,
                        sends, i + 1)

\* The sends of a step: the sends of the agent only grow.
NewSends(before, after) == SubSeq(after, Len(before) + 1, Len(after))

-----------------------------------------------------------------------------
(* The system                                                              *)

VARIABLES
    \* The calls of the agent A1, in order.
    calls,
    \* The state of A1: stored messages, stored consumes and sends.
    agent,
    \* The body receives of the translated receives.
    bodies,
    \* The number of translated receives so far.
    nbody,
    \* The number of steps so far.
    steps

vars == <<calls, agent, bodies, nbody, steps>>

Init ==
    /\ calls = << >>
    /\ agent = InitAgent
    /\ bodies = [b \in BodyIds |-> NoBody]
    /\ nbody = 0
    /\ steps = 0

\* A send on ch under ctx that routes to A1: the call produce(raw, v).
Send(ctx, ch, v) ==
    /\ steps < MaxSteps
    /\ Route(ctx, ch) = "A1"
    /\ LET call == [m |-> "produce", raw |-> Raw(ch), v |-> v, c |-> NoConsume, b |-> 0]
           st == Apply(agent, call)
       IN /\ calls' = Append(calls, call)
          /\ agent' = st
          /\ bodies' = DeliverFrom(bodies, NewSends(agent.sends, st.sends), 1)
    /\ steps' = steps + 1
    /\ UNCHANGED nbody

\* A receive with the binds bs, each <<channel, pattern>>, and the arrow,
\* under ctx, whose binds all route to A1: a new body receive and the call
\* consume(pairs, arrow, body).
Receive(ctx, bs, arrow) ==
    /\ steps < MaxSteps
    /\ JoinOutcome(ctx, [i \in DOMAIN bs |-> bs[i][1]]) = "A1"
    /\ LET b == nbody + 1
           c == [pairs |-> [i \in DOMAIN bs |-> <<bs[i][2], Raw(bs[i][1])>>], arrow |-> arrow,
                 body |-> b]
           call == [m |-> "consume", raw |-> "r1", v |-> 0, c |-> c, b |-> b]
           st == Apply(agent, call)
           opened == [bodies EXCEPT ![b] = [linear |-> arrow /= "<=", open |-> TRUE, runs |-> 0]]
       IN /\ nbody' = b
          /\ calls' = Append(calls, call)
          /\ agent' = st
          /\ bodies' = DeliverFrom(opened, NewSends(agent.sends, st.sends), 1)
    /\ steps' = steps + 1

\* A misbehaving agent sends to the body capability b again.
Resend(b) ==
    /\ steps < MaxSteps
    /\ b \in 1..nbody
    /\ LET call == [m |-> "resend", raw |-> "r1", v |-> 0, c |-> NoConsume, b |-> b]
           st == Apply(agent, call)
       IN /\ calls' = Append(calls, call)
          /\ agent' = st
          /\ bodies' = DeliverFrom(bodies, NewSends(agent.sends, st.sends), 1)
    /\ steps' = steps + 1
    /\ UNCHANGED nbody

Binds == {<<x>> : x \in Channels \X Patterns} \cup {<<x, y>> : x, y \in Channels \X Patterns}

Next ==
    \/ \E ctx \in Ctxs, ch \in Channels, v \in Values : Send(ctx, ch, v)
    \/ \E ctx \in Ctxs, bs \in Binds, arrow \in Arrows : Receive(ctx, bs, arrow)
    \/ \E b \in BodyIds : Resend(b)

Spec == Init /\ [][Next]_vars

-----------------------------------------------------------------------------
(* Replay                                                                  *)

\* The state of the agent after the calls of log from position i on.
RECURSIVE ReplayFrom(_, _, _)
ReplayFrom(log, st, i) ==
    IF i > Len(log) THEN st ELSE ReplayFrom(log, Apply(st, log[i]), i + 1)

\* The body receives that the consume calls of log open.
OpenedBodies(log) ==
    [b \in BodyIds |->
        IF \E i \in DOMAIN log : log[i].m = "consume" /\ log[i].c.body = b
        THEN LET i == CHOOSE i \in DOMAIN log : log[i].m = "consume" /\ log[i].c.body = b
             IN [linear |-> log[i].c.arrow /= "<=", open |-> TRUE, runs |-> 0]
        ELSE NoBody]

-----------------------------------------------------------------------------
(* Invariants                                                              *)

PairSeqs == UNION {[1..n -> Patterns \X Raws] : n \in 0..2}
ConsumeRecs == [pairs : PairSeqs, arrow : Arrows, body : 0..MaxSteps]
CallRecs == [m : {"produce", "consume", "resend"}, raw : Raws, v : 0..2,
             c : ConsumeRecs, b : 0..MaxSteps]

TypeOK ==
    /\ calls \in Seq(CallRecs)
    /\ Len(calls) <= MaxSteps
    /\ agent.msgs \in Seq(Raws \X Values)
    /\ agent.cons \in Seq(ConsumeRecs)
    /\ agent.sends \in Seq(BodyIds)
    /\ bodies \in [BodyIds -> [linear : BOOLEAN, open : BOOLEAN, runs : 0..MaxSteps]]
    /\ nbody \in 0..MaxSteps
    /\ steps \in 0..MaxSteps

RoutingIsolation ==
    /\ \A ctx \in Ctxs :
         /\ Route(ctx, "nN") = "N"
         /\ Route(ctx, "t1") = "A1"
         /\ Route(ctx, "t2") = "A2"
    /\ \A i \in DOMAIN calls :
         /\ calls[i].m = "produce" => calls[i].raw \notin {"r2", "nN"}
         /\ \A k \in DOMAIN calls[i].c.pairs : calls[i].c.pairs[k][2] \notin {"r2", "nN"}

SystemChannelsDefault ==
    /\ \A ctx \in Ctxs, ch \in {"sys", "obj", "u"} : Route(ctx, ch) = "D"
    /\ \A i \in DOMAIN calls :
         /\ calls[i].m = "produce" => calls[i].raw \notin {"sys", "obj", "u"}
         /\ \A k \in DOMAIN calls[i].c.pairs : calls[i].c.pairs[k][2] \notin {"sys", "obj", "u"}

JoinSameSpace ==
    \A ctx \in Ctxs, c1, c2 \in Channels :
        /\ (JoinOutcome(ctx, <<c1, c2>>) = "CrossSpaceJoin") <=> (Route(ctx, c1) /= Route(ctx, c2))
        /\ Route(ctx, c1) = Route(ctx, c2) => JoinOutcome(ctx, <<c1, c2>>) = Route(ctx, c1)

HandleUnwrapExact ==
    \A ref \in Refs :
        /\ (Classify(ref) = "HandleNotWritable") <=> (Bundle(ref) = "read")
        /\ Bundle(ref) /= "read" =>
             Classify(ref) = (IF Unwrapped(ref) = "H" THEN "N" ELSE Unwrapped(ref))

ReplayEquivalence ==
    LET st == ReplayFrom(calls, InitAgent, 1)
    IN /\ st = agent
       /\ DeliverFrom(OpenedBodies(calls), st.sends, 1) = bodies

BodyAtMostOnceLinear == \A b \in 1..nbody : bodies[b].linear => bodies[b].runs <= 1

NoMatchRemains == \A k \in DOMAIN agent.cons : ~Find(agent.cons[k].pairs, agent.msgs).ok

=============================================================================
