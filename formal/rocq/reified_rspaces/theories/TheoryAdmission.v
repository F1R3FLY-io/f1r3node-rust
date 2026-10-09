(* ═══════════════════════════════════════════════════════════════════════════
   TheoryAdmission.v — the theory seam of the native spaces (P7 step 4)

   A factory call may name a builtin theory, free Th() with Th one of Nat,
   Int, String, Bool and Any (design v3 TH-4). The config of the space keeps
   the identity of the theory: its content, never a handle (TH-2). Since
   decision 74 the config holds the canonical bytes of the identity in one
   GByteArray. Each config read resolves the identity to a checker of this
   build (TH-3, TH-7). The checker admits each value of a send before any
   staging (TH-5), and it checks each stored datum that a receive matches,
   after the where guard (TH-6). A verdict other than Proven fails closed.
   A transaction with two or more theory leases aborts before its commit
   (TH-9). Each check reserves its one unit of work before it reads, the
   same on a hit and a miss and in play and replay (TH-8).

   This module proves the five headlines that design v3 names for the
   theory seam, and the codec of decision 74. Match.v proves that a theory
   refusal at a match is a guard refusal (TH-6), and SpaceFactory.v proves
   gate 4 (TH-4).

   Sources
   - Design: .reified-rspaces-work/p7/design-v3.md section 7 (TH-1 to TH-10)
     and section 9 (the headlines of this module).
   - Plan: decision 9.1 (the F1r3lang-shaped theory seam), decision 74 (the
     compact bytes of the identity in the config).
   - F1r3lang: grammar-core/src/theorem.rs (AdmissionDecision with Proven,
     Refuted and Undetermined; STRUCTURAL_ADMISSION_WORK_UNITS = 1; the
     usage of an Undetermined verdict is the usage of the check).

   ─────────────────────────────────────────────────────────────────────────
   Traceability: Rocq definition → Rust code
   (paths under rholang/src/rust/interpreter/spaces/ unless stated)
   ─────────────────────────────────────────────────────────────────────────
   shape, contains, table   theory/builtin.rs:59-67 (BuiltinKind::contains),
                            69-81 (admits_expr), 86-118 (sole) and 122-127
                            (non_negative: the sign of a GBigInt)
   usage, verdict           theory/mod.rs:37-42 (TheoryUsage) and 66-79
                            (TheoryVerdict)
   builtin_check            theory/builtin.rs:159-179 (BuiltinTheory::check:
                            a limit profile below one unit is Undetermined
                            before any reservation or read, else one
                            reserved unit and the shape decides)
   builtin_check_match      theory/builtin.rs:192-219 (check_match: every
                            value of the stored payload, one unit)
   admit_from, admit_values theory/mod.rs:144-168 (admit_values: the values
                            in order, the first verdict that is not Proven
                            aborts with its index, the leases of the Proven
                            verdicts in order)
   commit_allowed           generic.rs:2206-2211 (check_leases: two or more
                            leases abort with NestedCommitGuard)
   cached_check, consistent the cache-free runtime (design v3 RH-1, F18):
                            no cache exists, and a cache that a later build
                            adds must keep this headline
   identity                 theory/identity.rs:59-76 (TheoryIdentity)
   builtin_identity         theory/identity.rs:80-91 (TheoryIdentity::builtin)
   structural, resolve      theory/builtin.rs:139-153 (of_identity) and
                            theory/mod.rs:127-136 (resolve)
   to_bytes                 theory/identity.rs:128-163 (to_bytes) and
                            240-243 (push_length)
   take, read_len,          theory/identity.rs:252-260 (Reader::take),
     read_string, parse     266-268 (length), 271-274 (string) and the body
                            of from_bytes
   from_bytes               theory/identity.rs:169-202 (from_bytes: the
                            parse, no trailing byte, and the re-encoded
                            bytes equal to the input)
   play_send, replay_send   types/config.rs:193-206 (from_par_metered: each
                            config read decodes the slot and resolves the
                            identity) and generic.rs:2183-2201
                            (Admission::of: the admission of the values of
                            a send)

   ─────────────────────────────────────────────────────────────────────────
   Abstraction
   ─────────────────────────────────────────────────────────────────────────
   - A value is its shape for the builtins: a sole GInt with its integer, a
     sole GBigInt with its sign, a sole GString, a sole GBool, a sole
     expression of another kind, or a value that is not sole. sole (the
     shape test of builtin.rs) is the constructor of the shape, so table
     restates TH-1 over the shapes.
   - A check returns its verdict and the VerificationOperations units that
     it reserved before it read the value. The other dimensions stay 0 for a
     builtin (builtin.rs charges meter.charge(1, 0, 0)).
   - A string of an identity is its bytes (list nat). An arg is any type A
     with an encoder enc_arg and a decoder dec_arg (prost in the code) such
     that dec_arg inverts enc_arg. The fixed-width numbers of the bytes are
     section variables with their size and their round trip: enc_len and
     dec_len (a u32 length in 4 bytes), enc_u64 and dec_u64 (a limit in 8
     bytes), enc_i64 and dec_i64 (the version in 8 bytes). The headlines
     hold for every such codec, the big-endian codecs of the standard
     library included. codec_instance shows that the premises can hold.
   - A length is below len_bound (2^32 in the code). The code saturates a
     longer length, and its decoder then rejects the bytes. A field of an
     identity is a name or a small Par, far below 4 GiB.
   - Play and replay run the same admission. Play resolves the identity that
     the factory built. Replay reads the bytes of the config from the
     exported state and resolves their decoding.

   ─────────────────────────────────────────────────────────────────────────
   Results (6 headlines, axiom-free)
   ─────────────────────────────────────────────────────────────────────────
   TA1 builtin_decidable_structural  with a limit of at least one unit, each
                                     builtin decides every shape as TH-1
                                     states, never Undetermined, with the
                                     usage {1, 0} and one reserved unit; a
                                     match checks every stored value for
                                     one unit; a limit of 0 gives
                                     Undetermined and reserves nothing
   TA2 fail_closed                   a send is admitted exactly when every
                                     value is Proven; otherwise it aborts at
                                     its first value that is not, with the
                                     kind of that verdict; an admitted send
                                     has one lease set per value; a commit
                                     with two or more leases is refused
   TA3 usage_cache_transparent       a cache whose entries are results of
                                     the check gives the result of the check
                                     on a hit and a miss, stays consistent,
                                     and gives every result of a sequence
   TA4 resolve_identity_deterministic the resolution is a function of the
                                     content of the identity: exactly the
                                     structural builtins resolve, with any
                                     limit profile, and every identity that
                                     the factory builds resolves to its
                                     kind
   TA5 replay_preserves_theory_admission replay, which decodes the persisted
                                     bytes, admits or refuses each send as
                                     play does, at the same index and with
                                     the same reserved units
   TA6 identity_bytes_roundtrip      decision 74: the decoder inverts the
                                     encoder, it accepts only the image of
                                     the encoder, and the encoder is
                                     injective

   Rust correspondence of the extracted tests: rholang/tests/
   reified_theory.rs (prop_theory_builtin_decidable,
   prop_theory_fail_closed, prop_theory_usage_cache_transparent,
   prop_resolve_identity_deterministic,
   prop_replay_preserves_theory_admission,
   prop_theory_identity_bytes_roundtrip).

   No Axiom, Parameter, Admitted, admit, Abort or Conjecture. Section
   variables and hypotheses are parameters of the closed theorems. The file
   imports only the Rocq standard library.

   ─────────────────────────────────────────────────────────────────────────
   Failed strategies (do not try them again)
   ─────────────────────────────────────────────────────────────────────────
   - A concrete big-endian codec over nat needs the div and mod lemmas of
     four and eight digits in every proof of the codec. The section
     variables with their round trip keep the framing proof separate from
     the arithmetic of the standard library.
   ═══════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import List Bool Arith Lia PeanoNat ZArith.
From Stdlib Require Strings.String Strings.Ascii.
Import ListNotations.

(* ── 1. Shapes, kinds and verdicts ─────────────────────────────────────── *)

(* The shape of a value as the builtins see it (builtin.rs sole and
   admits_expr). *)
Inductive shape : Type :=
| SInt (z : Z)          (* a sole GInt *)
| SBig (nonneg : bool)  (* a sole GBigInt, with the sign of its bytes *)
| SString               (* a sole GString *)
| SBool                 (* a sole GBool *)
| SExpr                 (* a sole expression of another kind *)
| SNotSole.             (* a value that is not sole *)

Inductive kind : Type := KNat | KInt | KString | KBool | KAny.

(* BuiltinKind::contains. *)
Definition contains (k : kind) (v : shape) : bool :=
  match k with
  | KAny => true
  | KNat => match v with SInt z => Z.leb 0 z | SBig b => b | _ => false end
  | KInt => match v with SInt _ | SBig _ => true | _ => false end
  | KString => match v with SString => true | _ => false end
  | KBool => match v with SBool => true | _ => false end
  end.

(* TH-1 as the design table states it, independent of contains. *)
Definition table (k : kind) (v : shape) : Prop :=
  match k, v with
  | KAny, _ => True
  | KNat, SInt z => (0 <= z)%Z
  | KNat, SBig b => b = true
  | KInt, SInt _ => True
  | KInt, SBig _ => True
  | KString, SString => True
  | KBool, SBool => True
  | _, _ => False
  end.

Lemma contains_table : forall k v, contains k v = true <-> table k v.
Proof.
  intros [] []; cbn; try tauto; try (split; [discriminate | intros []]).
  - apply Z.leb_le.
Qed.

Record usage : Type := mkUsage { work_units : nat; evidence_bytes : nat }.

(* STRUCTURAL_ADMISSION_WORK_UNITS and no evidence. *)
Definition structural_usage : usage := mkUsage 1 0.

Inductive reason : Type := OutsideTheory | WorkBudgetExhausted (required available : nat).

Inductive verdict : Type :=
| Proven (u : usage)
| Refuted (r : reason) (u : usage)
| Undetermined (r : reason) (u : usage).

Definition is_proven (v : verdict) : bool := match v with Proven _ => true | _ => false end.

Definition usage_of (v : verdict) : usage :=
  match v with Proven u | Refuted _ u | Undetermined _ u => u end.

(* A check: its verdict and the units that it reserved before it read. *)
Definition check_result : Type := (verdict * nat)%type.

(* BuiltinTheory::check: a limit below one unit is Undetermined before any
   reservation or read; otherwise one unit is reserved and the shape
   decides. *)
Definition builtin_check (k : kind) (limit : nat) (v : shape) : check_result :=
  if limit <? 1 then (Undetermined (WorkBudgetExhausted 1 limit) structural_usage, 0)
  else (if contains k v then Proven structural_usage else Refuted OutsideTheory structural_usage, 1).

(* BuiltinTheory::check_match: every value of the stored payload, one unit. *)
Definition builtin_check_match (k : kind) (limit : nat) (vs : list shape) : check_result :=
  if limit <? 1 then (Undetermined (WorkBudgetExhausted 1 limit) structural_usage, 0)
  else (if forallb (contains k) vs then Proven structural_usage else Refuted OutsideTheory structural_usage, 1).

Lemma forallb_table : forall k vs, forallb (contains k) vs = true <-> Forall (table k) vs.
Proof.
  intros k vs. rewrite forallb_forall, Forall_forall. split.
  - intros H x Hx. apply contains_table. exact (H x Hx).
  - intros H x Hx. apply contains_table. exact (H x Hx).
Qed.

Lemma builtin_proven_iff :
  forall k limit v, 1 <= limit -> is_proven (fst (builtin_check k limit v)) = true <-> table k v.
Proof.
  intros k limit v Hl. unfold builtin_check.
  assert (E : (limit <? 1) = false) by (apply Nat.ltb_ge; exact Hl). rewrite E. cbn [fst].
  rewrite <- contains_table. destruct (contains k v); cbn; split; intros H;
    solve [reflexivity | discriminate H].
Qed.

(* TA1. With a limit of at least one unit, each builtin decides every shape as
   TH-1 states: Proven exactly on the shapes of the table, Refuted with
   OutsideTheory on every other shape, never Undetermined, with the usage
   {1, 0} and one reserved unit. A match checks every value of the stored
   payload for one unit. A limit of 0 gives Undetermined before any
   reservation. *)
Theorem builtin_decidable_structural :
  (forall k limit v, 1 <= limit ->
     snd (builtin_check k limit v) = 1 /\
     usage_of (fst (builtin_check k limit v)) = structural_usage /\
     (is_proven (fst (builtin_check k limit v)) = true <-> table k v) /\
     (fst (builtin_check k limit v) = Refuted OutsideTheory structural_usage <-> ~ table k v) /\
     (forall r u, fst (builtin_check k limit v) <> Undetermined r u)) /\
  (forall k limit vs, 1 <= limit ->
     snd (builtin_check_match k limit vs) = 1 /\
     (is_proven (fst (builtin_check_match k limit vs)) = true <-> Forall (table k) vs)) /\
  (forall k v vs,
     builtin_check k 0 v = (Undetermined (WorkBudgetExhausted 1 0) structural_usage, 0) /\
     builtin_check_match k 0 vs = (Undetermined (WorkBudgetExhausted 1 0) structural_usage, 0)).
Proof.
  split; [| split].
  - intros k limit v Hl. pose proof (builtin_proven_iff k limit v Hl) as P. unfold builtin_check in *.
    assert (E : (limit <? 1) = false) by (apply Nat.ltb_ge; exact Hl). rewrite E in *. cbn [fst snd] in *.
    destruct (contains k v) eqn:C.
    + assert (T : table k v) by (apply P; reflexivity).
      split; [reflexivity |]. split; [reflexivity |]. split; [exact P |].
      split; [split; [intros H; discriminate H | intros N; contradiction (N T)] |].
      intros r u H. discriminate H.
    + assert (NT : ~ table k v) by (intros T; apply P in T; discriminate T).
      split; [reflexivity |]. split; [reflexivity |]. split; [exact P |].
      split; [split; [intros _; exact NT | intros _; reflexivity] |].
      intros r u H. discriminate H.
  - intros k limit vs Hl. unfold builtin_check_match.
    assert (E : (limit <? 1) = false) by (apply Nat.ltb_ge; exact Hl). rewrite E. cbn [fst snd].
    split; [reflexivity |]. rewrite <- forallb_table.
    destruct (forallb (contains k) vs); cbn; split; intros H;
      solve [reflexivity | discriminate H].
  - intros k v vs. unfold builtin_check, builtin_check_match. cbn. split; reflexivity.
Qed.

(* ── 2. The admission of a send and the commit guard ─────────────────────── *)

Inductive admission : Type :=
| AllProven (leases : nat)          (* every value is Proven; the leases *)
| AbortRefuted (index : nat)        (* TheoryRefuted at the value index *)
| AbortUndetermined (index : nat).  (* TheoryUndetermined at the value index *)

Section Admission.
  Variable A : Type.           (* a value of a send *)
  Variable check : A -> verdict.
  Variable lease : nat.        (* commit_lease: the leases of one Proven verdict *)

  (* admit_values: the values in order from the index i. *)
  Fixpoint admit_from (i : nat) (vs : list A) : admission :=
    match vs with
    | [] => AllProven 0
    | v :: rest =>
        match check v with
        | Proven _ =>
            match admit_from (S i) rest with
            | AllProven n => AllProven (lease + n)
            | stop => stop
            end
        | Refuted _ _ => AbortRefuted i
        | Undetermined _ _ => AbortUndetermined i
        end
    end.

  Definition admit_values (vs : list A) : admission := admit_from 0 vs.

  (* check_leases: the commit of a transaction with n leases. *)
  Definition commit_allowed (n : nat) : bool := n <=? 1.

  Definition proven_value (v : A) : Prop := is_proven (check v) = true.

  Lemma admitted_inv :
    forall vs i n, admit_from i vs = AllProven n -> Forall proven_value vs /\ n = lease * length vs.
  Proof.
    induction vs as [| v rest IH]; intros i n H; cbn in H.
    - injection H as <-. split; [constructor | cbn; rewrite Nat.mul_0_r; reflexivity].
    - destruct (check v) eqn:C; try discriminate H.
      destruct (admit_from (S i) rest) as [m | k | k] eqn:R; try discriminate H.
      injection H as <-. destruct (IH (S i) m R) as [F E]. split.
      + constructor; [unfold proven_value; rewrite C; reflexivity | exact F].
      + cbn [length]. rewrite E, Nat.mul_succ_r. lia.
  Qed.

  Lemma admitted_intro :
    forall vs i, Forall proven_value vs -> admit_from i vs = AllProven (lease * length vs).
  Proof.
    induction vs as [| v rest IH]; intros i H; cbn.
    - rewrite Nat.mul_0_r. reflexivity.
    - inversion H as [| v' rest' Hv Hrest]; subst. unfold proven_value in Hv.
      destruct (check v) eqn:C; try discriminate Hv.
      rewrite (IH (S i) Hrest). f_equal. rewrite Nat.mul_succ_r. lia.
  Qed.

  Lemma stop_inv :
    forall vs i k, (admit_from i vs = AbortRefuted k \/ admit_from i vs = AbortUndetermined k) ->
      i <= k /\ exists v, nth_error vs (k - i) = Some v /\ is_proven (check v) = false /\
        (forall j w, j < k - i -> nth_error vs j = Some w -> is_proven (check w) = true).
  Proof.
    induction vs as [| v rest IH]; intros i k H; cbn in H.
    - destruct H as [H | H]; discriminate H.
    - destruct (check v) eqn:C.
      + assert (H' : admit_from (S i) rest = AbortRefuted k \/ admit_from (S i) rest = AbortUndetermined k).
        { destruct (admit_from (S i) rest) as [m | k' | k'];
            destruct H as [H | H]; try discriminate H; injection H as ->; [left | right]; reflexivity. }
        destruct (IH (S i) k H') as [Hle [w [Hn [Hp Hpre]]]].
        split; [lia |]. exists w.
        replace (k - i) with (S (k - S i)) by lia. cbn [nth_error].
        split; [exact Hn |]. split; [exact Hp |].
        intros j x Hj Hx. destruct j as [| j']; cbn [nth_error] in Hx.
        * injection Hx as <-. rewrite C. reflexivity.
        * apply (Hpre j' x); [lia | exact Hx].
      + destruct H as [H | H]; [injection H as <- | discriminate H].
        split; [lia |]. exists v. rewrite Nat.sub_diag. cbn [nth_error].
        split; [reflexivity |]. split; [rewrite C; reflexivity |]. intros j x Hj. lia.
      + destruct H as [H | H]; [discriminate H | injection H as <-].
        split; [lia |]. exists v. rewrite Nat.sub_diag. cbn [nth_error].
        split; [reflexivity |]. split; [rewrite C; reflexivity |]. intros j x Hj. lia.
  Qed.

  Lemma stop_intro :
    forall vs i j v, nth_error vs j = Some v ->
      (forall j' w, j' < j -> nth_error vs j' = Some w -> is_proven (check w) = true) ->
      (forall r u, check v = Refuted r u -> admit_from i vs = AbortRefuted (i + j)) /\
      (forall r u, check v = Undetermined r u -> admit_from i vs = AbortUndetermined (i + j)).
  Proof.
    induction vs as [| x rest IH]; intros i j v Hn Hpre.
    - destruct j; discriminate Hn.
    - destruct j as [| j'].
      + cbn [nth_error] in Hn. injection Hn as <-. cbn [admit_from].
        split; intros r u C; rewrite C; f_equal; lia.
      + cbn [nth_error] in Hn.
        assert (Px : is_proven (check x) = true) by (apply (Hpre 0 x); [lia | reflexivity]).
        assert (Pre' : forall j'' w, j'' < j' -> nth_error rest j'' = Some w -> is_proven (check w) = true).
        { intros j'' w Hj Hw. apply (Hpre (S j'') w); [lia | exact Hw]. }
        destruct (IH (S i) j' v Hn Pre') as [IHr IHu]. cbn [admit_from].
        destruct (check x) as [ux | rx ux | rx ux] eqn:Cx; try discriminate Px.
        split; intros r u C.
        * rewrite (IHr r u C). f_equal. lia.
        * rewrite (IHu r u C). f_equal. lia.
  Qed.

  (* TA2. A send is admitted exactly when every value is Proven, and then it
     holds one lease set per value. Otherwise it aborts at its first value
     that is not Proven, with TheoryRefuted for a Refuted verdict and
     TheoryUndetermined for an Undetermined one. A commit with two or more
     leases is refused. *)
  Theorem fail_closed :
    (forall vs n, admit_values vs = AllProven n -> Forall proven_value vs /\ n = lease * length vs) /\
    (forall vs, Forall proven_value vs -> admit_values vs = AllProven (lease * length vs)) /\
    (forall vs k, (admit_values vs = AbortRefuted k \/ admit_values vs = AbortUndetermined k) ->
       exists v, nth_error vs k = Some v /\ is_proven (check v) = false /\
         (forall j w, j < k -> nth_error vs j = Some w -> is_proven (check w) = true)) /\
    (forall vs j v, nth_error vs j = Some v ->
       (forall j' w, j' < j -> nth_error vs j' = Some w -> is_proven (check w) = true) ->
       (forall r u, check v = Refuted r u -> admit_values vs = AbortRefuted j) /\
       (forall r u, check v = Undetermined r u -> admit_values vs = AbortUndetermined j)) /\
    (forall n, commit_allowed n = true <-> n <= 1).
  Proof.
    split; [| split; [| split; [| split]]].
    - intros vs n H. exact (admitted_inv vs 0 n H).
    - intros vs H. exact (admitted_intro vs 0 H).
    - intros vs k H. destruct (stop_inv vs 0 k H) as [_ [v [Hn [Hp Hpre]]]].
      rewrite Nat.sub_0_r in Hn, Hpre. exists v. split; [exact Hn |]. split; [exact Hp |].
      intros j w Hj Hw. exact (Hpre j w Hj Hw).
    - intros vs j v Hn Hpre. exact (stop_intro vs 0 j v Hn Hpre).
    - intros n. unfold commit_allowed. apply Nat.leb_le.
  Qed.
End Admission.

(* ── 3. A cache of checks ────────────────────────────────────────────────── *)

Section Cache.
  Variable A : Type.
  Variable A_eq_dec : forall a b : A, {a = b} + {a <> b}.
  Variable check : A -> check_result.

  Definition cache : Type := list (A * check_result).

  Fixpoint lookup (c : cache) (a : A) : option check_result :=
    match c with
    | [] => None
    | (a', r) :: rest => if A_eq_dec a a' then Some r else lookup rest a
    end.

  (* Every entry of the cache is the result of the check. *)
  Definition consistent (c : cache) : Prop := forall a r, In (a, r) c -> r = check a.

  (* A hit returns the cached result; a miss checks and caches. *)
  Definition cached_check (c : cache) (a : A) : check_result * cache :=
    match lookup c a with
    | Some r => (r, c)
    | None => (check a, (a, check a) :: c)
    end.

  Fixpoint run_cached (c : cache) (l : list A) : list check_result :=
    match l with
    | [] => []
    | a :: rest => fst (cached_check c a) :: run_cached (snd (cached_check c a)) rest
    end.

  Lemma lookup_in : forall c a r, lookup c a = Some r -> In (a, r) c.
  Proof.
    induction c as [| [a' r'] rest IH]; intros a r H; cbn in H; [discriminate H |].
    destruct (A_eq_dec a a') as [-> | _].
    - injection H as <-. left. reflexivity.
    - right. exact (IH a r H).
  Qed.

  Lemma cached_check_spec :
    forall c a, consistent c -> fst (cached_check c a) = check a /\ consistent (snd (cached_check c a)).
  Proof.
    intros c a Hc. unfold cached_check.
    destruct (lookup c a) as [r |] eqn:L; cbn [fst snd].
    - split; [exact (Hc a r (lookup_in c a r L)) | exact Hc].
    - split; [reflexivity |]. intros a' r' [H | H].
      + injection H as <- <-. reflexivity.
      + exact (Hc a' r' H).
  Qed.

  (* TA3. A consistent cache gives the result of the check, its verdict and
     its reserved units, on a hit and a miss, and it stays consistent. So a
     sequence of checks through the cache gives the results of the checks.
     The empty cache is consistent. *)
  Theorem usage_cache_transparent :
    (forall c a, consistent c -> fst (cached_check c a) = check a /\ consistent (snd (cached_check c a))) /\
    (forall l c, consistent c -> run_cached c l = map check l) /\
    consistent [].
  Proof.
    split; [exact cached_check_spec |]. split.
    - induction l as [| a rest IH]; intros c Hc; cbn; [reflexivity |].
      destruct (cached_check_spec c a Hc) as [Hr Hc']. rewrite Hr, (IH _ Hc'). reflexivity.
    - intros a r H. destruct H.
  Qed.
End Cache.

(* ── 4. Identities and their resolution ───────────────────────────────────── *)

Definition bytes : Type := list nat.

Definition bytes_eqb (a b : bytes) : bool := if list_eq_dec Nat.eq_dec a b then true else false.

Lemma bytes_eqb_true : forall a b, bytes_eqb a b = true <-> a = b.
Proof.
  intros a b. unfold bytes_eqb. destruct (list_eq_dec Nat.eq_dec a b) as [-> | N]; split; intros H;
    solve [reflexivity | discriminate H | contradiction (N H)].
Qed.

(* The ASCII bytes of the names of the builtins (theory/identity.rs and
   theory/builtin.rs). *)
Module Names.
  Import (notations) Stdlib.Strings.String.
  Local Open Scope string_scope.
  Definition b (s : String.string) : list nat :=
    map Ascii.nat_of_ascii (String.list_ascii_of_string s).
  Definition language := b "rrs:builtin".
  Definition proc := b "Proc".
  Definition abi := b "rrs:structural:v1".
  Definition nat_name := b "Nat".
  Definition int_name := b "Int".
  Definition string_name := b "String".
  Definition bool_name := b "Bool".
  Definition any_name := b "Any".
End Names.

(* TheoryIdentity, with its args of any type A. *)
Record identity (A : Type) : Type := mkIdentity {
  id_version : Z;
  id_language : bytes;
  id_category : bytes;
  id_category_name : bytes;
  id_theorem : bytes;
  id_abi : bytes;
  id_work : nat;
  id_evidence : nat;
  id_args : list A
}.
Arguments mkIdentity {A}.
Arguments id_version {A}.
Arguments id_language {A}.
Arguments id_category {A}.
Arguments id_category_name {A}.
Arguments id_theorem {A}.
Arguments id_abi {A}.
Arguments id_work {A}.
Arguments id_evidence {A}.
Arguments id_args {A}.

(* BuiltinKind::of_name. *)
Definition kind_of_name (n : bytes) : option kind :=
  if bytes_eqb n Names.nat_name then Some KNat
  else if bytes_eqb n Names.int_name then Some KInt
  else if bytes_eqb n Names.string_name then Some KString
  else if bytes_eqb n Names.bool_name then Some KBool
  else if bytes_eqb n Names.any_name then Some KAny
  else None.

(* TheoryIdentity::builtin: the structural identity of a name, with the
   limit profile {1, 0}. *)
Definition builtin_identity {A} (name : bytes) : identity A :=
  mkIdentity 1%Z Names.language Names.proc Names.proc name Names.abi 1 0 [].

(* The identity with another limit profile. *)
Definition with_limits {A} (id : identity A) (w e : nat) : identity A :=
  mkIdentity (id_version id) (id_language id) (id_category id) (id_category_name id)
    (id_theorem id) (id_abi id) w e (id_args id).

(* BuiltinTheory::of_identity: the structural fields, no args, any limit
   profile. *)
Definition structural {A} (id : identity A) : bool :=
  Z.eqb (id_version id) 1 && bytes_eqb (id_language id) Names.language &&
  bytes_eqb (id_category id) Names.proc && bytes_eqb (id_category_name id) Names.proc &&
  bytes_eqb (id_abi id) Names.abi && match id_args id with [] => true | _ :: _ => false end.

(* resolve: a structural identity with a builtin theorem id. *)
Definition resolve {A} (id : identity A) : option kind :=
  if structural id then kind_of_name (id_theorem id) else None.

Lemma structural_spec :
  forall A (id : identity A), structural id = true <->
    id_version id = 1%Z /\ id_language id = Names.language /\ id_category id = Names.proc /\
    id_category_name id = Names.proc /\ id_abi id = Names.abi /\ id_args id = [].
Proof.
  intros A id. unfold structural. rewrite !andb_true_iff, Z.eqb_eq, !bytes_eqb_true.
  destruct (id_args id) as [| a rest]; split; intros H.
  - destruct H as [[[[[H1 H2] H3] H4] H5] _]. repeat split; assumption.
  - destruct H as [H1 [H2 [H3 [H4 [H5 _]]]]]. repeat split; assumption.
  - destruct H as [_ H6]. discriminate H6.
  - destruct H as [_ [_ [_ [_ [_ H6]]]]]. discriminate H6.
Qed.

(* TA4. The resolution is a function of the content of the identity alone,
   with no handle, token or epoch: exactly the structural identities with a
   builtin theorem id resolve, with any limit profile, a resolved identity
   has the structural fields and no args, and every identity that the
   factory builds for a builtin name resolves to the kind of the name. *)
Theorem resolve_identity_deterministic :
  forall A,
    (forall (id : identity A) k, resolve id = Some k <->
       structural id = true /\ kind_of_name (id_theorem id) = Some k) /\
    (forall (id : identity A) w e, resolve (with_limits id w e) = resolve id) /\
    (forall (id : identity A), resolve id <> None ->
       id_version id = 1%Z /\ id_language id = Names.language /\ id_category id = Names.proc /\
       id_category_name id = Names.proc /\ id_abi id = Names.abi /\ id_args id = []) /\
    (forall name, resolve (builtin_identity (A:=A) name) = kind_of_name name).
Proof.
  intros A. split; [| split; [| split]].
  - intros id k. unfold resolve. destruct (structural id); split; intros H.
    + split; [reflexivity | exact H].
    + exact (proj2 H).
    + discriminate H.
    + destruct H as [H _]. discriminate H.
  - intros id w e. reflexivity.
  - intros id H. unfold resolve in H. destruct (structural id) eqn:S; [| contradiction (H eq_refl)].
    exact (proj1 (structural_spec A id) S).
  - intros name. unfold resolve, builtin_identity. cbn. reflexivity.
Qed.

(* ── 5. Play and replay of a send ──────────────────────────────────────────── *)

(* The units that the checks of a send reserve: each check up to the first
   value that is not Proven. *)
Fixpoint units_until_stop (k : kind) (limit : nat) (vs : list shape) : nat :=
  match vs with
  | [] => 0
  | v :: rest =>
      if is_proven (fst (builtin_check k limit v))
      then snd (builtin_check k limit v) + units_until_stop k limit rest
      else snd (builtin_check k limit v)
  end.

Inductive send_outcome : Type :=
| SendDone (a : admission) (units : nat)  (* the admission and its reserved units *)
| SendUnavailable                         (* TheoryUnavailable at the config read *)
| SendMalformed.                          (* a config record that does not decode *)

(* Admission::of on a send: the resolved builtin admits the values with the
   limit of its identity. A builtin gives no lease. *)
Definition admit_send (r : option kind) (limit : nat) (vs : list shape) : send_outcome :=
  match r with
  | Some k =>
      SendDone (admit_values shape (fun v => fst (builtin_check k limit v)) 0 vs)
        (units_until_stop k limit vs)
  | None => SendUnavailable
  end.

(* ── 6. The canonical bytes of an identity (decision 74) ─────────────────── *)

Section Codec.
  Variable A : Type.                       (* an arg: a Par in the code *)
  Variable enc_arg : A -> bytes.           (* Message::encode_to_vec *)
  Variable dec_arg : bytes -> option A.    (* Par::decode *)
  Hypothesis dec_enc_arg : forall a, dec_arg (enc_arg a) = Some a.

  Variable len_bound : nat.                (* 2^32 *)
  Variable enc_len : nat -> bytes.         (* u32::to_be_bytes of a length *)
  Variable dec_len : bytes -> nat.         (* u32::from_be_bytes *)
  Hypothesis enc_len_size : forall n, length (enc_len n) = 4.
  Hypothesis dec_enc_len : forall n, n < len_bound -> dec_len (enc_len n) = n.

  Variable u64_bound : nat.                (* 2^64 *)
  Variable enc_u64 : nat -> bytes.         (* u64::to_be_bytes *)
  Variable dec_u64 : bytes -> nat.         (* u64::from_be_bytes *)
  Hypothesis enc_u64_size : forall n, length (enc_u64 n) = 8.
  Hypothesis dec_enc_u64 : forall n, n < u64_bound -> dec_u64 (enc_u64 n) = n.

  Variable version_ok : Z -> Prop.         (* an i64 *)
  Variable enc_i64 : Z -> bytes.           (* i64::to_be_bytes *)
  Variable dec_i64 : bytes -> Z.           (* i64::from_be_bytes *)
  Hypothesis enc_i64_size : forall z, length (enc_i64 z) = 8.
  Hypothesis dec_enc_i64 : forall z, version_ok z -> dec_i64 (enc_i64 z) = z.

  (* A string: its length, then its bytes. *)
  Definition enc_str (s : bytes) : bytes := enc_len (length s) ++ s.

  (* An arg: the length of its bytes, then its bytes. *)
  Definition enc_one (a : A) : bytes := enc_len (length (enc_arg a)) ++ enc_arg a.

  (* TheoryIdentity::to_bytes. *)
  Definition to_bytes (id : identity A) : bytes :=
    enc_i64 (id_version id) ++ enc_str (id_language id) ++ enc_str (id_category id) ++
    enc_str (id_category_name id) ++ enc_str (id_theorem id) ++ enc_str (id_abi id) ++
    enc_u64 (id_work id) ++ enc_u64 (id_evidence id) ++
    enc_len (length (id_args id)) ++ flat_map enc_one (id_args id).

  (* Reader::take. *)
  Definition take (n : nat) (bs : bytes) : option (bytes * bytes) :=
    if n <=? length bs then Some (firstn n bs, skipn n bs) else None.

  (* Reader::length. *)
  Definition read_len (bs : bytes) : option (nat * bytes) :=
    match take 4 bs with Some (h, t) => Some (dec_len h, t) | None => None end.

  (* Reader::string. The UTF-8 check of the code is not modeled: the names
     of an identity come from Rust strings. *)
  Definition read_str (bs : bytes) : option (bytes * bytes) :=
    match read_len bs with Some (n, t) => take n t | None => None end.

  (* The args of from_bytes: count times a length and the bytes of a Par. *)
  Fixpoint read_args (count : nat) (bs : bytes) : option (list A * bytes) :=
    match count with
    | O => Some ([], bs)
    | S c =>
        match read_str bs with
        | Some (raw, t) =>
            match dec_arg raw with
            | Some a =>
                match read_args c t with
                | Some (rest, t') => Some (a :: rest, t')
                | None => None
                end
            | None => None
            end
        | None => None
        end
    end.

  (* The parse of from_bytes: the fields in order, and no trailing byte. *)
  Definition parse (bs : bytes) : option (identity A) :=
    match take 8 bs with None => None | Some (v, bs1) =>
    match read_str bs1 with None => None | Some (lang, bs2) =>
    match read_str bs2 with None => None | Some (cat, bs3) =>
    match read_str bs3 with None => None | Some (catn, bs4) =>
    match read_str bs4 with None => None | Some (thm, bs5) =>
    match read_str bs5 with None => None | Some (abi, bs6) =>
    match take 8 bs6 with None => None | Some (w, bs7) =>
    match take 8 bs7 with None => None | Some (e, bs8) =>
    match read_len bs8 with None => None | Some (count, bs9) =>
    match read_args count bs9 with None => None | Some (args, rest) =>
    match rest with
    | [] => Some (mkIdentity (dec_i64 v) lang cat catn thm abi (dec_u64 w) (dec_u64 e) args)
    | _ :: _ => None
    end end end end end end end end end end end.

  (* TheoryIdentity::from_bytes: the parse, then the re-encoded bytes equal to
     the input. *)
  Definition from_bytes (bs : bytes) : option (identity A) :=
    match parse bs with
    | Some id => if bytes_eqb (to_bytes id) bs then Some id else None
    | None => None
    end.

  (* An identity that the codec can write: its version is an i64, its limits
     are u64 values, and every length fits a u32. *)
  Definition wf (id : identity A) : Prop :=
    version_ok (id_version id) /\ id_work id < u64_bound /\ id_evidence id < u64_bound /\
    length (id_language id) < len_bound /\ length (id_category id) < len_bound /\
    length (id_category_name id) < len_bound /\ length (id_theorem id) < len_bound /\
    length (id_abi id) < len_bound /\ length (id_args id) < len_bound /\
    Forall (fun a => length (enc_arg a) < len_bound) (id_args id).

  Lemma take_app : forall l r, take (length l) (l ++ r) = Some (l, r).
  Proof.
    intros l r. unfold take. rewrite length_app.
    assert (E : (length l <=? length l + length r) = true) by (apply Nat.leb_le; lia). rewrite E.
    rewrite firstn_app, firstn_all, Nat.sub_diag, firstn_O, app_nil_r.
    rewrite skipn_app, skipn_all, Nat.sub_diag, skipn_O. reflexivity.
  Qed.

  Lemma take_fixed : forall n l r, length l = n -> take n (l ++ r) = Some (l, r).
  Proof. intros n l r <-. apply take_app. Qed.

  Lemma read_len_enc : forall n r, n < len_bound -> read_len (enc_len n ++ r) = Some (n, r).
  Proof.
    intros n r Hn. unfold read_len. rewrite (take_fixed 4 (enc_len n) r (enc_len_size n)).
    rewrite (dec_enc_len n Hn). reflexivity.
  Qed.

  Lemma read_str_enc : forall s r, length s < len_bound -> read_str (enc_str s ++ r) = Some (s, r).
  Proof.
    intros s r Hs. unfold read_str, enc_str. rewrite <- app_assoc.
    rewrite (read_len_enc (length s) (s ++ r) Hs). apply take_app.
  Qed.

  Lemma read_args_enc :
    forall args r, Forall (fun a => length (enc_arg a) < len_bound) args ->
      read_args (length args) (flat_map enc_one args ++ r) = Some (args, r).
  Proof.
    induction args as [| a rest IH]; intros r H; [reflexivity |].
    inversion H as [| a' rest' Ha Hrest]; subst.
    cbn [length flat_map]. rewrite <- app_assoc. cbn [read_args].
    change (enc_one a) with (enc_str (enc_arg a)).
    rewrite (read_str_enc (enc_arg a) _ Ha), dec_enc_arg, (IH r Hrest). reflexivity.
  Qed.

  Lemma read_args_enc_nil :
    forall args, Forall (fun a => length (enc_arg a) < len_bound) args ->
      read_args (length args) (flat_map enc_one args) = Some (args, []).
  Proof.
    intros args H. rewrite <- (app_nil_r (flat_map enc_one args)). exact (read_args_enc args [] H).
  Qed.

  Lemma parse_to_bytes : forall id, wf id -> parse (to_bytes id) = Some id.
  Proof.
    intros [v lang cat catn thm abi w e args] Hwf.
    destruct Hwf as [Hv [Hw [He [Hl [Hc [Hcn [Ht [Ha [Hn Hargs]]]]]]]]].
    cbn [id_version id_language id_category id_category_name id_theorem id_abi id_work
      id_evidence id_args] in *.
    unfold parse, to_bytes.
    cbn [id_version id_language id_category id_category_name id_theorem id_abi id_work
      id_evidence id_args].
    rewrite (take_fixed 8 (enc_i64 v) _ (enc_i64_size v)). cbv beta iota.
    rewrite (read_str_enc lang _ Hl). cbv beta iota.
    rewrite (read_str_enc cat _ Hc). cbv beta iota.
    rewrite (read_str_enc catn _ Hcn). cbv beta iota.
    rewrite (read_str_enc thm _ Ht). cbv beta iota.
    rewrite (read_str_enc abi _ Ha). cbv beta iota.
    rewrite (take_fixed 8 (enc_u64 w) _ (enc_u64_size w)). cbv beta iota.
    rewrite (take_fixed 8 (enc_u64 e) _ (enc_u64_size e)). cbv beta iota.
    rewrite (read_len_enc (length args) _ Hn). cbv beta iota.
    rewrite (read_args_enc_nil args Hargs). cbv beta iota.
    rewrite (dec_enc_i64 v Hv), (dec_enc_u64 w Hw), (dec_enc_u64 e He). reflexivity.
  Qed.

  Lemma from_bytes_to_bytes : forall id, wf id -> from_bytes (to_bytes id) = Some id.
  Proof.
    intros id Hwf. unfold from_bytes. rewrite (parse_to_bytes id Hwf).
    try rewrite (proj2 (bytes_eqb_true (to_bytes id) (to_bytes id)) eq_refl). reflexivity.
  Qed.

  Lemma from_bytes_strict : forall bs id, from_bytes bs = Some id -> to_bytes id = bs.
  Proof.
    intros bs id H. unfold from_bytes in H. destruct (parse bs) as [id' |]; [| discriminate H].
    destruct (bytes_eqb (to_bytes id') bs) eqn:E; [| discriminate H]. injection H as <-.
    exact (proj1 (bytes_eqb_true _ _) E).
  Qed.

  (* TA6 (decision 74). The decoder inverts the encoder on every identity
     that the codec can write. It accepts only the image of the encoder, so
     a decoded identity encodes to its input, and decodes again. The encoder
     is injective. *)
  Theorem identity_bytes_roundtrip :
    (forall id, wf id -> from_bytes (to_bytes id) = Some id) /\
    (forall bs id, from_bytes bs = Some id -> to_bytes id = bs) /\
    (forall id1 id2, wf id1 -> wf id2 -> to_bytes id1 = to_bytes id2 -> id1 = id2) /\
    (forall bs id, from_bytes bs = Some id -> from_bytes (to_bytes id) = Some id).
  Proof.
    split; [exact from_bytes_to_bytes |]. split; [exact from_bytes_strict |]. split.
    - intros id1 id2 H1 H2 E. pose proof (from_bytes_to_bytes id1 H1) as R1.
      rewrite E, (from_bytes_to_bytes id2 H2) in R1. injection R1 as Heq. exact (eq_sym Heq).
    - intros bs id H. rewrite (from_bytes_strict bs id H). exact H.
  Qed.

  (* Play admits a send with the identity that the factory built. *)
  Definition play_send (id : identity A) (vs : list shape) : send_outcome :=
    admit_send (resolve id) (id_work id) vs.

  (* Replay decodes the config bytes of the exported state, then admits the
     journaled values of the send. *)
  Definition replay_send (config : bytes) (vs : list shape) : send_outcome :=
    match from_bytes config with
    | Some id => admit_send (resolve id) (id_work id) vs
    | None => SendMalformed
    end.

  (* TA5. Replay reads the bytes that the factory wrote, so it admits or
     refuses each send as play does, at the same index and with the same
     reserved units. An identity that names no checker of this build fails
     the send. A resolved builtin admits a send exactly when every value is
     in the theory, and a refusal names a value outside it. *)
  Theorem replay_preserves_theory_admission :
    (forall id vs, wf id -> replay_send (to_bytes id) vs = play_send id vs) /\
    (forall id vs, resolve id = None -> play_send id vs = SendUnavailable) /\
    (forall id vs k, resolve id = Some k -> 1 <= id_work id ->
       exists a u, play_send id vs = SendDone a u /\
         (a = AllProven 0 <-> Forall (table k) vs) /\
         (forall i, a = AbortRefuted i -> exists v, nth_error vs i = Some v /\ ~ table k v)).
  Proof.
    split; [| split].
    - intros id vs Hwf. unfold replay_send. rewrite (from_bytes_to_bytes id Hwf). reflexivity.
    - intros id vs H. unfold play_send, admit_send. rewrite H. reflexivity.
    - intros id vs k H Hl. unfold play_send, admit_send. rewrite H.
      set (chk := fun v => fst (builtin_check k (id_work id) v)).
      destruct (fail_closed shape chk 0) as [Hinv [Hintro [Hstop _]]].
      eexists. eexists. split; [reflexivity |]. split.
      + split.
        * intros Ha. destruct (Hinv vs 0 Ha) as [F _]. rewrite Forall_forall in F |- *.
          intros v Hv. apply (builtin_proven_iff k (id_work id) v Hl). exact (F v Hv).
        * intros F. replace 0 with (0 * length vs) at 2 by lia. apply Hintro.
          rewrite Forall_forall in F |- *. intros v Hv. unfold proven_value, chk.
          apply (builtin_proven_iff k (id_work id) v Hl). exact (F v Hv).
      + intros i Hi. destruct (Hstop vs i (or_introl Hi)) as [v [Hn [Hp _]]].
        exists v. split; [exact Hn |]. intros T.
        apply (builtin_proven_iff k (id_work id) v Hl) in T. unfold chk in Hp. rewrite T in Hp.
        discriminate Hp.
  Qed.
End Codec.

(* ── 7. The premises of the codec can hold ─────────────────────────────────── *)

(* A codec that meets every premise of the section Codec, with each byte an
   unbounded nat: the premises are consistent, so TA5 and TA6 are not
   vacuous. *)
Definition inst_enc_len (n : nat) : bytes := [n; 0; 0; 0].
Definition inst_dec_len (bs : bytes) : nat := hd 0 bs.
Definition inst_enc_u64 (n : nat) : bytes := [n; 0; 0; 0; 0; 0; 0; 0].
Definition inst_dec_u64 (bs : bytes) : nat := hd 0 bs.
Definition inst_enc_i64 (z : Z) : bytes := [Z.to_nat z; 0; 0; 0; 0; 0; 0; 0].
Definition inst_dec_i64 (bs : bytes) : Z := Z.of_nat (hd 0 bs).
Definition inst_version_ok (z : Z) : Prop := (0 <= z)%Z.
Definition inst_enc_arg (a : nat) : bytes := [a].
Definition inst_dec_arg (bs : bytes) : option nat := match bs with [a] => Some a | _ => None end.

Lemma codec_instance :
  (forall a, inst_dec_arg (inst_enc_arg a) = Some a) /\
  (forall n, length (inst_enc_len n) = 4) /\ (forall n, inst_dec_len (inst_enc_len n) = n) /\
  (forall n, length (inst_enc_u64 n) = 8) /\ (forall n, inst_dec_u64 (inst_enc_u64 n) = n) /\
  (forall z, length (inst_enc_i64 z) = 8) /\ (forall z, inst_version_ok z -> inst_dec_i64 (inst_enc_i64 z) = z).
Proof.
  repeat split; try reflexivity.
  intros z Hz. unfold inst_dec_i64, inst_enc_i64. cbn [hd]. apply Z2Nat.id. exact Hz.
Qed.

(* TA6 applies to the instance. *)
Corollary identity_bytes_roundtrip_instance :
  forall id : identity nat, wf nat inst_enc_arg 64 64 inst_version_ok id ->
    from_bytes nat inst_enc_arg inst_dec_arg inst_enc_len inst_dec_len inst_enc_u64 inst_dec_u64
      inst_enc_i64 inst_dec_i64 (to_bytes nat inst_enc_arg inst_enc_len inst_enc_u64 inst_enc_i64 id) =
    Some id.
Proof.
  destruct codec_instance as [H1 [H2 [H3 [H4 [H5 [H6 H7]]]]]]. intros id Hwf.
  exact (proj1 (identity_bytes_roundtrip nat inst_enc_arg inst_dec_arg H1 64 inst_enc_len inst_dec_len
    H2 (fun n _ => H3 n) 64 inst_enc_u64 inst_dec_u64 H4 (fun n _ => H5 n) inst_version_ok inst_enc_i64
    inst_dec_i64 H6 H7) id Hwf).
Qed.

(* The identity that the factory builds for Nat, through the instance: the
   definitions compute, and the bytes decode to the identity. *)
Example nat_identity_roundtrip :
  from_bytes nat inst_enc_arg inst_dec_arg inst_enc_len inst_dec_len inst_enc_u64 inst_dec_u64
    inst_enc_i64 inst_dec_i64
    (to_bytes nat inst_enc_arg inst_enc_len inst_enc_u64 inst_enc_i64 (builtin_identity Names.nat_name)) =
  Some (builtin_identity Names.nat_name).
Proof. vm_compute. reflexivity. Qed.

(* ── Headline assumptions ─────────────────────────────────────────────────── *)
Print Assumptions builtin_decidable_structural.
Print Assumptions fail_closed.
Print Assumptions usage_cache_transparent.
Print Assumptions resolve_identity_deterministic.
Print Assumptions replay_preserves_theory_admission.
Print Assumptions identity_bytes_roundtrip.
