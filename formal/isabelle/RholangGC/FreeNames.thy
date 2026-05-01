(*
  FreeNames.thy --- free names, bound atoms, and escape analysis.

  Defines:
    free_names_par   :: par => name set
    bn_new_par       :: par => atom set
    sync_chans_send  :: par => name set
    sync_chans_recv  :: par => name set
    payload_names    :: par => name set
    escapes_in_par   :: par => atom => bool

  These are the building blocks for GC0 and GC1 in Garbage.thy.
*)

theory FreeNames
  imports Reduction
begin

text \<open>
  Free names of a process: names occurring in send-channel, receive-channel,
  payload, or guard positions, after stripping bundles.  Definitions are
  given by mutual recursion over the syntactic structure.
\<close>

primrec
  free_names_par  :: "par \<Rightarrow> name set" and
  free_names_name :: "name \<Rightarrow> name set"
where
  "free_names_par Nil = {}"
| "free_names_par (PPar p q) = free_names_par p \<union> free_names_par q"
| "free_names_par (Send c ds _) =
     {c} \<union> free_names_name c \<union> (\<Union>p \<in> set ds. free_names_par p)"
| "free_names_par (Recv binds body _ _ guard) =
     (\<Union>(ps, c) \<in> set binds. {c} \<union> free_names_name c \<union> (\<Union>p \<in> set ps. free_names_par p))
     \<union> free_names_par body
     \<union> (case guard of None \<Rightarrow> {} | Some g \<Rightarrow> free_names_par g)"
| "free_names_par (NewN _ body) = free_names_par body"
   \<comment> \<open>The bound atoms are stripped at the level of \<^const>\<open>atoms_of_par\<close>; here we only
       look at name shapes, and the binder does not remove name occurrences (a
       \<^const>\<open>GPrivate\<close> referring to a bound atom is still a name; whether it counts as
       \<^emph>\<open>free\<close> w.r.t.\ the binder is captured by the atom-level analyses below).\<close>
| "free_names_par (Match tgt cases) =
     free_names_par tgt
     \<union> (\<Union>(pat, gd, body) \<in> set cases.
          free_names_par pat
          \<union> (case gd of None \<Rightarrow> {} | Some g \<Rightarrow> free_names_par g)
          \<union> free_names_par body)"
| "free_names_par (IfThenElse c t e) =
     free_names_par c \<union> free_names_par t \<union> free_names_par e"
| "free_names_par (EvalQuote n) = {n} \<union> free_names_name n"
| "free_names_par (EExpr e) =
     expr_subterm_names e
     \<union> (\<Union>n \<in> expr_subterm_names e. free_names_name n)
     \<union> (\<Union>p \<in> expr_subterm_pars e. free_names_par p)"

| "free_names_name (GPrivate _)   = {}"
| "free_names_name (GDeployId _)  = {}"
| "free_names_name (GDeployerId _) = {}"
| "free_names_name GSysAuthToken  = {}"
| "free_names_name (GUri _)       = {}"
| "free_names_name (Quote p)      = free_names_par p"
| "free_names_name (Bundle _ n)   = {n} \<union> free_names_name n"

text \<open>Bound atoms introduced by all \<open>new\<close> binders in \<open>P\<close>.\<close>

primrec bn_new_par :: "par \<Rightarrow> atom set" where
  "bn_new_par Nil = {}"
| "bn_new_par (PPar p q) = bn_new_par p \<union> bn_new_par q"
| "bn_new_par (Send _ ds _) = (\<Union>p \<in> set ds. bn_new_par p)"
| "bn_new_par (Recv binds body _ _ guard) =
     (\<Union>(ps, _) \<in> set binds. (\<Union>p \<in> set ps. bn_new_par p))
     \<union> bn_new_par body
     \<union> (case guard of None \<Rightarrow> {} | Some g \<Rightarrow> bn_new_par g)"
| "bn_new_par (NewN bound body) = bound \<union> bn_new_par body"
| "bn_new_par (Match tgt cases) =
     bn_new_par tgt
     \<union> (\<Union>(pat, gd, body) \<in> set cases.
          bn_new_par pat
          \<union> (case gd of None \<Rightarrow> {} | Some g \<Rightarrow> bn_new_par g)
          \<union> bn_new_par body)"
| "bn_new_par (IfThenElse c t e) = bn_new_par c \<union> bn_new_par t \<union> bn_new_par e"
| "bn_new_par (EvalQuote _) = {}"
| "bn_new_par (EExpr e) = (\<Union>p \<in> expr_subterm_pars e. bn_new_par p)"

text \<open>
  Names appearing in send-channel position: the head of a \<^const>\<open>Send\<close>.  Used by
  GC1's one-sided analysis.
\<close>

primrec sync_chans_send :: "par \<Rightarrow> name set" where
  "sync_chans_send Nil = {}"
| "sync_chans_send (PPar p q) = sync_chans_send p \<union> sync_chans_send q"
| "sync_chans_send (Send c _ _) = {c}"
| "sync_chans_send (Recv _ body _ _ _) = sync_chans_send body"
| "sync_chans_send (NewN _ body) = sync_chans_send body"
| "sync_chans_send (Match _ cases) =
     (\<Union>(_, _, body) \<in> set cases. sync_chans_send body)"
| "sync_chans_send (IfThenElse _ t e) = sync_chans_send t \<union> sync_chans_send e"
| "sync_chans_send (EvalQuote _) = {}"
| "sync_chans_send (EExpr e) = (\<Union>p \<in> expr_subterm_pars e. sync_chans_send p)"

text \<open>Names appearing in receive-channel position (any bind of any \<^const>\<open>Recv\<close>).\<close>

primrec sync_chans_recv :: "par \<Rightarrow> name set" where
  "sync_chans_recv Nil = {}"
| "sync_chans_recv (PPar p q) = sync_chans_recv p \<union> sync_chans_recv q"
| "sync_chans_recv (Send _ _ _) = {}"
| "sync_chans_recv (Recv binds body _ _ _) =
     ((\<Union>(_, c) \<in> set binds. {c}) \<union> sync_chans_recv body)"
| "sync_chans_recv (NewN _ body) = sync_chans_recv body"
| "sync_chans_recv (Match _ cases) =
     (\<Union>(_, _, body) \<in> set cases. sync_chans_recv body)"
| "sync_chans_recv (IfThenElse _ t e) = sync_chans_recv t \<union> sync_chans_recv e"
| "sync_chans_recv (EvalQuote _) = {}"
| "sync_chans_recv (EExpr e) = (\<Union>p \<in> expr_subterm_pars e. sync_chans_recv p)"

text \<open>
  Names appearing as a sub-term of any send payload reachable in \<open>P\<close>,
  including through quotation.  An atom escapes \<open>P\<close> if it occurs inside
  \<^const>\<open>payload_names\<close>.
\<close>

primrec payload_names :: "par \<Rightarrow> name set" where
  "payload_names Nil = {}"
| "payload_names (PPar p q) = payload_names p \<union> payload_names q"
| "payload_names (Send _ ds _) =
     (\<Union>p \<in> set ds. payload_names p \<union> free_names_par p)"
| "payload_names (Recv binds body _ _ _) =
     ((\<Union>(_, _) \<in> set binds. {}) \<union> payload_names body)"
| "payload_names (NewN _ body) = payload_names body"
| "payload_names (Match _ cases) =
     (\<Union>(_, _, body) \<in> set cases. payload_names body)"
| "payload_names (IfThenElse _ t e) = payload_names t \<union> payload_names e"
| "payload_names (EvalQuote _) = {}"
| "payload_names (EExpr e) = (\<Union>p \<in> expr_subterm_pars e. payload_names p)"

text \<open>Atom-level escape: an atom escapes if it appears in any payload name.\<close>

definition escapes_in_par :: "par \<Rightarrow> atom \<Rightarrow> bool" where
  "escapes_in_par P a \<longleftrightarrow> (\<exists>n \<in> payload_names P. a \<in> atoms_of_name n)"

end
