import json
import os
import shutil
import subprocess
import sys
from pathlib import Path


root = Path(__file__).resolve().parent.parent
fixture = json.loads((root / "test-vectors/legacy-golden-corpus-v1.json").read_text())
pin = fixture["sources"]["current"]["revision"]
scratch = root / "target/verification/reified-rspaces"
checkout = scratch / "cost-base-checkout"
build = scratch / "cost-base-build"
probe = scratch / "cost-base-probe"
probe_build = scratch / "cost-base-probe-build"

if not checkout.exists():
    subprocess.run(
        ["git", "clone", "--shared", "--no-checkout", str(root), str(checkout)],
        check=True,
    )
    subprocess.run(["git", "-c", "core.fsmonitor=false", "-C", str(checkout), "checkout", "--detach", pin], check=True)

revision = subprocess.check_output(
    ["git", "-c", "core.fsmonitor=false", "-C", str(checkout), "rev-parse", "HEAD"], text=True
).strip()
assert revision == pin, (revision, pin)
status = subprocess.check_output(
    ["git", "-c", "core.fsmonitor=false", "-C", str(checkout), "status", "--porcelain"], text=True
)
assert not status, status

(probe / "tests").mkdir(parents=True, exist_ok=True)
(probe_build / "tmp").mkdir(parents=True, exist_ok=True)
(probe / "Cargo.toml").write_text(
    '[package]\n'
    'name = "cost-base-capture-probe"\n'
    'version = "0.1.0"\n'
    'edition = "2021"\n\n'
    '[workspace]\n\n'
    '[dependencies]\n'
    'casper = { path = "../cost-base-checkout/casper" }\n'
    'models = { path = "../cost-base-checkout/models" }\n'
    'rspace_plus_plus = { path = "../cost-base-checkout/rspace++" }\n'
    'tokio = { version = "1", features = ["macros", "rt-multi-thread"] }\n'
    'prost = "0.14.3"\n'
    'hex = "0.4.3"\n'
    'bincode = "1.3.3"\n'
)
shutil.copyfile(checkout / "Cargo.lock", probe / "Cargo.lock")
shutil.copyfile(
    root / "scripts/cost-base-rspace-capture.rs",
    probe / "tests/cost_base_rspace_capture.rs",
)
shutil.copyfile(
    root / "scripts/cost-base-models-capture.rs",
    probe / "tests/cost_base_models_capture.rs",
)

probe_env = os.environ.copy()
probe_env.update(
    TMPDIR=str(probe_build / "tmp"),
    CARGO_TARGET_DIR=str(probe_build),
    CARGO_BUILD_JOBS="2",
    CARGO_INCREMENTAL="0",
    CARGO_PROFILE_TEST_DEBUG="0",
)
probe_result = subprocess.run(
    [
        "cargo",
        "+nightly-2026-02-09",
        "test",
        "--manifest-path",
        str(probe / "Cargo.toml"),
        "--offline",
        "--",
        "--nocapture",
    ],
    cwd=root,
    env=probe_env,
    capture_output=True,
    text=True,
)
print(probe_result.stdout, end="")
print(probe_result.stderr[-12000 if probe_result.returncode else -1000:], end="", file=sys.stderr)
probe_result.check_returncode()
captured = dict(
    line.split("=", 1)
    for line in probe_result.stdout.splitlines()
    if line.startswith("COST_BASE_") and "=" in line
)
expected = {
    "COST_BASE_EMPTY_ROOT": fixture["state_roots"]["shared_empty_radix"],
    "COST_BASE_NONEMPTY_ROOT": fixture["state_roots"]["current_nonempty_rspace"],
    "COST_BASE_REPLAY_ROOT": fixture["state_roots"]["current_nonempty_rspace"],
    "COST_BASE_PRODUCE_HASH": fixture["event_log"]["produce_alpha_datum_linear"],
    "COST_BASE_EVENT_BYTES": fixture["event_log"]["current_produce_alpha_datum_linear_full_io_event_bytes"],
    "COST_BASE_EVENT_PROTO_BYTES": fixture["event_log"]["legacy_pre_feature_produce_alpha_datum_linear_event_proto_bytes"],
    "COST_BASE_PAR_NIL": fixture["par_wire"]["nil"],
    "COST_BASE_PAR_GINT_ONE": fixture["par_wire"]["gint_one"],
    "COST_BASE_SORT_TIE_FORWARD": json.dumps(
        fixture["sorting_ties"]["equal_score_send_locally_free_order"]
    ),
    "COST_BASE_SORT_TIE_REVERSE": json.dumps(
        fixture["sorting_ties"]["equal_score_send_locally_free_reverse_order"]
    ),
}
for key, value in expected.items():
    assert captured.get(key) == value, (key, captured.get(key), value)

(build / "tmp").mkdir(parents=True, exist_ok=True)
env = os.environ.copy()
env.update(
    TMPDIR=str(build / "tmp"),
    CARGO_TARGET_DIR=str(build),
    CARGO_BUILD_JOBS="2",
    CARGO_INCREMENTAL="0",
    CARGO_PROFILE_TEST_DEBUG="0",
)
result = subprocess.run(
    [
        "cargo",
        "+nightly-2026-02-09",
        "test",
        "--manifest-path",
        str(checkout / "Cargo.toml"),
        "--offline",
        "--locked",
        "-p",
        "casper",
        "--test",
        "mod",
        "runtime_spec",
    ],
    cwd=root,
    env=env,
    capture_output=True,
    text=True,
)
print(result.stdout, end="")
print(result.stderr[-12000 if result.returncode else -1000:], end="", file=sys.stderr)
result.check_returncode()
assert "empty_state_hash_should_be_the_same_as_hard_coded_cached_value ... ok" in result.stdout
assert "state_hash_after_fixed_rholang_term_execution_should_be_hash_fixed_without_hard_fork ... ok" in result.stdout
