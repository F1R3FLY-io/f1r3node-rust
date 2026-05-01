(*
  Names.thy --- operations on names: atoms_of, forgeability, bundle utilities.
*)

theory Names
  imports Syntax
begin

text \<open>
  The set of \<open>GPrivate\<close> atoms occurring anywhere inside a name or a process,
  including under quotation and under bundle wrappers.
\<close>

primrec
  atoms_of_name :: "name \<Rightarrow> atom set" and
  atoms_of_par  :: "par \<Rightarrow> atom set"
where
  "atoms_of_name (GPrivate a) = {a}"
| "atoms_of_name (GDeployId _) = {}"
| "atoms_of_name (GDeployerId _) = {}"
| "atoms_of_name GSysAuthToken = {}"
| "atoms_of_name (GUri _) = {}"
| "atoms_of_name (Quote p) = atoms_of_par p"
| "atoms_of_name (Bundle _ n) = atoms_of_name n"

| "atoms_of_par Nil = {}"
| "atoms_of_par (PPar p q) = atoms_of_par p \<union> atoms_of_par q"
| "atoms_of_par (Send c d _) = atoms_of_name c \<union> atoms_of_par d"
| "atoms_of_par (Recv pat c body _ _ guard) =
     atoms_of_par pat \<union> atoms_of_name c \<union> atoms_of_par body \<union> atoms_of_par guard"
| "atoms_of_par (NewN bound body) = atoms_of_par body - set bound"
| "atoms_of_par (MatchOne tgt pat gd body fall) =
     atoms_of_par tgt \<union> atoms_of_par pat \<union> atoms_of_par gd
     \<union> atoms_of_par body \<union> atoms_of_par fall"
| "atoms_of_par (IfThenElse c t e) =
     atoms_of_par c \<union> atoms_of_par t \<union> atoms_of_par e"
| "atoms_of_par (EvalQuote n) = atoms_of_name n"
| "atoms_of_par (EExpr ps ns) =
     \<Union> (set (map atoms_of_par ps)) \<union> \<Union> (set (map atoms_of_name ns))"

text \<open>
  Forgeability of a name relative to a knowledge set \<open>K\<close>.  A name is
  forgeable if every private atom inside it is either in \<open>K\<close> or in the
  public ambient set \<open>pub\<close>.
\<close>

definition forgeable_by :: "name \<Rightarrow> atom set \<Rightarrow> bool" where
  "forgeable_by n K \<longleftrightarrow> atoms_of_name n \<subseteq> K \<union> pub"

text \<open>Effective bundle capability of a name, collapsing nested wrappers.\<close>

primrec bundle_cap_of_name :: "name \<Rightarrow> cap" where
  "bundle_cap_of_name (GPrivate _)   = CapRW"
| "bundle_cap_of_name (GDeployId _)  = CapRW"
| "bundle_cap_of_name (GDeployerId _) = CapRW"
| "bundle_cap_of_name GSysAuthToken  = CapRW"
| "bundle_cap_of_name (GUri _)       = CapRW"
| "bundle_cap_of_name (Quote _)      = CapRW"
| "bundle_cap_of_name (Bundle c n)   = cap_meet c (bundle_cap_of_name n)"

abbreviation bundle_cap_of :: "name \<Rightarrow> cap" where
  "bundle_cap_of n \<equiv> bundle_cap_of_name n"

text \<open>The underlying name with all bundle wrappers stripped.\<close>

primrec strip_bundle :: "name \<Rightarrow> name" where
  "strip_bundle (GPrivate a)    = GPrivate a"
| "strip_bundle (GDeployId b)   = GDeployId b"
| "strip_bundle (GDeployerId b) = GDeployerId b"
| "strip_bundle GSysAuthToken   = GSysAuthToken"
| "strip_bundle (GUri u)        = GUri u"
| "strip_bundle (Quote p)       = Quote p"
| "strip_bundle (Bundle _ n)    = strip_bundle n"

text \<open>Bundle stripping preserves the atom set.\<close>

lemma atoms_of_strip_bundle: "atoms_of_name (strip_bundle n) = atoms_of_name n"
  by (induction n) auto

lemma strip_bundle_atoms_eq:
  assumes "strip_bundle n1 = strip_bundle n2"
  shows "atoms_of_name n1 = atoms_of_name n2"
  using assms atoms_of_strip_bundle by metis

text \<open>
  Bound atoms introduced by all \<open>new\<close> binders in a process or in any
  process reachable through quotations carried by a name.  We use a mutual
  primrec to recurse through \<open>Quote\<close> and \<open>Bundle\<close>.

  Defined here (rather than alongside the free-name analyses in
  FreeNames.thy) so that \<^file>\<open>Patterns.thy\<close> can refer to it in matcher
  safety axioms.
\<close>

primrec
  bn_new_name :: "name \<Rightarrow> atom set" and
  bn_new_par  :: "par \<Rightarrow> atom set"
where
  "bn_new_name (GPrivate _)    = {}"
| "bn_new_name (GDeployId _)   = {}"
| "bn_new_name (GDeployerId _) = {}"
| "bn_new_name GSysAuthToken   = {}"
| "bn_new_name (GUri _)        = {}"
| "bn_new_name (Quote p)       = bn_new_par p"
| "bn_new_name (Bundle _ n)    = bn_new_name n"

| "bn_new_par Nil = {}"
| "bn_new_par (PPar p q) = bn_new_par p \<union> bn_new_par q"
| "bn_new_par (Send c d _) = bn_new_name c \<union> bn_new_par d"
| "bn_new_par (Recv pat c body _ _ guard) =
     bn_new_par pat \<union> bn_new_name c \<union> bn_new_par body \<union> bn_new_par guard"
| "bn_new_par (NewN bound body) = set bound \<union> bn_new_par body"
| "bn_new_par (MatchOne tgt pat gd body fall) =
     bn_new_par tgt \<union> bn_new_par pat \<union> bn_new_par gd
     \<union> bn_new_par body \<union> bn_new_par fall"
| "bn_new_par (IfThenElse c t e) = bn_new_par c \<union> bn_new_par t \<union> bn_new_par e"
| "bn_new_par (EvalQuote n) = bn_new_name n"
| "bn_new_par (EExpr ps ns) =
     \<Union> (set (map bn_new_par ps)) \<union> \<Union> (set (map bn_new_name ns))"

end
