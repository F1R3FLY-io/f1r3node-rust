----------------------------- MODULE HeldProvenance -----------------------------
(***************************************************************************)
(* G3 step S3b (DR-135), Rule H: a held process runs for the sender of     *)
(* its value (design g3-s3b-rule-h.md, section 13.2, parts H0 to H7).      *)
(*                                                                         *)
(* A value that a COMM binds keeps the provenance of the datum that        *)
(* brought it (P1 rem:signed-subst: "There is no unwrapping"). When a body *)
(* runs that value as a process, the holder of the value pays, at the      *)
(* holder's own price. A literal of a body is the code of the body's       *)
(* context and never takes a holder.                                       *)
(*                                                                         *)
(* The world. The parties W, C and D each own a contract body (an owner    *)
(* body) and can send data. One system body has no recorded caller, and a  *)
(* stored system continuation records the caller C (residue of an earlier  *)
(* system run). The initial data are a code datum of C, a code datum of D, *)
(* a code-free datum of W, and a datum that an earlier forward of W left   *)
(* with mixed code of C and D (the Unit sentinel of H5).                   *)
(*                                                                         *)
(* A value is [author, code, traced]. The author is the party whose code   *)
(* it is. traced is a ghost: it is FALSE when a mixed forward lost the     *)
(* provenance (deviation D1), and then the runner pays by design.          *)
(*                                                                         *)
(* Actions (one body step after each firing):                              *)
(*   Fire        H0 and H6: data fire a continuation in some deployment.   *)
(*               Each captured value takes the holder h(d) of its datum.   *)
(*               A system body takes the single code holder as caller.     *)
(*   RunHeld     H1 and H4: the body runs a captured value as a process.   *)
(*   StoreNested H2: the body stores a nested receive that uses a value,   *)
(*               wrapped in a held term when the holder differs from the   *)
(*               future context (always in a system body).                 *)
(*   Resend      H5: the body sends values in a new datum and records the  *)
(*               holder, the Unit sentinel or nothing.                     *)
(*   Match       H3: the body matches a captured value (its parts keep the *)
(*               holder) or a literal of its own (no holder) and runs the  *)
(*               part.                                                     *)
(* H7 (no binding) is outside the model: every run here has a binding.    *)
(*                                                                         *)
(* Invariants:                                                             *)
(*   ChargedToHolder  every run of a received value charges its author,    *)
(*                    or the runner when a mixed forward lost the          *)
(*                    provenance (D1).                                     *)
(*   NoLaundering     every run of a literal charges the literal's author, *)
(*                    the owner of the body.                               *)
(*   HolderPrice      every charge uses the price of the party that pays.  *)
(*                                                                         *)
(* Mutation selects one negative control:                                  *)
(*   bodyOwnerPaysHeld      an owner body runs a held value for itself     *)
(*                          -> ChargedToHolder                             *)
(*   unwrappedStore         H2 never wraps -> ChargedToHolder              *)
(*   dropClientCaller       a system body records no holder and no caller  *)
(*                          that is the running deployment at store time   *)
(*                          (the S3 filter; the "map2 <-" exploit C8)      *)
(*                          -> ChargedToHolder                             *)
(*   bodyCallerOnMixedDatum a system body gives a code-carrying datum      *)
(*                          without one holder its own caller (6c)         *)
(*                          -> ChargedToHolder                             *)
(*   signerOnMixedForward   an owner body that forwards code from two      *)
(*                          senders records nothing and becomes their      *)
(*                          holder (T12) -> ChargedToHolder                *)
(*   literalHolder          a literal match target takes the holder of the *)
(*                          firing datum -> NoLaundering                   *)
(*   contextPrice           a held run is charged at the context price     *)
(*                          -> HolderPrice                                 *)
(*                                                                         *)
(* Rust correspondence: rholang/src/rust/interpreter/accounting/           *)
(* owner_funding.rs (datum_holder, seal_signer, system_body_caller,        *)
(* held_reducer, wrap_held_uses, code_free) and reduce.rs (the exits,      *)
(* eval_receive, eval_match, eval_send, eval_cost_signed_term). Rocq       *)
(* companion: the held-provenance section of SystemResidueSeal.v.          *)
(***************************************************************************)
EXTENDS Naturals, FiniteSets

CONSTANTS MaxSteps, Mutation

ASSUME Mutation \in {"none", "bodyOwnerPaysHeld", "unwrappedStore",
                     "dropClientCaller", "bodyCallerOnMixedDatum",
                     "signerOnMixedForward", "literalHolder", "contextPrice"}

ASSUME MaxSteps \in 1..8

Parties == {"W", "C", "D"}

\* Each party pays at its own price.
Price == [p \in Parties |-> CASE p = "W" -> 1 [] p = "C" -> 2 [] OTHER -> 3]

NoOne == "noOne"
UnitSentinel == "unit"
Sys == "sys"

Value(author, code, traced) == [author |-> author, code |-> code, traced |-> traced]

Datum(vals, signer, caller) == [vals |-> vals, signer |-> signer, caller |-> caller]

\* A continuation: an owner body of a party, or a system body with a recorded
\* caller. A nested continuation also stores one value and its held term.
Cont(kind, party, persistent, stored, holder) ==
  [kind |-> kind, party |-> party, persistent |-> persistent,
   stored |-> stored, holder |-> holder]

NoValue == Value(Sys, FALSE, TRUE)

InitialData ==
  { Datum({Value("C", TRUE, TRUE)}, "C", NoOne),
    Datum({Value("D", TRUE, TRUE)}, "D", NoOne),
    Datum({Value("W", FALSE, TRUE)}, "W", NoOne),
    Datum({Value("C", TRUE, FALSE), Value("D", TRUE, FALSE)}, "W", UnitSentinel) }

InitialConts ==
  { Cont("owner", p, TRUE, NoValue, NoOne) : p \in Parties }
    \cup { Cont("system", NoOne, TRUE, NoValue, NoOne),
           Cont("system", "C", TRUE, NoValue, NoOne) }

VARIABLES pool, conts, frame, charges, steps

vars == <<pool, conts, frame, charges, steps>>

NoFrame == [active |-> FALSE, kind |-> "owner", party |-> NoOne, deploy |-> NoOne,
            items |-> {}]

-----------------------------------------------------------------------------
(* Holders and contexts.                                                   *)

\* h(d): the recorded caller, else the signer. The Unit sentinel gives no
\* holder, and a system signer with no caller gives no holder.
DatumHolder(d) ==
  IF d.caller \in Parties THEN d.caller
  ELSE IF d.caller = UnitSentinel THEN NoOne
  ELSE IF d.signer \in Parties THEN d.signer
  ELSE NoOne

CarriesCode(d) == \E v \in d.vals : v.code

\* H6: the caller of a system body is the single member of K, also on a
\* client lane. K holds the continuation's recorded caller and the holder of
\* each fired datum that carries code.
SystemCaller(k, ds) ==
  LET K == (({k.party} \cap Parties)
              \cup {DatumHolder(d) : d \in {e \in ds : CarriesCode(e)}}) \ {NoOne}
  IN IF Cardinality(K) = 1 THEN CHOOSE x \in K : TRUE ELSE NoOne

\* The signature and payer of a body context: the owner, else the caller of a
\* system body, else the running deployment.
ContextPayer(f) ==
  IF f.kind = "owner" THEN f.party
  ELSE IF f.party \in Parties THEN f.party
  ELSE f.deploy

\* The future context of a body stored now: the owner of an owner body, and
\* "system" for a system body (H2 wraps every held use of a system seal).
FutureIsSystem(f) == f.kind = "system"

-----------------------------------------------------------------------------
(* Charges.                                                                *)

\* The expected payer of a run (the ghost side of the invariants).
ExpectedPayer(v, f) == IF v.traced THEN v.author ELSE ContextPayer(f)

\* H1 and H4: a value with a holder that differs from the context runs in
\* held_reducer(holder), at the holder's price. Otherwise the context pays.
ActualPayer(holder, f) ==
  IF holder \in Parties /\ holder # ContextPayer(f)
  THEN IF Mutation = "bodyOwnerPaysHeld" /\ f.kind = "owner"
       THEN ContextPayer(f)
       ELSE holder
  ELSE ContextPayer(f)

ActualPrice(holder, f) ==
  IF holder \in Parties /\ holder # ContextPayer(f)
     /\ ~(Mutation = "bodyOwnerPaysHeld" /\ f.kind = "owner")
  THEN IF Mutation = "contextPrice" THEN Price[ContextPayer(f)] ELSE Price[holder]
  ELSE Price[ContextPayer(f)]

Charge(kind, v, holder, f) ==
  [kind |-> kind, author |-> v.author, traced |-> v.traced,
   expected |-> ExpectedPayer(v, f),
   payer |-> ActualPayer(holder, f), price |-> ActualPrice(holder, f)]

-----------------------------------------------------------------------------
(* The behavior.                                                           *)

Init ==
  /\ pool = InitialData
  /\ conts = InitialConts
  /\ frame = NoFrame
  /\ charges = {}
  /\ steps = 0

\* H0 and H6: one or two data fire the continuation k in deployment dep.
\* Each captured value takes the holder of its datum. A nested continuation
\* adds its stored value with the holder of its held term.
Fire(ds, k, dep) ==
  /\ ~frame.active
  /\ steps < MaxSteps
  /\ pool' = pool \ ds
  /\ conts' = IF k.persistent THEN conts ELSE conts \ {k}
  /\ LET captured ==
           UNION {{[val |-> v, holder |-> DatumHolder(d)] : v \in d.vals} : d \in ds}
         storedItem == IF k.stored.code
                       THEN {[val |-> k.stored, holder |-> k.holder]}
                       ELSE {}
         party == IF k.kind = "owner" THEN k.party ELSE SystemCaller(k, ds)
     IN frame' = [active |-> TRUE, kind |-> k.kind, party |-> party,
                  deploy |-> dep, items |-> captured \cup storedItem]
  /\ steps' = steps + 1
  /\ UNCHANGED charges

\* The body step ends the firing.
EndBody == frame' = NoFrame /\ steps' = steps + 1

\* H1 and H4: run a captured value as a process.
RunHeld(it) ==
  /\ frame.active
  /\ it \in frame.items
  /\ it.val.code
  /\ charges' = charges \cup {Charge("held", it.val, it.holder, frame)}
  /\ EndBody
  /\ UNCHANGED <<pool, conts>>

\* H2: store a nested receive whose body uses the value of item it. The
\* value is wrapped in a held term when it is code with a holder, and the
\* future is a system seal or the holder differs from the future signature.
\* Control "dropClientCaller": a system body records no holder and no caller
\* that is the running deployment, as S3's client filter did. A client that
\* calls a system body such as Either's "map2 <-" then loses its code's
\* provenance, and the party that later feeds the stored chain pays (C8).
StoreNested(it) ==
  /\ frame.active
  /\ it \in frame.items
  /\ LET dropped(x) == /\ Mutation = "dropClientCaller"
                       /\ frame.kind = "system"
                       /\ x = frame.deploy
         wrap == /\ it.val.code
                 /\ it.holder \in Parties
                 /\ (FutureIsSystem(frame) \/ it.holder # frame.party)
                 /\ Mutation # "unwrappedStore"
                 /\ ~dropped(it.holder)
         caller == IF frame.kind = "system" /\ ~dropped(frame.party)
                   THEN frame.party ELSE NoOne
         party == IF frame.kind = "owner" THEN frame.party ELSE caller
     IN conts' = conts \cup
                   {Cont(frame.kind, party, FALSE, it.val,
                         IF wrap THEN it.holder ELSE NoOne)}
  /\ EndBody
  /\ UNCHANGED <<pool, charges>>

\* H5: send the values of the items S in a new datum. Each code item with a
\* holder gives that holder, and a code item without one makes the datum
\* mixed. Code-free items are neutral.
\* The ghost follows the correct rule, whatever the Mutation:
\*   - a recorded holder keeps every value as it is;
\*   - a mixed datum that the rule leaves without a holder (the Unit
\*     sentinel of an owner body, or no caller in a system body) loses the
\*     provenance of its code, so the runner pays (D1);
\*   - an owner body that records nothing becomes the holder of its datum
\*     (the signer). Code that had no holder is then the forwarder's
\*     responsibility (D13).
Resend(S) ==
  /\ frame.active
  /\ S # {}
  /\ S \subseteq frame.items
  /\ Cardinality(S) <= 2
  /\ LET codeItems == {it \in S : it.val.code}
         holders == {it.holder : it \in codeItems} \ {NoOne}
         mixed == (\E it \in codeItems : it.holder = NoOne) \/ Cardinality(holders) >= 2
         agreed == IF Cardinality(holders) = 1 THEN CHOOSE x \in holders : TRUE ELSE NoOne
         foreignCode == \E it \in codeItems : it.holder \in Parties /\ it.holder # frame.party
         \* The caller that the rule records, without a mutation.
         ruleCaller ==
           IF frame.kind = "owner"
           THEN IF ~mixed /\ agreed \in Parties /\ agreed # frame.party THEN agreed
                ELSE IF mixed /\ foreignCode THEN UnitSentinel
                ELSE NoOne
           ELSE IF codeItems = {} THEN frame.party
                ELSE IF ~mixed THEN agreed
                ELSE NoOne
         caller ==
           IF Mutation = "signerOnMixedForward" /\ frame.kind = "owner"
              /\ ruleCaller = UnitSentinel
           THEN NoOne
           ELSE IF Mutation = "bodyCallerOnMixedDatum" /\ frame.kind = "system"
                   /\ codeItems # {} /\ mixed
           THEN frame.party
           ELSE ruleCaller
         lost == codeItems # {} /\ ruleCaller \in {UnitSentinel, NoOne}
                   /\ (frame.kind = "system" \/ ruleCaller = UnitSentinel)
         takenOver == frame.kind = "owner" /\ ruleCaller = NoOne
         Ghost(it) ==
           IF ~it.val.code THEN it.val
           ELSE IF lost THEN Value(it.val.author, TRUE, FALSE)
           ELSE IF takenOver /\ it.holder = NoOne THEN Value(frame.party, TRUE, TRUE)
           ELSE it.val
         vals == {Ghost(it) : it \in S}
         signer == IF frame.kind = "owner" THEN frame.party ELSE Sys
     IN pool' = pool \cup {Datum(vals, signer, caller)}
  /\ EndBody
  /\ UNCHANGED <<conts, charges>>

\* H3: match a captured value. Its parts keep the holder of the value, and
\* the matched part runs.
MatchHeld(it) ==
  /\ frame.active
  /\ it \in frame.items
  /\ it.val.code
  /\ charges' = charges \cup {Charge("held", it.val, it.holder, frame)}
  /\ EndBody
  /\ UNCHANGED <<pool, conts>>

\* H3: match a literal of an owner body. The literal is the owner's code and
\* has no holder, so the matched part runs for the owner. Control
\* "literalHolder" gives the literal the holder of the captured value it.
MatchLiteral(it) ==
  /\ frame.active
  /\ frame.kind = "owner"
  /\ it \in frame.items
  /\ LET literal == Value(frame.party, TRUE, TRUE)
         holder == IF Mutation = "literalHolder" THEN it.holder ELSE NoOne
     IN charges' = charges \cup {Charge("literal", literal, holder, frame)}
  /\ EndBody
  /\ UNCHANGED <<pool, conts>>

Next ==
  \/ \E ds \in SUBSET pool, k \in conts, dep \in Parties :
       /\ ds # {}
       /\ Cardinality(ds) <= 2
       /\ Fire(ds, k, dep)
  \/ \E it \in frame.items : RunHeld(it)
  \/ \E it \in frame.items : StoreNested(it)
  \/ \E S \in SUBSET frame.items : Resend(S)
  \/ \E it \in frame.items : MatchHeld(it)
  \/ \E it \in frame.items : MatchLiteral(it)

Spec == Init /\ [][Next]_vars

-----------------------------------------------------------------------------
(* Invariants.                                                             *)

TypeOK ==
  /\ steps \in 0..MaxSteps
  /\ frame.active \in BOOLEAN
  /\ \A c \in charges :
       /\ c.kind \in {"held", "literal"}
       /\ c.payer \in Parties
       /\ c.price \in {Price[p] : p \in Parties}

\* Every run of a received value charges its author, or the runner when a
\* mixed forward lost the provenance (D1).
ChargedToHolder ==
  \A c \in charges : c.kind = "held" => c.payer = c.expected

\* Every run of a literal charges the owner of the body that wrote it.
NoLaundering ==
  \A c \in charges : c.kind = "literal" => c.payer = c.author

\* Every charge uses the price of the party that pays.
HolderPrice ==
  \A c \in charges : c.price = Price[c.payer]

=============================================================================
