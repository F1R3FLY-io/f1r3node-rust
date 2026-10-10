(* ═══════════════════════════════════════════════════════════════════════════
   SpaceRefs.v — space references, channel routing, the handle dispatch (P4),
   the routing of receive queries (P5b) and the routes and use-contexts of
   user agents (P8)

   This module models how the reducer of P4 finds the space of a channel and
   of a space reference. A space reference (`use s { P }`, `new x : s`,
   `getSpaceAgent!(ret, s)`) names a native space only through its handle
   id, bare or under a bundle with the write flag. A channel routes by its
   class (rules R1 to R5). System names, default objects, storage ids and
   other unforgeable names go to the default space. A space name goes to
   its own space under every context. An agent-tagged name goes to the
   agent. A forged channel goes to the space of the use-context, or to the
   default space outside every `use` block. The R7 backend of the system
   processes routes with no context. A body that the reducer stores under
   a context is wrapped in a USE tuple, so the context returns when the
   body runs. The handle process accepts a call only on a bare handle id,
   and replies only on a reply name. A receive with a QUERY tuple as its
   source routes the inner channel of the query (P5b): the tuple never
   takes the fast path, the inner channel takes the read rule of HEAD, and
   the default space rejects the query. P8 (step 1) adds the spaces of user
   agents: an agent route carries the agent reference and the raw channel,
   a use-context names a native space or an agent, a forged channel under
   the context of an agent routes to that agent, and a use block enters
   the context of an agent only when the bundle rule of eval_send gives
   the reference a call channel. P8 step 2 adds R7A: the R7 backend makes
   the call on the agent for the reply of a system process to an
   agent-tagged name, or to the bundle with both flags around one.

   Sources
   - Design: the P4 design report C.1 (routing), C.2 (space references and
     the use-context), C.6 (R7), C.8 (the handle process) and E.1 (the
     headlines of this module); the P5 design v3, A.8 (QUERY, the HOOKS
     row) and A.13 (the headline query_source_routes_inner_channel).
   - Plan: rules R1 to R7, A6, A23, decision 21 (the QUERY tuple) and I-20
     (the refresh before each strict decode).
   - P8 design v2 (plan decision 77): AG-1 (the route data), AG-2 (the
     use-context), AG-3 (agent identity) and AG-4 (the write rule).
   - Code: the current tree (P5). The line numbers below are the line
     numbers of that tree.

   ─────────────────────────────────────────────────────────────────────────
   Traceability: Rocq definition → Rust code
   ─────────────────────────────────────────────────────────────────────────
   kind_of_byte, decode_reified_id
                            rholang/src/rust/interpreter/spaces/namespace.rs:
                            340-370, 327-335 (decode_reified_id and
                            name_tail, copied from Namespace.v, which
                            proves their properties)
   space_scoped_id, handle_channel, name_id, op_channel
                            rholang/src/rust/interpreter/spaces/namespace.rs:
                            229-236, 255, 276-281, 284
   is_empty                 rholang/src/rust/interpreter/reified/scope.rs:103-130
   sole_unforgeable, sole_gprivate_id, reified_id_of
                            rholang/src/rust/interpreter/spaces/namespace.rs:
                            374-404, 407-414, 417-419
   single_bundle            models/src/rust/rholang/implicits.rs:65-81
   sole_expr                rholang/src/rust/interpreter/reified/route.rs:60-95
   is_agent_tagged          rholang/src/rust/interpreter/reified/route.rs:97-107
   classify_channel         rholang/src/rust/interpreter/reified/route.rs:
                            125-160
   key_of                   rholang/src/rust/interpreter/spaces/namespace.rs:
                            186-189, 206-212 (CollectionKey::of_channel and
                            canonical_inner_channel_bytes)
   route                    rholang/src/rust/interpreter/reified/route.rs:
                            173-225, 230-232 (R1 to R5, and route_channel)
   r7_produce, r7_of        rholang/src/rust/interpreter/reified/execution.rs:
                            175-200 (produce routes with no context)
   r7_agent_bundle          rholang/src/rust/interpreter/reified/execution.rs:
                            139-154 (r7_route, R7A-1)
   routes_to_default_       rholang/src/rust/interpreter/reified/
     without_context        reducer_hooks.rs:1295-1300
   classify_space_ref, unbundle
                            rholang/src/rust/interpreter/reified/scope.rs:
                            141-180
   wrap_body                rholang/src/rust/interpreter/reified/scope.rs:
                            185-193
   enter_use, fire_step     rholang/src/rust/interpreter/reified/
                            reducer_hooks.rs:254-271, 281-311, 437-452
                            (eval_marker, eval_use, space_ref),
                            rholang/src/rust/interpreter/dispatch.rs:62-71
                            (a fired body runs on the root reducer).
                            enter_use models eval_use of P8 step 4: a
                            reference of a user agent passes the write
                            rule and enters the context of the agent
   use_kind, target, ctx_key
                            rholang/src/rust/interpreter/reified/scope.rs:
                            34-59 (UseKind, UseTarget and its native)
   agent_parts              rholang/src/rust/interpreter/reified/route.rs:
                            109-123
   call_channel             rholang/src/rust/interpreter/reified/scope.rs:
                            166-180
   handle_of                rholang/src/rust/interpreter/reified/
                            system_channels.rs:96-101
   parse_message            rholang/src/rust/interpreter/reified/handle.rs:
                            151-180, 194-203, 883-919, 921-926, 928-933,
                            935-944, 953-958 (ParsedMessage::parse,
                            is_reply_name and the sole readers)
   method_of, arity         rholang/src/rust/interpreter/reified/handle.rs:
                            104-113, 116-122
   call, run                rholang/src/rust/interpreter/reified/handle.rs:
                            233-288, 290-347
   ensure_available, space_entry
                            rholang/src/rust/interpreter/reified/runtime.rs
                            (ReifiedRuntime::host, ensure_available, and
                            run_op, create_space and read_config, which call
                            host before submit_space), reified/
                            system_processes.rs (read_config of
                            getSpaceAgent)
   marker_of, sole_marker_tuple
                            models/src/rust/reified/encoding.rs:346-356,
                            369-374 (marker_of, sole_marker_tuple)
   decode_source, h4_passes rholang/src/rust/interpreter/reified/
                            reducer_hooks.rs:1280-1291 (reject_query_source),
                            models/src/rust/reified/encoding.rs:841-848
                            (decode_query_source)
   takes_fast_path          rholang/src/rust/interpreter/reified/
                            reducer_hooks.rs:1307-1309
   read_channel             rholang/src/rust/interpreter/reified/
                            reducer_hooks.rs:1339-1347 (readable_channel)
   read_bind, route_bind    rholang/src/rust/interpreter/reified/
                            reducer_hooks.rs:1320-1334, 865-885 (query_bind,
                            route_receive)
   single_target            rholang/src/rust/interpreter/reified/
                            reducer_hooks.rs:1352-1397 (single_bind_route)
   receive_one              rholang/src/rust/interpreter/reified/
                            reducer_hooks.rs:701-788 (reified_consume)

   ─────────────────────────────────────────────────────────────────────────
   Abstraction
   ─────────────────────────────────────────────────────────────────────────
   - A Par keeps what the readers look at. The field procs counts the
     sends, receives, news and matches. The field conns counts the
     connectives. The field conds counts the conditionals,
     cost_signed_terms and cost_stacks. No reader looks at locally_free.
     Only marker_of (section 8) looks at connective_used, as the marker_of
     of encoding.rs does. single_bundle (implicits.rs) does
     not look at conds, so a bundle next to a conditional still unbundles,
     as in HEAD's eval_send. The model keeps that rule.
   - The canonical bytes of a channel (bincode of the sorted term) are a
     section variable canon: every theorem holds for every serializer.
   - The USE tuple is the tuple (USE, space, body) with the marker name
     GPrivate([0xE0, 0xD0]) at its head. decode_use reads it, and
     Encoding.v proves the strict decoder of the code against its encoder.
   - The evaluation of a space reference (eval_expr, then
     substitute_and_charge) is a section variable eval_ref. The section
     hypothesis eval_space_ref states that a space reference, native or
     agent, is a value, which evaluation leaves as it is. The corollary
     use_wrapper_restores_ctx_instance discharges it.
   - The handle process: split_trailing_params, the config read, the
     param schema and the method bodies are section variables. The theorem
     covers the checks that run before a method body.
   - Receive queries: the strict decoder of the elements of a QUERY tuple
     (decode_query) is a section variable strict_query, and the query
     itself is a section type. Encoding.v proves the decoder against its
     encoder, so this module models only what the routing reads: the
     marker head, the inner channel, and whether the tuple decodes. The
     model covers a receive with one bind. A join with a query keeps the
     join errors of P4 (reducer_hooks.rs join_route) until P5c.

   Headline theorems (each one has a Print Assumptions line at the end)
    1. classify_space_ref_total_deterministic  the five rules give exactly
                                               one result
    2. handle_requires_write_flag         a native space needs a bare handle
                                          or a writable bundle of one
    3. reified_non_handle_not_a_space_ref other Reified ids are no agents
    4. route_total_deterministic          a route fails only on the key, and
                                          the context counts only by its
                                          native space
    5. route_rules                        R1 to R5
    6. space_name_route_ignores_ctx       a space name routes to its space
                                          under every context
    7. r7_route_ignores_ctx               R7 agrees with the reducer on every
                                          channel that is not forged, and
                                          sends a forged one to the default
                                          space, except the bundle of an
                                          agent-minted name (R7A)
    8. ordinary_channels_route_default    the channels of an ordinary
                                          program route to the default space
    9. use_wrapper_restores_ctx           a stored body runs in its context
   10. handle_dispatch_total              the handle process drops, aborts or
                                          replies on a reply name
   11. native_execution_runs_spaces       native play and a bound native replay
                                          runtime run a space entry, and an
                                          unbound one fails closed (decision 47)
   12. query_source_routes_inner_channel  a receive with a QUERY tuple routes
                                          the inner channel of the query, and
                                          a source without one keeps the
                                          receive of P4 (P5b)
   13. agent_ref_requires_write_flag      a use block enters the context of a
                                          user agent only when the reference
                                          has a call channel under the bundle
                                          rule of eval_send (P8, AG-4)
   14. r7_agent_reply_bundle_exact        the R7 backend calls the agent for
                                          exactly the bundle with both flags
                                          around an agent-tagged tuple (P8,
                                          R7A-1)
   15. tag_untag_id                       an agent-minted name unbundles to
                                          its agent-tagged tuple, which routes
                                          to its agent with its raw channel
                                          under every context (P8 step 4,
                                          TN-3)

   No Axiom, Parameter, Admitted, admit, Abort or Conjecture. Section
   variables are parameters of the closed theorems. The file imports only
   the Rocq standard library.

   ─────────────────────────────────────────────────────────────────────────
   Failed strategies (do not try them again)
   ─────────────────────────────────────────────────────────────────────────
   - `From Stdlib Require Import String` makes `length` mean String.length,
     and a `%string` literal needs the scope of an imported String. The
     module models strings as byte lists instead, as Namespace.v does.
   - `t1; t2; [a | b]` applies [a | b] to every goal of the chain ("expected
     6 tactics"). Put the split in parentheses: `(split; [a | b])`.
   - `injection H; intros; subst` can eliminate a named variable (rest
     became args). Close such goals with `do 2 eexists` and
     `first [exact El | reflexivity]`.
   - `cbn [sole_unforgeable]` does not unfold gprivate, so reified_id_of
     stays stuck. Use `change (reified_id_of (gprivate id)) with
     (decode_reified_id id)`.
   ═══════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import List Arith Bool Lia PeanoNat.
Import ListNotations.

(* ── 1. Bytes, kinds and the decoder (a copy of Namespace.v) ──────────── *)

Definition bytes : Type := list nat.

Definition reified_tag : nat := 224.
Definition sid_len : nat := 32.
Definition ephemeral_bit : nat := 32.
Definition agent_tag : nat := 255.
Definition use_marker : nat := 208.

Inductive kind : Type :=
| KData | KConfig | KPathIndex | KAllocator | KSequence | KHandle
| KName | KMatcher | KBody | KEngine | KTheory.

Definition kind_letter (k : kind) : nat :=
  match k with
  | KData => 68 | KConfig => 67 | KPathIndex => 73 | KAllocator => 65 | KSequence => 83
  | KHandle => 72 | KName => 78 | KMatcher => 88 | KBody => 66 | KEngine => 77
  | KTheory => 84
  end.

Inductive durability : Type := Durable | Ephemeral.

Definition has_ephemeral_form (k : kind) : bool :=
  match k with KEngine | KTheory => false | _ => true end.

Definition scoped_kind_byte (k : kind) (d : durability) : nat :=
  match d with
  | Durable => kind_letter k
  | Ephemeral => Nat.lor (kind_letter k) ephemeral_bit
  end.

Definition letter_kind (b : nat) : option kind :=
  if b =? 68 then Some KData
  else if b =? 67 then Some KConfig
  else if b =? 73 then Some KPathIndex
  else if b =? 65 then Some KAllocator
  else if b =? 83 then Some KSequence
  else if b =? 72 then Some KHandle
  else if b =? 78 then Some KName
  else if b =? 88 then Some KMatcher
  else if b =? 66 then Some KBody
  else if b =? 77 then Some KEngine
  else if b =? 84 then Some KTheory
  else None.

Definition kind_of_byte (b : nat) : option (kind * durability) :=
  match letter_kind (Nat.land b 223) with
  | None => None
  | Some k =>
      if Nat.land b ephemeral_bit =? 0 then Some (k, Durable)
      else if has_ephemeral_form k then Some (k, Ephemeral) else None
  end.

Record native_space : Type := mkSpace { sp_sid : bytes; sp_dur : durability }.

Inductive name_tail : Type :=
| TailRandom : bytes -> name_tail
| TailIndex : bytes -> name_tail.

Definition tail_bytes (t : name_tail) : bytes :=
  match t with TailRandom b => b | TailIndex b => b end.

Definition tail_ok (t : name_tail) : Prop :=
  match t with
  | TailRandom b => length b = 32
  | TailIndex b => length b = 8
  end.

Definition space_scoped_id (k : kind) (s : native_space) (tail : bytes) : bytes :=
  reified_tag :: scoped_kind_byte k (sp_dur s) :: sp_sid s ++ tail.

Definition handle_channel (s : native_space) : bytes := space_scoped_id KHandle s [].
Definition name_id (s : native_space) (t : name_tail) : bytes := space_scoped_id KName s (tail_bytes t).
Definition op_channel (op : nat) : bytes := [reified_tag; op].

Inductive reified_id : Type :=
| ROp : nat -> reified_id
| RConfig : native_space -> reified_id
| RSpaceCounter : native_space -> reified_id
| RCollectionCounter : native_space -> bytes -> reified_id
| RData : native_space -> bytes -> reified_id
| RPathIndex : native_space -> bytes -> reified_id
| RAllocator : native_space -> reified_id
| RHandle : native_space -> reified_id
| RName : native_space -> name_tail -> reified_id
| RMatcher : durability -> bytes -> reified_id
| RBody : durability -> bytes -> reified_id
| RReserved : kind -> bytes -> reified_id.

Definition decode_name_tail (tail : bytes) : option name_tail :=
  if length tail =? 32 then Some (TailRandom tail)
  else if length tail =? 8 then Some (TailIndex tail)
  else None.

Definition decode_payload (k : kind) (d : durability) (payload : bytes) : option reified_id :=
  match k, skipn sid_len payload with
  | KConfig, [] => Some (RConfig (mkSpace (firstn sid_len payload) d))
  | KHandle, [] => Some (RHandle (mkSpace (firstn sid_len payload) d))
  | KAllocator, [] => Some (RAllocator (mkSpace (firstn sid_len payload) d))
  | KBody, [] => Some (RBody d (firstn sid_len payload))
  | KSequence, [] => Some (RSpaceCounter (mkSpace (firstn sid_len payload) d))
  | KSequence, _ :: _ =>
      Some (RCollectionCounter (mkSpace (firstn sid_len payload) d) (skipn sid_len payload))
  | KData, _ :: _ => Some (RData (mkSpace (firstn sid_len payload) d) (skipn sid_len payload))
  | KPathIndex, _ =>
      Some (RPathIndex (mkSpace (firstn sid_len payload) d) (skipn sid_len payload))
  | KName, _ =>
      match decode_name_tail (skipn sid_len payload) with
      | Some t => Some (RName (mkSpace (firstn sid_len payload) d) t)
      | None => None
      end
  | KMatcher, _ => Some (RMatcher d payload)
  | KEngine, _ | KTheory, _ => Some (RReserved k payload)
  | KConfig, _ :: _ | KHandle, _ :: _ | KAllocator, _ :: _ | KBody, _ :: _ | KData, [] => None
  end.

Definition decode_reified_id (id : bytes) : option reified_id :=
  match id with
  | [] => None
  | tag :: rest =>
      if negb (tag =? reified_tag) then None else
      match rest with
      | [] => None
      | kb :: payload =>
          match payload with
          | [] => Some (ROp kb)
          | _ :: _ =>
              match kind_of_byte kb with
              | None => None
              | Some (k, d) =>
                  if length payload <? sid_len then None else decode_payload k d payload
              end
          end
      end
  end.

Definition is_handle (x : reified_id) : bool := match x with RHandle _ => true | _ => false end.

Lemma firstn_app_length : forall (A : Type) (l1 l2 : list A), firstn (length l1) (l1 ++ l2) = l1.
Proof. intros A l1 l2. induction l1 as [| x l1 IH]; [reflexivity | cbn; rewrite IH; reflexivity]. Qed.

Lemma skipn_app_length : forall (A : Type) (l1 l2 : list A), skipn (length l1) (l1 ++ l2) = l2.
Proof. intros A l1 l2. induction l1 as [| x l1 IH]; [reflexivity | exact IH]. Qed.

Lemma kind_of_name_byte : forall d, kind_of_byte (scoped_kind_byte KName d) = Some (KName, d).
Proof. intros [|]; reflexivity. Qed.

(* A name id that gensym or a binder of a use block makes decodes to its
   space. *)
Lemma decode_name_id :
  forall s t, length (sp_sid s) = sid_len -> tail_ok t ->
    decode_reified_id (name_id s t) = Some (RName s t).
Proof.
  intros [sid d] t Hs Ht. cbn [sp_sid sp_dur] in Hs.
  assert (Hlen : length (tail_bytes t) = 32 \/ length (tail_bytes t) = 8)
    by (destruct t; cbn in *; lia).
  unfold name_id, space_scoped_id, decode_reified_id. cbn [sp_sid sp_dur].
  replace (reified_tag =? reified_tag) with true by reflexivity. cbn [negb].
  destruct (sid ++ tail_bytes t) as [| b rest] eqn:E.
  - apply (f_equal (@length nat)) in E. rewrite length_app in E. cbn in E. unfold sid_len in Hs. lia.
  - rewrite kind_of_name_byte, <- E.
    assert (Hl : (length (sid ++ tail_bytes t) <? sid_len) = false).
    { rewrite length_app. apply Nat.ltb_ge. unfold sid_len in *. lia. }
    rewrite Hl. unfold decode_payload.
    rewrite <- Hs, skipn_app_length, firstn_app_length.
    unfold decode_name_tail. destruct t as [b0 | b0]; cbn [tail_bytes] in *; cbn in Ht.
    + rewrite Ht. reflexivity.
    + rewrite Ht. reflexivity.
Qed.

(* An id of 32 bytes, such as the id of a name that `new` makes, decodes
   to nothing. *)
Lemma decode_32_none : forall id, length id = 32 -> decode_reified_id id = None.
Proof.
  intros [| tag [| kb payload]] H; cbn in H; try discriminate.
  unfold decode_reified_id. destruct (negb (tag =? reified_tag)); [reflexivity |].
  destruct payload as [| x rest]; [discriminate |].
  destruct (kind_of_byte kb) as [[k d] |]; [| reflexivity].
  replace (length (x :: rest) <? sid_len) with true by (symmetry; apply Nat.ltb_lt; unfold sid_len; cbn in *; lia).
  reflexivity.
Qed.

(* ── 2. The Par model ──────────────────────────────────────────────────── *)

Inductive unf : Type :=
| UPrivate : bytes -> unf       (* GPrivate id *)
| UOther : nat -> unf.          (* GDeployId, GDeployerId, GSysAuthToken, ... *)

Inductive par : Type :=
| mkPar : nat -> nat -> nat -> list expr -> list unf -> list bundle -> list nat -> bool -> par
with expr : Type :=
| ENone : expr                  (* an Expr whose expr_instance is None *)
| ETuple : list par -> expr
| EList : list par -> bool -> expr   (* the flag: the list has a remainder *)
| EString : bytes -> expr      (* the UTF-8 bytes of a GString *)
| EBool : bool -> expr
| EOther : nat -> expr
with bundle : Type :=
| mkBundle : option par -> bool -> bool -> bundle.   (* body, write_flag, read_flag *)

Definition empty_par : par := mkPar 0 0 0 [] [] [] [] false.

(* namespace::gprivate *)
Definition gprivate (id : bytes) : par := mkPar 0 0 0 [] [UPrivate id] [] [] false.

Definition is_empty (p : par) : bool :=
  match p with mkPar 0 0 0 [] [] [] _ _ => true | _ => false end.

Definition sole_unforgeable (p : par) : option unf :=
  match p with mkPar 0 0 0 [] [u] [] _ _ => Some u | _ => None end.

Definition sole_gprivate_id (p : par) : option bytes :=
  match sole_unforgeable p with Some (UPrivate id) => Some id | _ => None end.

Definition reified_id_of (p : par) : option reified_id :=
  match sole_gprivate_id p with Some id => decode_reified_id id | None => None end.

(* implicits.rs single_bundle: it does not look at conds. *)
Definition single_bundle (p : par) : option bundle :=
  match p with mkPar 0 0 _ [] [] [b] _ _ => Some b | _ => None end.

(* route.rs sole_expr and handle.rs sole_expr: one Expr with an instance,
   and nothing else. *)
Definition sole_expr (p : par) : option expr :=
  match p with
  | mkPar 0 0 0 [e] [] [] _ _ => match e with ENone => None | _ => Some e end
  | _ => None
  end.

(* handle.rs sole_bundle: one bundle and nothing else (conds included). *)
Definition sole_bundle (p : par) : option bundle :=
  match p with mkPar 0 0 0 [] [] [b] _ _ => Some b | _ => None end.

Definition bytes_eqb (a b : bytes) : bool := if list_eq_dec Nat.eq_dec a b then true else false.

Lemma sole_unforgeable_shape :
  forall p u, sole_unforgeable p = Some u -> exists lf cu, p = mkPar 0 0 0 [] [u] [] lf cu.
Proof.
  intros [[|a] [|b] [|c] [|e es] [|u1 [|u2 us]] [|bd bs] lf cu] u H; cbn in H; try discriminate.
  injection H as <-. exists lf, cu. reflexivity.
Qed.

Lemma single_bundle_shape :
  forall p b, single_bundle p = Some b -> exists c lf cu, p = mkPar 0 0 c [] [] [b] lf cu.
Proof.
  intros [[|a] [|b0] c [|e es] [|u us] [|bd [|bd2 bs]] lf cu] b H; cbn in H; try discriminate.
  injection H as <-. exists c, lf, cu. reflexivity.
Qed.

Lemma reified_id_of_shape :
  forall p x, reified_id_of p = Some x ->
    exists id lf cu, p = mkPar 0 0 0 [] [UPrivate id] [] lf cu /\ decode_reified_id id = Some x.
Proof.
  intros p x H. unfold reified_id_of, sole_gprivate_id in H.
  destruct (sole_unforgeable p) as [[id | n] |] eqn:E; try discriminate.
  apply sole_unforgeable_shape in E as [lf [cu ->]]. exists id, lf, cu. split; [reflexivity | exact H].
Qed.

(* ── 3. Space references (scope.rs) ────────────────────────────────────── *)

Inductive space_ref : Type := SNative : native_space -> space_ref | SAgent : par -> space_ref.
Inductive ref_error : Type := EmptySpaceRef | HandleNotWritable | NotASpaceRef.
Inductive sr_result : Type := SROk : space_ref -> sr_result | SRErr : ref_error -> sr_result.

(* At most one bundle is removed, with the rule of eval_send. *)
Definition unbundle (p : par) : par * bool :=
  match single_bundle p with
  | Some (mkBundle (Some body) w _) => (body, w)
  | Some (mkBundle None w _) => (empty_par, w)
  | None => (p, true)
  end.

Definition classify_space_ref (p : par) : sr_result :=
  if is_empty p then SRErr EmptySpaceRef else
  match reified_id_of (fst (unbundle p)), snd (unbundle p) with
  | Some (RHandle n), true => SROk (SNative n)
  | Some (RHandle _), false => SRErr HandleNotWritable
  | Some _, _ => SRErr NotASpaceRef
  | None, _ => SROk (SAgent p)
  end.

(* The five rules of the doc comment of classify_space_ref. *)
Inductive space_ref_rule (p : par) : sr_result -> Prop :=
| rule_empty : is_empty p = true -> space_ref_rule p (SRErr EmptySpaceRef)
| rule_native : forall n, is_empty p = false -> reified_id_of (fst (unbundle p)) = Some (RHandle n) ->
    snd (unbundle p) = true -> space_ref_rule p (SROk (SNative n))
| rule_not_writable : forall n, is_empty p = false -> reified_id_of (fst (unbundle p)) = Some (RHandle n) ->
    snd (unbundle p) = false -> space_ref_rule p (SRErr HandleNotWritable)
| rule_other_id : forall x, is_empty p = false -> reified_id_of (fst (unbundle p)) = Some x ->
    is_handle x = false -> space_ref_rule p (SRErr NotASpaceRef)
| rule_agent : is_empty p = false -> reified_id_of (fst (unbundle p)) = None ->
    space_ref_rule p (SROk (SAgent p)).

(* Headline 1. Every reference meets exactly one rule, and the result of
   classify_space_ref is the result of that rule. *)
Theorem classify_space_ref_total_deterministic :
  forall p, space_ref_rule p (classify_space_ref p) /\
            (forall r, space_ref_rule p r -> r = classify_space_ref p).
Proof.
  intros p. split.
  - unfold classify_space_ref. destruct (is_empty p) eqn:He; [apply rule_empty; exact He |].
    destruct (reified_id_of (fst (unbundle p))) as [x |] eqn:Ex; [| apply rule_agent; assumption].
    destruct (snd (unbundle p)) eqn:Ew; destruct x;
      first [ eapply rule_native; eassumption
            | eapply rule_not_writable; eassumption
            | eapply rule_other_id; [eassumption | eassumption | reflexivity] ].
  - intros r Hr. unfold classify_space_ref.
    destruct Hr as [He | n He Hx Hw | n He Hx Hw | x He Hx Hh | He Hx]; rewrite He; try reflexivity.
    + rewrite Hx, Hw. reflexivity.
    + rewrite Hx, Hw. reflexivity.
    + rewrite Hx. destruct x; try discriminate Hh; destruct (snd (unbundle p)); reflexivity.
    + rewrite Hx. reflexivity.
Qed.

Lemma reified_id_not_empty :
  forall p x, reified_id_of p = Some x -> is_empty p = false /\ single_bundle p = None.
Proof.
  intros p x H. apply reified_id_of_shape in H as [id [lf [cu [-> _]]]]. split; reflexivity.
Qed.

Lemma single_bundle_not_empty : forall p b, single_bundle p = Some b -> is_empty p = false.
Proof. intros p b H. apply single_bundle_shape in H as [c [lf [cu ->]]]. destruct c; reflexivity. Qed.

(* Headline 2. A reference names a native space exactly when it is the
   bare handle id of the space or a bundle with the write flag around it.
   Under a bundle without the write flag, a handle is HandleNotWritable. *)
Theorem handle_requires_write_flag :
  forall p n,
    (classify_space_ref p = SROk (SNative n) <->
     reified_id_of p = Some (RHandle n) \/
     exists body r, single_bundle p = Some (mkBundle (Some body) true r) /\
                    reified_id_of body = Some (RHandle n)) /\
    (forall body r, single_bundle p = Some (mkBundle (Some body) false r) ->
                    reified_id_of body = Some (RHandle n) -> classify_space_ref p = SRErr HandleNotWritable).
Proof.
  intros p n. split; [split |].
  - unfold classify_space_ref. destruct (is_empty p) eqn:He; [discriminate |].
    unfold unbundle. destruct (single_bundle p) as [[[body |] w r] |] eqn:Eb; cbn [fst snd].
    + destruct (reified_id_of body) as [x |] eqn:Ex; [| discriminate].
      destruct x; destruct w; try discriminate. intros H. injection H as <-.
      right. exists body, r. split; [reflexivity | exact Ex].
    + intros H. exfalso. destruct (reified_id_of empty_par) as [x |] eqn:Ex; [| discriminate].
      discriminate Ex.
    + destruct (reified_id_of p) as [x |] eqn:Ex; [| discriminate].
      destruct x; try discriminate. intros H. injection H as <-. left. reflexivity.
  - intros [H | [body [r [Hb Hx]]]].
    + destruct (reified_id_not_empty p _ H) as [He Hb].
      unfold classify_space_ref, unbundle. rewrite He, Hb. cbn [fst snd]. rewrite H. reflexivity.
    + unfold classify_space_ref, unbundle. rewrite (single_bundle_not_empty p _ Hb), Hb.
      cbn [fst snd]. rewrite Hx. reflexivity.
  - intros body r Hb Hx. unfold classify_space_ref, unbundle.
    rewrite (single_bundle_not_empty p _ Hb), Hb. cbn [fst snd]. rewrite Hx. reflexivity.
Qed.

(* Headline 3. A Reified id that is not a handle, bare or under one bundle,
   is NotASpaceRef. So capabilities, storage ids, names and op channels are
   never agents. *)
Theorem reified_non_handle_not_a_space_ref :
  (forall p x, is_empty p = false -> reified_id_of (fst (unbundle p)) = Some x -> is_handle x = false ->
               classify_space_ref p = SRErr NotASpaceRef) /\
  (forall id x q, decode_reified_id id = Some x -> classify_space_ref (gprivate id) <> SROk (SAgent q)).
Proof.
  split.
  - intros p x He Hx Hh. unfold classify_space_ref. rewrite He, Hx.
    destruct x; try discriminate Hh; destruct (snd (unbundle p)); reflexivity.
  - intros id x q Hd. unfold classify_space_ref. cbn [is_empty gprivate].
    unfold unbundle. cbn [single_bundle gprivate fst snd].
    change (reified_id_of (gprivate id)) with (decode_reified_id id). rewrite Hd.
    destruct x; discriminate.
Qed.

(* ── 4. Channel classes and routes (route.rs) ─────────────────────────── *)

Inductive chan_class : Type :=
| CSystem | CDefaultObject | CStorage | CSpaceName : native_space -> chan_class
| CAgentTagged | CUnforgeable | CForged.

Definition is_agent_tagged (c : par) : bool :=
  match sole_expr c with
  | Some (ETuple [h; _; _]) =>
      match sole_gprivate_id h with
      | Some id => bytes_eqb id [reified_tag; agent_tag]
      | None => false
      end
  | _ => false
  end.

Definition class_of_id (id : bytes) : chan_class :=
  if (length id =? 1) || (length id =? 2) then CSystem else
  match decode_reified_id id with
  | Some (RConfig _) | Some (RHandle _) | Some (RMatcher _ _) | Some (RBody _ _) => CDefaultObject
  | Some (RData _ _) | Some (RPathIndex _ _) | Some (RAllocator _) | Some (RSpaceCounter _)
  | Some (RCollectionCounter _ _) => CStorage
  | Some (RName n _) => CSpaceName n
  | Some (ROp _) | Some (RReserved _ _) | None => CUnforgeable
  end.

Definition classify_channel (c : par) : chan_class :=
  match sole_unforgeable c with
  | Some (UPrivate id) => class_of_id id
  | Some (UOther _) => CUnforgeable
  | None => if is_agent_tagged c then CAgentTagged else CForged
  end.

(* Changed by P8 step 1 (design v2 AG-2): a use-context names a native space
   or a user agent, so the target holds a kind instead of a native space.
   The P4 definitions were:
   Record target : Type := mkTarget { t_space : par; t_native : native_space }.
   Definition ctx_native (cx : ctx) : option native_space :=
     match cx with Some t => Some (t_native t) | None => None end. *)

(* What a use block names: a native space, or a user agent whose reference
   is the evaluated reference itself (P8 AG-2, AG-3). *)
Inductive use_kind : Type := UKNative : native_space -> use_kind | UKAgent.

(* The use-context: the evaluated reference and what it names. *)
Record target : Type := mkTarget { t_space : par; t_kind : use_kind }.
Definition ctx : Type := option target.

(* What a route reads from a context: the native space, or the agent
   reference (P8). *)
Definition ctx_key (cx : ctx) : option (native_space + par) :=
  match cx with
  | Some (mkTarget _ (UKNative n)) => Some (inl n)
  | Some (mkTarget s UKAgent) => Some (inr s)
  | None => None
  end.

(* Changed by P8 step 1 (design v2 AG-1): an agent route carries the agent
   reference A and the raw channel. The P4 constructor was `| RAgent.` *)
Inductive route_t : Type :=
| RDefault
| RSpace : native_space -> bytes -> route_t
| RAgent : par -> par -> route_t.

(* The parts (A, raw) of an agent-tagged tuple (AGENT_TAG, A, raw)
   (route.rs agent_parts). *)
Definition agent_parts (c : par) : option (par * par) :=
  match sole_expr c with Some (ETuple [_; a; r]) => Some (a, r) | _ => None end.

Lemma agent_tagged_parts :
  forall c, is_agent_tagged c = true -> exists a r, agent_parts c = Some (a, r).
Proof.
  intros c H. unfold is_agent_tagged in H. unfold agent_parts.
  destruct (sole_expr c) as [e |]; [| discriminate H].
  destruct e as [| ps | ps rem | str | bo | m]; try discriminate H.
  destruct ps as [| h [| a [| r [| x rest]]]]; try discriminate H.
  exists a, r. reflexivity.
Qed.

Lemma classify_agent_tagged :
  forall c, classify_channel c = CAgentTagged -> is_agent_tagged c = true.
Proof.
  intros c H. unfold classify_channel in H.
  destruct (sole_unforgeable c) as [[id | m] |].
  - unfold class_of_id in H.
    destruct ((length id =? 1) || (length id =? 2)); [discriminate H |].
    destruct (decode_reified_id id) as [[] |]; discriminate H.
  - discriminate H.
  - destruct (is_agent_tagged c); [reflexivity | discriminate H].
Qed.

(* RouteErr: CollectionKey::of_channel fails. *)
Inductive route_result : Type := RouteOk : route_t -> route_result | RouteErr.

Definition routes_to_default_without_context (c : par) : bool :=
  match classify_channel c with CSpaceName _ | CAgentTagged => false | _ => true end.

Section Routing.
  (* canonical_inner_channel_bytes: None when the channel does not
     serialize. *)
  Variable canon : par -> option bytes.

  (* CollectionKey::of_channel: the canonical bytes, never empty. *)
  Definition key_of (c : par) : option bytes :=
    match canon c with Some (b :: bs) => Some (b :: bs) | _ => None end.

  Definition space_route (n : native_space) (c : par) : route_result :=
    match key_of c with Some k => RouteOk (RSpace n k) | None => RouteErr end.

  (* Changed by P8 step 1 (design v2 AG-1): R4 carries the parts of the tag,
     and R5 under the context of a user agent routes to that agent with the
     forged channel as the raw channel. The P4 arms were
     `| CAgentTagged, _ => RouteOk RAgent` and
     `| CForged, Some t => space_route (t_native t) c`. *)
  Definition route (cx : ctx) (c : par) : route_result :=
    match classify_channel c, cx with
    | CSystem, _ | CDefaultObject, _ | CUnforgeable, _ | CStorage, _ => RouteOk RDefault
    | CSpaceName n, _ => space_route n c
    | CAgentTagged, _ =>
        match agent_parts c with Some (a, r) => RouteOk (RAgent a r) | None => RouteErr end
    | CForged, Some (mkTarget _ (UKNative n)) => space_route n c
    | CForged, Some (mkTarget s UKAgent) => RouteOk (RAgent s c)
    | CForged, None => RouteOk RDefault
    end.

  (* Headline 4. A route fails only when the key of a space route fails,
     every channel with a key has a route, and the route depends on the
     context only through the native space or the agent reference of the
     context. *)
  Theorem route_total_deterministic :
    forall cx c,
      (route cx c = RouteErr ->
       key_of c = None /\
       ((exists n, classify_channel c = CSpaceName n) \/
        (classify_channel c = CForged /\ exists s n, cx = Some (mkTarget s (UKNative n))))) /\
      (key_of c <> None -> exists r, route cx c = RouteOk r) /\
      (forall cx', ctx_key cx = ctx_key cx' -> route cx c = route cx' c).
  Proof.
    intros cx c. unfold route, space_route.
    split; [| split].
    - destruct (classify_channel c) as [| | | n | | |] eqn:Ec; intros H.
      + discriminate H.
      + discriminate H.
      + discriminate H.
      + destruct (key_of c) as [k |] eqn:Ek; [discriminate H |].
        split; [reflexivity | left; exists n; reflexivity].
      + exfalso. destruct (agent_tagged_parts c (classify_agent_tagged c Ec)) as [a [r Ep]].
        rewrite Ep in H. discriminate H.
      + discriminate H.
      + destruct cx as [[s [n |]] |]; try discriminate H.
        destruct (key_of c) as [k |] eqn:Ek; [discriminate H |].
        split; [reflexivity | right; split; [reflexivity | exists s, n; reflexivity]].
    - intros Hk. destruct (key_of c) as [k |] eqn:Ek; [| contradiction Hk; reflexivity].
      destruct (classify_channel c) eqn:Ec; try (eexists; reflexivity).
      + destruct (agent_tagged_parts c (classify_agent_tagged c Ec)) as [a [r Ep]].
        rewrite Ep. eexists; reflexivity.
      + destruct cx as [[s [n |]] |]; eexists; reflexivity.
    - intros cx' Hn. destruct (classify_channel c); try reflexivity.
      destruct cx as [[s [n |]] |]; destruct cx' as [[s' [n' |]] |]; cbn in Hn;
        try discriminate Hn; try reflexivity; injection Hn as Hn; subst; reflexivity.
  Qed.

  (* Headline 5. The rules R1 to R5 of route.rs, with the agent routes of
     P8. *)
  Theorem route_rules :
    forall cx c,
      ((classify_channel c = CSystem \/ classify_channel c = CDefaultObject \/
        classify_channel c = CUnforgeable) -> route cx c = RouteOk RDefault) /\
      (classify_channel c = CStorage -> route cx c = RouteOk RDefault) /\
      (forall n, classify_channel c = CSpaceName n -> route cx c = space_route n c) /\
      (classify_channel c = CAgentTagged ->
       exists a r, agent_parts c = Some (a, r) /\ route cx c = RouteOk (RAgent a r)) /\
      (classify_channel c = CForged ->
       route cx c = match cx with
                    | Some (mkTarget _ (UKNative n)) => space_route n c
                    | Some (mkTarget s UKAgent) => RouteOk (RAgent s c)
                    | None => RouteOk RDefault
                    end).
  Proof.
    intros cx c. unfold route.
    split; [intros [H | [H | H]]; rewrite H; reflexivity |].
    split; [intros H; rewrite H; reflexivity |].
    split; [intros n H; rewrite H; reflexivity |].
    split.
    - intros H. destruct (agent_tagged_parts c (classify_agent_tagged c H)) as [a [r Ep]].
      exists a, r. rewrite H, Ep. split; reflexivity.
    - intros H. rewrite H. destruct cx as [[s [n |]] |]; reflexivity.
  Qed.

  (* Headline 6. A space name (the name id of gensym, or a binder of a use
     block) routes to its own space under every context, the context of
     another space included. *)
  Theorem space_name_route_ignores_ctx :
    (forall c n, classify_channel c = CSpaceName n ->
                 forall cx cx', route cx c = route cx' c /\ route cx c = space_route n c) /\
    (forall s t, length (sp_sid s) = sid_len -> tail_ok t ->
                 classify_channel (gprivate (name_id s t)) = CSpaceName s).
  Proof.
    split.
    - intros c n H cx cx'. unfold route. rewrite H. split; reflexivity.
    - intros s t Hs Ht. unfold classify_channel. cbn [sole_unforgeable gprivate].
      unfold class_of_id. rewrite (decode_name_id s t Hs Ht).
      assert (Hl : length (name_id s t) = 34 + length (tail_bytes t)).
      { unfold name_id, space_scoped_id. cbn [length]. rewrite length_app, Hs. unfold sid_len. lia. }
      rewrite Hl. reflexivity.
  Qed.

  (* The R7 backend (rholang/src/rust/interpreter/reified/execution.rs,
     produce and r7_route): the produce of a system process routes with no
     context. Changed by P8 step 2 (design v2 R7A-1): an agent route makes
     the call of TS-1 on the agent, so the constructor carries the agent and
     the raw channel. The P4 constructor was `| R7AgentFail`. *)
  Inductive r7_action : Type :=
  | R7Inner
  | R7Space : native_space -> bytes -> r7_action
  | R7Agent : par -> par -> r7_action
  | R7KeyFail.

  (* R7A-1: exactly one bundle with both flags around an agent-tagged
     tuple names the agent of the tuple. The translation of `new` binds an
     agent-minted name to that bundle. The Rust agent_parts checks the tag
     at the head, as is_agent_tagged does here. *)
  Definition r7_agent_bundle (c : par) : option (par * par) :=
    match single_bundle c with
    | Some (mkBundle (Some body) true true) =>
        if is_agent_tagged body then agent_parts body else None
    | _ => None
    end.

  Definition r7_of (r : route_result) : r7_action :=
    match r with
    | RouteOk RDefault => R7Inner
    | RouteOk (RSpace n k) => R7Space n k
    | RouteOk (RAgent a raw) => R7Agent a raw
    | RouteErr => R7KeyFail
    end.

  (* Changed by P8 step 2 (design v2 R7A-1): the bundle of an agent-minted
     name comes first, and every other channel keeps its route with no
     context. The P4 definition matched `route None c` with the arms of
     r7_of, and its agent arm gave R7AgentFail. *)
  Definition r7_produce (c : par) : r7_action :=
    match r7_agent_bundle c with
    | Some (a, raw) => R7Agent a raw
    | None => r7_of (route None c)
    end.

  (* A bundle is a forged channel: it holds no unforgeable name and no
     expression. *)
  Lemma single_bundle_forged :
    forall c b, single_bundle c = Some b -> classify_channel c = CForged.
  Proof.
    intros c b H. apply single_bundle_shape in H as [k [lf [cu ->]]].
    unfold classify_channel, is_agent_tagged, sole_expr.
    destruct k; reflexivity.
  Qed.

  Lemma r7_agent_bundle_forged :
    forall c p, r7_agent_bundle c = Some p -> classify_channel c = CForged.
  Proof.
    intros c p H. unfold r7_agent_bundle in H.
    destruct (single_bundle c) as [b |] eqn:Eb; [| discriminate H].
    exact (single_bundle_forged c b Eb).
  Qed.

  (* Headline 7. The R7 backend takes no context. On every channel that is
     not forged it acts on the route that the reducer computes under any
     context, so the reply of a system process to a space name reaches
     the same space, and the reply to an agent-tagged name reaches the
     agent. A forged channel goes to the wrapped space, except the bundle
     of an agent-minted name, which reaches its agent (P8 step 2, R7A-1).
     The P4 form of the second conjunct was
     `classify_channel c = CForged -> r7_produce c = R7Inner`. *)
  Theorem r7_route_ignores_ctx :
    forall c,
      (forall cx, classify_channel c <> CForged -> r7_produce c = r7_of (route cx c)) /\
      (classify_channel c = CForged ->
       r7_produce c = match r7_agent_bundle c with
                      | Some (a, raw) => R7Agent a raw
                      | None => R7Inner
                      end).
  Proof.
    intros c. split.
    - intros cx Hc. unfold r7_produce.
      destruct (r7_agent_bundle c) as [p |] eqn:Ea.
      + contradiction Hc. exact (r7_agent_bundle_forged c p Ea).
      + unfold route. destruct (classify_channel c); try reflexivity.
        contradiction Hc; reflexivity.
    - intros Hc. unfold r7_produce.
      destruct (r7_agent_bundle c) as [[a raw] |]; [reflexivity |].
      unfold route. rewrite Hc. reflexivity.
  Qed.

  (* Headline 14. R7A-1 (P8 design v2): for every single bundle, the
     backend makes the call on an agent exactly when the bundle has both
     flags and its body is an agent-tagged tuple, the form that the
     translation of `new` binds to an agent-minted name. The call goes to
     the agent of the tuple with its raw channel. Every other bundle keeps
     the route of HEAD: the default space. *)
  Theorem r7_agent_reply_bundle_exact :
    forall body w rd k lf cu,
      r7_produce (mkPar 0 0 k [] [] [mkBundle body w rd] lf cu) =
        match body with
        | Some t =>
            if w && rd && is_agent_tagged t then
              match agent_parts t with Some (a, raw) => R7Agent a raw | None => R7Inner end
            else R7Inner
        | None => R7Inner
        end.
  Proof.
    intros body w rd k lf cu.
    assert (Hb : classify_channel (mkPar 0 0 k [] [] [mkBundle body w rd] lf cu) = CForged).
    { apply (single_bundle_forged _ (mkBundle body w rd)). reflexivity. }
    unfold r7_produce, r7_agent_bundle. cbn [single_bundle].
    destruct body as [t |];
      [destruct w, rd; cbn [andb] | unfold route; rewrite Hb; reflexivity];
      try (unfold route; rewrite Hb; reflexivity).
    destruct (is_agent_tagged t) eqn:Et.
    - destruct (agent_tagged_parts t Et) as [a [raw Ep]]. rewrite Ep. reflexivity.
    - unfold route. rewrite Hb. reflexivity.
  Qed.

  (* The agent-tagged tuple (AGENT_TAG, a, raw) with the bit sets lf and cu,
     and the agent-minted name around it: the bundle with both flags that
     the translation of `new` binds (user_agent.rs agent_minted_name, P8
     step 4, design v2 TN-3). *)
  Definition agent_tuple (a raw : par) (lf : list nat) (cu : bool) : par :=
    mkPar 0 0 0 [ETuple [gprivate [reified_tag; agent_tag]; a; raw]] [] [] lf cu.

  Definition agent_minted_name (a raw : par) (lf : list nat) (cu : bool)
             (blf : list nat) (bcu : bool) : par :=
    mkPar 0 0 0 [] [] [mkBundle (Some (agent_tuple a raw lf cu)) true true] blf bcu.

  Lemma agent_tuple_tagged : forall a raw lf cu, is_agent_tagged (agent_tuple a raw lf cu) = true.
  Proof.
    intros a raw lf cu. unfold is_agent_tagged, agent_tuple. cbn [sole_expr].
    cbn [sole_gprivate_id sole_unforgeable gprivate]. unfold bytes_eqb.
    destruct (list_eq_dec Nat.eq_dec [reified_tag; agent_tag] [reified_tag; agent_tag])
      as [_ | Hne]; [reflexivity | contradiction Hne; reflexivity].
  Qed.

  (* Headline 15 (P8 step 4, design v2 TN-3). An agent-minted name is one
     bundle with both flags around the agent-tagged tuple of its agent and
     its raw channel. So the write rule of eval_send and the read rule of
     unbundle_receive both give the tuple, the tuple routes to its agent
     with its raw channel under every context (R4), and the R7 backend reads
     the same agent and raw channel from the name (R7A-1). The bit sets of
     the tuple and of the name do not matter. *)
  Theorem tag_untag_id :
    forall a raw lf cu blf bcu cx,
      single_bundle (agent_minted_name a raw lf cu blf bcu) =
        Some (mkBundle (Some (agent_tuple a raw lf cu)) true true) /\
      classify_channel (agent_tuple a raw lf cu) = CAgentTagged /\
      route cx (agent_tuple a raw lf cu) = RouteOk (RAgent a raw) /\
      r7_produce (agent_minted_name a raw lf cu blf bcu) = R7Agent a raw.
  Proof.
    intros a raw lf cu blf bcu cx.
    pose proof (agent_tuple_tagged a raw lf cu) as Ht.
    assert (Hc : classify_channel (agent_tuple a raw lf cu) = CAgentTagged).
    { unfold classify_channel. replace (sole_unforgeable (agent_tuple a raw lf cu)) with
        (@None unf) by reflexivity. rewrite Ht. reflexivity. }
    split; [reflexivity |]. split; [exact Hc |]. split.
    - unfold route. rewrite Hc. reflexivity.
    - unfold r7_produce, r7_agent_bundle, agent_minted_name. cbn [single_bundle].
      rewrite Ht. reflexivity.
  Qed.

  (* The ids that an ordinary program can name: byte names have one byte,
     and the names of `new` have 32 bytes (Blake2b-256). *)
  Definition ordinary_ids (c : par) : Prop :=
    (forall id, sole_gprivate_id c = Some id -> length id = 1 \/ length id = 32) /\
    (forall h a r id, sole_expr c = Some (ETuple [h; a; r]) -> sole_gprivate_id h = Some id ->
                      length id = 1 \/ length id = 32).

  (* Headline 8. The fast path of reified_consume is exact: a channel passes
     routes_to_default_without_context exactly when it routes to the
     default space with no context. Every channel of an ordinary program
     passes it, so ordinary programs keep HEAD's behavior. *)
  Theorem ordinary_channels_route_default :
    forall c,
      (routes_to_default_without_context c = true <-> route None c = RouteOk RDefault) /\
      (ordinary_ids c -> routes_to_default_without_context c = true /\ route None c = RouteOk RDefault).
  Proof.
    intros c.
    assert (Hiff : routes_to_default_without_context c = true <-> route None c = RouteOk RDefault).
    { unfold routes_to_default_without_context, route, space_route.
      destruct (classify_channel c); split; intros H; try reflexivity; try discriminate H.
      - destruct (key_of c); discriminate H.
      - destruct (agent_parts c) as [[a r] |]; discriminate H. }
    split; [exact Hiff |]. intros [Hid Htup].
    assert (Hd : routes_to_default_without_context c = true).
    { unfold routes_to_default_without_context, classify_channel.
      destruct (sole_unforgeable c) as [[id | m] |] eqn:Eu.
      - assert (Hl : length id = 1 \/ length id = 32).
        { apply Hid. unfold sole_gprivate_id. rewrite Eu. reflexivity. }
        unfold class_of_id. destruct Hl as [Hl | Hl]; rewrite Hl; [reflexivity |].
        cbn. rewrite (decode_32_none id Hl). reflexivity.
      - reflexivity.
      - unfold is_agent_tagged. destruct (sole_expr c) as [e |] eqn:Ee; [| reflexivity].
        destruct e as [| ps | ps rem | s | b | n]; try reflexivity.
        destruct ps as [| h [| a [| r [| x rest]]]]; try reflexivity.
        destruct (sole_gprivate_id h) as [id |] eqn:Eh; [| reflexivity].
        unfold bytes_eqb. destruct (list_eq_dec Nat.eq_dec id [reified_tag; agent_tag]) as [-> | _];
          [| reflexivity].
        exfalso. destruct (Htup h a r _ eq_refl Eh) as [H | H]; cbn in H; discriminate H. }
    split; [exact Hd | apply Hiff; exact Hd].
  Qed.
End Routing.

(* ── 5. The USE wrapper (scope.rs wrap_body, reducer_hooks.rs eval_use) ── *)

(* The USE tuple (USE, space, body) with the marker GPrivate([0xE0, 0xD0]). *)
Definition use_tuple (space body : par) : par :=
  mkPar 0 0 0 [ETuple [gprivate [reified_tag; use_marker]; space; body]] [] [] [] false.

Definition decode_use (p : par) : option (par * par) :=
  match sole_expr p with
  | Some (ETuple [m; s; b]) =>
      match sole_gprivate_id m with
      | Some id => if bytes_eqb id [reified_tag; use_marker] then Some (s, b) else None
      | None => None
      end
  | _ => None
  end.

Lemma decode_use_tuple : forall s b, decode_use (use_tuple s b) = Some (s, b).
Proof. intros s b. reflexivity. Qed.

Definition wrap_body (cx : ctx) (body : par) : par :=
  match cx with Some t => use_tuple (t_space t) body | None => body end.

(* AG-4 (P8): the call channel of an agent reference under the bundle rule of
   eval_send (reduce.rs): a bundle with the write flag gives its body, a
   bundle without the write flag or without a body gives none, and a value
   that is no bundle is its own call channel (scope.rs call_channel). *)
Definition call_channel (a : par) : option par :=
  match single_bundle a with
  | Some (mkBundle (Some body) true _) => Some body
  | Some _ => None
  | None => Some a
  end.

(* Changed by P8 step 1: a use block on a user agent no longer fails closed,
   so the error UseAgent has no site. The P4 type was
   `Inductive use_error : Type := UseAgent | UseRef : ref_error -> use_error.` *)
Inductive use_error : Type := UseRef : ref_error -> use_error.
Inductive enter_result : Type := Entered : target -> enter_result | UseFailed : use_error -> enter_result.
Inductive fire_result : Type := Runs : ctx -> par -> fire_result | Aborts : use_error -> fire_result.

(* Changed by P8 step 1: a valid context names a native space through its
   reference, or a user agent through a reference that has a call channel.
   The P4 definition was
   `match cx with None => True | Some t => classify_space_ref (t_space t) =
    SROk (SNative (t_native t)) end`. *)
Definition ctx_valid (cx : ctx) : Prop :=
  match cx with
  | None => True
  | Some (mkTarget s (UKNative n)) => classify_space_ref s = SROk (SNative n)
  | Some (mkTarget s UKAgent) => classify_space_ref s = SROk (SAgent s) /\ call_channel s <> None
  end.

(* classify_space_ref names an agent by the reference itself. *)
Lemma classify_agent_is_reference :
  forall p q, classify_space_ref p = SROk (SAgent q) -> q = p.
Proof.
  intros p q H. unfold classify_space_ref in H. destruct (is_empty p); [discriminate H |].
  destruct (reified_id_of (fst (unbundle p))) as [x |].
  - destruct x; destruct (snd (unbundle p)); discriminate H.
  - injection H as <-. reflexivity.
Qed.

Section UseWrapper.
  Variable env : Type.
  (* eval_expr, then substitute_and_charge, of a space reference. *)
  Variable eval_ref : env -> par -> par.
  (* Changed by P8 step 1: a space reference is a value, native or agent:
     evaluation leaves it as it is. The P4 hypothesis covered native
     references only:
     `forall e p n, classify_space_ref p = SROk (SNative n) -> eval_ref e p = p`. *)
  Hypothesis eval_space_ref : forall e p r, classify_space_ref p = SROk r -> eval_ref e p = p.

  (* eval_use: evaluate the reference, classify it, and enter its context.
     Changed by P8 step 1 (AG-2, AG-4): an agent reference enters the
     context of the agent when it has a call channel, and it fails with
     HandleNotWritable otherwise. The P4 arm was
     `| SROk (SAgent _) => UseFailed UseAgent`. *)
  Definition enter_use (e : env) (space : par) : enter_result :=
    let v := eval_ref e space in
    match classify_space_ref v with
    | SROk (SNative n) => Entered (mkTarget v (UKNative n))
    | SROk (SAgent _) =>
        match call_channel v with
        | Some _ => Entered (mkTarget v UKAgent)
        | None => UseFailed (UseRef HandleNotWritable)
        end
    | SRErr err => UseFailed (UseRef err)
    end.

  (* The first step of a fired body: the dispatcher evaluates it on the
     root reducer, whose context is None
     (rholang/src/rust/interpreter/dispatch.rs:62-71). Inside a reduction,
     eval_continuation runs the body in a detached task of that root
     reducer, so the context is None there as well. A USE tuple enters its
     context (eval_marker). *)
  Definition fire_step (e : env) (p : par) : fire_result :=
    match decode_use p with
    | Some (space, body) =>
        match enter_use e space with
        | Entered t => Runs (Some t) body
        | UseFailed err => Aborts err
        end
    | None => Runs None p
    end.

  (* Headline 9. Every context that a use block enters is valid, and a body
     that the reducer wraps under a valid context runs in that context when
     it fires. Without a context the body is unchanged, and it runs as the
     root reducer runs it. *)
  Theorem use_wrapper_restores_ctx :
    (forall e space t, enter_use e space = Entered t -> ctx_valid (Some t)) /\
    (forall cx body e, ctx_valid cx ->
       fire_step e (wrap_body cx body) =
       match cx with Some t => Runs (Some t) body | None => fire_step e body end).
  Proof.
    split.
    - intros e space t H. unfold enter_use in H.
      destruct (classify_space_ref (eval_ref e space)) as [[n | q] | err] eqn:Ec; try discriminate H.
      + injection H as <-. cbn. exact Ec.
      + destruct (call_channel (eval_ref e space)) as [ch |] eqn:Ecc; [| discriminate H].
        injection H as <-. cbn.
        rewrite (classify_agent_is_reference _ _ Ec) in Ec.
        split; [exact Ec | rewrite Ecc; discriminate].
    - intros [[s [n |]] |] body e Hv; [| | reflexivity].
      + cbn [wrap_body t_space]. unfold fire_step. rewrite decode_use_tuple. unfold enter_use.
        cbn [ctx_valid] in Hv. rewrite (eval_space_ref e _ _ Hv), Hv. reflexivity.
      + cbn [wrap_body t_space]. unfold fire_step. rewrite decode_use_tuple. unfold enter_use.
        cbn [ctx_valid] in Hv. destruct Hv as [Hc Hw].
        rewrite (eval_space_ref e _ _ Hc), Hc.
        destruct (call_channel s) as [ch |]; [reflexivity | contradiction Hw; reflexivity].
  Qed.
End UseWrapper.

(* Headline 13 (P8, AG-4). A use block enters the context of a user agent
   exactly when the evaluated reference has a call channel under the bundle
   rule of eval_send. Under a bundle without the write flag the block fails
   with HandleNotWritable, as eval_send fails on such a bundle. So no
   context ever holds an agent that a call cannot reach. *)
Theorem agent_ref_requires_write_flag :
  forall (env : Type) (eval_ref : env -> par -> par) e space,
    classify_space_ref (eval_ref e space) = SROk (SAgent (eval_ref e space)) ->
    (enter_use env eval_ref e space = Entered (mkTarget (eval_ref e space) UKAgent) <->
     call_channel (eval_ref e space) <> None) /\
    (forall body r, single_bundle (eval_ref e space) = Some (mkBundle body false r) ->
       enter_use env eval_ref e space = UseFailed (UseRef HandleNotWritable)).
Proof.
  intros env eval_ref e space Hc. split.
  - unfold enter_use. cbv zeta. rewrite Hc.
    destruct (call_channel (eval_ref e space)) as [ch |] eqn:Ecc; split; intros H.
    + discriminate.
    + reflexivity.
    + discriminate H.
    + contradiction H; reflexivity.
  - intros body r Hb. unfold enter_use. cbv zeta. rewrite Hc.
    unfold call_channel. rewrite Hb. destruct body; reflexivity.
Qed.

(* The hypothesis holds for the evaluation that leaves values as they are. *)
Corollary use_wrapper_restores_ctx_instance :
  forall cx body (e : unit), ctx_valid cx ->
    fire_step unit (fun _ p => p) e (wrap_body cx body) =
    match cx with Some t => Runs (Some t) body | None => fire_step unit (fun _ p => p) e body end.
Proof.
  intros cx body e Hv.
  exact (proj2 (use_wrapper_restores_ctx unit (fun _ p => p) (fun _ _ _ _ => eq_refl)) cx body e Hv).
Qed.

(* ── 6. The handle process (handle.rs) ─────────────────────────────────── *)

Definition handle_of (this : par) : option native_space :=
  match reified_id_of this with Some (RHandle n) => Some n | _ => None end.

Definition is_reply_name (ret : par) : bool :=
  match sole_unforgeable ret with
  | Some _ => true
  | None => match sole_bundle ret with Some (mkBundle _ true _) => true | _ => false end
  end.

Definition sole_list (p : par) : option (list par) :=
  match sole_expr p with Some (EList ps false) => Some ps | _ => None end.

Definition sole_string (p : par) : option bytes :=
  match sole_expr p with Some (EString s) => Some s | _ => None end.

Inductive method : Type := MGensym | MQualifier | MConfig | MProduce | MConsume.

(* The ASCII bytes of the method names. *)
Definition name_gensym : bytes := [103; 101; 110; 115; 121; 109].
Definition name_qualifier : bytes := [113; 117; 97; 108; 105; 102; 105; 101; 114].
Definition name_config : bytes := [99; 111; 110; 102; 105; 103].
Definition name_produce : bytes := [112; 114; 111; 100; 117; 99; 101].
Definition name_consume : bytes := [99; 111; 110; 115; 117; 109; 101].

Definition method_of (name : bytes) : option method :=
  if bytes_eqb name name_gensym then Some MGensym
  else if bytes_eqb name name_qualifier then Some MQualifier
  else if bytes_eqb name name_config then Some MConfig
  else if bytes_eqb name name_produce then Some MProduce
  else if bytes_eqb name name_consume then Some MConsume
  else None.

Definition arity (m : method) : nat :=
  match m with MGensym | MQualifier | MConfig => 0 | MProduce => 2 | MConsume => 3 end.

Inductive reply_code : Type := CMalformedMessage | CUnknownMethod | CBadArgument | CUnknownParam.
Inductive reply : Type := ReplyError : reply_code -> reply | ReplyBody : nat -> reply.
Inductive abort_reason : Type := ADangling | APositional | ASchema | ABody : nat -> abort_reason.
Inductive outcome : Type := ODrop | OAbort : abort_reason -> outcome | OReply : par -> reply -> outcome.

Section HandleDispatch.
  Variable sendparams : Type.
  Variable no_params : sendparams.
  Variable positional_of : sendparams -> list par.
  Variable named_keys : sendparams -> list bytes.
  (* split_trailing_params: Some None (no PARAMS), Some (Some ps), or None
     (a malformed PARAMS tuple). *)
  Variable split_params : list par -> option (option sendparams).
  Variable cfg : Type.
  (* read_config, then SpaceConfig::from_par: None for a dangling handle or
     an invalid config record. *)
  Variable read_config : native_space -> option cfg.
  (* param_schema: None when the family has no send schema. *)
  Variable schema : method -> cfg -> option (list bytes).
  (* The body of a method: a reply (by its index) or an abort. *)
  Variable method_body : native_space -> cfg -> method -> list par -> sendparams -> reply + abort_reason.

  Inductive parsed : Type :=
  | PDropped
  | PMalformed : par -> bytes -> parsed
  | PMessage : par -> bytes -> list par -> sendparams -> parsed.

  Definition parse_message (msg : par) : parsed :=
    match sole_list msg with
    | Some (ret :: mt :: rest) =>
        match sole_string mt with
        | Some m =>
            if is_reply_name ret then
              match split_params rest with
              | Some None => PMessage ret m rest no_params
              | Some (Some ps) => PMessage ret m (removelast rest) ps
              | None => PMalformed ret m
              end
            else PDropped
        | None => PDropped
        end
    | _ => PDropped
    end.

  Definition run (n : native_space) (c : cfg) (m : bytes) (args : list par) (ps : sendparams)
    : reply + abort_reason :=
    match positional_of ps with
    | _ :: _ => inr APositional
    | [] =>
        match method_of m with
        | None => inl (ReplyError CUnknownMethod)
        | Some hm =>
            if negb (length args =? arity hm) then inl (ReplyError CBadArgument) else
            match schema hm c with
            | None => inr ASchema
            | Some keys =>
                if existsb (fun k => negb (existsb (bytes_eqb k) keys)) (named_keys ps)
                then inl (ReplyError CUnknownParam)
                else method_body n c hm args ps
            end
        end
    end.

  Definition call (this msg : par) : outcome :=
    match handle_of this with
    | None => ODrop
    | Some n =>
        match read_config n with
        | None => OAbort ADangling
        | Some c =>
            match parse_message msg with
            | PDropped => ODrop
            | PMalformed ret _ => OReply ret (ReplyError CMalformedMessage)
            | PMessage ret m args ps =>
                match run n c m args ps with inl r => OReply ret r | inr a => OAbort a end
            end
        end
    end.

  Lemma parse_message_ret :
    forall msg,
      (forall ret m, parse_message msg = PMalformed ret m ->
         is_reply_name ret = true /\ exists mt rest, sole_list msg = Some (ret :: mt :: rest)) /\
      (forall ret m args ps, parse_message msg = PMessage ret m args ps ->
         is_reply_name ret = true /\ exists mt rest, sole_list msg = Some (ret :: mt :: rest)).
  Proof.
    intros msg. unfold parse_message.
    destruct (sole_list msg) as [[| ret0 [| mt rest]] |] eqn:El; try (split; intros; discriminate).
    destruct (sole_string mt) as [m0 |]; [| split; intros; discriminate].
    destruct (is_reply_name ret0) eqn:Er; [| split; intros; discriminate].
    destruct (split_params rest) as [[ps0 |] |]; split; intros; try discriminate;
      match goal with H : _ = _ |- _ => injection H; intros; subst end;
      (split; [exact Er | do 2 eexists; first [exact El | reflexivity]]).
  Qed.

  (* Headline 10. The handle process gives every call one outcome:
     - a call whose `this` is not a bare handle id is dropped (A6)
     - a reply goes only to the first element of the message, which is a
       reply name (A23), and only for a bare handle
     - a dangling handle aborts, whatever the message
     - a message that the parser drops gets no reply
     - positional params abort before the method checks. *)
  Theorem handle_dispatch_total :
    forall this msg,
      (handle_of this = None -> call this msg = ODrop) /\
      (forall ret r, call this msg = OReply ret r ->
         is_reply_name ret = true /\ (exists mt rest, sole_list msg = Some (ret :: mt :: rest)) /\
         exists n, handle_of this = Some n) /\
      (forall n, handle_of this = Some n -> read_config n = None -> call this msg = OAbort ADangling) /\
      (parse_message msg = PDropped -> call this msg = ODrop \/ call this msg = OAbort ADangling) /\
      (forall ret m args ps n c,
         parse_message msg = PMessage ret m args ps -> positional_of ps <> [] ->
         handle_of this = Some n -> read_config n = Some c -> call this msg = OAbort APositional).
  Proof.
    intros this msg. unfold call.
    split; [intros H; rewrite H; reflexivity |].
    split.
    - intros ret r H. destruct (handle_of this) as [n |] eqn:Eh; [| discriminate H].
      destruct (read_config n) as [c |]; [| discriminate H].
      destruct (parse_message msg) as [| ret0 m0 | ret0 m0 args0 ps0] eqn:Ep; [discriminate H | |].
      + injection H as <- _. destruct (proj1 (parse_message_ret msg) ret0 m0 Ep) as [Hr Hl].
        split; [exact Hr | split; [exact Hl | exists n; reflexivity]].
      + destruct (run n c m0 args0 ps0) as [r0 | a0]; [| discriminate H].
        injection H as <- _. destruct (proj2 (parse_message_ret msg) ret0 m0 args0 ps0 Ep) as [Hr Hl].
        split; [exact Hr | split; [exact Hl | exists n; reflexivity]].
    - split; [intros n Hn Hc; rewrite Hn, Hc; reflexivity |].
      split.
      + intros Hp. destruct (handle_of this) as [n |]; [| left; reflexivity].
        destruct (read_config n); [rewrite Hp; left; reflexivity | right; reflexivity].
      + intros ret m args ps n c Hp Hpos Hn Hc. rewrite Hn, Hc, Hp. unfold run.
        destruct (positional_of ps) as [| x xs]; [contradiction Hpos; reflexivity | reflexivity].
  Qed.
End HandleDispatch.

(* ── 7. Availability in native execution (runtime.rs, decisions 46, 47) ─ *)

(* Decision 47 amends the model of decision 46: native play and native
   replay run spaces. The model of decision 46 stays here as a comment.

   (* The state of the Reified runtime of a reducer: a host runtime over the
      space of the runtime, a detached runtime, or the runtime of the native
      replay environment. A host runtime also sees whether its budget runs a
      native deploy (RuntimeBudget::native_execution_active). *)
   Inductive runtime_state : Type := RtHost | RtDetached | RtNativeReplay.

   Inductive availability_error : Type :=
   | RuntimeUnavailable
   | SpacesUnsupportedInNativeExecution.

   (* ReifiedRuntime::host and ensure_available: the first check of every space
      entry point (run_op, create_space, read_config, getSpaceAgent). *)
   Definition ensure_available (st : runtime_state) (native : bool)
       : option availability_error :=
     match st with
     | RtHost => if native then Some SpacesUnsupportedInNativeExecution else None
     | RtDetached => Some RuntimeUnavailable
     | RtNativeReplay => Some SpacesUnsupportedInNativeExecution
     end.

   (* A space entry point: when the runtime is unavailable it stages nothing
      and fails with the error of the check, before any space work; otherwise
      it runs its job, which stages the operations ops. *)
   Definition space_entry {Op : Type} (st : runtime_state) (native : bool) (ops : list Op)
       : list Op * option availability_error :=
     match ensure_available st native with
     | Some e => ([], Some e)
     | None => (ops, None)
     end.

   (* Headline 11. In native execution, native play (a host runtime whose budget
      runs a native deploy) and native replay (the runtime of the native replay
      environment) give the same result for every space entry: no staged
      operation and the error SpacesUnsupportedInNativeExecution. Outside native
      execution a host runtime runs the job unchanged. So a space deploy fails
      at one point in play and in replay, and replay reproduces play. *)
   Theorem native_execution_fails_closed :
     forall (Op : Type) (ops : list Op) (replay_native : bool),
       space_entry RtHost true ops = space_entry RtNativeReplay replay_native ops /\
       space_entry RtHost true ops = ([], Some SpacesUnsupportedInNativeExecution) /\
       space_entry RtHost false ops = (ops, None).
   Proof.
     intros Op ops replay_native. unfold space_entry, ensure_available.
     destruct replay_native; repeat split; reflexivity.
   Qed.
*)

(* The state of the Reified runtime of a reducer: a host runtime over the
   space of the runtime, a detached runtime, or the runtime of the native
   replay environment, which the environment binds once to the store of its
   native replay session (bound = true). A host runtime also sees whether its
   budget runs a native deploy (RuntimeBudget::native_execution_active), and
   it runs space jobs in both cases. *)
Inductive runtime_state : Type := RtHost | RtDetached | RtNativeReplay (bound : bool).

Inductive availability_error : Type := RuntimeUnavailable.

(* ReifiedRuntime::host and ensure_available: the first check of every space
   entry point (run_op, create_space, read_config, allocate, getSpaceAgent). *)
Definition ensure_available (st : runtime_state) (native : bool)
    : option availability_error :=
  match st with
  | RtHost => None
  | RtDetached => Some RuntimeUnavailable
  | RtNativeReplay bound => if bound then None else Some RuntimeUnavailable
  end.

(* A space entry point: when the runtime is unavailable it stages nothing
   and fails with the error of the check, before any space work; otherwise
   it runs its job, which stages the operations ops. *)
Definition space_entry {Op : Type} (st : runtime_state) (native : bool) (ops : list Op)
    : list Op * option availability_error :=
  match ensure_available st native with
  | Some e => ([], Some e)
  | None => (ops, None)
  end.

(* Headline 11 (decision 47). Native play (a host runtime whose budget runs a
   native deploy) runs a space entry as legacy execution runs it. A native
   replay runtime that the environment bound runs it as native play, so play
   and replay stage the same operations. A native replay runtime that is not
   bound yet, and a detached runtime, stage nothing and fail with
   RuntimeUnavailable. *)
Theorem native_execution_runs_spaces :
  forall (Op : Type) (ops : list Op) (native : bool),
    space_entry RtHost true ops = space_entry RtHost false ops /\
    space_entry (RtNativeReplay true) native ops = space_entry RtHost true ops /\
    space_entry RtHost true ops = (ops, None) /\
    space_entry (RtNativeReplay false) native ops = ([], Some RuntimeUnavailable) /\
    space_entry RtDetached native ops = ([], Some RuntimeUnavailable).
Proof.
  intros Op ops native. unfold space_entry, ensure_available.
  repeat split; reflexivity.
Qed.

(* ── 8. Receive queries (reducer_hooks.rs, P5b) ─────────────────────────── *)

(* The marker names of encoding.rs (marker_of): GPrivate([0xE0, m]) with m in
   0xD0..0xD4 (208..212), on a Par without the connective flag. QUERY is
   0xD3 (211). *)
Definition query_marker : nat := 211.

Definition is_marker_byte (m : nat) : bool := (208 <=? m) && (m <=? 212).

Definition marker_of (p : par) : option nat :=
  match p with
  | mkPar 0 0 0 [] [UPrivate [tag; m]] [] _ false =>
      if (tag =? reified_tag) && is_marker_byte m then Some m else None
  | _ => None
  end.

(* encoding.rs sole_marker_tuple: a Par that is exactly one tuple whose head
   is a marker name, with the marker and the elements after the head. *)
Definition sole_marker_tuple (p : par) : option (nat * list par) :=
  match sole_expr p with
  | Some (ETuple (h :: rest)) =>
      match marker_of h with Some m => Some (m, rest) | None => None end
  | _ => None
  end.

Definition is_marker_tuple (p : par) : bool :=
  match sole_marker_tuple p with Some _ => true | None => false end.

(* reducer_hooks.rs takes_fast_path: outside every use block, reified_consume
   calls the baseline consume only for a channel that routes to the default
   space and is no marker tuple. *)
Definition takes_fast_path (c : par) : bool :=
  routes_to_default_without_context c && negb (is_marker_tuple c).

Lemma sole_expr_shape :
  forall p e, sole_expr p = Some e -> exists lf cu, p = mkPar 0 0 0 [e] [] [] lf cu.
Proof.
  intros [[|a] [|b] [|c] [|e1 [|e2 es]] [|u us] [|bd bs] lf cu] e H; try discriminate H.
  destruct e1; try discriminate H; cbn in H; injection H as <-; exists lf, cu; reflexivity.
Qed.

Lemma marker_of_gprivate :
  forall h m, marker_of h = Some m ->
    sole_gprivate_id h = Some [reified_tag; m] /\ is_marker_byte m = true.
Proof.
  intros [[|a] [|b] [|c] [|e es] [| [id | n] [| u2 us]] [|bd bs] lf [|]] m H;
    try (cbv beta iota delta [marker_of] in H; discriminate H).
  all: destruct id as [| tag [| m0 [| x xs]]];
    cbv beta iota delta [marker_of] in H; try discriminate H.
  destruct ((tag =? reified_tag) && is_marker_byte m0) eqn:E; [| discriminate H].
  injection H as <-. apply andb_prop in E as [E1 E2]. apply Nat.eqb_eq in E1. subst tag.
  split; [reflexivity | exact E2].
Qed.

(* A marker tuple is a forged channel: it is no unforgeable name, and its head
   is a marker name, never the agent tag. *)
Lemma marker_tuple_forged : forall c, is_marker_tuple c = true -> classify_channel c = CForged.
Proof.
  intros c H. unfold is_marker_tuple, sole_marker_tuple in H.
  destruct (sole_expr c) as [e |] eqn:Ee; [| discriminate].
  destruct e as [| ps | ps rem | s | b | n]; try discriminate.
  destruct ps as [| h rest]; [discriminate |].
  destruct (marker_of h) as [m |] eqn:Em; [| discriminate].
  destruct (marker_of_gprivate h m Em) as [Hid Hm].
  destruct (sole_expr_shape c _ Ee) as [lf [cu ->]].
  unfold classify_channel, is_agent_tagged. cbn [sole_unforgeable sole_expr].
  destruct rest as [| a [| r [| x xs]]]; try reflexivity.
  rewrite Hid. unfold bytes_eqb.
  destruct (list_eq_dec Nat.eq_dec [reified_tag; m] [reified_tag; agent_tag]) as [E | E];
    [| reflexivity].
  exfalso.
  assert (Hmeq : nth 1 [reified_tag; m] 0 = nth 1 [reified_tag; agent_tag] 0)
    by (rewrite E; reflexivity).
  cbn [nth] in Hmeq. unfold is_marker_byte in Hm. apply andb_prop in Hm as [_ Hle].
  apply Nat.leb_le in Hle. unfold agent_tag in Hmeq. lia.
Qed.

Section Queries.
  Variable canon : par -> option bytes.
  (* The payload and the named params of a query (agent.rs QueryParams). *)
  Variable query : Type.
  (* The strict decoder of the elements of a QUERY tuple after its head, after
     the I-20 refresh (encoding.rs decode_query, which Encoding.v proves
     against its encoder): Some (inner channel, query) for the canonical image
     of a query, and None for every other QUERY tuple. *)
  Variable strict_query : list par -> option (par * query).

  Inductive source_read : Type :=
  | SrcPlain : source_read
  | SrcQuery : par -> query -> source_read
  | SrcMalformed : source_read.

  (* decode_query_source after refreshed_marker_par: a source without a QUERY
     tuple is the channel itself (SrcPlain). *)
  Definition decode_source (c : par) : source_read :=
    match sole_marker_tuple c with
    | Some (m, rest) =>
        if m =? query_marker then
          match strict_query rest with Some (ch, q) => SrcQuery ch q | None => SrcMalformed end
        else SrcPlain
    | None => SrcPlain
    end.

  (* Hook H4, reject_query_source: only a malformed QUERY tuple fails. *)
  Definition h4_passes (c : par) : bool :=
    match decode_source c with SrcMalformed => false | SrcPlain | SrcQuery _ _ => true end.

  (* readable_channel: the read rule of unbundle_receive. A bundle without the
     read flag fails with the text of HEAD; a bundle without a body fails with
     the error of unwrap_option_safe. *)
  Inductive read_result : Type := Readable : par -> read_result | NotReadable | NoBody.

  Definition read_channel (ch : par) : read_result :=
    match single_bundle ch with
    | Some (mkBundle (Some body) _ true) => Readable body
    | Some (mkBundle None _ true) => NoBody
    | Some (mkBundle _ _ false) => NotReadable
    | None => Readable ch
    end.

  Inductive recv_error : Type :=
  | RMalformed | RNotReadable | RNoBody | RKey | RQuerySpecUnsupported
  (* Changed by P8 step 3 (design v2 TR-1, TR-2): a receive on a user agent is
     translated, and a query on it fails with QuerySpecUnsupportedOnAgentSpace,
     so no receive fails with AgentSpacesUnavailable.
  | RAgentSpaces *)
  | RQuerySpecUnsupportedOnAgent.

  Inductive bind_read : Type :=
  | BindOk : par -> option query -> route_t -> bind_read
  | BindErr : recv_error -> bind_read.

  (* route_channel of the channel of a bind (route_receive). *)
  Definition route_bind (cx : ctx) (ch : par) (q : option query) : bind_read :=
    match route canon cx ch with RouteOk r => BindOk ch q r | RouteErr => BindErr RKey end.

  (* query_bind, then route_channel (route_receive). *)
  Definition read_bind (cx : ctx) (c : par) : bind_read :=
    match decode_source c with
    | SrcMalformed => BindErr RMalformed
    | SrcPlain => route_bind cx c None
    | SrcQuery inner q =>
        match read_channel inner with
        | Readable ch => route_bind cx ch (Some q)
        | NotReadable => BindErr RNotReadable
        | NoBody => BindErr RNoBody
        end
    end.

  (* TAgent a raw: the translation T_recv of a receive on the user agent a
     with the raw channel raw (P8 step 3, ReceiveRoute::Agent). *)
  Inductive recv_target : Type :=
  | THost : par -> recv_target
  | TSpace : native_space -> bytes -> par -> option query -> recv_target
  | TAgent : par -> par -> recv_target
  | TFail : recv_error -> recv_target.

  (* single_bind_route. *)
  Definition single_target (b : bind_read) : recv_target :=
    match b with
    | BindErr e => TFail e
    | BindOk ch None RDefault => THost ch
    | BindOk _ (Some _) RDefault => TFail RQuerySpecUnsupported
    | BindOk ch q (RSpace n k) => TSpace n k ch q
    (* Changed by P8 step 3 (design v2 TR-1, TR-2): a bind on a user agent is
       translated, and a query on it fails.
    | BindOk _ _ (RAgent _ _) => TFail RAgentSpaces *)
    | BindOk _ None (RAgent a raw) => TAgent a raw
    | BindOk _ (Some _) (RAgent _ _) => TFail RQuerySpecUnsupportedOnAgent
    end.

  (* reified_consume for one bind: the fast path outside every use block, and
     the routing path otherwise. *)
  Definition receive_one (cx : ctx) (c : par) : recv_target :=
    match cx with
    | None => if takes_fast_path c then THost c else single_target (read_bind cx c)
    | Some _ => single_target (read_bind cx c)
    end.

  (* The receive of P4 for one bind on a source that H4 passes: the fast path
     tests only routes_to_default_without_context, and the routing path routes
     the source itself. *)
  Definition p4_receive_one (cx : ctx) (c : par) : recv_target :=
    match cx with
    | None =>
        if routes_to_default_without_context c then THost c
        else single_target (route_bind cx c None)
    | Some _ => single_target (route_bind cx c None)
    end.

  (* The specification: the target of a receive on the inner channel
     inner_channel of a query q, under the context cx. *)
  Definition query_target (cx : ctx) (inner_channel : par) (q : query) : recv_target :=
    match read_channel inner_channel with
    | Readable ch =>
        match route canon cx ch with
        | RouteOk RDefault => TFail RQuerySpecUnsupported
        | RouteOk (RSpace n k) => TSpace n k ch (Some q)
        (* Changed by P8 step 3 (design v2 TR-2): a query on a user agent fails
           with QuerySpecUnsupportedOnAgentSpace.
        | RouteOk (RAgent _ _) => TFail RAgentSpaces *)
        | RouteOk (RAgent _ _) => TFail RQuerySpecUnsupportedOnAgent
        | RouteErr => TFail RKey
        end
    | NotReadable => TFail RNotReadable
    | NoBody => TFail RNoBody
    end.

  (* Headline 12. A receive with a QUERY tuple as its source routes the inner
     channel of the query (P5b):
     - H4 fails exactly on a malformed QUERY tuple, so a well-formed query
       passes it;
     - a QUERY tuple never takes the fast path, and the receive never becomes
       a consume of the host on any channel;
     - the receive applies the read rule of HEAD to the inner channel, and the
       route of the inner channel decides under every context: a native space
       gets the query, the default space fails with QuerySpecUnsupported, and
       a user agent fails with QuerySpecUnsupportedOnAgentSpace (P8 step 3);
     - a source without a QUERY tuple keeps the receive of P4. *)
  Theorem query_source_routes_inner_channel :
    forall cx c,
      (h4_passes c = false <-> decode_source c = SrcMalformed) /\
      (forall inner q, decode_source c = SrcQuery inner q ->
         takes_fast_path c = false /\
         (forall h, receive_one cx c <> THost h) /\
         receive_one cx c = query_target cx inner q) /\
      (decode_source c = SrcPlain -> receive_one cx c = p4_receive_one cx c).
  Proof.
    intros cx c. split; [| split].
    - unfold h4_passes. destruct (decode_source c); split; intros H;
        first [reflexivity | discriminate H].
    - intros inner q Hd.
      assert (Hfast : takes_fast_path c = false).
      { unfold takes_fast_path.
        assert (Hm : is_marker_tuple c = true).
        { unfold decode_source in Hd. unfold is_marker_tuple.
          destruct (sole_marker_tuple c) as [[m rest] |]; [reflexivity | discriminate Hd]. }
        rewrite Hm. cbn [negb]. apply andb_false_r. }
      assert (Hrecv : receive_one cx c = query_target cx inner q).
      { assert (Hroute : receive_one cx c = single_target (read_bind cx c)).
        { unfold receive_one. destruct cx; [reflexivity | rewrite Hfast; reflexivity]. }
        rewrite Hroute. unfold read_bind, query_target. rewrite Hd. cbv beta iota.
        destruct (read_channel inner) as [ch | |]; [| reflexivity | reflexivity].
        unfold route_bind. destruct (route canon cx ch) as [[| n k | a r] |]; reflexivity. }
      split; [exact Hfast | split; [| exact Hrecv]].
      intros h Hh. rewrite Hrecv in Hh. unfold query_target in Hh.
      destruct (read_channel inner) as [ch | |]; try discriminate Hh.
      destruct (route canon cx ch) as [[| n k | a r] |]; discriminate Hh.
    - intros Hd. unfold receive_one, p4_receive_one, read_bind. rewrite Hd.
      destruct cx as [t |]; [reflexivity |].
      unfold takes_fast_path. destruct (is_marker_tuple c) eqn:Em; cbn [negb].
      + rewrite andb_false_r.
        unfold routes_to_default_without_context, route_bind, route.
        rewrite (marker_tuple_forged c Em). reflexivity.
      + rewrite andb_true_r. reflexivity.
  Qed.
End Queries.

(* ── Headline assumptions ────────────────────────────────────────────── *)
Print Assumptions classify_space_ref_total_deterministic.
Print Assumptions handle_requires_write_flag.
Print Assumptions reified_non_handle_not_a_space_ref.
Print Assumptions route_total_deterministic.
Print Assumptions route_rules.
Print Assumptions space_name_route_ignores_ctx.
Print Assumptions r7_route_ignores_ctx.
Print Assumptions ordinary_channels_route_default.
Print Assumptions use_wrapper_restores_ctx.
Print Assumptions handle_dispatch_total.
Print Assumptions native_execution_runs_spaces.
Print Assumptions query_source_routes_inner_channel.
Print Assumptions agent_ref_requires_write_flag.
Print Assumptions r7_agent_reply_bundle_exact.
Print Assumptions tag_untag_id.
