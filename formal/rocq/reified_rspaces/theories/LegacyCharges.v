(* ═══════════════════════════════════════════════════════════════════════════
   LegacyCharges.v — the legacy storage charges of a space transaction
   (decision 45, Option P)

   In a legacy deploy, ChargingRSpace charges phlo for each host operation
   of an ordinary program. For each operation i, the reducer first
   registers the introduction authority of the operation (reg i). Then
   ChargingRSpace charges the storage of the record or of the continuation
   (the pre-charge, pre i). Then the host operation makes its observer
   calls and its events (the body of i). Then ChargingRSpace charges a list
   of refunds and event fees (the post-charges, post i). A charge that runs
   out of phlo (OOP) stops the deploy. A failing pre-charge of operation i
   stops before the body of i. A failing post-charge of i stops after the
   body of i.

   A space transaction stages host-equivalent operations 0 .. n-1. Its
   commit applies them in order, without ChargingRSpace. The commit runs
   the before_observe callback of each staged operation before the
   observer calls of that operation. Option P puts the charges into these
   callbacks and into a tail:
   - The callback of operation 0 registers, then it charges pre 0.
   - The callback of operation i > 0 first charges post (i-1), the carry.
     Then it registers, and then it charges pre i.
   - After the commit, the tail charges post (n-1).
   So the legacy log of the space is pre 0 | post 0, pre 1 | ... |
   post (n-1). The symbol | marks a callback boundary. The host makes the
   same sequence of charges. A failing charge in callback i stops the commit
   before the body of i, and the operations 0 .. i-1 stay applied (prefix
   atomicity, D6). A failing charge of the tail stops after the full
   commit. Replay stages the operations of play and runs the same
   callbacks. Under decision 36, an unpaired replay fire makes the observer
   calls of play before it fails. So replay makes the charges of play and
   stops at the same point.

   This module proves that the space run and the host program make the
   same charges, with the same budget, stop point and applied operations
   (Q79). It also proves that replay stops where play stops (Q80).

   Sources
   - Design: .reified-rspaces-work/d45/design-report.md, section D: D.1
     (the charges of each staged shape), D.2 (the sequences of the space
     and of the host), D.3 (the chain), D.4 (out of phlo and replay) and
     D.7 (the deviations). Section E.5 plans this model. Decision 45
     (Option P) of docs/rspace/reified-rspaces/decision-register.md.
   - Code: the host files at HEAD e9b082454, with the line numbers below.
     The chain is new code of decision 45, and this header cites its items
     by name only. The header also cites the P1 substrate files by name,
     as EventCorrelation.v does, because their line numbers change often.

   ─────────────────────────────────────────────────────────────────────────
   Traceability: Rocq definition → Rust code
   (a path of rholang/src/rust/interpreter/ is given without that
   prefix, and rspace++/src/rspace/ is abbreviated as rs/)
   ─────────────────────────────────────────────────────────────────────────
   cost, value              Cost and its field value
                            (accounting/costs.rs:14-17)
   state, initial           the legacy budget of RuntimeBudget (remaining
                            and log) and the history of the calls when the
                            transaction commits. bodies is a ghost: the
                            operations of the transaction whose bodies ran
   charge                   RuntimeBudget::charge_legacy
                            (accounting/mod.rs:871-890): OOP when
                            remaining < 0 before the charge (876-878), the
                            subtraction and the log entry (879-885), then
                            OOP when remaining < 0 after it (886-888)
   debit                    i64::saturating_sub (accounting/mod.rs:879)
   charge_all               a loop of charge_legacy calls that stops at the
                            first error: the charges of handle_result
                            (storage/charging_rspace.rs:330-347), the
                            carry of a callback, and the tail
   pre, post                the charges of one staged operation (design
                            report D.1). The pre-charge of ChargingRSpace
                            (storage/charging_rspace.rs:96-100,
                            133-134) is storage_cost_consume or
                            storage_cost_produce
                            (accounting/costs.rs:359-380). The post list
                            is the list of handle_result
                            (storage/charging_rspace.rs:298-381), with
                            event_storage_cost and comm_event_storage_cost
                            (accounting/costs.rs:382-396)
   point, label_post,       ghost labels of the model: the operation of a
     post_charges           step, and the position of a post-charge
   status                   Done, or the error of a run. OutOfPhlo is
                            InterpreterError::OutOfPhlogistonsError.
                            Rejected is an error of a registration or of
                            an observer call. Diverged is
                            SpaceTxnError::ReplayDivergence
   call, accept, call_step  the fallible calls. CReg is
                            register_introduction_authority: the reducer
                            calls it before ChargingRSpace
                            (reduce.rs:543-547, 655-659), and a
                            callback calls it between the carry and the
                            pre-charge. CObserve is the observer calls of
                            a body
   body, run_body, paired   the body of one operation: the inner host call
                            of ChargingRSpace
                            (storage/charging_rspace.rs:104-107, 137),
                            or the observed body of apply_produce and
                            apply_consume (rs/rspace/space_play.rs,
                            rs/replay_rspace/space_replay.rs). An unpaired
                            replay fire fails with ReplayDivergence after
                            its observer calls accept (decision 36)
   host_op, host_ops,       the host program: the registration of the
     host_run               reducer, then ChargingRSpace::consume or
                            ChargingRSpace::produce
                            (storage/charging_rspace.rs:79-119,
                            121-151), for each operation in order. With a
                            replay pairing, host_ops is a proof device: the
                            flat form of a space replay (space_run_in_flat)
   chain, chain_new,        LegacyChain (reified/legacy_charges.rs) and
     attach, stage          LegacyChain::attach: the callback of an
                            operation charges the carry, registers and
                            charges the pre-charge. The post-charges of
                            the operation become the carry
   before_observe           the callback that LegacyChain::attach makes.
                            The commit calls it through run_before_observe
                            (rs/space_store_handle.rs) in apply_produce and
                            apply_consume, before the observer starts
   commit                   commit_staged (rs/space_store_handle.rs): the
                            staged operations in order. A failure of
                            operation i leaves the operations before i
                            applied (D6)
   finish                   LegacyChain::finish: the tail. The job runs it
                            after the commit and before its outcome
   space_run, replay_run    one transaction in play and in replay

   ─────────────────────────────────────────────────────────────────────────
   Abstraction
   ─────────────────────────────────────────────────────────────────────────
   - The model covers one committed transaction of a legacy deploy with an
     active chain: a legacy budget that is metered. An inactive chain
     charges nothing. ChargingRSpace charges nothing in that case either
     (storage/charging_rspace.rs:90-95, 130-132, and
     accounting/mod.rs:872-874).
   - An operation is abstract. Its charges pre and post are Section
     variables, so the theorems hold for all charges of all operations.
     The chain computes the charges from the staged operation (design
     report D.1). So play and replay attach the same charges to the same
     staged operations.
   - A cost is abstract, with its value. The log holds every charge that
     passes the first check, in order. The code keeps only the last
     max_log_entries entries of this sequence, or none. That is a function
     of the sequence, so equal sequences give equal logs.
   - The subtraction debit is abstract. The theorems hold for every
     subtraction, so they also cover the saturation of the i64 arithmetic.
   - The registration and the observer calls are fallible calls. Their
     decision (accept) is a function of the history of the calls, as in
     EventCorrelation.v. Play and replay share the registry and the
     observer. A staged operation without a registration callback has a
     registration that accept accepts.
   - The body of an operation is one observer call, then its events and
     its store effects (run_body). The effects are a function of the
     sequence of applied bodies, so the model records that sequence
     (bodies).
   - The host program has its host operations one after the other. The
     reducer can make other legacy charges between two host operations.
     Such charges are outside both runs of this model.
   - The model has no staging failure and no commit check before a
     callback. Design report D.7 lists the deviations on these paths,
     which a correct run never reaches. For example, a failed read-only
     check of operation i+1 skips the post-charges of operation i.
   - The points PReg, PPre, PObserve and PPost are ghost labels. They name
     the step where a run stops, and the status of a stop holds its point.
   - Each round of decision 43 is a transaction with its own chain and its
     own tail. Q79 holds from any budget, log and call history, so it
     applies to each round in turn.
   - H23 of EventCorrelation.v proves two facts about a replay with the
     commit log of play. Replay stages the operations of play. Replay also
     pairs each fire that play committed. Modules do not import each
     other, so Q80 states these facts as premises. The premises are the
     same operations, and paired i for each operation i whose body play
     applied.

   ─────────────────────────────────────────────────────────────────────────
   Headlines (each is closed under the global context)
   ─────────────────────────────────────────────────────────────────────────
   Q79 legacy_rebracket_eq_host       the space run (the callbacks with
                                      the carry, then the tail) equals the
                                      host program. The two runs have the
                                      same log, remaining budget, status
                                      with its stop point, calls and
                                      applied bodies. A stop at the
                                      pre-charge of operation i applies
                                      the first i operations. A stop at a
                                      post-charge of operation i applies
                                      the first i + 1 operations. A
                                      completed run applies all the
                                      operations.
   Q80 legacy_replay_stops_with_play  replay stages the operations of
                                      play, and it pairs each operation
                                      that play applied. Then replay has
                                      the log, remaining budget, status
                                      with its stop point, calls and
                                      applied bodies of play. Replay never
                                      diverges.

   Negative controls (lemmas, not headlines):
   - legacy_tail_dropped_breaks_eq: without the tail, a run that the host
     completes misses the post-charges of the last operation. So the logs
     differ when that list is not empty.
   - legacy_pre_after_observe_breaks_prefix: with the pre-charge after the
     body, a pre-charge that runs out of phlo stops at the same point as
     in the host. But the body of the operation has run, and the host
     never runs it. So the applied prefix differs from the host's. This is
     the simplest faithful witness. A replay witness would depend on where
     the pairing check is, relative to the late pre-charge. This order does
     not fix that place.
   - legacy_negative_controls_computed: a computed instance of both.

   No Axiom, Parameter, Admitted, admit, Abort or Conjecture. Section
   variables become universally quantified parameters of the closed
   theorems. The file imports only the Rocq standard library.

   ─────────────────────────────────────────────────────────────────────────
   Failed strategies (do not try them again)
   ─────────────────────────────────────────────────────────────────────────
   - A Section with the name of the file (LegacyCharges) makes Rocq 9.1.1
     warn "Trying to mask the absolute name". The Section is Chain.
   - `cbn [before_observe]` leaves before_observe folded, because the
     unfolded term does not reduce. Use `unfold before_observe`.
   - `congruence` does not reduce `fst (s, Done)`. Reduce the pair with
     `cbn [fst snd]` first.
   - host_op_pairing first destructed the steps of the unfolded goal and
     then unfolded the premise. The premise still held the steps, not
     their results, so the rewrite found no subterm. Restate the result of
     play with the step equations (play_result).
   - `destruct (paired i) eqn:E` also rewrites `paired i` in the premises
     (Rocq 9.1.1). An `apply` of such a premise against the goal
     `paired i = true` then fails. Derive the premise and close with
     `congruence`.
   ═══════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import List ZArith Lia PeanoNat.
Import ListNotations.

(* ── Points and statuses ────────────────────────────────────────────────── *)

(* The place of a step in one transaction. The index i is the position of
   the operation in the staging order. The index k is the position of a
   charge in the post list of its operation. *)
Inductive point : Type :=
| PReg (i : nat)
| PPre (i : nat)
| PObserve (i : nat)
| PPost (i k : nat).

(* The status of a run. Inside a run, Done means "continue". *)
Inductive status : Type :=
| Done
| OutOfPhlo (p : point)
| Rejected (p : point)
| Diverged (p : point).

Section Chain.

(* A staged operation, as the chain reads it. *)
Variable op : Type.
(* A Cost: a value and a label. *)
Variable cost : Type.
Variable value : cost -> Z.
(* The subtraction of charge_legacy (i64::saturating_sub). *)
Variable debit : Z -> Z -> Z.
(* The charges of an operation: the pre-charge and the post list. *)
Variable pre : op -> cost.
Variable post : op -> list cost.

(* A fallible call: a registration, or the observer calls of a body. *)
Inductive call : Type :=
| CReg (o : op)
| CObserve (o : op).

(* The decision of the registry and of the observer: a function of the
   history of the calls. *)
Variable accept : list call -> call -> bool.

(* The legacy budget, the legacy log, the history of the calls, and the
   operations of this transaction whose bodies ran. *)
Record state : Type := mkState {
  remaining : Z;
  legacy_log : list cost;
  calls : list call;
  bodies : list op
}.

Definition result : Type := (state * status)%type.

(* Run k after r when r continues. Otherwise stop with r. *)
Definition bind (r : result) (k : state -> result) : result :=
  let (s, st) := r in
  match st with
  | Done => k s
  | _ => (s, st)
  end.

Lemma bind_done_l : forall s k, bind (s, Done) k = k s.
Proof. reflexivity. Qed.

Lemma bind_assoc : forall r f g,
  bind (bind r f) g = bind r (fun s => bind (f s) g).
Proof. intros [s st] f g. destruct st; reflexivity. Qed.

Lemma bind_ext : forall r f g,
  (forall s, f s = g s) -> bind r f = bind r g.
Proof.
  intros [s st] f g same. destruct st; cbn [bind]; [apply same | reflexivity ..].
Qed.

Lemma bind_done_r : forall r, bind r (fun s => (s, Done)) = r.
Proof. intros [s st]. destruct st; reflexivity. Qed.

(* ── The steps ──────────────────────────────────────────────────────────── *)

(* RuntimeBudget::charge_legacy. *)
Definition charge (p : point) (c : cost) (s : state) : result :=
  if Z.ltb (remaining s) 0 then (s, OutOfPhlo p)
  else
    let after := debit (remaining s) (value c) in
    let s' := mkState after (legacy_log s ++ [c]) (calls s) (bodies s) in
    if Z.ltb after 0 then (s', OutOfPhlo p) else (s', Done).

(* Charge each entry in order, and stop at the first error. *)
Fixpoint charge_all (charges : list (point * cost)) (s : state) : result :=
  match charges with
  | [] => (s, Done)
  | (p, c) :: rest => bind (charge p c s) (charge_all rest)
  end.

Fixpoint label_post (i k : nat) (cs : list cost) : list (point * cost) :=
  match cs with
  | [] => []
  | c :: rest => (PPost i k, c) :: label_post i (S k) rest
  end.

(* The post-charges of operation i, labeled with their points. *)
Definition post_charges (i : nat) (o : op) : list (point * cost) :=
  label_post i 0 (post o).

Definition call_step (p : point) (c : call) (s : state) : result :=
  let s' := mkState (remaining s) (legacy_log s) (calls s ++ [c]) (bodies s) in
  if accept (calls s) c then (s', Done) else (s', Rejected p).

(* The events and the store effects of a body. *)
Definition run_body (o : op) (s : state) : state :=
  mkState (remaining s) (legacy_log s) (calls s) (bodies s ++ [o]).

(* The body of operation i: its observer calls, then the pairing check of
   a replay (decision 36), then its events and its effects. *)
Definition body (paired : nat -> bool) (i : nat) (o : op) (s : state) : result :=
  bind (call_step (PObserve i) (CObserve o) s) (fun s1 =>
    if paired i then (run_body o s1, Done) else (s1, Diverged (PObserve i))).

(* Play has no pairing check. *)
Definition play_pairs (i : nat) : bool := true.

(* ── The host program ───────────────────────────────────────────────────── *)

(* One host operation: the registration of the reducer, the pre-charge of
   ChargingRSpace, the inner host call, and the post list of
   handle_result. host_ops play_pairs is the host program. Under another
   pairing, host_ops is the flat form of a space replay (space_run_in_flat),
   which the proof of Q80 uses. *)
Definition host_op (paired : nat -> bool) (i : nat) (o : op) (s : state) : result :=
  bind (call_step (PReg i) (CReg o) s) (fun s1 =>
  bind (charge (PPre i) (pre o) s1) (fun s2 =>
  bind (body paired i o s2) (fun s3 =>
  charge_all (post_charges i o) s3))).

Fixpoint host_ops (paired : nat -> bool) (i : nat) (ops : list op) (s : state) : result :=
  match ops with
  | [] => (s, Done)
  | o :: rest => bind (host_op paired i o s) (host_ops paired (S i) rest)
  end.

(* The state when the transaction starts: any budget, log and call
   history, and no body of this transaction yet. *)
Definition initial (budget : Z) (log0 : list cost) (history : list call) : state :=
  mkState budget log0 history [].

Definition host_run (ops : list op) (s0 : state) : result := host_ops play_pairs 0 ops s0.

(* ── The space transaction ──────────────────────────────────────────────── *)

(* LegacyChain: the index of the next operation (a ghost) and the carry. *)
Record chain : Type := mkChain {
  chain_next : nat;
  chain_carry : list (point * cost)
}.

Definition chain_new : chain := mkChain 0 [].

(* The before_observe callback of a staged operation. *)
Record callback : Type := mkCallback {
  cb_index : nat;
  cb_op : op;
  cb_carried : list (point * cost)
}.

(* LegacyChain::attach: the callback takes the carry, and the post-charges
   of the operation become the carry. *)
Definition attach (ch : chain) (o : op) : callback * chain :=
  (mkCallback (chain_next ch) o (chain_carry ch),
   mkChain (S (chain_next ch)) (post_charges (chain_next ch) o)).

(* Stage the operations in order through the chain. *)
Fixpoint stage (ch : chain) (ops : list op) : list callback * chain :=
  match ops with
  | [] => ([], ch)
  | o :: rest =>
      let (cb, ch1) := attach ch o in
      let (cbs, ch2) := stage ch1 rest in
      (cb :: cbs, ch2)
  end.

(* The callback: the carry, then the registration, then the pre-charge. *)
Definition before_observe (cb : callback) (s : state) : result :=
  bind (charge_all (cb_carried cb) s) (fun s1 =>
  bind (call_step (PReg (cb_index cb)) (CReg (cb_op cb)) s1) (fun s2 =>
  charge (PPre (cb_index cb)) (pre (cb_op cb)) s2)).

(* commit_staged: for each staged operation, the callback, then the body.
   A failure stops the commit, and the operations before it stay applied
   (D6). *)
Fixpoint commit (paired : nat -> bool) (cbs : list callback) (s : state) : result :=
  match cbs with
  | [] => (s, Done)
  | cb :: rest =>
      bind (before_observe cb s) (fun s1 =>
      bind (body paired (cb_index cb) (cb_op cb) s1) (fun s2 =>
      commit paired rest s2))
  end.

(* LegacyChain::finish: the tail charges the last carry. *)
Definition finish (ch : chain) (s : state) : result := charge_all (chain_carry ch) s.

(* One transaction: stage, commit, then the tail after a full commit. *)
Definition space_run_in (paired : nat -> bool) (ops : list op) (s0 : state) : result :=
  let (cbs, ch) := stage chain_new ops in
  bind (commit paired cbs s0) (finish ch).

Definition space_run (ops : list op) (s0 : state) : result := space_run_in play_pairs ops s0.

Definition replay_run (paired : nat -> bool) (ops : list op) (s0 : state) : result :=
  space_run_in paired ops s0.

(* ── Rebracketing ───────────────────────────────────────────────────────── *)

(* The commit with the tail equals the carry followed by the flat run of
   the operations. *)
Lemma commit_finish_rebracket : forall paired ops ch s cbs ch',
  stage ch ops = (cbs, ch') ->
  bind (commit paired cbs s) (finish ch') =
  bind (charge_all (chain_carry ch) s) (host_ops paired (chain_next ch) ops).
Proof.
  intros paired ops. induction ops as [| o rest IH]; intros ch s cbs ch' staged.
  - cbn [stage] in staged. injection staged as <- <-.
    cbn [commit host_ops]. rewrite bind_done_l, bind_done_r. reflexivity.
  - cbn [stage attach] in staged.
    destruct (stage (mkChain (S (chain_next ch)) (post_charges (chain_next ch) o)) rest)
      as [cbs0 ch2] eqn:staged_rest.
    injection staged as <- <-.
    cbn [commit host_ops]. unfold before_observe. cbn [cb_index cb_op cb_carried].
    rewrite !bind_assoc. apply bind_ext. intros s1.
    unfold host_op. rewrite !bind_assoc. apply bind_ext. intros s2.
    rewrite !bind_assoc. apply bind_ext. intros s3.
    rewrite !bind_assoc. apply bind_ext. intros s4.
    rewrite (IH _ s4 _ _ staged_rest). reflexivity.
Qed.

Lemma space_run_in_flat : forall paired ops s0,
  space_run_in paired ops s0 = host_ops paired 0 ops s0.
Proof.
  intros paired ops s0. unfold space_run_in.
  destruct (stage chain_new ops) as [cbs ch] eqn:staged.
  rewrite (commit_finish_rebracket paired ops chain_new s0 cbs ch staged).
  reflexivity.
Qed.

Lemma space_run_eq_host_run : forall ops s0, space_run ops s0 = host_run ops s0.
Proof. intros ops s0. apply space_run_in_flat. Qed.

(* ── The outcomes of each step ──────────────────────────────────────────── *)

Lemma call_step_cases : forall p c s,
  bodies (fst (call_step p c s)) = bodies s /\
  (snd (call_step p c s) = Done \/ snd (call_step p c s) = Rejected p).
Proof.
  intros p c s. unfold call_step.
  destruct (accept (calls s) c); cbn [fst snd bodies]; auto.
Qed.

Lemma charge_cases : forall p c s,
  bodies (fst (charge p c s)) = bodies s /\
  (snd (charge p c s) = Done \/ snd (charge p c s) = OutOfPhlo p).
Proof.
  intros p c s. unfold charge.
  destruct (Z.ltb (remaining s) 0); [cbn [fst snd]; auto |].
  destruct (Z.ltb (debit (remaining s) (value c)) 0); cbn [fst snd bodies]; auto.
Qed.

Lemma charge_all_post_cases : forall i cs k s,
  bodies (fst (charge_all (label_post i k cs) s)) = bodies s /\
  (snd (charge_all (label_post i k cs) s) = Done \/
   exists k', snd (charge_all (label_post i k cs) s) = OutOfPhlo (PPost i k')).
Proof.
  intros i cs. induction cs as [| c rest IH]; intros k s.
  - cbn [label_post charge_all fst snd]. auto.
  - cbn [label_post charge_all].
    pose proof (charge_cases (PPost i k) c s) as [same_bodies outcome].
    destruct (charge (PPost i k) c s) as [s1 st1].
    cbn [fst snd] in same_bodies, outcome.
    destruct outcome as [-> | ->]; cbn [bind fst snd].
    + destruct (IH (S k) s1) as [same_rest outcome_rest].
      split; [rewrite same_rest; exact same_bodies | exact outcome_rest].
    + split; [exact same_bodies | right; exists k; reflexivity].
Qed.

Lemma body_play_cases : forall i o s,
  (snd (body play_pairs i o s) = Done /\
   bodies (fst (body play_pairs i o s)) = bodies s ++ [o]) \/
  (snd (body play_pairs i o s) = Rejected (PObserve i) /\
   bodies (fst (body play_pairs i o s)) = bodies s).
Proof.
  intros i o s. unfold body.
  pose proof (call_step_cases (PObserve i) (CObserve o) s) as [same_bodies outcome].
  destruct (call_step (PObserve i) (CObserve o) s) as [s1 st1].
  cbn [fst snd] in same_bodies, outcome.
  destruct outcome as [-> | ->]; cbn [bind play_pairs fst snd run_body bodies].
  - left. split; [reflexivity | rewrite same_bodies; reflexivity].
  - right. split; [reflexivity | exact same_bodies].
Qed.

(* The outcomes of one host operation in play. *)
Lemma host_op_cases : forall i o s,
  (snd (host_op play_pairs i o s) = Done /\
   bodies (fst (host_op play_pairs i o s)) = bodies s ++ [o]) \/
  (exists k, snd (host_op play_pairs i o s) = OutOfPhlo (PPost i k) /\
   bodies (fst (host_op play_pairs i o s)) = bodies s ++ [o]) \/
  (snd (host_op play_pairs i o s) = OutOfPhlo (PPre i) /\
   bodies (fst (host_op play_pairs i o s)) = bodies s) \/
  (snd (host_op play_pairs i o s) = Rejected (PReg i) /\
   bodies (fst (host_op play_pairs i o s)) = bodies s) \/
  (snd (host_op play_pairs i o s) = Rejected (PObserve i) /\
   bodies (fst (host_op play_pairs i o s)) = bodies s).
Proof.
  intros i o s. unfold host_op.
  pose proof (call_step_cases (PReg i) (CReg o) s) as [bodies1 outcome1].
  destruct (call_step (PReg i) (CReg o) s) as [s1 st1].
  cbn [fst snd] in bodies1, outcome1.
  destruct outcome1 as [-> | ->]; cbn [bind fst snd];
    [| right; right; right; left; split; [reflexivity | exact bodies1]].
  pose proof (charge_cases (PPre i) (pre o) s1) as [bodies2 outcome2].
  destruct (charge (PPre i) (pre o) s1) as [s2 st2].
  cbn [fst snd] in bodies2, outcome2.
  destruct outcome2 as [-> | ->]; cbn [bind fst snd];
    [| right; right; left; split; [reflexivity | congruence]].
  pose proof (body_play_cases i o s2) as [[outcome3 bodies3] | [outcome3 bodies3]];
    destruct (body play_pairs i o s2) as [s3 st3];
    cbn [fst snd] in bodies3, outcome3; subst st3; cbn [bind fst snd].
  - pose proof (charge_all_post_cases i (post o) 0 s3) as [bodies4 outcome4].
    unfold post_charges.
    destruct (charge_all (label_post i 0 (post o)) s3) as [s4 st4].
    cbn [fst snd] in bodies4, outcome4.
    destruct outcome4 as [-> | [k ->]]; cbn [fst snd].
    + left. split; [reflexivity | congruence].
    + right; left. exists k. split; [reflexivity | congruence].
  - right; right; right; right. split; [reflexivity | congruence].
Qed.

(* The stop point of the host program determines the operations whose
   bodies ran. A pre-charge of operation i stops before its body, and a
   post-charge of operation i stops after its body. *)
Lemma host_ops_stops : forall ops i s,
  (forall j, snd (host_ops play_pairs i ops s) = OutOfPhlo (PPre j) ->
     exists m, j = i + m /\ m < length ops /\
       bodies (fst (host_ops play_pairs i ops s)) = bodies s ++ firstn m ops) /\
  (forall j k, snd (host_ops play_pairs i ops s) = OutOfPhlo (PPost j k) ->
     exists m, j = i + m /\ m < length ops /\
       bodies (fst (host_ops play_pairs i ops s)) = bodies s ++ firstn (S m) ops) /\
  (snd (host_ops play_pairs i ops s) = Done ->
     bodies (fst (host_ops play_pairs i ops s)) = bodies s ++ ops).
Proof.
  induction ops as [| o rest IH]; intros i s.
  - cbn [host_ops fst snd]. rewrite app_nil_r.
    split; [intros j stop; discriminate |].
    split; [intros j k stop; discriminate | reflexivity].
  - cbn [host_ops].
    pose proof (host_op_cases i o s) as cases.
    destruct (host_op play_pairs i o s) as [s1 st1].
    cbn [fst snd] in cases.
    destruct cases as [[-> grown] | [[k [-> grown]] | [[-> same] | [[-> same] | [-> same]]]]];
      cbn [bind fst snd].
    + destruct (IH (S i) s1) as [at_pre [at_post at_done]].
      split; [| split].
      * intros j stop. destruct (at_pre j stop) as [m [index [bound prefix]]].
        exists (S m). split; [lia |]. split; [cbn [length]; lia |].
        rewrite prefix, grown, <- app_assoc. reflexivity.
      * intros j k stop. destruct (at_post j k stop) as [m [index [bound prefix]]].
        exists (S m). split; [lia |]. split; [cbn [length]; lia |].
        rewrite prefix, grown, <- app_assoc. reflexivity.
      * intros done. rewrite (at_done done), grown, <- app_assoc. reflexivity.
    + split; [intros j stop; discriminate |].
      split; [| intros done; discriminate].
      intros j k' stop. injection stop as <- <-.
      exists 0. split; [lia |]. split; [cbn [length]; lia |]. exact grown.
    + split; [| split; [intros j k stop; discriminate | intros done; discriminate]].
      intros j stop. injection stop as <-.
      exists 0. split; [lia |]. split; [cbn [length]; lia |].
      rewrite same. cbn [firstn]. rewrite app_nil_r. reflexivity.
    + split; [intros j stop; discriminate |].
      split; [intros j k stop; discriminate | intros done; discriminate].
    + split; [intros j stop; discriminate |].
      split; [intros j k stop; discriminate | intros done; discriminate].
Qed.

Lemma host_ops_grow : forall ops i s,
  exists extra, bodies (fst (host_ops play_pairs i ops s)) = bodies s ++ extra.
Proof.
  induction ops as [| o rest IH]; intros i s.
  - exists []. cbn [host_ops fst]. rewrite app_nil_r. reflexivity.
  - cbn [host_ops].
    pose proof (host_op_cases i o s) as cases.
    destruct (host_op play_pairs i o s) as [s1 st1].
    cbn [fst snd] in cases.
    destruct cases as [[-> grown] | [[k [-> grown]] | [[-> same] | [[-> same] | [-> same]]]]];
      cbn [bind fst snd].
    + destruct (IH (S i) s1) as [extra more]. exists (o :: extra).
      rewrite more, grown, <- app_assoc. reflexivity.
    + exists [o]. exact grown.
    + exists []. rewrite app_nil_r. exact same.
    + exists []. rewrite app_nil_r. exact same.
    + exists []. rewrite app_nil_r. exact same.
Qed.

Lemma host_ops_never_diverge : forall ops i s p,
  snd (host_ops play_pairs i ops s) <> Diverged p.
Proof.
  induction ops as [| o rest IH]; intros i s p.
  - cbn [host_ops snd]. discriminate.
  - cbn [host_ops].
    pose proof (host_op_cases i o s) as cases.
    destruct (host_op play_pairs i o s) as [s1 st1].
    cbn [fst snd] in cases.
    destruct cases as [[-> _] | [[k [-> _]] | [[-> _] | [[-> _] | [-> _]]]]];
      cbn [bind snd]; [apply IH | discriminate ..].
Qed.

(* ── Replay pairing ─────────────────────────────────────────────────────── *)

(* The pairing check of a replay body matters only at its own index, and
   only after the observer accepts the calls of the body. So the replay of
   an operation equals its play when play does not apply its body, or when
   replay pairs it. *)
Lemma host_op_pairing : forall paired i o s,
  (bodies (fst (host_op play_pairs i o s)) = bodies s ++ [o] -> paired i = true) ->
  host_op paired i o s = host_op play_pairs i o s.
Proof.
  intros paired i o s applied_paired. unfold host_op.
  destruct (call_step (PReg i) (CReg o) s) as [s1 st1] eqn:reg_step.
  destruct st1; cbn [bind]; try reflexivity.
  destruct (charge (PPre i) (pre o) s1) as [s2 st2] eqn:pre_step.
  destruct st2; cbn [bind]; try reflexivity.
  unfold body.
  destruct (call_step (PObserve i) (CObserve o) s2) as [s3 st3] eqn:observe_step.
  destruct st3; cbn [bind]; try reflexivity.
  unfold play_pairs. destruct (paired i) eqn:pairing; [reflexivity | exfalso].
  assert (play_result :
    host_op play_pairs i o s = charge_all (post_charges i o) (run_body o s3)).
  { unfold host_op, body. rewrite reg_step. cbn [bind]. rewrite pre_step. cbn [bind].
    rewrite observe_step. cbn [bind play_pairs]. reflexivity. }
  pose proof (call_step_cases (PReg i) (CReg o) s) as [bodies1 _].
  pose proof (charge_cases (PPre i) (pre o) s1) as [bodies2 _].
  pose proof (call_step_cases (PObserve i) (CObserve o) s2) as [bodies3 _].
  pose proof (charge_all_post_cases i (post o) 0 (run_body o s3)) as [bodies4 _].
  rewrite reg_step in bodies1. rewrite pre_step in bodies2. rewrite observe_step in bodies3.
  cbn [fst] in bodies1, bodies2, bodies3.
  assert (applied : bodies (fst (host_op play_pairs i o s)) = bodies s ++ [o]).
  { rewrite play_result. unfold post_charges. rewrite bodies4.
    cbn [run_body bodies]. rewrite bodies3, bodies2, bodies1. reflexivity. }
  pose proof (applied_paired applied) as pairing_true. congruence.
Qed.

(* A replay that pairs each operation whose body play applied is play. *)
Lemma host_ops_pairing : forall ops paired i s,
  length (bodies s) = i ->
  (forall j, j < length (bodies (fst (host_ops play_pairs i ops s))) -> paired j = true) ->
  host_ops paired i ops s = host_ops play_pairs i ops s.
Proof.
  induction ops as [| o rest IH]; intros paired i s at_index paired_prefix.
  - reflexivity.
  - cbn [host_ops] in *.
    assert (op_paired :
      bodies (fst (host_op play_pairs i o s)) = bodies s ++ [o] -> paired i = true).
    { intros applied. apply paired_prefix. revert applied.
      destruct (host_op play_pairs i o s) as [s1 st1]. cbn [fst]. intros applied.
      destruct st1; cbn [bind fst];
        [destruct (host_ops_grow rest (S i) s1) as [extra more]; rewrite more |..];
        rewrite ?applied, !length_app, at_index; cbn [length]; lia. }
    rewrite (host_op_pairing paired i o s op_paired).
    pose proof (host_op_cases i o s) as cases.
    destruct (host_op play_pairs i o s) as [s1 st1].
    cbn [fst snd] in cases.
    destruct cases as [[-> grown] | [[k [-> _]] | [[-> _] | [[-> _] | [-> _]]]]];
      cbn [bind] in *; [| reflexivity ..].
    apply IH; [rewrite grown, length_app, at_index; cbn [length]; lia | exact paired_prefix].
Qed.

(* ── Headlines ──────────────────────────────────────────────────────────── *)

(* Q79. The space run and the host program have the same log, budget,
   status, calls and applied bodies. The status holds the stop point, so a
   stop at any charge, pre or post, in one run is the same stop in the
   other. The last three conjuncts give the applied prefix at each stop. *)
Theorem legacy_rebracket_eq_host :
  forall (ops : list op) (budget : Z) (log0 : list cost) (history : list call),
    let s0 := initial budget log0 history in
    let sp := space_run ops s0 in
    let hp := host_run ops s0 in
    legacy_log (fst sp) = legacy_log (fst hp) /\
    remaining (fst sp) = remaining (fst hp) /\
    snd sp = snd hp /\
    bodies (fst sp) = bodies (fst hp) /\
    calls (fst sp) = calls (fst hp) /\
    (forall i, snd hp = OutOfPhlo (PPre i) ->
       i < length ops /\ bodies (fst hp) = firstn i ops) /\
    (forall i k, snd hp = OutOfPhlo (PPost i k) ->
       i < length ops /\ bodies (fst hp) = firstn (S i) ops) /\
    (snd hp = Done -> bodies (fst hp) = ops).
Proof.
  intros ops budget log0 history s0 sp hp.
  assert (same_run : sp = hp) by apply space_run_eq_host_run.
  rewrite same_run.
  destruct (host_ops_stops ops 0 s0) as [at_pre [at_post at_done]].
  split; [reflexivity |]. split; [reflexivity |]. split; [reflexivity |].
  split; [reflexivity |]. split; [reflexivity |].
  split; [| split].
  - intros i stop. destruct (at_pre i stop) as [m [index [bound prefix]]].
    subst i. split; [exact bound | exact prefix].
  - intros i k stop. destruct (at_post i k stop) as [m [index [bound prefix]]].
    subst i. split; [exact bound | exact prefix].
  - intros done. exact (at_done done).
Qed.

(* Q80. Replay stages the operations of play (the first premise) and pairs
   each operation whose body play applied (the second premise). Both are
   facts of H23 in EventCorrelation.v. Then replay makes the charges of
   play and stops at the same point, and it never diverges. *)
Theorem legacy_replay_stops_with_play :
  forall (ops_play ops_replay : list op) (paired : nat -> bool)
         (budget : Z) (log0 : list cost) (history : list call),
    let s0 := initial budget log0 history in
    let pr := space_run ops_play s0 in
    ops_replay = ops_play ->
    (forall i, i < length (bodies (fst pr)) -> paired i = true) ->
    let rr := replay_run paired ops_replay s0 in
    legacy_log (fst rr) = legacy_log (fst pr) /\
    remaining (fst rr) = remaining (fst pr) /\
    snd rr = snd pr /\
    bodies (fst rr) = bodies (fst pr) /\
    calls (fst rr) = calls (fst pr) /\
    (forall p, snd rr <> Diverged p).
Proof.
  intros ops_play ops_replay paired budget log0 history s0 pr same_ops paired_prefix rr.
  assert (same_run : rr = pr).
  { subst rr pr. subst ops_replay. unfold replay_run, space_run.
    rewrite !space_run_in_flat.
    unfold space_run in paired_prefix. rewrite space_run_in_flat in paired_prefix.
    apply host_ops_pairing; [reflexivity | exact paired_prefix]. }
  rewrite same_run.
  split; [reflexivity |]. split; [reflexivity |]. split; [reflexivity |].
  split; [reflexivity |]. split; [reflexivity |].
  intros p. subst pr. unfold space_run. rewrite space_run_in_flat.
  apply host_ops_never_diverge.
Qed.

(* ── Negative controls ──────────────────────────────────────────────────── *)

Lemma charge_done_log : forall p c s s',
  charge p c s = (s', Done) -> legacy_log s' = legacy_log s ++ [c].
Proof.
  intros p c s s' charged. unfold charge in charged.
  destruct (Z.ltb (remaining s) 0); [discriminate |].
  destruct (Z.ltb (debit (remaining s) (value c)) 0); [discriminate |].
  injection charged as <-. reflexivity.
Qed.

Lemma charge_all_done_log : forall charges s s',
  charge_all charges s = (s', Done) -> legacy_log s' = legacy_log s ++ map snd charges.
Proof.
  induction charges as [| [p c] rest IH]; intros s s' charged.
  - cbn [charge_all] in charged. injection charged as <-. cbn [map]. rewrite app_nil_r.
    reflexivity.
  - cbn [charge_all] in charged.
    destruct (charge p c s) as [s1 st1] eqn:first_charge.
    destruct st1; cbn [bind] in charged; try discriminate.
    rewrite (IH s1 s' charged), (charge_done_log p c s s1 first_charge).
    cbn [map snd]. rewrite <- app_assoc. reflexivity.
Qed.

Lemma label_post_values : forall i k cs, map snd (label_post i k cs) = cs.
Proof.
  intros i k cs. revert k.
  induction cs as [| c rest IH]; intros k; cbn [label_post map snd]; [reflexivity |].
  rewrite IH. reflexivity.
Qed.

(* The tail carries the post-charges of the last staged operation. *)
Lemma stage_last_carry : forall ops o ch cbs ch',
  stage ch (ops ++ [o]) = (cbs, ch') ->
  chain_carry ch' = post_charges (chain_next ch + length ops) o.
Proof.
  induction ops as [| x rest IH]; intros o ch cbs ch' staged.
  - cbn [app stage attach] in staged. injection staged as <- <-.
    cbn [chain_carry length]. rewrite Nat.add_0_r. reflexivity.
  - cbn [app stage attach] in staged.
    destruct (stage (mkChain (S (chain_next ch)) (post_charges (chain_next ch) x))
                    (rest ++ [o]))
      as [cbs0 ch2] eqn:staged_rest.
    injection staged as <- <-.
    rewrite (IH o _ cbs0 ch2 staged_rest). cbn [chain_next length].
    rewrite Nat.add_succ_r. reflexivity.
Qed.

(* Without the tail (the job never calls LegacyChain::finish), a run that
   the host completes misses the post-charges of the last operation. *)
Definition space_run_without_tail (ops : list op) (s0 : state) : result :=
  commit play_pairs (fst (stage chain_new ops)) s0.

Lemma legacy_tail_dropped_breaks_eq : forall ops o s0,
  snd (host_run (ops ++ [o]) s0) = Done ->
  legacy_log (fst (host_run (ops ++ [o]) s0)) =
    legacy_log (fst (space_run_without_tail (ops ++ [o]) s0)) ++ post o /\
  (post o <> [] ->
   legacy_log (fst (space_run_without_tail (ops ++ [o]) s0)) <>
   legacy_log (fst (host_run (ops ++ [o]) s0))).
Proof.
  intros ops o s0 completed.
  rewrite <- space_run_eq_host_run in *.
  unfold space_run, space_run_in, space_run_without_tail in *.
  destruct (stage chain_new (ops ++ [o])) as [cbs ch] eqn:staged. cbn [fst] in *.
  destruct (commit play_pairs cbs s0) as [s1 st1] eqn:committed.
  destruct st1; cbn [bind snd] in completed |- *; try discriminate.
  unfold finish in *. rewrite (stage_last_carry ops o chain_new cbs ch staged) in *.
  destruct (charge_all (post_charges (chain_next chain_new + length ops) o) s1)
    as [s2 st2] eqn:tail.
  cbn [snd] in completed. subst st2. cbn [fst].
  rewrite (charge_all_done_log _ s1 s2 tail). unfold post_charges.
  rewrite label_post_values.
  split; [reflexivity |].
  intros nonempty same_log. apply nonempty.
  apply (f_equal (@length cost)) in same_log.
  rewrite length_app in same_log.
  destruct (post o) as [| c rest]; [reflexivity | cbn [length] in same_log; lia].
Qed.

(* The rejected order: the callback charges the carry and registers, and
   the pre-charge comes after the observer calls and the events of the
   operation (its body). *)
Definition before_observe_late_pre (cb : callback) (s : state) : result :=
  bind (charge_all (cb_carried cb) s) (fun s1 =>
  call_step (PReg (cb_index cb)) (CReg (cb_op cb)) s1).

Fixpoint commit_late_pre (cbs : list callback) (s : state) : result :=
  match cbs with
  | [] => (s, Done)
  | cb :: rest =>
      bind (before_observe_late_pre cb s) (fun s1 =>
      bind (body play_pairs (cb_index cb) (cb_op cb) s1) (fun s2 =>
      bind (charge (PPre (cb_index cb)) (pre (cb_op cb)) s2) (fun s3 =>
      commit_late_pre rest s3)))
  end.

Definition space_run_late_pre (ops : list op) (s0 : state) : result :=
  let (cbs, ch) := stage chain_new ops in
  bind (commit_late_pre cbs s0) (finish ch).

(* When the pre-charge of an operation runs out of phlo, the host stops
   before the body of the operation. The late pre-charge stops at the same
   charge, but the body has run, so the applied prefix is not the host's. *)
Lemma legacy_pre_after_observe_breaks_prefix : forall o budget log0 history,
  accept history (CReg o) = true ->
  accept (history ++ [CReg o]) (CObserve o) = true ->
  (0 <= budget)%Z ->
  (debit budget (value (pre o)) < 0)%Z ->
  let s0 := initial budget log0 history in
  snd (host_run [o] s0) = OutOfPhlo (PPre 0) /\
  bodies (fst (host_run [o] s0)) = [] /\
  snd (space_run_late_pre [o] s0) = OutOfPhlo (PPre 0) /\
  bodies (fst (space_run_late_pre [o] s0)) = [o].
Proof.
  intros o budget log0 history registered observed funded short s0.
  assert (not_spent : Z.ltb budget 0 = false) by (apply Z.ltb_ge; exact funded).
  assert (spent : Z.ltb (debit budget (value (pre o))) 0 = true)
    by (apply Z.ltb_lt; exact short).
  subst s0. unfold host_run, space_run_late_pre, initial.
  cbn [stage attach chain_new chain_next chain_carry host_ops commit_late_pre].
  unfold host_op, before_observe_late_pre, body, call_step, charge.
  cbn [remaining legacy_log calls bodies cb_index cb_op cb_carried charge_all bind].
  rewrite registered. cbn [bind remaining calls].
  rewrite not_spent, spent. cbn [bind fst snd bodies].
  rewrite observed. cbn [bind play_pairs run_body remaining bodies calls legacy_log].
  rewrite not_spent, spent. cbn [bind finish fst snd bodies app].
  repeat split.
Qed.

End Chain.

(* ── A computed instance of the negative controls ───────────────────────── *)

(* The operations have type unit. The costs have type Z, and the value of a
   cost is the cost. The subtraction is exact. The pre-charge is 10, the
   post list is [64], and the registry and the observer accept every
   call. *)
Definition unit_pre (_ : unit) : Z := 10%Z.

Definition unit_post (_ : unit) : list Z := [64%Z].

Definition accept_every (_ : list (call unit)) (_ : call unit) : bool := true.

(* With a budget of 100, the host charges 10 and 64, and the run without
   the tail charges only 10. With a budget of 5, the pre-charge runs out of
   phlo. The host applies no body, and the late pre-charge applies the
   body first. *)
Lemma legacy_negative_controls_computed :
  let host := host_run unit Z (fun c => c) Z.sub unit_pre unit_post accept_every in
  let without_tail :=
    space_run_without_tail unit Z (fun c => c) Z.sub unit_pre unit_post accept_every in
  let late_pre :=
    space_run_late_pre unit Z (fun c => c) Z.sub unit_pre unit_post accept_every in
  let funded := initial unit Z 100%Z [] [] in
  let short := initial unit Z 5%Z [] [] in
  legacy_log _ _ (fst (host [tt] funded)) = [10%Z; 64%Z] /\
  legacy_log _ _ (fst (without_tail [tt] funded)) = [10%Z] /\
  snd (host [tt] short) = OutOfPhlo (PPre 0) /\
  bodies _ _ (fst (host [tt] short)) = [] /\
  snd (late_pre [tt] short) = OutOfPhlo (PPre 0) /\
  bodies _ _ (fst (late_pre [tt] short)) = [tt].
Proof. cbv zeta. repeat split; vm_compute; reflexivity. Qed.

Print Assumptions legacy_rebracket_eq_host.
Print Assumptions legacy_replay_stops_with_play.
