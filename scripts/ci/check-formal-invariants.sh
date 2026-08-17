#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

run_tla=false
run_rocq=false
exhaustive=false

usage() {
    cat <<'EOF'
Usage: scripts/ci/check-formal-invariants.sh [--all|--tla|--rocq] [--exhaustive]

  --all         Run TLA+ and Rocq verification. This is the default.
  --tla         Run only TLA+ verification.
  --rocq        Run only Rocq verification.
  --exhaustive  Add the exhaustive TLA+ configurations.
EOF
}

while (($#)); do
    case "$1" in
        --all)
            run_tla=true
            run_rocq=true
            ;;
        --tla)
            run_tla=true
            ;;
        --rocq)
            run_rocq=true
            ;;
        --exhaustive)
            exhaustive=true
            ;;
        --help|-h)
            usage
            exit 0
            ;;
        *)
            echo "ERROR: unknown option: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
    shift
done

if [[ "$run_tla" == false && "$run_rocq" == false ]]; then
    run_tla=true
    run_rocq=true
fi

cleanup_paths=()
cleanup() {
    if ((${#cleanup_paths[@]})); then
        rm -f "${cleanup_paths[@]}"
    fi
}
trap cleanup EXIT

run_tla_checks() {
    echo "== TLA+ invariant verification =="
    if [[ "$exhaustive" == true ]]; then
        RUN_EXHAUSTIVE_TLA=1 bash "$SCRIPT_DIR/check-tla-invariants.sh"
    else
        RUN_EXHAUSTIVE_TLA=0 bash "$SCRIPT_DIR/check-tla-invariants.sh"
    fi
}

build_rocq_project() {
    local project="$1"
    local namespace="$2"
    local project_dir="$REPO_ROOT/formal/rocq/$project"
    local makefile="Makefile.verify.$$"

    test -d "$project_dir/theories"
    cleanup_paths+=(
        "$project_dir/$makefile"
        "$project_dir/$makefile.conf"
        "$project_dir/.$makefile.d"
    )

    (
        cd "$project_dir"
        coq_makefile -f _CoqProject -o "$makefile"
        make -f "$makefile" -j1
        coqchk -Q theories "$namespace" "$namespace.MainTheorem" \
            >"/tmp/${project}-coqchk.log" 2>&1
    )

    grep -q "Modules were successfully checked" "/tmp/${project}-coqchk.log"
}

check_assumptions() {
    local project="$1"
    local namespace="$2"
    local expected="$3"
    shift 3
    local project_dir="$REPO_ROOT/formal/rocq/$project"
    local check_file
    local output
    local closed

    check_file="$(mktemp "/tmp/${namespace}AssumptionsXXXXXX.v")"
    cleanup_paths+=("$check_file")

    {
        echo "From $namespace Require Import MainTheorem."
        for theorem in "$@"; do
            echo "Print Assumptions $theorem."
        done
    } >"$check_file"

    output="$(cd "$project_dir" && coqc -Q theories "$namespace" "$check_file" 2>&1)"
    printf '%s\n' "$output"
    closed="$(grep -c "Closed under the global context" <<<"$output" || true)"
    if [[ "$closed" -ne "$expected" ]]; then
        echo "ERROR: expected $expected closed $project trust bases, found $closed" >&2
        return 1
    fi
}

run_rocq_checks() {
    echo "== Rocq kernel verification =="
    command -v coq_makefile >/dev/null
    command -v coqc >/dev/null
    command -v coqchk >/dev/null

    ! grep -rnE "^[[:space:]]*(Axioms?|Admitted|Parameters?|Conjectures?|Hypothes[ie]s)\b" \
        "$REPO_ROOT/formal/rocq/slashing/theories/"
    ! grep -rnE "^[[:space:]]*(Axioms?|Admitted|Parameters?|Conjectures?)\b" \
        "$REPO_ROOT/formal/rocq/fork_choice/theories/"

    build_rocq_project slashing Slashing
    check_assumptions slashing Slashing 2 \
        main_bisimilarity_theorem \
        main_bisimilarity_strong

    build_rocq_project fork_choice ForkChoice
    check_assumptions fork_choice ForkChoice 4 \
        fork_choice_determinism_correct \
        fork_choice_ghost_correct \
        fork_choice_bound_correct \
        fork_choice_bridge_correct
}

if [[ "$run_tla" == true ]]; then
    run_tla_checks
fi

if [[ "$run_rocq" == true ]]; then
    run_rocq_checks
fi

echo "Formal verification completed successfully."
