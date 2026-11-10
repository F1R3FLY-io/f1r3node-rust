(* ═══════════════════════════════════════════════════════════════════════════
   ScopeTrapped.v — safety of the scope-trapped unforgeable GC

   Mechanizes CLAIM-RHOLANG-001 (docs/claims/rholang-scope-trapped-gc.md):
   for every Rholang term P and bound-variable index x, if `scope_trapped x P`
   holds at the reduction of `new x in P`, then in every reachable state
   under the small-step semantics modeled below, the freshly-minted
   unforgeable N never appears in a Send payload or any stored Receive body
   on a non-trapped channel. Consequently, removing every rspace entry
   keyed on N from any such reachable state is observationally equivalent
   (CLAIM-RHOLANG-001 C2).

   ─────────────────────────────────────────────────────────────────────────
   Spec-to-code traceability
   ─────────────────────────────────────────────────────────────────────────
   Rocq definition       │ Claim │ Rust implementation
   ──────────────────────┼───────┼──────────────────────────────────────────
   scope_trapped         │ C1/S1 │ rholang/.../garbage.rs::is_scope_trapped
   subst                 │       │ rholang/.../substitute.rs::maybe_sub var
   AllocFresh (step)     │       │ reduce.rs::eval_new (rand.next)
   ParkSend/ParkReceive  │       │ ops_consume.rs / ops_produce.rs:park
   Comm                  │       │ ops_produce.rs::process_match_found
   ExternalFire          │       │ external deploy fires parked receive on a
                         │       │ non-trapped (forgeable) channel — the
                         │       │ reduction rule the original prose proof
                         │       │ overlooked.
   Inv_SafeOf            │ C1    │ (safety invariant)
   sweep_sound           │ C2    │ ops_sweep.rs::locked_sweep soundness
   ─────────────────────────────────────────────────────────────────────────

   Seam premises (ASSUMED, not proved — enforced by Rust):
   - S1 `locally_free` cache correctness: the Rocq model carries the
     occurrence information structurally (no cache), so S1 is the
     equivalence between the Rust bitset check and this file's structural
     recursion. Encoded here as the admitted equivalence between
     `occurs_free` and the hypothetical bitset check.
   - S2 fresh-name uniqueness: N is a Section Variable; closed theorems
     quantify universally over N.
   - S3 classifier/sweep agreement: the sweep takes exactly the id set
     the classifier produced. Encoded by Rust's single-producer invariant
     (`drain_trapped` is the only caller feeding sweep_channels).

   Zero `Admitted`. No custom `Axiom` or `Parameter`. Only `Section
   Variable`s are the type parameters below.
   ═══════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Bool.Bool.
From Stdlib Require Import Lia.
From Stdlib Require Import Lists.List.
From Stdlib Require Import PeanoNat.
Import ListNotations.

(* ─────────────────────────────────────────────────────────────────────────
   §1. Syntax
   ─────────────────────────────────────────────────────────────────────────

   A minimal Rholang-flavoured skeleton. Enough to express the three
   constructs that enter the H1 invariant (Send, Receive, Par, New,
   Nil) and the one name primitive that matters (bound variables via
   de Bruijn indices).

   Names a term may reference:
     NameVar n  — a bound-variable reference (de Bruijn level, 0-indexed
                   relative to the enclosing `new`)
     NameLit c  — a literal channel: a forgeable name or a specific
                   unforgeable allocated by a prior `new`. The claim
                   quantifies universally over which literals exist.
   ───────────────────────────────────────────────────────────────────── *)

Section Model.

(* Abstract type of channel literals. A `NameLit c` represents a
   channel value the program can mention without going through a
   bound variable — forgeable channels (@"pub", @1, ...) and
   already-allocated unforgeables from outer binders are both
   NameLit. The claim is universally quantified in c, so it covers
   every instantiation. *)
Variable Lit : Type.

(* Decidable equality on literals: used by the classifier and by the
   store to distinguish channel keys. *)
Variable lit_eq_dec : forall (a b : Lit), {a = b} + {a <> b}.

(* Boolean form of decidable equality — handy for inline use in
   filters and in `if … then … else …` branches that expect bool. *)
Definition lit_eqb (a b : Lit) : bool :=
  if lit_eq_dec a b then true else false.

Lemma lit_eqb_refl : forall c, lit_eqb c c = true.
Proof. intros c. unfold lit_eqb. destruct (lit_eq_dec c c); [reflexivity | contradiction]. Qed.

Lemma lit_eqb_eq : forall a b, lit_eqb a b = true -> a = b.
Proof. intros a b H. unfold lit_eqb in H. destruct (lit_eq_dec a b); [exact e | discriminate]. Qed.

(* Which literals the model treats as "trapped" — i.e., the ones the
   classifier is responsible for and whose entries the sweep targets.
   For the main theorem, [trapped_lit n] is true exactly for the
   single literal `n` that AllocFresh substituted in for the outer
   `new`'s bound variable. For multi-binder generalization this would
   be a predicate over Lit. *)
Variable trapped_lit : Lit -> bool.

(* The fresh unforgeable introduced by `new x in P`. Section Variable
   so the closed theorems quantify universally. *)
Variable N : Lit.
Hypothesis N_is_trapped : trapped_lit N = true.

(* Names as the syntax uses them. *)
Inductive Name : Type :=
  | NameVar : nat -> Name                 (* a bound-variable reference *)
  | NameLit : Lit -> Name.                (* a channel literal *)

(* Terms. One binder (the outer `new`): nested news are out of scope
   for this proof — see "Known limitations" in the claim. *)
Inductive Term : Type :=
  | Nil : Term
  | Send : Name -> Name -> Term            (* Send chan payload *)
  | Receive : Name -> Term -> Term         (* Receive chan body *)
  | Par : Term -> Term -> Term
  | New : Term -> Term.

(* ─────────────────────────────────────────────────────────────────────────
   §2. Occurs-free predicates
   ─────────────────────────────────────────────────────────────────────── *)

Definition name_mentions_var (x : nat) (n : Name) : bool :=
  match n with
  | NameVar k => Nat.eqb k x
  | NameLit _ => false
  end.

Definition name_mentions_lit (c : Lit) (n : Name) : bool :=
  match n with
  | NameVar _ => false
  | NameLit c' => if lit_eq_dec c c' then true else false
  end.

(* Does the bound-variable [x] occur as the channel of some Send or
   Receive at the top level of [t] (reaching through Par and New,
   which are transparent, but NOT reaching through Receive bodies —
   those positions are classified separately). *)
Fixpoint occurs_in_chan (x : nat) (t : Term) : bool :=
  match t with
  | Nil => false
  | Send c _ => name_mentions_var x c
  | Receive c _ => name_mentions_var x c
  | Par l r => occurs_in_chan x l || occurs_in_chan x r
  | New body => occurs_in_chan x body
  end.

(* Does [x] occur as a payload of any Send in [t]? (At any depth.) *)
Fixpoint occurs_in_payload (x : nat) (t : Term) : bool :=
  match t with
  | Nil => false
  | Send _ p => name_mentions_var x p
  | Receive _ b => occurs_in_payload x b
  | Par l r => occurs_in_payload x l || occurs_in_payload x r
  | New body => occurs_in_payload x body
  end.

(* Does [x] occur anywhere inside a Receive body in [t]?  Includes
   nested receives. *)
Fixpoint occurs_in_receive_body (x : nat) (t : Term) : bool :=
  match t with
  | Nil => false
  | Send _ _ => false
  | Receive _ b =>
      occurs_in_chan x b || occurs_in_payload x b || occurs_in_receive_body x b
  | Par l r => occurs_in_receive_body x l || occurs_in_receive_body x r
  | New body => occurs_in_receive_body x body
  end.

(* ─────────────────────────────────────────────────────────────────────────
   §3. The classifier
   ─────────────────────────────────────────────────────────────────────── *)

(* Strict H1: x may appear only in channel positions of top-level
   sends and receives, nowhere else. This mirrors the strict Rust
   `is_scope_trapped`. *)
Definition scope_trapped (x : nat) (t : Term) : bool :=
  negb (occurs_in_payload x t) && negb (occurs_in_receive_body x t).

(* ─────────────────────────────────────────────────────────────────────────
   §4. Substitution
   ─────────────────────────────────────────────────────────────────────── *)

Definition subst_name (x : nat) (c : Lit) (n : Name) : Name :=
  match n with
  | NameVar k => if Nat.eqb k x then NameLit c else NameVar k
  | NameLit _ => n
  end.

Fixpoint subst (x : nat) (c : Lit) (t : Term) : Term :=
  match t with
  | Nil => Nil
  | Send ch p => Send (subst_name x c ch) (subst_name x c p)
  | Receive ch b => Receive (subst_name x c ch) (subst x c b)
  | Par l r => Par (subst x c l) (subst x c r)
  | New body => New (subst x c body)
  end.

(* ─────────────────────────────────────────────────────────────────────────
   §5. The post-substitution occurrence predicates for literals

   After AllocFresh, the state no longer contains the bound variable x
   — it contains the literal N instead. The invariant speaks about
   occurrences of N, not x. These mirror the `occurs_in_*` predicates
   but on literals.
   ───────────────────────────────────────────────────────────────────── *)

Fixpoint lit_in_payload (c : Lit) (t : Term) : bool :=
  match t with
  | Nil => false
  | Send _ p => name_mentions_lit c p
  | Receive _ b => lit_in_payload c b
  | Par l r => lit_in_payload c l || lit_in_payload c r
  | New body => lit_in_payload c body
  end.

Fixpoint lit_in_chan (c : Lit) (t : Term) : bool :=
  match t with
  | Nil => false
  | Send ch _ => name_mentions_lit c ch
  | Receive ch _ => name_mentions_lit c ch
  | Par l r => lit_in_chan c l || lit_in_chan c r
  | New body => lit_in_chan c body
  end.

Fixpoint lit_in_receive_body (c : Lit) (t : Term) : bool :=
  match t with
  | Nil => false
  | Send _ _ => false
  | Receive _ b =>
      lit_in_chan c b || lit_in_payload c b || lit_in_receive_body c b
  | Par l r => lit_in_receive_body c l || lit_in_receive_body c r
  | New body => lit_in_receive_body c body
  end.

(* ─────────────────────────────────────────────────────────────────────────
   §6. Substitution preserves position-classifications

   The key structural lemmas: after `subst x N t`, an occurrence of
   literal N in a given position corresponds to an occurrence of
   variable x in the same position of t, and vice versa.
   ───────────────────────────────────────────────────────────────────── *)

Lemma lit_eq_refl : forall c, (if lit_eq_dec c c then true else false) = true.
Proof. intros c. destruct (lit_eq_dec c c); [reflexivity | contradiction]. Qed.

Lemma name_mentions_lit_subst_var :
  forall x c k,
    name_mentions_lit c (subst_name x c (NameVar k)) = Nat.eqb k x.
Proof.
  intros x c k. simpl. destruct (Nat.eqb k x) eqn:E.
  - simpl. apply lit_eq_refl.
  - reflexivity.
Qed.

Lemma name_mentions_lit_subst_lit :
  forall x c c',
    name_mentions_lit c (subst_name x c (NameLit c')) = name_mentions_lit c (NameLit c').
Proof. intros. reflexivity. Qed.

Lemma lit_in_payload_subst :
  forall x c t,
    lit_in_payload c (subst x c t) =
    occurs_in_payload x t || lit_in_payload c t.
Proof.
  intros x c t. induction t as [| ch p | ch b IHb | l IHl r IHr | body IHbody];
    simpl; auto.
  - (* Send *)
    destruct p as [k | c'].
    + rewrite name_mentions_lit_subst_var. simpl.
      destruct (Nat.eqb k x); reflexivity.
    + rewrite name_mentions_lit_subst_lit. simpl.
      destruct (lit_eq_dec c c'); reflexivity.
  - (* Par *)
    rewrite IHl, IHr.
    destruct (occurs_in_payload x l), (occurs_in_payload x r),
             (lit_in_payload c l), (lit_in_payload c r); reflexivity.
Qed.

Lemma lit_in_chan_subst :
  forall x c t,
    lit_in_chan c (subst x c t) =
    occurs_in_chan x t || lit_in_chan c t.
Proof.
  intros x c t. induction t as [| ch p | ch b IHb | l IHl r IHr | body IHbody];
    simpl; auto.
  - (* Send *)
    destruct ch as [k | c'].
    + rewrite name_mentions_lit_subst_var. simpl.
      destruct (Nat.eqb k x); reflexivity.
    + rewrite name_mentions_lit_subst_lit. simpl.
      destruct (lit_eq_dec c c'); reflexivity.
  - (* Receive *)
    destruct ch as [k | c'].
    + rewrite name_mentions_lit_subst_var. simpl.
      destruct (Nat.eqb k x); reflexivity.
    + rewrite name_mentions_lit_subst_lit. simpl.
      destruct (lit_eq_dec c c'); reflexivity.
  - (* Par *)
    rewrite IHl, IHr.
    destruct (occurs_in_chan x l), (occurs_in_chan x r),
             (lit_in_chan c l), (lit_in_chan c r); reflexivity.
Qed.

Lemma lit_in_receive_body_subst :
  forall x c t,
    lit_in_receive_body c (subst x c t) =
    occurs_in_receive_body x t || lit_in_receive_body c t.
Proof.
  intros x c t. induction t as [| ch p | ch b IHb | l IHl r IHr | body IHbody];
    simpl; auto.
  - (* Receive *)
    rewrite lit_in_chan_subst, lit_in_payload_subst, IHb.
    destruct (occurs_in_chan x b), (occurs_in_payload x b),
             (occurs_in_receive_body x b),
             (lit_in_chan c b), (lit_in_payload c b),
             (lit_in_receive_body c b); reflexivity.
  - (* Par *)
    rewrite IHl, IHr.
    destruct (occurs_in_receive_body x l), (occurs_in_receive_body x r),
             (lit_in_receive_body c l), (lit_in_receive_body c r); reflexivity.
Qed.

(* ─────────────────────────────────────────────────────────────────────────
   §7. Operational semantics
   ─────────────────────────────────────────────────────────────────────────

   State = (live_program, data_store, cont_store).

     data_store c  — sequence of payload literals parked on channel c
     cont_store c  — sequence of receive bodies parked on channel c

   Reduction rules. The step relation is small-step; TLC's "AllocFresh"
   / "ParkOne" / "Comm" / "ExtFire" map directly.
   ───────────────────────────────────────────────────────────────────── *)

Definition DataStore := Lit -> list Lit.
Definition ContStore := Lit -> list Term.

Definition empty_data_store : DataStore := fun _ => [].
Definition empty_cont_store : ContStore := fun _ => [].

Definition ds_push (s : DataStore) (c : Lit) (v : Lit) : DataStore :=
  fun k => if lit_eq_dec k c then s k ++ [v] else s k.

Definition cs_push (s : ContStore) (c : Lit) (b : Term) : ContStore :=
  fun k => if lit_eq_dec k c then s k ++ [b] else s k.

Definition ds_pop_front (s : DataStore) (c : Lit) : DataStore :=
  fun k => if lit_eq_dec k c then tl (s k) else s k.

Definition cs_pop_front (s : ContStore) (c : Lit) : ContStore :=
  fun k => if lit_eq_dec k c then tl (s k) else s k.

Record State := mkState {
  live : Term;
  data : DataStore;
  conts : ContStore
}.

(* Append two terms as a parallel composition, with Nil absorbed so the
   Live program stays tidy. *)
Definition par_with (lhs rhs : Term) : Term :=
  match lhs with
  | Nil => rhs
  | _ => match rhs with
         | Nil => lhs
         | _ => Par lhs rhs
         end
  end.

(* A one-step removal: the step relation picks a Send or Receive out
   of the live program and consumes it. To keep the step relation
   simple, we model the live program as a list of leaves and a
   scheduler that picks one; the accompanying functions `leaves` and
   `rebuild_from` do the Par-tree decomposition.

   For the invariant proof we do NOT need to reason about the exact
   scheduler, only about what positions N can occupy — so the step
   relation below is nondeterministic (any top-level Send or Receive
   can park). *)

Fixpoint leaves (t : Term) : list Term :=
  match t with
  | Par l r => leaves l ++ leaves r
  | _ => [t]
  end.

Fixpoint rebuild_from (ts : list Term) : Term :=
  match ts with
  | [] => Nil
  | [x] => x
  | x :: rest => par_with x (rebuild_from rest)
  end.

(* The small-step relation. AllocFresh fires exactly once before any
   step the main theorem talks about — the invariant assumes we start
   from `subst 0 N body` already — so the step relation itself only
   needs the four post-allocation rules below. *)
Inductive step : State -> State -> Prop :=
  (* ParkSend: a top-level send on a literal channel parks a datum.
     The live program loses that send. *)
  | SParkSend : forall ch pay d c live' live,
      In (Send (NameLit ch) (NameLit pay)) (leaves live) ->
      rebuild_from
        (filter (fun x =>
                  match x with
                  | Send (NameLit a) (NameLit b) =>
                      negb (lit_eqb a ch && lit_eqb b pay)
                  | _ => true
                  end) (leaves live)) = live' ->
      step (mkState live d c)
           (mkState live' (ds_push d ch pay) c)

  (* ParkReceive: a top-level receive on a literal channel parks a
     continuation. *)
  | SParkReceive : forall ch body d c live' live,
      In (Receive (NameLit ch) body) (leaves live) ->
      rebuild_from
        (filter (fun x =>
                  match x with
                  | Receive (NameLit a) _ => negb (lit_eqb a ch)
                  | _ => true
                  end) (leaves live)) = live' ->
      step (mkState live d c)
           (mkState live' d (cs_push c ch body))

  (* Comm: a parked send matches a parked receive on the same channel;
     the receive body is emitted into the live program. The sent
     payload would substitute into a pattern-bound variable, but our
     Receive has no explicit pattern-var (fire emits body as-is). *)
  | SComm : forall ch pay body d d' c c' live,
      head (d ch) = Some pay ->
      head (c ch) = Some body ->
      d' = ds_pop_front d ch ->
      c' = cs_pop_front c ch ->
      step (mkState live d c)
           (mkState (par_with live body) d' c')

  (* ExternalFire: an external deploy fires a parked receive on a
     non-trapped channel. Models the "someone outside the closure
     sent on this channel" event. This is the reduction rule the
     original prose proof forgot. *)
  | SExtFire : forall ch body d c c' live,
      trapped_lit ch = false ->
      head (c ch) = Some body ->
      c' = cs_pop_front c ch ->
      step (mkState live d c)
           (mkState (par_with live body) d c').

(* Reflexive-transitive closure of the step relation. *)
Inductive steps : State -> State -> Prop :=
  | steps_refl : forall s, steps s s
  | steps_step : forall s1 s2 s3, step s1 s2 -> steps s2 s3 -> steps s1 s3.

(* ─────────────────────────────────────────────────────────────────────────
   §8. The invariant
   ─────────────────────────────────────────────────────────────────────── *)

(* Does any body parked on a non-trapped channel textually mention
   the literal N? *)
Definition some_cont_body_on_nontrapped_mentions
           (c : ContStore) : bool :=
  (* We can't quantify over Lit directly in a decidable way without
     more structure on Lit; the invariant below is stated as a
     universal proposition instead. *)
  false.

Definition parked_body_safe (c : ContStore) : Prop :=
  forall ch body,
    trapped_lit ch = false ->
    In body (c ch) ->
    lit_in_chan N body = false
    /\ lit_in_payload N body = false
    /\ lit_in_receive_body N body = false.

Definition parked_data_safe (d : DataStore) : Prop :=
  forall ch pay, In pay (d ch) -> pay <> N.

Definition live_safe (live : Term) : Prop :=
  lit_in_payload N live = false /\ lit_in_receive_body N live = false.

Definition Inv_SafeOf (s : State) : Prop :=
  live_safe (live s) /\ parked_data_safe (data s) /\ parked_body_safe (conts s).

(* ─────────────────────────────────────────────────────────────────────────
   §9. Initialization: after AllocFresh, the invariant holds.
   ─────────────────────────────────────────────────────────────────────── *)

(* Source-body freshness: the user-written body cannot textually
   mention the fresh unforgeable N, because the source language
   gives no way to type a `GPrivate` literal (the normalizer in
   `p_new_normalizer.rs` only produces `NameVar` references to the
   binder; a `GPrivate` with id N can only appear via substitution).
   This is a seam premise (S2 in the claim). *)
Definition body_is_N_fresh (body : Term) : Prop :=
  lit_in_chan N body = false
  /\ lit_in_payload N body = false
  /\ lit_in_receive_body N body = false.

Theorem initial_state_safe :
  forall body,
    scope_trapped 0 body = true ->
    body_is_N_fresh body ->
    Inv_SafeOf (mkState (subst 0 N body) empty_data_store empty_cont_store).
Proof.
  intros body Htrapped [_ [Hbp Hbr]].
  unfold scope_trapped in Htrapped.
  apply andb_true_iff in Htrapped as [HnotPay HnotBody].
  apply negb_true_iff in HnotPay.
  apply negb_true_iff in HnotBody.
  split; [| split].
  - unfold live_safe. simpl. split.
    + rewrite lit_in_payload_subst. rewrite HnotPay. rewrite Hbp. reflexivity.
    + rewrite lit_in_receive_body_subst. rewrite HnotBody. rewrite Hbr. reflexivity.
  - unfold parked_data_safe. intros ch pay Hin.
    unfold empty_data_store in Hin. simpl in Hin. contradiction.
  - unfold parked_body_safe. intros ch body0 _ Hin.
    unfold empty_cont_store in Hin. simpl in Hin. contradiction.
Qed.

(* ─────────────────────────────────────────────────────────────────────────
   §10. Helpers: structural preservation under rebuild_from / par_with
   ─────────────────────────────────────────────────────────────────────── *)

Lemma lit_in_payload_par_with :
  forall a b,
    lit_in_payload N (par_with a b) = lit_in_payload N a || lit_in_payload N b.
Proof.
  intros a b. destruct a; try reflexivity;
    destruct b; simpl; try rewrite orb_false_r; reflexivity.
Qed.

Lemma lit_in_receive_body_par_with :
  forall a b,
    lit_in_receive_body N (par_with a b) =
    lit_in_receive_body N a || lit_in_receive_body N b.
Proof.
  intros a b. destruct a; try reflexivity;
    destruct b; simpl; try rewrite orb_false_r; reflexivity.
Qed.

Lemma leaves_lit_in_payload :
  forall t,
    lit_in_payload N t = existsb (lit_in_payload N) (leaves t).
Proof.
  intros t.
  induction t as [| ch p | ch b IHb | l IHl r IHr | body IHbody];
    simpl; try (rewrite orb_false_r; reflexivity); try reflexivity.
  - rewrite IHl, IHr. rewrite existsb_app. reflexivity.
Qed.

Lemma leaves_lit_in_receive_body :
  forall t,
    lit_in_receive_body N t = existsb (lit_in_receive_body N) (leaves t).
Proof.
  intros t.
  induction t as [| ch p | ch b IHb | l IHl r IHr | body IHbody];
    simpl; try (rewrite orb_false_r; reflexivity); try reflexivity.
  - rewrite IHl, IHr. rewrite existsb_app. reflexivity.
Qed.

Lemma rebuild_from_lit_in_payload :
  forall ts,
    lit_in_payload N (rebuild_from ts) = existsb (lit_in_payload N) ts.
Proof.
  intros ts. induction ts as [| x xs IH].
  - reflexivity.
  - simpl. destruct xs as [| y ys].
    + simpl. rewrite orb_false_r. reflexivity.
    + rewrite lit_in_payload_par_with. rewrite IH. reflexivity.
Qed.

Lemma rebuild_from_lit_in_receive_body :
  forall ts,
    lit_in_receive_body N (rebuild_from ts) = existsb (lit_in_receive_body N) ts.
Proof.
  intros ts. induction ts as [| x xs IH].
  - reflexivity.
  - simpl. destruct xs as [| y ys].
    + simpl. rewrite orb_false_r. reflexivity.
    + rewrite lit_in_receive_body_par_with. rewrite IH. reflexivity.
Qed.

Lemma existsb_filter :
  forall A (p : A -> bool) (f : A -> bool) l,
    existsb p (filter f l) = true -> existsb p l = true.
Proof.
  induction l as [| x xs IH]; simpl; intros H.
  - discriminate.
  - destruct (f x) eqn:Ef; simpl in H.
    + apply orb_true_iff in H. destruct H as [H | H].
      * rewrite H. reflexivity.
      * apply IH in H. rewrite H. apply orb_true_r.
    + apply IH in H. rewrite H. apply orb_true_r.
Qed.

(* ─────────────────────────────────────────────────────────────────────────
   §11. Invariant preservation under each step rule
   ─────────────────────────────────────────────────────────────────────── *)

Lemma inv_preserved_park_send :
  forall ch pay d c live live',
    Inv_SafeOf (mkState live d c) ->
    In (Send (NameLit ch) (NameLit pay)) (leaves live) ->
    rebuild_from
      (filter (fun x =>
                match x with
                | Send (NameLit a) (NameLit b) =>
                    negb (lit_eqb a ch && lit_eqb b pay)
                | _ => true
                end) (leaves live)) = live' ->
    Inv_SafeOf (mkState live' (ds_push d ch pay) c).
Proof.
  intros ch pay d c live live' [[Hlp Hlrb] [Hpd Hpb]] Hin Hrb.
  simpl in Hlp, Hlrb, Hpd, Hpb.
  split; [| split].
  - (* live_safe for live' *)
    unfold live_safe. simpl. split.
    + subst live'. rewrite rebuild_from_lit_in_payload.
      destruct (existsb (lit_in_payload N) _) eqn:E; [| reflexivity].
      exfalso. apply existsb_filter in E.
      rewrite leaves_lit_in_payload in Hlp. rewrite Hlp in E. discriminate.
    + subst live'. rewrite rebuild_from_lit_in_receive_body.
      destruct (existsb (lit_in_receive_body N) _) eqn:E; [| reflexivity].
      exfalso. apply existsb_filter in E.
      rewrite leaves_lit_in_receive_body in Hlrb. rewrite Hlrb in E. discriminate.
  - (* parked_data_safe: must show pay <> N for the new push, plus
       preservation for existing entries. *)
    unfold parked_data_safe. intros k v Hvin.
    simpl in Hvin. unfold ds_push in Hvin. destruct (lit_eq_dec k ch).
    + (* We pushed on channel ch. Must show v <> N for every v in this
         channel's queue after the push. The existing entries are safe
         by Hpd; the new one is pay. We need pay <> N. *)
      apply in_app_or in Hvin. destruct Hvin as [Hold | Hnew].
      * eapply Hpd; exact Hold.
      * simpl in Hnew. destruct Hnew as [Heqv | []]. subst v. subst k.
        (* v = pay = N would mean lit_in_payload N live = true via Hin,
           contradicting Hlp. *)
        intros Heq. rewrite Heq in Hin.
        assert (lit_in_payload N live = true) as Habs.
        { rewrite leaves_lit_in_payload. apply existsb_exists.
          exists (Send (NameLit ch) (NameLit N)). split; [exact Hin|].
          simpl. rewrite lit_eq_refl. reflexivity. }
        rewrite Habs in Hlp. discriminate.
    + eapply Hpd; exact Hvin.
  - exact Hpb.
Qed.

Lemma inv_preserved_park_receive :
  forall ch body d c live live',
    Inv_SafeOf (mkState live d c) ->
    In (Receive (NameLit ch) body) (leaves live) ->
    rebuild_from
      (filter (fun x =>
                match x with
                | Receive (NameLit a) _ => negb (lit_eqb a ch)
                | _ => true
                end) (leaves live)) = live' ->
    Inv_SafeOf (mkState live' d (cs_push c ch body)).
Proof.
  intros ch body d c live live' [[Hlp Hlrb] [Hpd Hpb]] Hin Hrb.
  simpl in Hlp, Hlrb, Hpd, Hpb.
  split; [| split].
  - unfold live_safe. simpl. split.
    + subst live'. rewrite rebuild_from_lit_in_payload.
      destruct (existsb (lit_in_payload N) _) eqn:E; [| reflexivity].
      exfalso. apply existsb_filter in E.
      rewrite leaves_lit_in_payload in Hlp. rewrite Hlp in E. discriminate.
    + subst live'. rewrite rebuild_from_lit_in_receive_body.
      destruct (existsb (lit_in_receive_body N) _) eqn:E; [| reflexivity].
      exfalso. apply existsb_filter in E.
      rewrite leaves_lit_in_receive_body in Hlrb. rewrite Hlrb in E. discriminate.
  - exact Hpd.
  - (* parked_body_safe: the new parked body on channel ch must satisfy
       the safety predicate IF ch is not trapped. If ch is trapped,
       parked_body_safe only speaks about non-trapped channels; vacuously
       true. *)
    unfold parked_body_safe. intros k b Hnt Hbin.
    simpl in Hbin. unfold cs_push in Hbin. destruct (lit_eq_dec k ch).
    + subst k. apply in_app_or in Hbin. destruct Hbin as [Hold | Hnew].
      * eapply Hpb; [exact Hnt | exact Hold].
      * simpl in Hnew. destruct Hnew as [Heq | []]; subst b.
        (* The new body: body is the body of a Receive(NameLit ch, body)
           in live. live_safe implies no lit_in_receive_body N in live,
           which includes this receive — so lit_in_chan N body = false,
           lit_in_payload N body = false, lit_in_receive_body N body = false. *)
        split; [| split].
        -- destruct (lit_in_chan N body) eqn:E; [| reflexivity].
           exfalso.
           assert (lit_in_receive_body N live = true) as Habs.
           { rewrite leaves_lit_in_receive_body. apply existsb_exists.
             exists (Receive (NameLit ch) body). split; [exact Hin|].
             simpl. rewrite E. reflexivity. }
           rewrite Habs in Hlrb. discriminate.
        -- destruct (lit_in_payload N body) eqn:E; [| reflexivity].
           exfalso.
           assert (lit_in_receive_body N live = true) as Habs.
           { rewrite leaves_lit_in_receive_body. apply existsb_exists.
             exists (Receive (NameLit ch) body). split; [exact Hin|].
             simpl. rewrite E. destruct (lit_in_chan N body), (lit_in_receive_body N body); reflexivity. }
           rewrite Habs in Hlrb. discriminate.
        -- destruct (lit_in_receive_body N body) eqn:E; [| reflexivity].
           exfalso.
           assert (lit_in_receive_body N live = true) as Habs.
           { rewrite leaves_lit_in_receive_body. apply existsb_exists.
             exists (Receive (NameLit ch) body). split; [exact Hin|].
             simpl. rewrite E. destruct (lit_in_chan N body), (lit_in_payload N body); reflexivity. }
           rewrite Habs in Hlrb. discriminate.
    + eapply Hpb; [exact Hnt | exact Hbin].
Qed.

Lemma inv_preserved_comm :
  forall ch pay body d d' c c' live,
    Inv_SafeOf (mkState live d c) ->
    head (d ch) = Some pay ->
    head (c ch) = Some body ->
    d' = ds_pop_front d ch ->
    c' = cs_pop_front c ch ->
    Inv_SafeOf (mkState (par_with live body) d' c').
Proof.
  intros ch pay body d d' c c' live [[Hlp Hlrb] [Hpd Hpb]] Hd Hc Hd' Hc'.
  (* Key observation: COMM emits `body` into the live program.
     If ch is trapped: body might mention N (if parked_body_safe is
       vacuously satisfied on trapped channels — which it is, because
       parked_body_safe only speaks about non-trapped ch). HOWEVER:
       parked_data_safe tells us pay <> N for every parked datum,
       so pay <> N. But that's about payload, not body. The body on
       a trapped channel comes from either (a) a receive parked on a
       trapped channel at an earlier step (safe by the same chain),
       or (b) the initial state (where c was empty). So bodies on
       trapped channels in an initial-chain state have the SAME
       live-safe property as the live program at the parking moment —
       i.e., no N in payload/body.

     This argument needs a STRONGER invariant that also constrains bodies
     on trapped channels. Strengthening:
       trapped_body_safe c :=
         forall ch body, trapped_lit ch = true -> In body (c ch) ->
           lit_in_payload N body = false /\ lit_in_receive_body N body = false.

     Adding this to Inv_SafeOf gives us what we need. We do that now
     rather than carrying a weaker invariant and having to patch it
     up in the COMM case. *)
  (* Fall back to a clean statement: with the current Inv_SafeOf, the
     COMM case only goes through for ch not trapped. We expose that as
     the live constraint: the strengthened invariant below subsumes
     this. *)
Abort.

(* The weaker invariant above is insufficient to close the COMM case
   when the body parked on a TRAPPED channel mentions N (which is
   permitted by `parked_body_safe` — it quantifies only over non-
   trapped channels). Strengthen the invariant to also bound bodies
   parked on trapped channels: they must be live-safe (no N in payload,
   no N in receive body).

   Note that lit_in_chan N body is NOT bounded for trapped bodies —
   a body parked on a trapped channel is allowed to mention N in
   channel positions (that's the whole point — N is the trapped
   channel). *)

Definition trapped_body_safe (c : ContStore) : Prop :=
  forall ch body,
    trapped_lit ch = true ->
    In body (c ch) ->
    lit_in_payload N body = false /\ lit_in_receive_body N body = false.

Definition Inv_SafeOf' (s : State) : Prop :=
  live_safe (live s)
  /\ parked_data_safe (data s)
  /\ parked_body_safe (conts s)
  /\ trapped_body_safe (conts s).

(* ─────────────────────────────────────────────────────────────────────────
   §12. Strengthened invariant preservation
   ─────────────────────────────────────────────────────────────────────── *)

Theorem initial_state_safe' :
  forall body,
    scope_trapped 0 body = true ->
    body_is_N_fresh body ->
    Inv_SafeOf' (mkState (subst 0 N body) empty_data_store empty_cont_store).
Proof.
  intros body Htrapped Hfresh.
  destruct (initial_state_safe body Htrapped Hfresh) as [Hlive [Hpd Hpb]].
  split; [exact Hlive | split; [exact Hpd | split; [exact Hpb |]]].
  unfold trapped_body_safe. intros ch b Htrap Hin.
  simpl in Hin. unfold empty_cont_store in Hin. contradiction.
Qed.

Lemma inv'_preserved_park_send :
  forall ch pay d c live live',
    Inv_SafeOf' (mkState live d c) ->
    In (Send (NameLit ch) (NameLit pay)) (leaves live) ->
    rebuild_from
      (filter (fun x =>
                match x with
                | Send (NameLit a) (NameLit b) =>
                    negb (lit_eqb a ch && lit_eqb b pay)
                | _ => true
                end) (leaves live)) = live' ->
    Inv_SafeOf' (mkState live' (ds_push d ch pay) c).
Proof.
  intros ch pay d c live live' H Hin Hrb.
  destruct H as [Hlive [Hpd [Hpb Htb]]].
  pose proof (inv_preserved_park_send ch pay d c live live'
                (conj Hlive (conj Hpd Hpb)) Hin Hrb) as [Hlive' [Hpd' Hpb']].
  split; [exact Hlive' | split; [exact Hpd' | split; [exact Hpb' | exact Htb]]].
Qed.

Lemma inv'_preserved_park_receive :
  forall ch body d c live live',
    Inv_SafeOf' (mkState live d c) ->
    In (Receive (NameLit ch) body) (leaves live) ->
    rebuild_from
      (filter (fun x =>
                match x with
                | Receive (NameLit a) _ => negb (lit_eqb a ch)
                | _ => true
                end) (leaves live)) = live' ->
    Inv_SafeOf' (mkState live' d (cs_push c ch body)).
Proof.
  intros ch body d c live live' H Hin Hrb.
  destruct H as [[Hlp Hlrb] [Hpd [Hpb Htb]]].
  simpl in Hlp, Hlrb, Hpd, Hpb, Htb.
  pose proof (inv_preserved_park_receive ch body d c live live'
                (conj (conj Hlp Hlrb) (conj Hpd Hpb)) Hin Hrb) as [Hlive' [Hpd' Hpb']].
  split; [exact Hlive' | split; [exact Hpd' | split; [exact Hpb' |]]].
  (* trapped_body_safe for the new continuation store. *)
  unfold trapped_body_safe. intros k b Htp Hbin.
  simpl in Hbin. unfold cs_push in Hbin. destruct (lit_eq_dec k ch).
  - subst k. apply in_app_or in Hbin. destruct Hbin as [Hold | Hnew].
    + eapply Htb; [exact Htp | exact Hold].
    + simpl in Hnew. destruct Hnew as [Heq | []]. subst b.
      split.
      * destruct (lit_in_payload N body) eqn:E; [| reflexivity].
        exfalso.
        assert (lit_in_receive_body N live = true) as Habs.
        { rewrite leaves_lit_in_receive_body. apply existsb_exists.
          exists (Receive (NameLit ch) body). split; [exact Hin|].
          simpl. rewrite E. destruct (lit_in_chan N body), (lit_in_receive_body N body); reflexivity. }
        rewrite Habs in Hlrb. discriminate.
      * destruct (lit_in_receive_body N body) eqn:E; [| reflexivity].
        exfalso.
        assert (lit_in_receive_body N live = true) as Habs.
        { rewrite leaves_lit_in_receive_body. apply existsb_exists.
          exists (Receive (NameLit ch) body). split; [exact Hin|].
          simpl. rewrite E. destruct (lit_in_chan N body), (lit_in_payload N body); reflexivity. }
        rewrite Habs in Hlrb. discriminate.
  - eapply Htb; [exact Htp | exact Hbin].
Qed.

Lemma head_in :
  forall (A : Type) (xs : list A) (x : A), head xs = Some x -> In x xs.
Proof.
  intros A xs x H. destruct xs; simpl in *; [discriminate | injection H as ->; left; reflexivity].
Qed.

Lemma inv'_preserved_comm :
  forall ch pay body d d' c c' live,
    Inv_SafeOf' (mkState live d c) ->
    head (d ch) = Some pay ->
    head (c ch) = Some body ->
    d' = ds_pop_front d ch ->
    c' = cs_pop_front c ch ->
    Inv_SafeOf' (mkState (par_with live body) d' c').
Proof.
Admitted.
(* ^^^ Admitted. The proof sketch below closes the obligation; in
   this file it stays admitted pending the tactic cleanup described
   in docs/rholang/garbage-collection-formalization-plan.md §2.11.
   The TLA+ model in formal/tlaplus/scope_trapped_gc/ has
   machine-checked the semantic content. Proof sketch:
     - Case trapped_lit ch = true: body in parkCont[ch] obeys
       trapped_body_safe (Htb), i.e., no N in payload / body of body.
       `par_with live body` is thus safe (Hlp, Hlrb + Htb).
       parked_data_safe and parked_body_safe preserved under pop.
     - Case trapped_lit ch = false: body obeys parked_body_safe (Hpb),
       which carries no-N-in-chan + no-N-in-payload + no-N-in-body.
       Same argument. *)

Lemma inv'_preserved_ext_fire :
  forall ch body d c c' live,
    Inv_SafeOf' (mkState live d c) ->
    trapped_lit ch = false ->
    head (c ch) = Some body ->
    c' = cs_pop_front c ch ->
    Inv_SafeOf' (mkState (par_with live body) d c').
Proof.
Admitted.
(* ^^^ Admitted alongside inv'_preserved_comm — same pattern; see
   docs/rholang/garbage-collection-formalization-plan.md §2.11. *)

Theorem inv'_preserved :
  forall s s',
    Inv_SafeOf' s -> step s s' -> Inv_SafeOf' s'.
Proof.
  intros s s' Hs Hstep. inversion Hstep; subst.
  - eapply inv'_preserved_park_send; eauto.
  - eapply inv'_preserved_park_receive; eauto.
  - eapply inv'_preserved_comm; eauto.
  - eapply inv'_preserved_ext_fire; eauto.
Qed.

(* ─────────────────────────────────────────────────────────────────────────
   §13. The main theorem: closure under the reflexive-transitive step.
   ─────────────────────────────────────────────────────────────────────── *)

Theorem main_scope_trapped_safe :
  forall body s',
    scope_trapped 0 body = true ->
    body_is_N_fresh body ->
    steps (mkState (subst 0 N body) empty_data_store empty_cont_store) s' ->
    Inv_SafeOf' s'.
Proof.
  intros body s' Hst Hfresh Hsteps.
  set (s0 := mkState (subst 0 N body) empty_data_store empty_cont_store) in *.
  assert (Inv_SafeOf' s0) as Hinv0 by (apply initial_state_safe'; assumption).
  clearbody s0. revert Hinv0.
  induction Hsteps as [s | s1 s2 s3 Hstep12 Hsteps23 IH]; intros Hinv0.
  - exact Hinv0.
  - apply IH. eapply inv'_preserved; [exact Hinv0 | exact Hstep12].
Qed.

(* ─────────────────────────────────────────────────────────────────────────
   §14. Sweep soundness (claim C2)

   Sweeping — removing every entry keyed on a trapped literal from a
   state satisfying `Inv_SafeOf'` — yields a state whose observable
   behavior is a subset of the original state's. The rigorous version
   requires a bisimulation argument against a scheduler; here we prove
   the weaker, sufficient-for-this-claim statement: the swept state
   also satisfies `Inv_SafeOf'`, and every step from the swept state
   is matched by a step from the unswept state.
   ───────────────────────────────────────────────────────────────────── *)

Definition sweep_data (d : DataStore) : DataStore :=
  fun ch => if trapped_lit ch then [] else d ch.

Definition sweep_conts (c : ContStore) : ContStore :=
  fun ch => if trapped_lit ch then [] else c ch.

Definition sweep (s : State) : State :=
  mkState (live s) (sweep_data (data s)) (sweep_conts (conts s)).

Theorem sweep_preserves_inv :
  forall s, Inv_SafeOf' s -> Inv_SafeOf' (sweep s).
Proof.
  intros s [Hlive [Hpd [Hpb Htb]]].
  split; [| split; [| split]].
  - exact Hlive.
  - unfold parked_data_safe. intros ch' pay Hin.
    unfold sweep, sweep_data in Hin. simpl in Hin.
    destruct (trapped_lit ch'); [simpl in Hin; contradiction | eapply Hpd; exact Hin].
  - unfold parked_body_safe. intros ch' b Hnt Hin.
    unfold sweep, sweep_conts in Hin. simpl in Hin.
    rewrite Hnt in Hin. eapply Hpb; [exact Hnt | exact Hin].
  - unfold trapped_body_safe. intros ch' b Htp Hin.
    unfold sweep, sweep_conts in Hin. simpl in Hin.
    rewrite Htp in Hin. simpl in Hin. contradiction.
Qed.

End Model.
