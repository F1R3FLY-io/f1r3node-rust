(* ═══════════════════════════════════════════════════════════════════════════
   Match.v — the matcher of a space plan is the injected matcher (P5a)

   A plan of a native space chooses its fires with the matcher of its host
   store, never with a matcher of its own. The matcher has two parts: the
   spatial match of a pattern against a payload (SpaceMatcher::bind) and the
   where guard of a continuation over the bound values
   (SpaceMatcher::admits). Until the F1r3lang rule of P12, a space plan
   uses the current guard rule of this tree (decision 37), as the host
   does:
   - consume side: take the first record, in the canonical order, that
     matches spatially, and check the guard once. A refusal parks the
     continuation.
   - produce side: try the waiters in the canonical order. A waiter that
     matches spatially and whose guard admits fires. A guard refusal moves
     to the next waiter.
   A head-only discipline (queue, cell, stack) offers one candidate on each
   side, so both rules agree with the head driver there.

   This module models the choice of the first fire of the scan driver of
   bag and set (RECEIVE-SCAN and OFFER-SCAN of the P5 design v3, A.7), the
   choice of the head driver (offer_head and receive_head of the engine
   spine S1), and the choice of the host (ops_consume.rs and
   space_matcher.rs). It proves that the plans use only the injected
   matcher, that the scan driver chooses as the host chooses, and that the
   legacy WildcardMatch is the degenerate instance under which a pattern
   for 42 receives 43 (D6).

   Sources
   - Design: .reified-rspaces-work/p5/p5-design-v3.md A.7 (the scan driver
     pseudocode) and A.13 (the P5a row); p5-design-v2.md 2.8.1 (F17, F18,
     F19 and the F18 mutant "the guard checked on every candidate").
   - Plan: decision 37 (the current guard rule), decision 11 (head-only
     disciplines), decision 41 (the canonical order of bag and set).
   - Code: the engine spine S1 at commit 21b2b27fd. The scan driver of A.7
     is written by batch BAG in spaces/disciplines/scan.rs in the same wave,
     so this module models the design pseudocode of A.7, which is the
     reference.

   ─────────────────────────────────────────────────────────────────────────
   Traceability: Rocq definition → Rust code
   (paths under rholang/src/rust/interpreter/ unless stated)
   ─────────────────────────────────────────────────────────────────────────
   matcher, spatial, admits spaces/matching.rs:28-32, 52-63, 65-103
                            (SpaceMatcher: the injected matcher of the
                            host store, env.store.matcher(); bind and
                            admits call get_metered and
                            check_commit_metered, or get and check_commit)
   matches                  spaces/matching.rs:105-120 (SpaceMatcher::matches:
                            bind, then the guard) and spaces/generic.rs:
                            789-825 (Cx::bind, Cx::admits, Cx::matches)
   first_spatial,           the RECEIVE-SCAN step of A.7: the first record
     scan_receive           of canonical_data_order with a spatial match,
                            then admits once (spaces/disciplines/scan.rs,
                            batch BAG); the order is Cx::canonical_data_order
                            (spaces/generic.rs:774-779)
   first_unrefused,         the OFFER-SCAN step of A.7: the first waiter of
     scan_offer_loop,       canonical_waiter_order outside the refused set
     scan_offer             with a spatial match; a refusal joins the
                            refused set (spaces/disciplines/scan.rs, batch
                            BAG); the order is Cx::canonical_waiter_order
                            (spaces/generic.rs:781-787)
   head_receive, head_offer the head driver of S1: receive_head and
                            offer_head match the head with Cx::matches
                            (spaces/generic.rs:1524-1591, 1600-1684)
   host_consume             the host consume (rspace++/src/rspace/rspace/
                            ops_consume.rs:95-151): extract_data_candidates
                            takes the first datum of the bind that matches
                            spatially, then check_commit runs once, and a
                            refusal parks the continuation
   host_produce             the host produce (rspace++/src/rspace/
                            space_matcher.rs:155-219, extract_first_match):
                            the waiters in order; a spatial match whose
                            check_commit refuses rolls back and tries the
                            next waiter
   guard_filter_receive     the F1r3lang rule of P12 (decision 37, notes):
                            the first candidate that matches and passes the
                            guard. It is not implemented in this tree.
   wildcard_matcher         the legacy WildcardMatch (legacy formal
                            Match.v:95-120 and GenericRSpace.tla
                            "Matches == TRUE"): every pattern matches
                            every payload

   ─────────────────────────────────────────────────────────────────────────
   Abstraction
   ─────────────────────────────────────────────────────────────────────────
   - A payload, a pattern, a guard and the bound values are abstract types.
     The matcher is any pair of functions, so every theorem holds for every
     matcher.
   - The canonical order of DR-75 is the list order of the candidates. The
     theorems hold for every list, so they hold for every order.
   - A waiter has an id (its index in the staged view). The ids of one view
     are distinct. The scan driver keeps the refused waiters as a list of
     ids.
   - The model chooses the first fire of a scan. The later rounds of a scan
     (decision 30 and the fire rounds) start the same choice again over the
     staged view after the fire, so the choice of one round is the whole
     matching rule. GenericRSpace.v models the rounds.
   - The metering of the matcher calls (decision 42) is not modeled here.
     AgentReadCoverage.v models it.

   ─────────────────────────────────────────────────────────────────────────
   Results (3 headlines, axiom-free)
   ─────────────────────────────────────────────────────────────────────────
   F17 space_match_is_injected_match   every fire of a plan carries the
                                       spatial match of the injected
                                       matcher, which its guard admitted;
                                       a plan depends on the matcher only
                                       through its results; and no matcher
                                       is hard-wired: a refusing matcher
                                       parks or stores, an accepting
                                       matcher fires
   F18 guard_once_greedy_like_head     the scan driver chooses as the host
                                       chooses on both sides (decision 37),
                                       and on one candidate it chooses as
                                       the head driver and as the F1r3lang
                                       rule
   F19 wildcard_matcher_degenerate     under the legacy WildcardMatch, a
                                       pattern for 42 receives 43 on both
                                       sides, and the pattern plays no part
                                       in a choice; the real matcher parks
                                       and stores

   Rust correspondence of the extracted tests: rholang/tests/
   reified_engine_props.rs (prop_engine_match_is_the_injected_matcher,
   prop_engine_guard_checked_once_as_the_host_checks_it,
   prop_engine_wildcard_matcher_breaks_pattern_enforcement).

   No Axiom, Parameter, Admitted, admit, Abort or Conjecture. Section
   variables are parameters of the closed theorems. The file imports only
   the Rocq standard library.

   ─────────────────────────────────────────────────────────────────────────
   Failed strategies (do not try them again)
   ─────────────────────────────────────────────────────────────────────────
   - A refused set of waiters (not of ids) needs a decidable equality of
     the abstract types of patterns and guards. Ids keep the types
     abstract.
   ═══════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import List Bool Arith Lia PeanoNat.
Import ListNotations.

(* ── 1. The injected matcher ──────────────────────────────────────────── *)

Section Plans.
  Variable V : Type.   (* a payload: a datum without its envelope *)
  Variable P : Type.   (* the pattern of a bind *)
  Variable G : Type.   (* the where guard of a continuation *)
  Variable M : Type.   (* the bound values of a match *)

  (* SpaceMatcher: the spatial match and the guard of the host store. *)
  Record matcher : Type := mkMatcher {
    spatial : P -> V -> option M;
    admits : G -> M -> bool
  }.

  (* Cx::matches: the spatial match, then the guard. *)
  Definition matches (mt : matcher) (p : P) (g : G) (v : V) : option M :=
    match spatial mt p v with
    | Some m => if admits mt g m then Some m else None
    | None => None
    end.

  (* Two matchers that give the same results. *)
  Definition same_matcher (mt1 mt2 : matcher) : Prop :=
    (forall p v, spatial mt1 p v = spatial mt2 p v) /\
    (forall g m, admits mt1 g m = admits mt2 g m).

  (* The matcher that refuses every payload, and the matcher that binds every
     payload to m and admits every binding. *)
  Definition refusing : matcher := mkMatcher (fun _ _ => None) (fun _ _ => true).
  Definition accepting (m : M) : matcher := mkMatcher (fun _ _ => Some m) (fun _ _ => true).

  (* ── 2. The consume side ─────────────────────────────────────────────── *)

  Variable D : Type.          (* a stored record *)
  Variable payload : D -> V.  (* its payload: record::unwrap_datum *)

  Inductive receive_choice : Type :=
  | RFire (d : D) (m : M)     (* fire the continuation with d, binding m *)
  | RPark.                    (* park the continuation *)

  (* The first record, in the canonical order, that matches spatially. *)
  Fixpoint first_spatial (mt : matcher) (p : P) (l : list D) : option (D * M) :=
    match l with
    | [] => None
    | d :: rest =>
        match spatial mt p (payload d) with
        | Some m => Some (d, m)
        | None => first_spatial mt p rest
        end
    end.

  (* The F1r3lang rule of P12 (not implemented in this tree): the first
     record that matches spatially and passes the guard. *)
  Fixpoint guard_filter_receive (mt : matcher) (p : P) (g : G) (l : list D) : receive_choice :=
    match l with
    | [] => RPark
    | d :: rest =>
        match matches mt p g (payload d) with
        | Some m => RFire d m
        | None => guard_filter_receive mt p g rest
        end
    end.

  (* RECEIVE-SCAN (A.7, decision 37): the first spatial match, then the guard
     once. A refusal parks. *)
  Definition scan_receive (mt : matcher) (p : P) (g : G) (l : list D) : receive_choice :=
    match first_spatial mt p l with
    | Some (d, m) => if admits mt g m then RFire d m else RPark
    | None => RPark
    end.

  (* A record matches spatially. *)
  Definition spatially_matches (mt : matcher) (p : P) (d : D) : bool :=
    match spatial mt p (payload d) with
    | Some _ => true
    | None => false
    end.

  (* The host consume: the first datum of the bind that matches spatially
     (extract_data_candidates), then check_commit once. *)
  Definition host_consume (mt : matcher) (p : P) (g : G) (l : list D) : receive_choice :=
    match find (spatially_matches mt p) l with
    | Some d =>
        match spatial mt p (payload d) with
        | Some m => if admits mt g m then RFire d m else RPark
        | None => RPark
        end
    | None => RPark
    end.

  (* receive_head: the head record fires the continuation when Cx::matches
     accepts it. *)
  Definition head_receive (mt : matcher) (p : P) (g : G) (head : D) : receive_choice :=
    match matches mt p g (payload head) with
    | Some m => RFire head m
    | None => RPark
    end.

  (* ── 3. The produce side ─────────────────────────────────────────────── *)

  Variable W : Type.            (* a waiting continuation *)
  Variable w_id : W -> nat.     (* its index in the staged view *)
  Variable w_pat : W -> P.      (* the pattern of its one bind *)
  Variable w_guard : W -> G.    (* its where guard *)

  Inductive offer_choice : Type :=
  | OFire (w : W) (m : M)       (* the datum fires w, binding m *)
  | OStore.                     (* the datum is stored *)

  Definition refused_by (refused : list nat) (w : W) : bool :=
    existsb (Nat.eqb (w_id w)) refused.

  (* The first waiter, in the canonical order, outside the refused set that
     matches the datum spatially. *)
  Fixpoint first_unrefused (mt : matcher) (v : V) (refused : list nat) (ws : list W)
      : option (W * M) :=
    match ws with
    | [] => None
    | w :: rest =>
        if refused_by refused w then first_unrefused mt v refused rest
        else
          match spatial mt (w_pat w) v with
          | Some m => Some (w, m)
          | None => first_unrefused mt v refused rest
          end
    end.

  (* OFFER-SCAN (A.7, decision 37): refused ← ∅; the first unrefused spatial
     match fires when its guard admits; a refusal joins the refused set and
     the scan goes on. The fuel bounds the scan: each refusal adds a new
     waiter to the refused set. *)
  Fixpoint scan_offer_loop (fuel : nat) (mt : matcher) (v : V) (refused : list nat)
      (ws : list W) : offer_choice :=
    match fuel with
    | O => OStore
    | S fuel' =>
        match first_unrefused mt v refused ws with
        | None => OStore
        | Some (w, m) =>
            if admits mt (w_guard w) m then OFire w m
            else scan_offer_loop fuel' mt v (w_id w :: refused) ws
        end
    end.

  Definition scan_offer (mt : matcher) (v : V) (ws : list W) : offer_choice :=
    scan_offer_loop (S (length ws)) mt v [] ws.

  (* The host produce (extract_first_match): the waiters in order; the first
     that matches spatially and whose guard admits fires; a guard refusal
     moves to the next waiter. *)
  Fixpoint host_produce (mt : matcher) (v : V) (ws : list W) : offer_choice :=
    match ws with
    | [] => OStore
    | w :: rest =>
        match spatial mt (w_pat w) v with
        | Some m => if admits mt (w_guard w) m then OFire w m else host_produce mt v rest
        | None => host_produce mt v rest
        end
    end.

  (* offer_head: the datum fires the head waiter when Cx::matches accepts
     it. *)
  Definition head_offer (mt : matcher) (v : V) (head : W) : offer_choice :=
    match matches mt (w_pat head) (w_guard head) v with
    | Some m => OFire head m
    | None => OStore
    end.

  (* ── 4. Lemmas: the consume side ─────────────────────────────────────── *)

  Lemma matches_some :
    forall mt p g v m, matches mt p g v = Some m -> spatial mt p v = Some m /\ admits mt g m = true.
  Proof.
    intros mt p g v m H. unfold matches in H.
    destruct (spatial mt p v) as [m' |] eqn:E; [| discriminate].
    destruct (admits mt g m') eqn:A; [| discriminate].
    injection H as Hm. subst m'. split; first [reflexivity | assumption].
  Qed.

  Lemma first_spatial_find :
    forall mt p l,
      first_spatial mt p l =
      match find (spatially_matches mt p) l with
      | Some d => option_map (fun m => (d, m)) (spatial mt p (payload d))
      | None => None
      end.
  Proof.
    intros mt p l. induction l as [| d rest IH]; [reflexivity |].
    cbn [first_spatial find]. unfold spatially_matches at 1.
    destruct (spatial mt p (payload d)) as [m |] eqn:E.
    - cbn. rewrite E. reflexivity.
    - exact IH.
  Qed.

  Lemma scan_receive_host :
    forall mt p g l, scan_receive mt p g l = host_consume mt p g l.
  Proof.
    intros mt p g l. unfold scan_receive, host_consume.
    rewrite first_spatial_find.
    destruct (find (spatially_matches mt p) l) as [d |]; [| reflexivity].
    destruct (spatial mt p (payload d)) as [m |]; reflexivity.
  Qed.

  (* A fire of the host consume binds the spatial match of the injected
     matcher, which the injected guard admitted. *)
  Lemma host_consume_fire :
    forall mt p g l d m, host_consume mt p g l = RFire d m ->
      In d l /\ spatial mt p (payload d) = Some m /\ admits mt g m = true.
  Proof.
    intros mt p g l d m H. unfold host_consume in H.
    destruct (find (spatially_matches mt p) l) as [d' |] eqn:F; [| discriminate].
    destruct (spatial mt p (payload d')) as [m' |] eqn:E; [| discriminate].
    destruct (admits mt g m') eqn:A; [| discriminate].
    injection H as Hd Hm. subst d' m'.
    split; [exact (proj1 (find_some _ _ F)) | split; first [reflexivity | assumption]].
  Qed.

  (* The scan driver chooses as the host, so its fires carry the injected
     match too. *)
  Lemma scan_receive_fire :
    forall mt p g l d m, scan_receive mt p g l = RFire d m ->
      In d l /\ spatial mt p (payload d) = Some m /\ admits mt g m = true.
  Proof.
    intros mt p g l d m H. rewrite scan_receive_host in H. exact (host_consume_fire mt p g l d m H).
  Qed.

  Lemma scan_receive_one :
    forall mt p g d, scan_receive mt p g [d] = head_receive mt p g d.
  Proof.
    intros mt p g d. unfold scan_receive, head_receive, matches. cbn [first_spatial].
    destruct (spatial mt p (payload d)) as [m |]; [| reflexivity].
    destruct (admits mt g m); reflexivity.
  Qed.

  Lemma guard_filter_one :
    forall mt p g d, guard_filter_receive mt p g [d] = head_receive mt p g d.
  Proof.
    intros mt p g d. unfold head_receive. cbn [guard_filter_receive].
    destruct (matches mt p g (payload d)); reflexivity.
  Qed.

  (* ── 5. Lemmas: the produce side ─────────────────────────────────────── *)

  (* The waiters outside the refused set. *)
  Definition unrefused (refused : list nat) (ws : list W) : list W :=
    filter (fun w => negb (refused_by refused w)) ws.

  (* The refused waiters refuse the datum: a spatial match of a refused
     waiter fails its guard. *)
  Definition refusing_set (mt : matcher) (v : V) (refused : list nat) (ws : list W) : Prop :=
    forall w, In w ws -> refused_by refused w = true ->
      forall m, spatial mt (w_pat w) v = Some m -> admits mt (w_guard w) m = false.

  Lemma first_unrefused_some :
    forall mt v refused ws w m, first_unrefused mt v refused ws = Some (w, m) ->
      In w ws /\ refused_by refused w = false /\ spatial mt (w_pat w) v = Some m.
  Proof.
    intros mt v refused ws. induction ws as [| x rest IH]; intros w m H; [discriminate |].
    cbn [first_unrefused] in H. destruct (refused_by refused x) eqn:R.
    - destruct (IH w m H) as (Hin & Hr & Hs). split; [right; exact Hin | split; assumption].
    - destruct (spatial mt (w_pat x) v) as [m' |] eqn:E.
      + injection H as -> ->. split; [left; reflexivity | split; [exact R | exact E]].
      + destruct (IH w m H) as (Hin & Hr & Hs). split; [right; exact Hin | split; assumption].
  Qed.

  (* No unrefused spatial match, and every refused waiter refuses: the host
     stores. *)
  Lemma host_store_when_none :
    forall mt v refused ws,
      first_unrefused mt v refused ws = None -> refusing_set mt v refused ws ->
      host_produce mt v ws = OStore.
  Proof.
    intros mt v refused ws. induction ws as [| x rest IH]; intros H Href; [reflexivity |].
    assert (Href' : refusing_set mt v refused rest).
    { intros w Hin Hr m Hs. exact (Href w (or_intror Hin) Hr m Hs). }
    cbn [first_unrefused] in H. cbn [host_produce].
    destruct (refused_by refused x) eqn:R.
    - destruct (spatial mt (w_pat x) v) as [m |] eqn:E.
      + rewrite (Href x (or_introl eq_refl) R m E). exact (IH H Href').
      + exact (IH H Href').
    - destruct (spatial mt (w_pat x) v) as [m |] eqn:E; [discriminate |].
      exact (IH H Href').
  Qed.

  (* The first unrefused spatial match admits, and every refused waiter
     refuses: the host fires the same waiter with the same binding. *)
  Lemma host_fire_when_admits :
    forall mt v refused ws w m,
      first_unrefused mt v refused ws = Some (w, m) -> admits mt (w_guard w) m = true ->
      refusing_set mt v refused ws -> host_produce mt v ws = OFire w m.
  Proof.
    intros mt v refused ws. induction ws as [| x rest IH]; intros w m H A Href; [discriminate |].
    assert (Href' : refusing_set mt v refused rest).
    { intros u Hin Hr m' Hs. exact (Href u (or_intror Hin) Hr m' Hs). }
    cbn [first_unrefused] in H. cbn [host_produce].
    destruct (refused_by refused x) eqn:R.
    - destruct (spatial mt (w_pat x) v) as [m' |] eqn:E.
      + rewrite (Href x (or_introl eq_refl) R m' E). exact (IH w m H A Href').
      + exact (IH w m H A Href').
    - destruct (spatial mt (w_pat x) v) as [m' |] eqn:E.
      + injection H as -> ->. rewrite A. reflexivity.
      + exact (IH w m H A Href').
  Qed.

  Lemma filter_length_lt :
    forall (X : Type) (f g : X -> bool) (l : list X) (x : X),
      (forall y, g y = true -> f y = true) -> In x l -> f x = true -> g x = false ->
      length (filter g l) < length (filter f l).
  Proof.
    intros X f g l x Hgf. induction l as [| y rest IH]; intros Hin Hf Hg; [destruct Hin |].
    assert (Hle : forall l', length (filter g l') <= length (filter f l')).
    { intros l'. induction l' as [| z rest' IH']; [cbn; lia |].
      cbn. destruct (g z) eqn:Gz.
      - rewrite (Hgf z Gz). cbn. lia.
      - destruct (f z); cbn; lia. }
    destruct Hin as [-> | Hin].
    - cbn. rewrite Hf, Hg. cbn. pose proof (Hle rest). lia.
    - cbn. destruct (g y) eqn:Gy.
      + rewrite (Hgf y Gy). cbn. pose proof (IH Hin Hf Hg). lia.
      + destruct (f y); cbn; [pose proof (Hle rest) | pose proof (IH Hin Hf Hg)]; lia.
  Qed.

  Lemma in_map_injective_ids :
    forall (ws : list W) u w,
      NoDup (map w_id ws) -> In u ws -> In w ws -> w_id u = w_id w -> u = w.
  Proof.
    intros ws. induction ws as [| x rest IH]; intros u w Hnd Hu Hw Heq; [destruct Hu |].
    cbn in Hnd. inversion Hnd as [| a b Hnotin Hnd' Hab]; subst.
    destruct Hu as [<- | Hu]; destruct Hw as [<- | Hw].
    - reflexivity.
    - exfalso. apply Hnotin. rewrite Heq. apply in_map. exact Hw.
    - exfalso. apply Hnotin. rewrite <- Heq. apply in_map. exact Hu.
    - exact (IH u w Hnd' Hu Hw Heq).
  Qed.

  (* The scan driver and the host agree while the fuel covers the waiters
     outside the refused set. *)
  Lemma scan_offer_loop_host :
    forall fuel mt v refused ws,
      NoDup (map w_id ws) ->
      length (unrefused refused ws) < fuel ->
      refusing_set mt v refused ws ->
      scan_offer_loop fuel mt v refused ws = host_produce mt v ws.
  Proof.
    induction fuel as [| fuel IH]; intros mt v refused ws Hnd Hlen Href; [lia |].
    cbn [scan_offer_loop].
    destruct (first_unrefused mt v refused ws) as [[w m] |] eqn:F.
    - destruct (admits mt (w_guard w) m) eqn:A.
      + symmetry. exact (host_fire_when_admits mt v refused ws w m F A Href).
      + destruct (first_unrefused_some mt v refused ws w m F) as (Hin & Hr & Hs).
        apply IH; [exact Hnd | |].
        * assert (Hlt : length (unrefused (w_id w :: refused) ws) < length (unrefused refused ws)).
          { unfold unrefused. apply (filter_length_lt W _ _ ws w).
            - intros y Hy. apply negb_true_iff in Hy. apply negb_true_iff.
              unfold refused_by in *. cbn in Hy. apply orb_false_iff in Hy as [_ Hy]. exact Hy.
            - exact Hin.
            - rewrite Hr. reflexivity.
            - unfold refused_by. cbn. rewrite Nat.eqb_refl. reflexivity. }
          lia.
        * intros u Hu Hru m' Hs'. unfold refused_by in Hru. cbn in Hru.
          apply orb_true_iff in Hru as [Hid | Hru].
          -- apply Nat.eqb_eq in Hid. rewrite (in_map_injective_ids ws u w Hnd Hu Hin Hid) in Hs' |- *.
             rewrite Hs in Hs'. injection Hs' as <-. exact A.
          -- exact (Href u Hu Hru m' Hs').
    - symmetry. exact (host_store_when_none mt v refused ws F Href).
  Qed.

  Lemma scan_offer_host :
    forall mt v ws, NoDup (map w_id ws) -> scan_offer mt v ws = host_produce mt v ws.
  Proof.
    intros mt v ws Hnd. unfold scan_offer. apply scan_offer_loop_host; [exact Hnd | |].
    - unfold unrefused.
      pose proof (filter_length_le (fun w => negb (refused_by [] w)) ws) as Hle. lia.
    - intros w _ Hr. unfold refused_by in Hr. discriminate.
  Qed.

  Lemma scan_offer_loop_fire :
    forall fuel mt v refused ws w m, scan_offer_loop fuel mt v refused ws = OFire w m ->
      In w ws /\ spatial mt (w_pat w) v = Some m /\ admits mt (w_guard w) m = true.
  Proof.
    induction fuel as [| fuel IH]; intros mt v refused ws w m H; [discriminate |].
    cbn [scan_offer_loop] in H.
    destruct (first_unrefused mt v refused ws) as [[w' m'] |] eqn:F; [| discriminate].
    destruct (admits mt (w_guard w') m') eqn:A.
    - injection H as -> ->. destruct (first_unrefused_some mt v refused ws w m F) as (Hin & _ & Hs).
      split; [exact Hin | split; [exact Hs | exact A]].
    - exact (IH mt v (w_id w' :: refused) ws w m H).
  Qed.

  Lemma scan_offer_one :
    forall mt v w, scan_offer mt v [w] = head_offer mt v w.
  Proof.
    intros mt v w. unfold scan_offer, head_offer, matches. cbn [length scan_offer_loop first_unrefused].
    unfold refused_by at 1. cbn [existsb].
    destruct (spatial mt (w_pat w) v) as [m |]; [| reflexivity].
    destruct (admits mt (w_guard w) m); [reflexivity |].
    cbn [scan_offer_loop first_unrefused]. unfold refused_by. cbn. rewrite Nat.eqb_refl. reflexivity.
  Qed.

  (* ── 6. Lemmas: no matcher is hard-wired ─────────────────────────────── *)

  Lemma first_spatial_same :
    forall mt1 mt2 p l, same_matcher mt1 mt2 -> first_spatial mt1 p l = first_spatial mt2 p l.
  Proof.
    intros mt1 mt2 p l [Hs _]. induction l as [| d rest IH]; [reflexivity |].
    cbn [first_spatial]. rewrite Hs. destruct (spatial mt2 p (payload d)); [reflexivity | exact IH].
  Qed.

  Lemma scan_receive_same :
    forall mt1 mt2 p g l, same_matcher mt1 mt2 -> scan_receive mt1 p g l = scan_receive mt2 p g l.
  Proof.
    intros mt1 mt2 p g l Hsame. unfold scan_receive. rewrite (first_spatial_same mt1 mt2 p l Hsame).
    destruct Hsame as [_ Ha]. destruct (first_spatial mt2 p l) as [[d m] |]; [| reflexivity].
    rewrite Ha. reflexivity.
  Qed.

  Lemma first_unrefused_same :
    forall mt1 mt2 v refused ws, same_matcher mt1 mt2 ->
      first_unrefused mt1 v refused ws = first_unrefused mt2 v refused ws.
  Proof.
    intros mt1 mt2 v refused ws [Hs _]. induction ws as [| w rest IH]; [reflexivity |].
    cbn [first_unrefused]. destruct (refused_by refused w); [exact IH |].
    rewrite Hs. destruct (spatial mt2 (w_pat w) v); [reflexivity | exact IH].
  Qed.

  Lemma scan_offer_same :
    forall mt1 mt2 v ws, same_matcher mt1 mt2 -> scan_offer mt1 v ws = scan_offer mt2 v ws.
  Proof.
    intros mt1 mt2 v ws Hsame. unfold scan_offer. generalize (S (length ws)) as fuel.
    generalize (@nil nat) as refused.
    intros refused fuel. revert refused.
    induction fuel as [| fuel IH]; intros refused; [reflexivity |].
    cbn [scan_offer_loop]. rewrite (first_unrefused_same mt1 mt2 v refused ws Hsame).
    destruct (first_unrefused mt2 v refused ws) as [[w m] |]; [| reflexivity].
    destruct Hsame as [_ Ha]. rewrite Ha.
    destruct (admits mt2 (w_guard w) m); [reflexivity |]. apply IH.
  Qed.

  Lemma refusing_parks_and_stores :
    forall p g l v ws, scan_receive refusing p g l = RPark /\ scan_offer refusing v ws = OStore.
  Proof.
    intros p g l v ws. split.
    - unfold scan_receive. assert (first_spatial refusing p l = None) as ->; [| reflexivity].
      induction l as [| d rest IH]; [reflexivity | exact IH].
    - unfold scan_offer. generalize (S (length ws)) as fuel. generalize (@nil nat) as refused.
      intros refused fuel. destruct fuel as [| fuel]; [reflexivity |]. cbn [scan_offer_loop].
      assert (first_unrefused refusing v refused ws = None) as ->; [| reflexivity].
      induction ws as [| w rest IH]; [reflexivity |].
      cbn [first_unrefused]. destruct (refused_by refused w); exact IH.
  Qed.

  Lemma accepting_fires_first :
    forall m p g d rest v w ws,
      scan_receive (accepting m) p g (d :: rest) = RFire d m /\
      scan_offer (accepting m) v (w :: ws) = OFire w m.
  Proof.
    intros m p g d rest v w ws. split; [reflexivity |].
    unfold scan_offer. cbn [length scan_offer_loop first_unrefused].
    unfold refused_by at 1. cbn [existsb]. reflexivity.
  Qed.

  (* ── 7. Headlines ────────────────────────────────────────────────────── *)

  (* F17: the match of a plan is the injected match.
     1. A fire of the scan driver or of the head driver binds the spatial
        match that the injected matcher computes, and the injected guard
        admitted it. The fired partner is a candidate of the plan.
     2. A plan depends on the matcher only through its results.
     3. No matcher is hard-wired: under a refusing matcher every receive
        parks and every send stores, and under an accepting matcher the first
        candidate fires. *)
  Theorem space_match_is_injected_match :
    (forall mt p g l d m, scan_receive mt p g l = RFire d m ->
       In d l /\ spatial mt p (payload d) = Some m /\ admits mt g m = true) /\
    (forall mt v ws w m, scan_offer mt v ws = OFire w m ->
       In w ws /\ spatial mt (w_pat w) v = Some m /\ admits mt (w_guard w) m = true) /\
    (forall mt p g d d' m, head_receive mt p g d = RFire d' m ->
       d' = d /\ spatial mt p (payload d) = Some m /\ admits mt g m = true) /\
    (forall mt v w w' m, head_offer mt v w = OFire w' m ->
       w' = w /\ spatial mt (w_pat w) v = Some m /\ admits mt (w_guard w) m = true) /\
    (forall mt1 mt2, same_matcher mt1 mt2 ->
       (forall p g l, scan_receive mt1 p g l = scan_receive mt2 p g l) /\
       (forall v ws, scan_offer mt1 v ws = scan_offer mt2 v ws)) /\
    (forall p g l v ws, scan_receive refusing p g l = RPark /\ scan_offer refusing v ws = OStore) /\
    (forall m p g d rest v w ws,
       scan_receive (accepting m) p g (d :: rest) = RFire d m /\
       scan_offer (accepting m) v (w :: ws) = OFire w m).
  Proof.
    split; [exact scan_receive_fire |].
    split.
    { intros mt v ws w m H. unfold scan_offer in H. exact (scan_offer_loop_fire _ mt v [] ws w m H). }
    split.
    { intros mt p g d d' m H. unfold head_receive in H.
      destruct (matches mt p g (payload d)) as [m' |] eqn:E; [| discriminate].
      injection H as Hd Hm. subst d' m'. split; [reflexivity | exact (matches_some mt p g _ m E)]. }
    split.
    { intros mt v w w' m H. unfold head_offer in H.
      destruct (matches mt (w_pat w) (w_guard w) v) as [m' |] eqn:E; [| discriminate].
      injection H as Hw Hm. subst w' m'. split; [reflexivity | exact (matches_some mt _ _ v m E)]. }
    split.
    { intros mt1 mt2 Hsame. split.
      - intros p g l. exact (scan_receive_same mt1 mt2 p g l Hsame).
      - intros v ws. exact (scan_offer_same mt1 mt2 v ws Hsame). }
    split; [exact refusing_parks_and_stores | exact accepting_fires_first].
  Qed.

  (* F18: the guard is checked once, greedily, as the head driver checks it.
     1. RECEIVE-SCAN chooses as the host consume: the first spatial match,
        then the guard once, and a refusal parks.
     2. OFFER-SCAN chooses as the host produce: a guard refusal moves to the
        next waiter (the ids of one staged view are distinct).
     3. On one candidate, RECEIVE-SCAN, the F1r3lang rule and receive_head
        choose alike, and OFFER-SCAN and offer_head choose alike. So a
        head-only discipline keeps decision 37 with either rule. *)
  Theorem guard_once_greedy_like_head :
    (forall mt p g l, scan_receive mt p g l = host_consume mt p g l) /\
    (forall mt v ws, NoDup (map w_id ws) -> scan_offer mt v ws = host_produce mt v ws) /\
    (forall mt p g d,
       scan_receive mt p g [d] = head_receive mt p g d /\
       guard_filter_receive mt p g [d] = head_receive mt p g d) /\
    (forall mt v w, scan_offer mt v [w] = head_offer mt v w).
  Proof.
    split; [exact scan_receive_host |].
    split; [exact scan_offer_host |].
    split; [intros mt p g d; split; [apply scan_receive_one | apply guard_filter_one] |].
    exact scan_offer_one.
  Qed.
End Plans.

Arguments mkMatcher {V P G M} spatial admits.
Arguments spatial {V P G M} m _ _.
Arguments admits {V P G M} m _ _.
Arguments RFire {M D} d m.
Arguments RPark {M D}.
Arguments OFire {M W} w m.
Arguments OStore {M W}.

(* ── 8. The legacy WildcardMatch (D6) ───────────────────────────────────── *)

(* A pattern over integer payloads: an integer, or any payload. *)
Inductive int_pattern : Type := PInt (n : nat) | PAny.

(* The real matcher of the instance: PInt n matches exactly n. *)
Definition int_spatial (p : int_pattern) (v : nat) : option nat :=
  match p with
  | PInt n => if Nat.eqb n v then Some v else None
  | PAny => Some v
  end.

(* A continuation without a where guard admits every binding. *)
Definition no_guard (g : unit) (m : nat) : bool := true.

Definition real_matcher : matcher nat int_pattern unit nat := mkMatcher int_spatial no_guard.

(* The legacy WildcardMatch: matches := true. Every pattern binds every
   payload. *)
Definition wildcard_matcher : matcher nat int_pattern unit nat :=
  mkMatcher (fun (_ : int_pattern) (v : nat) => Some v) no_guard.

(* A record of the instance is its payload, and a waiter is its index and
   its pattern. *)
Definition int_payload (d : nat) : nat := d.
Definition int_waiter_id (w : nat * int_pattern) : nat := fst w.
Definition int_waiter_pat (w : nat * int_pattern) : int_pattern := snd w.
Definition int_waiter_guard (w : nat * int_pattern) : unit := tt.

Lemma wildcard_first_spatial :
  forall p l, first_spatial nat int_pattern unit nat nat int_payload wildcard_matcher p l =
              match l with [] => None | d :: _ => Some (d, d) end.
Proof. intros p l. destruct l; reflexivity. Qed.

(* F19: the legacy WildcardMatch is the degenerate instance. A pattern for
   42 receives 43 on the consume side and on the produce side, and the
   pattern plays no part in a choice. The real matcher parks and stores. *)
Theorem wildcard_matcher_degenerate :
  scan_receive nat int_pattern unit nat nat int_payload wildcard_matcher (PInt 42) tt [43]
    = RFire 43 43 /\
  scan_receive nat int_pattern unit nat nat int_payload real_matcher (PInt 42) tt [43] = RPark /\
  scan_offer nat int_pattern unit nat (nat * int_pattern) int_waiter_id int_waiter_pat
    int_waiter_guard wildcard_matcher 43 [(0, PInt 42)] = OFire (0, PInt 42) 43 /\
  scan_offer nat int_pattern unit nat (nat * int_pattern) int_waiter_id int_waiter_pat
    int_waiter_guard real_matcher 43 [(0, PInt 42)] = OStore /\
  (forall p1 p2 g l,
     scan_receive nat int_pattern unit nat nat int_payload wildcard_matcher p1 g l =
     scan_receive nat int_pattern unit nat nat int_payload wildcard_matcher p2 g l).
Proof.
  split; [reflexivity |].
  split; [reflexivity |].
  split; [reflexivity |].
  split; [reflexivity |].
  intros p1 p2 g l. unfold scan_receive. rewrite !wildcard_first_spatial. reflexivity.
Qed.

Print Assumptions space_match_is_injected_match.
Print Assumptions guard_once_greedy_like_head.
Print Assumptions wildcard_matcher_degenerate.
