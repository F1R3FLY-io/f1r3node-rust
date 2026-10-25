#!/usr/bin/env bash
#
# embedded.sh - assertion/design-for-verification CbC adapter for smart contracts
# and systems code with embedded requires/ensures. The prover is selected via
# CBC_PROVER (e.g. move, verus, dafny, z3, pi-formal-verify). For Dafny and
# Z3, the adapter prefers the pi-formal-verify CLI so CbC records objective
# formal-verifier evidence in addition to its ledger entry. Availability-gated:
# when no prover is configured/installed, verification is reported as
# unavailable (exit 3) rather than silently passing.
#
# Part of /cbc (EPIC-025). See docs/designs/cbc-skill.md.

cbc_adapter_name() { echo "embedded"; }

_CBC_EMBEDDED_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
_CBC_PROFILE_ROOT="$(cd "$_CBC_EMBEDDED_DIR/../../../.." && pwd)"

cbc_formal_verify_cli() {
	local candidate root
	if [[ -n "${PI_FORMAL_VERIFY_CLI:-}" && -x "$PI_FORMAL_VERIFY_CLI" ]]; then
		printf '%s\n' "$PI_FORMAL_VERIFY_CLI"
		return 0
	fi
	if [[ -n "${PI_FORMAL_VERIFY_PATH:-}" ]]; then
		candidate="$PI_FORMAL_VERIFY_PATH/scripts/formal-verify-cli.mjs"
		if [[ -x "$candidate" ]]; then
			printf '%s\n' "$candidate"
			return 0
		fi
	fi
	if command -v pi-formal-verify >/dev/null 2>&1; then
		command -v pi-formal-verify
		return 0
	fi

	# Standard SA workspace layout. Include both the source-tree root and the
	# caller's git root because this adapter can run from generated Pi skill
	# bundles as well as from AItools/scripts in the profile checkout.
	for root in "$_CBC_PROFILE_ROOT" "$(pwd)" "$(git rev-parse --show-toplevel 2>/dev/null || true)"; do
		[[ -n "$root" ]] || continue
		candidate="$root/../Issued_SmartAssets/pi-formal-verify/scripts/formal-verify-cli.mjs"
		if [[ -x "$candidate" ]]; then
			printf '%s\n' "$candidate"
			return 0
		fi
	done
	return 1
}

cbc_embedded_backend_for() {
	local prover="${1:-}" artifact="${2:-}"
	case "$prover" in
	dafny | z3) printf '%s\n' "$prover" ;;
	pi-formal-verify | formal-verify)
		case "$artifact" in
		*.dfy) printf 'dafny\n' ;;
		*.smt | *.smt2) printf 'z3\n' ;;
		*) return 1 ;;
		esac
		;;
	*) return 1 ;;
	esac
}

cbc_adapter_available() {
	local prover="${CBC_PROVER:-}"
	[[ -n "$prover" ]] || return 1
	if cbc_embedded_backend_for "$prover" "${CBC_ARTIFACT_HINT:-}" >/dev/null 2>&1; then
		cbc_formal_verify_cli >/dev/null 2>&1 || command -v "$prover" >/dev/null 2>&1
		return $?
	fi
	command -v "$prover" >/dev/null 2>&1
}

cbc_formal_evidence_path() {
	local artifact="$1" backend="$2" dir id ts run_dir
	dir=$(python3 "$_CBC_EMBEDDED_DIR/../cbc-attributes.py" evidence-formal "$artifact") || return 3
	mkdir -p "$dir" || return 3
	id=$(printf '%s' "$artifact" | tr -c 'A-Za-z0-9' '-' | tr -s '-')
	id="${id#-}"
	id="${id%-}"
	ts=$(date -u +%Y%m%dT%H%M%SZ)
	# Allocate a unique report path so repeated runs cannot overwrite evidence.
	run_dir=$(mktemp -d "$dir/$backend-$id-$ts-XXXXXX") || return 3
	printf '%s/evidence.json\n' "$run_dir"
}

cbc_adapter_verify_formal() {
	local artifact="$1" claim_file="$2" out="$3" backend="$4" cli="$5"
	local formal_out rc=0
	formal_out="$(cbc_formal_evidence_path "$artifact" "$backend")" || return 3

	local args=(verify --backend "$backend" --path "$artifact" --output "$formal_out")
	[[ -n "$claim_file" && -f "$claim_file" ]] && args+=(--claim-file "$claim_file")

	"$cli" "${args[@]}" >/dev/null 2>&1 || rc=$?
	if [[ ! -f "$formal_out" ]]; then
		jq -n --arg detail "pi-formal-verify did not write evidence (exit $rc)" \
			'{kind:"error", ref:null, counterexample:null, detail:$detail}' >"$out"
		return 3
	fi

	local status
	status=$(jq -r '.status // "tool_error"' "$formal_out" 2>/dev/null || echo tool_error)
	jq -n --slurpfile formal "$formal_out" --arg ref "$formal_out" --arg backend "$backend" \
		'$formal[0] as $f |
         {kind:"formal-verification",
          ref:$ref,
          counterexample:(if ($f.status // "") == "refuted" then ((($f.stdout // "") + "\n" + ($f.stderr // "")) | ltrimstr("\n") | rtrimstr("\n")) else null end),
          detail:("pi-formal-verify " + $backend + " status: " + ($f.status // "tool_error")),
          formal_evidence:$f}' >"$out"

	case "$status" in
	discharged) return 0 ;;
	refuted) return 2 ;;
	*) return 3 ;;
	esac
}

cbc_adapter_verify_z3_raw() {
	local artifact="$1" out="$2" log rc=0 first=""
	log=$(z3 "$artifact" 2>&1) || rc=$?
	if [[ $rc -ne 0 ]]; then
		jq -n --arg detail "$log" '{kind:"error", ref:"z3", counterexample:null, detail:$detail}' >"$out"
		return 3
	fi
	first=$(printf '%s\n' "$log" | awk '/^(sat|unsat|unknown)$/ { print; exit }')
	case "$first" in
	unsat)
		jq -n --arg detail "$log" '{kind:"formal-verification", ref:"z3", counterexample:null, detail:("z3 status: discharged\n" + $detail)}' >"$out"
		return 0
		;;
	sat)
		jq -n --arg ce "$log" '{kind:"formal-verification", ref:"z3", counterexample:$ce, detail:"z3 status: refuted"}' >"$out"
		return 2
		;;
	*)
		jq -n --arg detail "$log" '{kind:"error", ref:"z3", counterexample:null, detail:("z3 status: unknown\n" + $detail)}' >"$out"
		return 3
		;;
	esac
}

cbc_adapter_verify() {
	local artifact="$1" claim_file="$2" out="$3"
	local prover="${CBC_PROVER:-}"
	if [[ -z "$prover" ]]; then
		jq -n '{kind:"error", ref:null, counterexample:null, detail:"embedded: prover not available (set CBC_PROVER to an installed prover)"}' >"$out"
		return 3
	fi

	local backend=""
	backend=$(cbc_embedded_backend_for "$prover" "$artifact" 2>/dev/null || true)
	if [[ -n "$backend" ]]; then
		local cli=""
		cli=$(cbc_formal_verify_cli 2>/dev/null || true)
		if [[ -n "$cli" ]]; then
			cbc_adapter_verify_formal "$artifact" "$claim_file" "$out" "$backend" "$cli"
			return $?
		fi
		if [[ "$backend" == "z3" ]] && command -v z3 >/dev/null 2>&1; then
			cbc_adapter_verify_z3_raw "$artifact" "$out"
			return $?
		fi
	fi

	if ! command -v "$prover" >/dev/null 2>&1; then
		jq -n '{kind:"error", ref:null, counterexample:null, detail:"embedded: prover not available (set CBC_PROVER to an installed prover)"}' >"$out"
		return 3
	fi
	local log rc=0
	log=$("$prover" "$artifact" 2>&1) || rc=$?
	if [[ $rc -eq 0 ]]; then
		jq -n --arg ref "$prover" '{kind:"discharged-assertions", ref:$ref, counterexample:null, detail:"assertions discharged by prover"}' >"$out"
		return 0
	fi
	jq -n --arg ce "$log" '{kind:"counterexample", ref:null, counterexample:$ce, detail:"prover refuted"}' >"$out"
	return 2
}
