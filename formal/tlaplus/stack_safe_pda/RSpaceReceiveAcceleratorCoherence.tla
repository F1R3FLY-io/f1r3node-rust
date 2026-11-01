----------------- MODULE RSpaceReceiveAcceleratorCoherence -----------------
EXTENDS FiniteSets, Naturals, Sequences

(***************************************************************************
Bounded lifecycle model of the derived receive-occurrence accelerator of
task 8010 (design section 2.6; invariants RAC-TLA-01 to RAC-TLA-11; the
negative controls RAC-TMUT-01 to RAC-TMUT-10 are in
RSpaceReceiveAcceleratorCoherenceMutants.tla).

Semantic state.  The receive relation is a pure function of the semantic
continuation root (ContinuationMap, variable `catalog`: the resting records
of every group) and of the versioned installed input (variable `installed`:
at most one installed record per group).  The resting JoinBag (`joins`) is
kept beside the catalog.  DataBag is not modelled, because the derivation
does not read it (RAC-RC-07).  A record value stands for one continuation
identity together with the source bytes of its first insertion; an insert
of a known identity is therefore a no-op ("first source kept").

Derived relation.  A group key is an ordered tuple of channels.  For a
group g and a channel c, Positions(g, c) is the set of 1-based indices i
with Members(g)[i] = c.  The docs use 0-based positions; the mapping is
i |-> i - 1, so {1, 3} here is [0, 2] in the docs.  A target is a
(kind, group, record) triple, kind "inst" for the installed record and
"rest" for a resting one, so an installed record and a byte-identical
resting record are distinct targets.  The occurrence relation Occ(c)
contains (c, g, Positions(g, c), t) for every group g that contains c and
every target t of g.

Revisions and freshness.  A revision is the triple (gen, store, instv) of
tokens.  Tokens come from the append-only set `issued`; every action issues
at most one fresh token, the least token not yet issued, and uses it for
every revision component the action refreshes.  A held token is therefore
never reissued, and equality of a revision component with a held value
means that no action refreshed that component since the value was read.
Every group also carries an epoch `gepoch[g]` drawn from the same pool; it
is the per-group ticket of the accepted-match protocol (task 6013).  A
mutation of group g refreshes `gepoch[g]`; a reset refreshes every epoch.

Accelerator.  The published accelerator `acc` records the revision and the
lineage manifest it was built for, the set of groups it indexes completely
(`scope`), the occurrences of those groups (`entries`), and the revisions
of its three artifacts (catalog, occurrence, automaton).  A class is
computed against the current world, tested in this order:
  absent      nothing is published;
  building    nothing is published and a private build is in progress;
  foreign     the lineage manifest or the generation differs;
  stale       the store or the installed-version token differs;
  incomplete  the scope does not cover every group that has a target;
  coherent    otherwise.
A private build (`staging`) indexes hydrated (`loaded`) groups one at a
time and publishes only if no revision component and no lineage changed
since it began.  Publication does not require the scope to cover the
support: a build may publish the hydrated groups alone, and the result is
Incomplete until history loads or a rebuild cover the support.  This
over-approximates the builder of RAC-RC-23, which refuses an incomplete
scope; every other action is shared, so the states that builder reaches are
among the states checked here, and every invariant below holds for it too.
A current accelerator is maintained in the same action as every mutation;
maintenance applies a change only inside the scope and never widens it, so
a mutation that gives an out-of-scope group its first target leaves the
accelerator Incomplete (the Rocq maintained view widens the scope in that
case, RAC-RC-15 and RAC-RC-18).  A history load extends the scope of a
current accelerator without changing any revision.  A reset replaces the
semantic state, takes one fresh token as its new generation, store and
installed-version token, and adopts the persisted accelerator only when its
manifest equals the reset target.

Readers.  A reader holds a snapshot `snap` of the published accelerator.
READ answers from the snapshot only when the snapshot is coherent and
derives the relation otherwise; it never answers the empty set by default.
A candidate taken from a read records the epoch of its group, and a commit
succeeds only while that epoch is unchanged.

Observations.  `answer` records the last read (its channel, its source,
the class of the snapshot, the result and the derived relation at the time
of the read) and `lastFault` records the state before the last failed
update.  Answer sets `answer` and clears `lastFault`, FailUpdate sets
`lastFault` and clears `answer`, and every other action clears both, so
their invariants describe the state immediately after the observed action.

Bounds.  `steps` counts actions; every action requires steps < MaxSteps,
and the declared state constraint StateBound repeats the bound.  MaxToken
bounds the token space; since every action issues at most one token and
the initial state issues one, MaxSteps < MaxToken guarantees a fresh token
for every step.
***************************************************************************)

CONSTANTS Records, NoRecord, MaxToken, MaxSteps

ASSUME /\ Records # {}
       /\ NoRecord \notin Records
       /\ MaxToken \in Nat
       /\ MaxSteps \in Nat
       /\ MaxSteps < MaxToken

(***************************************************************************
Channels, groups and positions.
***************************************************************************)
Channels == {"a", "b"}
Groups == {"aba", "b"}

Members(g) == IF g = "aba" THEN <<"a", "b", "a">> ELSE <<"b">>

Arity(g) == Len(Members(g))

MaxArity == CHOOSE n \in {Arity(g) : g \in Groups} :
                \A g \in Groups : Arity(g) <= n

ChannelsOf(g) == {Members(g)[i] : i \in 1..Arity(g)}

Positions(g, c) == {i \in 1..Arity(g) : Members(g)[i] = c}

(***************************************************************************
Targets and the derived occurrence relation.
***************************************************************************)
Kinds == {"inst", "rest"}

TargetUniverse == [kind : Kinds, group : Groups, rec : Records]

RestTarget(g, r) == [kind |-> "rest", group |-> g, rec |-> r]
InstTarget(g, r) == [kind |-> "inst", group |-> g, rec |-> r]

OccUniverse ==
    [chan : Channels, group : Groups, pos : SUBSET (1..MaxArity),
     target : TargetUniverse]

TargetOcc(t) ==
    {[chan |-> c, group |-> t.group, pos |-> Positions(t.group, c),
      target |-> t] : c \in ChannelsOf(t.group)}

GroupTargets(cat, ins, g) ==
    {RestTarget(g, r) : r \in cat[g]}
    \union (IF ins[g] = NoRecord THEN {} ELSE {InstTarget(g, ins[g])})

Support(cat, ins) == {g \in Groups : GroupTargets(cat, ins, g) # {}}

GroupOcc(cat, ins, g) ==
    UNION {TargetOcc(t) : t \in GroupTargets(cat, ins, g)}

ScopedOcc(cat, ins, scope) == UNION {GroupOcc(cat, ins, g) : g \in scope}

DerivedOcc(cat, ins) == ScopedOcc(cat, ins, Groups)

(***************************************************************************
The resting JoinBag and the semantic identity.  The identity is computed
from the modelled semantic roots only (ContinuationMap and JoinBag); the
installed input is versioned runtime input, not a root.
***************************************************************************)
JoinsOf(cat) ==
    [c \in Channels |-> {g \in Groups : c \in ChannelsOf(g) /\ cat[g] # {}}]

AddJoins(j, g) ==
    [c \in Channels |-> IF c \in ChannelsOf(g) THEN j[c] \union {g} ELSE j[c]]

ClearJoins(j, g) == [c \in Channels |-> j[c] \ {g}]

SemanticIdentity(cat, j) == [catalog |-> cat, joins |-> j]

(***************************************************************************
Reset targets.  A target name stands for the ObjectRefV1 of its semantic
state manifest.  "seed" is the initial state: group "aba" holds one resting
record and an installed copy of the same bytes, and group "b" is empty.
"empty" holds nothing.  One accelerator is persisted, bound to the "seed"
manifest, covering every group with exactly the seed occurrences.
***************************************************************************)
ResetTargets == {"seed", "empty"}

SeedRecord == CHOOSE r \in Records : TRUE

TargetCatalog(t) ==
    [g \in Groups |-> IF t = "seed" /\ g = "aba" THEN {SeedRecord} ELSE {}]

TargetInstalled(t) ==
    [g \in Groups |-> IF t = "seed" /\ g = "aba" THEN SeedRecord ELSE NoRecord]

PersistedManifest == "seed"
PersistedScope == Groups
PersistedEntries ==
    DerivedOcc(TargetCatalog(PersistedManifest),
               TargetInstalled(PersistedManifest))

(***************************************************************************
Tokens and revisions.  Token 0 is never issued; it marks the revision of an
absent accelerator or an inactive build.
***************************************************************************)
NoToken == 0
Tokens == 1..MaxToken
TokenOrNone == 0..MaxToken
InitialToken == 1
Revisions == TokenOrNone \X TokenOrNone \X TokenOrNone
NoRev == <<NoToken, NoToken, NoToken>>

VARIABLES catalog, installed, joins, loaded,
          gen, store, instv, issued, gepoch, lineage,
          acc, staging, snap, answer, ticket, commits,
          lastFault, semid, steps

worldVars == <<catalog, installed, joins, loaded,
               gen, store, instv, issued, gepoch, lineage, semid>>

vars == <<catalog, installed, joins, loaded,
          gen, store, instv, issued, gepoch, lineage,
          acc, staging, snap, answer, ticket, commits,
          lastFault, semid, steps>>

FreshAvailable == Tokens \ issued # {}

FreshToken ==
    CHOOSE t \in Tokens \ issued : \A u \in Tokens \ issued : t <= u

CurrentRev == <<gen, store, instv>>

Occ(c) == {o \in DerivedOcc(catalog, installed) : o.chan = c}

(***************************************************************************
Accelerator values, classes and maintenance.
***************************************************************************)
AccKinds == {"absent", "building", "published"}
Lineages == ResetTargets \union {"none"}
Classes ==
    {"absent", "building", "foreign", "stale", "incomplete", "coherent"}

AccType ==
    [kind : AccKinds, gen : TokenOrNone, store : TokenOrNone,
     instv : TokenOrNone, lineage : Lineages, scope : SUBSET Groups,
     entries : SUBSET OccUniverse, catRev : Revisions,
     occRev : Revisions, autRev : Revisions]

AbsentAcc ==
    [kind |-> "absent", gen |-> NoToken, store |-> NoToken,
     instv |-> NoToken, lineage |-> "none", scope |-> {}, entries |-> {},
     catRev |-> NoRev, occRev |-> NoRev, autRev |-> NoRev]

BuildingAcc == [AbsentAcc EXCEPT !.kind = "building"]

Rev(a) == <<a.gen, a.store, a.instv>>

Class(a) ==
    IF a.kind = "absent" THEN "absent"
    ELSE IF a.kind = "building" THEN "building"
    ELSE IF a.lineage # lineage \/ a.gen # gen THEN "foreign"
    ELSE IF a.store # store \/ a.instv # instv THEN "stale"
    ELSE IF ~(Support(catalog, installed) \subseteq a.scope) THEN "incomplete"
    ELSE "coherent"

IsCurrent(a) ==
    /\ a.kind = "published"
    /\ a.lineage = lineage
    /\ Rev(a) = CurrentRev

\* The maintained update of a current accelerator for a mutation of group g:
\* the occurrences of the removed and added targets are applied when g is in
\* the scope, and all three artifacts move to the new revision together.
Maintain(a, g, removed, added, newStore, newInstv) ==
    IF IsCurrent(a)
    THEN [a EXCEPT
            !.store = newStore,
            !.instv = newInstv,
            !.entries = IF g \in a.scope
                        THEN (a.entries \ removed) \union added
                        ELSE a.entries,
            !.catRev = <<a.gen, newStore, newInstv>>,
            !.occRev = <<a.gen, newStore, newInstv>>,
            !.autRev = <<a.gen, newStore, newInstv>>]
    ELSE a

StagingType ==
    [active : BOOLEAN, gen : TokenOrNone, store : TokenOrNone,
     instv : TokenOrNone, lineage : Lineages, scope : SUBSET Groups,
     entries : SUBSET OccUniverse]

NoStaging ==
    [active |-> FALSE, gen |-> NoToken, store |-> NoToken,
     instv |-> NoToken, lineage |-> "none", scope |-> {}, entries |-> {}]

StagingIsCurrent ==
    /\ staging.lineage = lineage
    /\ <<staging.gen, staging.store, staging.instv>> = CurrentRev

PublishedFrom(s) ==
    [kind |-> "published", gen |-> s.gen, store |-> s.store,
     instv |-> s.instv, lineage |-> s.lineage, scope |-> s.scope,
     entries |-> s.entries,
     catRev |-> <<s.gen, s.store, s.instv>>,
     occRev |-> <<s.gen, s.store, s.instv>>,
     autRev |-> <<s.gen, s.store, s.instv>>]

Adopted(t) ==
    [kind |-> "published", gen |-> t, store |-> t, instv |-> t,
     lineage |-> PersistedManifest, scope |-> PersistedScope,
     entries |-> PersistedEntries,
     catRev |-> <<t, t, t>>, occRev |-> <<t, t, t>>,
     autRev |-> <<t, t, t>>]

(***************************************************************************
Reads, candidates and commits.
***************************************************************************)
ReadResult(a, c) ==
    IF Class(a) = "coherent" THEN {o \in a.entries : o.chan = c} ELSE Occ(c)

AnswerType ==
    [chan : Channels \union {"none"}, src : {"none", "acc", "derive"},
     cls : Classes \union {"none"}, result : SUBSET OccUniverse,
     expected : SUBSET OccUniverse]

NoAnswer ==
    [chan |-> "none", src |-> "none", cls |-> "none", result |-> {},
     expected |-> {}]

TicketType == [group : Groups, target : TargetUniverse, epoch : Tokens]

CommitType ==
    [group : Groups, target : TargetUniverse, ticket : Tokens,
     epoch : Tokens, present : BOOLEAN]

TargetPresent(t) ==
    IF t.kind = "rest"
    THEN t.rec \in catalog[t.group]
    ELSE installed[t.group] = t.rec

CommitRecord(k) ==
    [group |-> k.group, target |-> k.target, ticket |-> k.epoch,
     epoch |-> gepoch[k.group], present |-> TargetPresent(k.target)]

(***************************************************************************
The state a failed update must leave untouched.
***************************************************************************)
IdentityType ==
    [catalog : [Groups -> SUBSET Records],
     joins : [Channels -> SUBSET Groups]]

FaultViewType ==
    [catalog : [Groups -> SUBSET Records],
     installed : [Groups -> Records \union {NoRecord}],
     joins : [Channels -> SUBSET Groups], loaded : SUBSET Groups,
     gen : Tokens, store : Tokens, instv : Tokens,
     issued : SUBSET Tokens, gepoch : [Groups -> Tokens],
     lineage : ResetTargets, acc : AccType, staging : StagingType,
     snap : AccType, ticket : SUBSET TicketType,
     commits : SUBSET CommitType, semid : IdentityType]

FaultView ==
    [catalog |-> catalog, installed |-> installed, joins |-> joins,
     loaded |-> loaded, gen |-> gen, store |-> store, instv |-> instv,
     issued |-> issued, gepoch |-> gepoch, lineage |-> lineage,
     acc |-> acc, staging |-> staging, snap |-> snap, ticket |-> ticket,
     commits |-> commits, semid |-> semid]

(***************************************************************************
Initial state: the world is the "seed" checkpoint with nothing hydrated,
every revision component and every group epoch holds the initial token,
and no accelerator exists.
***************************************************************************)
Init ==
    /\ catalog = TargetCatalog("seed")
    /\ installed = TargetInstalled("seed")
    /\ joins = JoinsOf(TargetCatalog("seed"))
    /\ loaded = {}
    /\ gen = InitialToken
    /\ store = InitialToken
    /\ instv = InitialToken
    /\ issued = {InitialToken}
    /\ gepoch = [g \in Groups |-> InitialToken]
    /\ lineage = "seed"
    /\ acc = AbsentAcc
    /\ staging = NoStaging
    /\ snap = AbsentAcc
    /\ answer = NoAnswer
    /\ ticket = {}
    /\ commits = {}
    /\ lastFault = {}
    /\ semid = SemanticIdentity(TargetCatalog("seed"),
                                JoinsOf(TargetCatalog("seed")))
    /\ steps = 0

Tick ==
    /\ steps < MaxSteps
    /\ steps' = steps + 1

ClearObservations ==
    /\ answer' = NoAnswer
    /\ lastFault' = {}

(***************************************************************************
Every successful mutation of group g issues one fresh token, uses it as the
new store token (and as the new installed-version token when the installed
input changes), refreshes the epoch of g, and maintains a current
accelerator in the same step.  GroupStateChange leaves the semantic identity
to its caller so that a negative control can replace that one conjunct.
***************************************************************************)
GroupStateChange(g, newCatalog, newInstalled, newJoins, removed, added,
                 refreshInstalled, newLoaded) ==
    /\ FreshAvailable
    /\ LET t == FreshToken
           v == IF refreshInstalled THEN t ELSE instv
       IN  /\ catalog' = newCatalog
           /\ installed' = newInstalled
           /\ joins' = newJoins
           /\ loaded' = newLoaded
           /\ store' = t
           /\ instv' = v
           /\ issued' = issued \union {t}
           /\ gepoch' = [gepoch EXCEPT ![g] = t]
           /\ acc' = Maintain(acc, g, removed, added, t, v)
    /\ UNCHANGED <<gen, lineage>>

GroupMutation(g, newCatalog, newInstalled, newJoins, removed, added,
              refreshInstalled, newLoaded) ==
    /\ GroupStateChange(g, newCatalog, newInstalled, newJoins, removed,
                        added, refreshInstalled, newLoaded)
    /\ semid' = SemanticIdentity(newCatalog, newJoins)

(***************************************************************************
Lifecycle operations (design table 2.2).
***************************************************************************)
\* Insert of a new identity: add the record and its joins; fresh store token.
Insert(g, r) ==
    /\ Tick
    /\ r \notin catalog[g]
    /\ GroupMutation(g, [catalog EXCEPT ![g] = @ \union {r}], installed,
                     AddJoins(joins, g), {}, TargetOcc(RestTarget(g, r)),
                     FALSE, loaded \union {g})
    /\ ClearObservations
    /\ UNCHANGED <<staging, snap, ticket, commits>>

\* Insert of a known identity (byte-equal or alias source): nothing changes;
\* the first source is kept, no token is issued and no epoch moves.
DuplicateInsert(g, r) ==
    /\ Tick
    /\ r \in catalog[g]
    /\ ClearObservations
    /\ UNCHANGED <<worldVars, acc, staging, snap, ticket, commits>>

\* Removal of one of at least two resting records: fresh store token.
RemoveNonfinal(g, r) ==
    /\ Tick
    /\ r \in catalog[g]
    /\ Cardinality(catalog[g]) >= 2
    /\ GroupMutation(g, [catalog EXCEPT ![g] = @ \ {r}], installed, joins,
                     TargetOcc(RestTarget(g, r)), {}, FALSE,
                     loaded \union {g})
    /\ ClearObservations
    /\ UNCHANGED <<staging, snap, ticket, commits>>

\* Removal of the last resting record: fresh store token; the resting joins
\* of the group are cleared.
RemoveFinal(g, r) ==
    /\ Tick
    /\ catalog[g] = {r}
    /\ GroupMutation(g, [catalog EXCEPT ![g] = {}], installed,
                     ClearJoins(joins, g), TargetOcc(RestTarget(g, r)), {},
                     FALSE, loaded \union {g})
    /\ ClearObservations
    /\ UNCHANGED <<staging, snap, ticket, commits>>

\* Install or reinstall (also of byte-equal bytes): replace the installed
\* slot; fresh store and installed-version tokens.
Install(g, r) ==
    /\ Tick
    /\ GroupMutation(g, catalog, [installed EXCEPT ![g] = r], joins,
                     IF installed[g] = NoRecord
                     THEN {}
                     ELSE TargetOcc(InstTarget(g, installed[g])),
                     TargetOcc(InstTarget(g, r)), TRUE, loaded)
    /\ ClearObservations
    /\ UNCHANGED <<staging, snap, ticket, commits>>

\* Uninstall: clear the slot; fresh store and installed-version tokens.
Uninstall(g) ==
    /\ Tick
    /\ installed[g] # NoRecord
    /\ GroupMutation(g, catalog, [installed EXCEPT ![g] = NoRecord], joins,
                     TargetOcc(InstTarget(g, installed[g])), {}, TRUE,
                     loaded)
    /\ ClearObservations
    /\ UNCHANGED <<staging, snap, ticket, commits>>

\* History load hydrates group g into the hot view.  No semantic content and
\* no revision changes; a current accelerator extends its scope with g.
HistoryLoad(g) ==
    /\ Tick
    /\ g \notin loaded
    /\ loaded' = loaded \union {g}
    /\ acc' = IF IsCurrent(acc) /\ g \notin acc.scope
              THEN [acc EXCEPT !.scope = @ \union {g},
                               !.entries = @ \union
                                   GroupOcc(catalog, installed, g)]
              ELSE acc
    /\ ClearObservations
    /\ UNCHANGED <<catalog, installed, joins, gen, store, instv, issued,
                   gepoch, lineage, staging, snap, ticket, commits, semid>>

\* Reset, checkpoint, soft revert or clear to target t: replace the semantic
\* state and the installed input, take one fresh token for the generation,
\* the store and the installed version, refresh every group epoch, and
\* adopt the persisted accelerator exactly when `adopt` holds.  Without
\* adoption the previous accelerator stays published under the old
\* generation, where it is Foreign.
ResetTo(t, adopt) ==
    /\ Tick
    /\ FreshAvailable
    /\ LET tok == FreshToken
           newCatalog == TargetCatalog(t)
           newJoins == JoinsOf(newCatalog)
       IN  /\ catalog' = newCatalog
           /\ installed' = TargetInstalled(t)
           /\ joins' = newJoins
           /\ loaded' = {}
           /\ gen' = tok
           /\ store' = tok
           /\ instv' = tok
           /\ issued' = issued \union {tok}
           /\ gepoch' = [g \in Groups |-> tok]
           /\ lineage' = t
           /\ acc' = IF adopt THEN Adopted(tok) ELSE acc
           /\ semid' = SemanticIdentity(newCatalog, newJoins)
    /\ ClearObservations
    /\ UNCHANGED <<staging, snap, ticket, commits>>

Reset(t) == ResetTo(t, PersistedManifest = t)

(***************************************************************************
Private builds.  A build records the revision and lineage it started
from, indexes hydrated groups one at a time, and publishes all three
artifacts at its base revision only if that revision and the lineage are
still current.  Abort models both an abort and a crash of the builder; the
published accelerator is untouched.
***************************************************************************)
BeginBuild ==
    /\ Tick
    /\ ~staging.active
    /\ staging' = [active |-> TRUE, gen |-> gen, store |-> store,
                   instv |-> instv, lineage |-> lineage, scope |-> {},
                   entries |-> {}]
    /\ acc' = IF acc.kind = "absent" THEN BuildingAcc ELSE acc
    /\ ClearObservations
    /\ UNCHANGED <<worldVars, snap, ticket, commits>>

BuildGroup(g) ==
    /\ Tick
    /\ staging.active
    /\ g \in loaded \ staging.scope
    /\ staging' = [staging EXCEPT !.scope = @ \union {g},
                                  !.entries = @ \union
                                      GroupOcc(catalog, installed, g)]
    /\ ClearObservations
    /\ UNCHANGED <<worldVars, acc, snap, ticket, commits>>

PublishBuild ==
    /\ Tick
    /\ staging.active
    /\ StagingIsCurrent
    /\ acc' = PublishedFrom(staging)
    /\ staging' = NoStaging
    /\ ClearObservations
    /\ UNCHANGED <<worldVars, snap, ticket, commits>>

AbortBuild ==
    /\ Tick
    /\ staging.active
    /\ staging' = NoStaging
    /\ acc' = IF acc.kind = "building" THEN AbsentAcc ELSE acc
    /\ ClearObservations
    /\ UNCHANGED <<worldVars, snap, ticket, commits>>

\* A failed update (allocation failure, aborted copy-on-write replacement)
\* changes nothing and records the state it left untouched.
FailUpdate ==
    /\ Tick
    /\ lastFault' = {FaultView}
    /\ answer' = NoAnswer
    /\ UNCHANGED <<worldVars, acc, staging, snap, ticket, commits>>

(***************************************************************************
Readers.
***************************************************************************)
AcquireSnapshot ==
    /\ Tick
    /\ snap # acc
    /\ snap' = acc
    /\ ClearObservations
    /\ UNCHANGED <<worldVars, acc, staging, ticket, commits>>

\* READ: answer from the held snapshot only when it is coherent; otherwise
\* derive the relation from the semantic state.
Answer(c) ==
    /\ Tick
    /\ LET k == Class(snap)
           expected == Occ(c)
       IN  answer' = [chan |-> c,
                      src |-> IF k = "coherent" THEN "acc" ELSE "derive",
                      cls |-> k,
                      result |-> IF k = "coherent"
                                 THEN {o \in snap.entries : o.chan = c}
                                 ELSE expected,
                      expected |-> expected]
    /\ lastFault' = {}
    /\ UNCHANGED <<worldVars, acc, staging, snap, ticket, commits>>

\* Take one candidate target from a read of channel c, recording the epoch
\* of its group as the ticket.
TakeCandidate(c) ==
    /\ Tick
    /\ ticket = {}
    /\ \E o \in ReadResult(snap, c) :
         ticket' = {[group |-> o.group, target |-> o.target,
                     epoch |-> gepoch[o.group]]}
    /\ ClearObservations
    /\ UNCHANGED <<worldVars, acc, staging, snap, commits>>

\* Consuming a resting target removes it (non-persistent consumption); the
\* resting joins are cleared when it was the last resting record.
ConsumeResting(g, r) ==
    GroupMutation(g, [catalog EXCEPT ![g] = @ \ {r}], installed,
                  IF catalog[g] = {r} THEN ClearJoins(joins, g) ELSE joins,
                  TargetOcc(RestTarget(g, r)), {}, FALSE, loaded \union {g})

\* A stale ticket is rejected and changes nothing.  An accepted installed
\* target persists, so no root and no revision changes.  An accepted resting
\* target is consumed.
CommitTicket(k) ==
    \/ /\ k.epoch # gepoch[k.group]
       /\ ticket' = {}
       /\ ClearObservations
       /\ UNCHANGED <<worldVars, acc, staging, snap, commits>>
    \/ /\ k.epoch = gepoch[k.group]
       /\ k.target.kind = "inst"
       /\ commits' = commits \union {CommitRecord(k)}
       /\ ticket' = {}
       /\ ClearObservations
       /\ UNCHANGED <<worldVars, acc, staging, snap>>
    \/ /\ k.epoch = gepoch[k.group]
       /\ k.target.kind = "rest"
       /\ ConsumeResting(k.group, k.target.rec)
       /\ commits' = commits \union {CommitRecord(k)}
       /\ ticket' = {}
       /\ ClearObservations
       /\ UNCHANGED <<staging, snap>>

Commit ==
    /\ Tick
    /\ \E k \in ticket : CommitTicket(k)

Next ==
    \/ \E g \in Groups, r \in Records : Insert(g, r)
    \/ \E g \in Groups, r \in Records : DuplicateInsert(g, r)
    \/ \E g \in Groups, r \in Records : RemoveNonfinal(g, r)
    \/ \E g \in Groups, r \in Records : RemoveFinal(g, r)
    \/ \E g \in Groups, r \in Records : Install(g, r)
    \/ \E g \in Groups : Uninstall(g)
    \/ \E g \in Groups : HistoryLoad(g)
    \/ \E t \in ResetTargets : Reset(t)
    \/ BeginBuild
    \/ \E g \in Groups : BuildGroup(g)
    \/ PublishBuild
    \/ AbortBuild
    \/ FailUpdate
    \/ AcquireSnapshot
    \/ \E c \in Channels : Answer(c)
    \/ \E c \in Channels : TakeCandidate(c)
    \/ Commit

Spec == Init /\ [][Next]_vars

StateBound == steps <= MaxSteps

(***************************************************************************
Invariants RAC-TLA-01 to RAC-TLA-11.
***************************************************************************)
\* RAC-TLA-01
TypeInvariant ==
    /\ catalog \in [Groups -> SUBSET Records]
    /\ installed \in [Groups -> Records \union {NoRecord}]
    /\ joins \in [Channels -> SUBSET Groups]
    /\ loaded \subseteq Groups
    /\ gen \in Tokens
    /\ store \in Tokens
    /\ instv \in Tokens
    /\ issued \subseteq Tokens
    /\ {gen, store, instv} \subseteq issued
    /\ gepoch \in [Groups -> issued]
    /\ lineage \in ResetTargets
    /\ acc \in AccType
    /\ staging \in StagingType
    /\ snap \in AccType
    /\ answer \in AnswerType
    /\ ticket \subseteq TicketType
    /\ Cardinality(ticket) <= 1
    /\ commits \subseteq CommitType
    /\ lastFault \subseteq FaultViewType
    /\ Cardinality(lastFault) <= 1
    /\ semid \in IdentityType
    /\ steps \in 0..MaxSteps

\* RAC-TLA-02: the catalog, occurrence and automaton artifacts of a published
\* accelerator (or of a held snapshot of one) carry its one revision.
PublishedArtifactsShareOneRevision ==
    \A a \in {acc, snap} :
      a.kind = "published" =>
        /\ a.catRev = Rev(a)
        /\ a.occRev = Rev(a)
        /\ a.autRev = Rev(a)

\* RAC-TLA-03: a coherent accelerator holds exactly the derived relation.
CoherentAcceleratorEqualsDerivation ==
    \A a \in {acc, snap} :
      Class(a) = "coherent" => a.entries = DerivedOcc(catalog, installed)

\* RAC-TLA-04: every answered occurrence is derived at the time of the read.
AnswerIsSound == answer.result \subseteq answer.expected

\* RAC-TLA-05: every derived occurrence of the channel is answered.
NoEnabledReceiveHidden == answer.expected \subseteq answer.result

\* RAC-TLA-06
StaleAcceleratorNeverAnswers ==
    answer.src = "acc" => answer.cls # "stale"

\* RAC-TLA-07
IncompleteAcceleratorNeverAnswers ==
    answer.src = "acc" => answer.cls # "incomplete"

\* RAC-TLA-08: an accelerator bound to the current generation was built or
\* adopted for the current lineage manifest, and a foreign one never answers.
ForeignAcceleratorNeverAdopted ==
    /\ \A a \in {acc, snap} :
         (a.kind = "published" /\ a.gen = gen) => a.lineage = lineage
    /\ answer.src = "acc" => answer.cls # "foreign"

\* RAC-TLA-09: every commit used a ticket equal to its group's epoch at the
\* commit, and the committed target was present.
StaleCandidateNeverCommits ==
    \A k \in commits : k.ticket = k.epoch /\ k.present

\* RAC-TLA-10: immediately after a failed update the state equals the state
\* recorded before it.
FailedUpdateIsNonMutating == \A f \in lastFault : f = FaultView

\* RAC-TLA-11: the semantic identity is a function of the semantic roots.
SemanticIdentityIsAcceleratorFree == semid = SemanticIdentity(catalog, joins)

=============================================================================
