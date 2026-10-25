#!/usr/bin/env python3
"""Content-bound CbC records and independently signed maintainer acceptance."""

import hashlib
import json
import os
import re
import stat
import subprocess
import sys
import tempfile
import time
import uuid
from collections.abc import Iterator
from contextlib import contextmanager
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from typing import Any


def unique_object(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate CbC JSON key")
        result[key] = value
    return result


def parse_json(text: str) -> Any:
    try:
        return json.loads(text, object_pairs_hook=unique_object)
    except (ValueError, TypeError) as error:
        raise ValueError("invalid CbC JSON") from error


def canonical(value: Any) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":")).encode()


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def git(*args: str) -> str:
    return subprocess.check_output(["git", *args], stderr=subprocess.DEVNULL).decode().strip()


def checked_path(path: Path) -> Path:
    root = Path(git("rev-parse", "--show-toplevel")).resolve()
    try:
        local = path.resolve().relative_to(root)
    except (ValueError, OSError, RuntimeError) as error:
        raise ValueError("evidence must remain inside the worktree") from error
    if any(part.casefold() == ".git" for part in local.parts) or (path.is_file() and path.stat().st_nlink > 1):
        raise ValueError("unsafe evidence destination")
    return path


def artifact_path(value: str) -> str:
    root = Path(git("rev-parse", "--show-toplevel")).resolve()
    path = Path(os.path.abspath(value))
    if not path.resolve(strict=True).is_relative_to(root):
        raise ValueError("artifact or claim resolves outside the repository")
    return path.relative_to(root).as_posix()


def snapshot(artifact: str, claim_file: str) -> dict[str, Any]:
    path = artifact_path(artifact)
    claim = None
    if claim_file:
        claim = {"path": artifact_path(claim_file),
                 "sha256": digest(Path(claim_file).read_bytes())}
    return {"artifact": path, "source_sha256": digest(Path(artifact).read_bytes()),
            "claim": claim}


def archive(path: Path) -> None:
    """Copy old bytes into content-addressed history without changing the original."""
    checked_path(path)
    if not path.exists():
        return
    archive_bytes(path, path.read_bytes())


def archive_bytes(path: Path, data: bytes) -> None:
    target = checked_path(path.parent / "history" / (digest(data) + "-" + path.name))
    target.parent.mkdir(parents=True, exist_ok=True)
    if target.exists():
        if target.read_bytes() != data:
            raise ValueError("history collision")
    else:
        with target.open("xb") as stream:
            stream.write(data)


def replace(path: Path, data: bytes) -> None:
    checked_path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    archive(path)
    archive_bytes(path, data)
    fd, tmp = tempfile.mkstemp(dir=path.parent)
    try:
        with os.fdopen(fd, "wb") as stream:
            stream.write(data)
        os.replace(tmp, path)
    finally:
        if os.path.exists(tmp):
            os.unlink(tmp)


def read_record(ledger: Path) -> dict[str, Any]:
    text = checked_path(ledger).read_text()
    blocks = re.findall(r"^```json\n(.*?)^```[ \t]*$", text, re.MULTILINE | re.DOTALL)
    if len(blocks) != 1:
        raise ValueError("expected one evidence record")
    record = parse_json(blocks[0])
    if not isinstance(record, dict):
        raise ValueError("invalid evidence record")
    return record


CLAIM_ID = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]{0,127}\Z")
JSON_BLOCK = re.compile(rb"^```json\r?\n(.*?)^```[ \t]*\r?$", re.MULTILINE | re.DOTALL)
STATUS_ORDER = ("invalid_record", "refuted", "error", "pending", "none",
                "acceptance_required", "waived", "discharged")
RECORD_STATUSES = {"discharged", "waived", "pending", "refuted", "error"}


def claim_identifier(value: Any) -> str:
    if not isinstance(value, str) or not CLAIM_ID.fullmatch(value):
        raise ValueError("invalid claim identifier")
    return value


def inventory_path(ledger: Path) -> Path:
    return checked_path(ledger.with_suffix(".claims.json"))


def claim_acceptance_path(ledger: Path, claim_id: str) -> Path:
    directory = checked_path(ledger.with_suffix(".claims"))
    return checked_path(directory / (digest(claim_identifier(claim_id).encode()) + ".acceptance.json"))


def has_inventory_history(ledger: Path) -> bool:
    history = checked_path(ledger.parent / "history")
    if not history.exists() and not history.is_symlink():
        return False
    if not history.is_dir():
        raise ValueError("invalid evidence history directory")
    pattern = re.compile(r"[0-9a-f]{64}-" + re.escape(ledger.with_suffix(".claims.json").name))
    found = False
    for path in history.iterdir():
        if pattern.fullmatch(path.name):
            checked_path(path)
            if not path.is_file():
                raise ValueError("invalid archived claim inventory")
            found = True
    return found


@dataclass
class LedgerView:
    data: bytes
    blocks: list[Any]
    records: dict[str, dict[str, Any]]
    inventory: dict[str, Any] | None
    named: bool
    artifact: str | None
    errors: list[str]

    @property
    def ids(self) -> list[str]:
        return self.inventory["claim_ids"] if self.inventory is not None else sorted(self.records)


def read_view(artifact: str, ledger: Path, *, declaring: bool = False) -> LedgerView:
    data = checked_path(ledger).read_bytes() if ledger.is_file() else b""
    blocks = list(JSON_BLOCK.finditer(data))
    inventory_file = inventory_path(ledger)
    claim_directory = checked_path(ledger.with_suffix(".claims"))
    has_claim_directory = claim_directory.exists() or claim_directory.is_symlink()
    if has_claim_directory and not claim_directory.is_dir():
        raise ValueError("invalid claim acceptance directory")
    prior_named_metadata = has_inventory_history(ledger) or has_claim_directory
    has_inventory = inventory_file.exists() or inventory_file.is_symlink()
    typed = False
    for block in blocks:
        try:
            value = parse_json(block.group(1).decode("utf-8"))
            typed = typed or (isinstance(value, dict) and value.get("schema") == "cbc-claim/v1")
        except ValueError:
            continue
    named = has_inventory or prior_named_metadata or len(blocks) > 1 or typed
    errors: list[str] = []
    inventory = None
    expected_artifact = artifact_path(artifact) if named and artifact else None
    if has_inventory:
        try:
            value = parse_json(inventory_file.read_text())
            if (not isinstance(value, dict) or set(value) != {"schema", "artifact", "claim_ids"}
                    or value.get("schema") != "cbc-claim-set/v1"
                    or not isinstance(value.get("artifact"), str)
                    or not isinstance(value.get("claim_ids"), list) or not value["claim_ids"]):
                raise ValueError("invalid claim inventory")
            ids = [claim_identifier(v) for v in value["claim_ids"]]
            if len(ids) != len(set(ids)):
                raise ValueError("duplicate declared claim identifier")
            if expected_artifact is not None and value["artifact"] != expected_artifact:
                raise ValueError("claim inventory belongs to another artifact")
            expected_artifact = expected_artifact or value["artifact"]
            inventory = {**value, "claim_ids": sorted(ids)}
        except (ValueError, TypeError, KeyError):
            errors.append("invalid claim inventory")
    elif named and (not declaring or prior_named_metadata):
        errors.append("multi-claim ledger requires a declared inventory")
    records: dict[str, dict[str, Any]] = {}
    reported_artifact = expected_artifact
    for block in blocks:
        try:
            record = parse_json(block.group(1).decode("utf-8"))
            if not isinstance(record, dict):
                raise ValueError("invalid evidence record")
            raw_id = record.get("claim_id")
            key = claim_identifier(raw_id) if named else (raw_id if isinstance(raw_id, str) else "")
            if key in records:
                raise ValueError("duplicate claim identifier")
            path = record.get("artifact", {}).get("path")
            if path is not None and (not isinstance(path, str) or any(ord(c) < 32 or ord(c) == 127 for c in path)):
                raise ValueError("invalid evidence artifact")
            if named:
                if not path or (expected_artifact is not None and path != expected_artifact):
                    raise ValueError("claim record belongs to another artifact")
                expected_artifact = expected_artifact or path
                if inventory is not None and key not in inventory["claim_ids"]:
                    raise ValueError("undeclared claim record")
            reported_artifact = reported_artifact or path
            records[key] = record
        except (ValueError, TypeError, AttributeError):
            errors.append("invalid or unidentified claim record")
    if not named and ledger.is_file() and len(blocks) != 1:
        errors.append("expected one evidence record")
    return LedgerView(data, blocks, records, inventory, named, reported_artifact, errors)


def selected_record(view: LedgerView, claim_id: str, *, missing: bool = False) -> dict[str, Any] | None:
    if claim_id:
        claim_identifier(claim_id)
    if view.named:
        if view.errors or view.inventory is None:
            raise ValueError("invalid multi-claim ledger")
        claim_identifier(claim_id)
        if claim_id not in view.ids:
            raise ValueError("claim identifier is not declared")
        record = view.records.get(claim_id)
        if record is None and not missing:
            raise ValueError("claim has no verification record")
        return record
    if claim_id:
        raise ValueError("named selection requires a declared inventory")
    if view.errors:
        raise ValueError("invalid evidence record")
    return next(iter(view.records.values()), None)


@contextmanager
def ledger_lock(ledger: Path) -> Iterator[None]:
    try:
        import fcntl
    except ImportError as error:
        raise ValueError("evidence updates require filesystem locking") from error
    lock = checked_path(ledger.with_suffix(".claims.lock")).resolve()
    lock.parent.mkdir(parents=True, exist_ok=True)
    fd = os.open(lock, os.O_CREAT | os.O_RDWR | getattr(os, "O_NOFOLLOW", 0), 0o600)
    try:
        info = os.fstat(fd)
        if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1:
            raise ValueError("unsafe claim lock")
        deadline = time.monotonic() + 5
        while True:
            try:
                fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
                break
            except BlockingIOError:
                if time.monotonic() >= deadline:
                    raise ValueError("claim ledger is busy") from None
                time.sleep(0.02)
        yield
    finally:
        os.close(fd)


def declare_claims(artifact: str, ledger: Path, ids: list[str]) -> None:
    ids = [claim_identifier(v) for v in ids]
    if not ids or len(ids) != len(set(ids)):
        raise ValueError("declare unique claim identifiers")
    path = artifact_path(artifact)
    with ledger_lock(ledger):
        view = read_view(artifact, ledger, declaring=True)
        if view.errors:
            raise ValueError("repair malformed records before declaring claims")
        existing = {claim_identifier(r.get("claim_id")) for r in view.records.values()}
        if view.inventory is not None:
            existing.update(view.ids)
        if not existing.issubset(ids):
            raise ValueError("claim declaration cannot remove existing claims")
        value = {"schema": "cbc-claim-set/v1", "artifact": path, "claim_ids": sorted(ids)}
        if view.inventory != value:
            replace(inventory_path(ledger), canonical(value) + b"\n")
    print(f"cbc claim-set: {artifact} -> {len(ids)} declared claims")


def aggregate(statuses: list[str]) -> str:
    return next((status for status in STATUS_ORDER if status in statuses), "none")


def record_status(record: dict[str, Any] | None) -> str:
    if record is None:
        return "none"
    value = record.get("status")
    return value if isinstance(value, str) and value in RECORD_STATUSES else "invalid_record"


def report(artifact: str, ledger: Path, policy: str = "unspecified") -> dict[str, Any]:
    view = read_view(artifact, ledger)
    statuses = []
    details = []
    for claim_id in view.ids:
        status = record_status(view.records.get(claim_id))
        if policy == "required" and status in ("discharged", "waived"):
            try:
                check_acceptance(artifact, ledger, claim_id if view.named else "")
            except (OSError, ValueError, KeyError, TypeError, AttributeError, subprocess.SubprocessError):
                status = "acceptance_required"
        statuses.append(status)
        if view.named:
            details.append({"claim_id": claim_id, "status": status})
    status = "invalid_record" if view.errors else aggregate(statuses)
    result: dict[str, Any] = {"path": artifact, "status": status}
    if view.named and (view.inventory is not None or view.records):
        result["claims"] = details
    return result


def evidence_files(record: dict[str, Any]) -> dict[str, str]:
    ref = record.get("evidence", {}).get("ref")
    if isinstance(ref, str):
        path = checked_path(Path(ref))
        if path.is_file():
            return {ref: digest(path.read_bytes())}
    # Adapter labels such as 'mock' and 'z3' are not file references.
    if isinstance(ref, str) and ("/" in ref or ref.endswith(".json")):
        raise ValueError("referenced evidence is unavailable")
    return {}


def record_write_single(artifact: str, ledger: Path, claim_file: str, inputs: str) -> None:
    record = parse_json(sys.stdin.read())
    current = snapshot(artifact, claim_file)
    if inputs and parse_json(inputs) != current:
        raise ValueError("source or claim changed during verification")
    record["artifact"]["path"] = current["artifact"]
    record["verification_id"] = str(uuid.uuid4())
    record["inputs"] = current
    record["evidence_files"] = evidence_files(record)
    # Archive referenced reports as well as the ledger before a later run.
    archive(ledger)
    if ledger.exists():
        try:
            old = read_record(ledger)
        except ValueError:
            old = {}  # The historical bytes remain archived, even for legacy prose.
        for ref in old.get("evidence_files", {}):
            archive(Path(ref))
    quote = "\n".join("> " + line for line in (record.get("claim") or "(none)").splitlines())
    text = (f"# CbC Evidence: {artifact}\n\n"
            f"- **Status:** {record['status']}\n"
            f"- **Adapter:** {record['adapter']}\n"
            f"- **Commit:** {record['artifact']['commit']}\n"
            f"- **Verified:** {record['verified_at']}\n\n"
            "Claim:\n\n" + quote + "\n\n"
            "```json\n" + json.dumps(record, indent=2) + "\n```\n")
    replace(ledger, text.encode())
    print(ledger)


def record_write(artifact: str, ledger: Path, claim_file: str, inputs: str, claim_id: str = "") -> None:
    with ledger_lock(ledger):
        view = read_view(artifact, ledger)
        if not view.named and not claim_id:
            record_write_single(artifact, ledger, claim_file, inputs)
            return
        record = parse_json(sys.stdin.read())
        old = selected_record(view, claim_id, missing=True)
        claim_acceptance_path(ledger, claim_id)
        preserved_claim = None
        if record.get("adapter") == "waiver":
            if claim_file:
                record["claim"] = checked_path(Path(claim_file)).read_text()
            elif old is not None:
                record["claim"] = old.get("claim", "")
                previous = old.get("inputs", {}).get("claim")
                if previous:
                    claim_file = str(Path(git("rev-parse", "--show-toplevel")) / previous["path"])
                    preserved_claim = previous
            else:
                raise ValueError("a new named waiver requires a claim document")
        current = snapshot(artifact, claim_file)
        if preserved_claim is not None and preserved_claim != current["claim"]:
            raise ValueError("claim document changed: provide --claim for a new waiver")
        if inputs and parse_json(inputs) != current:
            raise ValueError("source or claim changed during verification")
        record["artifact"]["path"] = current["artifact"]
        record["claim_id"] = claim_id
        record["schema"] = "cbc-claim/v1"
        record["verification_id"] = str(uuid.uuid4())
        record["inputs"] = current
        record["evidence_files"] = evidence_files(record)
        if old:
            for ref in old.get("evidence_files", {}):
                archive(Path(ref))
        block = b"```json\n" + json.dumps(record, indent=2).encode() + b"\n```"
        if old is not None:
            selected = next(b for b in view.blocks if parse_json(b.group(1).decode())["claim_id"] == claim_id)
            data = view.data[:selected.start()] + block + view.data[selected.end():]
        else:
            original = view.data or f"# CbC Evidence: {current['artifact']}\n".encode()
            data = original + f"\n\n## Claim {claim_id}\n\n".encode() + block + b"\n"
        replace(ledger, data)
    print(ledger)


def record_binding(artifact: str, record: dict[str, Any]) -> dict[str, Any]:
    path = artifact_path(artifact)
    if record.get("status") not in ("discharged", "waived"):
        raise ValueError("verification is not discharged or waived")
    if record.get("artifact", {}).get("path") != path:
        raise ValueError("evidence belongs to another artifact")
    inputs = record.get("inputs")
    if not isinstance(inputs, dict):
        raise ValueError("historical record requires current verification")
    claim = inputs.get("claim")
    root = Path(git("rev-parse", "--show-toplevel"))
    current = snapshot(artifact, str(root / claim["path"]) if claim else "")
    if current != inputs:
        raise ValueError("source or claim is stale")
    if evidence_files(record) != record.get("evidence_files"):
        raise ValueError("referenced evidence is stale")
    return {"artifact": path, "source_sha256": inputs["source_sha256"],
            "claim": claim or {"path": None, "sha256": digest(canonical(record.get("claim", "")))}}


def binding(artifact: str, ledger: Path, claim_id: str = "") -> dict[str, Any]:
    view = read_view(artifact, ledger)
    if view.named:
        record = selected_record(view, claim_id)
        if record is None:
            raise ValueError("claim has no verification record")
        return {**record_binding(artifact, record), "claim_id": claim_id,
                "claim_set_sha256": digest(canonical(view.inventory)),
                "evidence_sha256": digest(canonical(record))}
    if claim_id:
        selected_record(view, claim_id)
    return {**record_binding(artifact, read_record(ledger)), "evidence_sha256": digest(ledger.read_bytes())}


def validate(artifact: str, ledger: Path, approval: str, signature: str, claim_id: str = "") -> None:
    payload = parse_json(approval)
    expected = binding(artifact, ledger, claim_id)
    schema = "cbc-acceptance/v2" if "claim_id" in expected else "cbc-acceptance/v1"
    if not isinstance(payload, dict) or payload.get("schema") != schema:
        raise ValueError("invalid acceptance schema")
    for key, value in expected.items():
        if payload.get(key) != value:
            raise ValueError(f"acceptance binding mismatch: {key}")
    for key in ("accepted_by", "approved_at", "approval_ref"):
        if not isinstance(payload.get(key), str) or not payload[key].strip():
            raise ValueError(f"missing approval field: {key}")
    if any(char.isspace() or char in "*?![],\\\\" for char in payload["accepted_by"]):
        raise ValueError("maintainer identity must be a literal SSH principal")
    stamp = datetime.fromisoformat(payload["approved_at"].replace("Z", "+00:00"))
    if stamp.tzinfo is None:
        raise ValueError("approval time requires a timezone")
    trust = Path(git("config", "--local", "--get", "cbc.acceptanceAllowedSigners"))
    root = Path(git("rev-parse", "--show-toplevel")).resolve()
    if not trust.is_absolute() or trust.resolve().is_relative_to(root):
        raise ValueError("allowed signers must be an absolute path outside the worktree")
    # Only explicit identities authorize acceptance. Wildcards are insufficient.
    entries = [line for line in trust.read_text().splitlines()
               if line.strip() and not line.lstrip().startswith("#")
               and payload["accepted_by"] in line.split()[0].split(",")]
    if not entries:
        raise ValueError("maintainer identity is not explicitly authorized")
    with tempfile.TemporaryDirectory() as tmp:
        allowed = Path(tmp) / "allowed-signers"
        allowed.write_text("\n".join(entries) + "\n")
        sig = Path(tmp) / "approval.sig"
        sig.write_text(signature)
        result = subprocess.run(
            ["ssh-keygen", "-Y", "verify", "-f", str(allowed), "-I", payload["accepted_by"],
             "-n", "cbc-acceptance", "-s", str(sig)],
            input=approval.encode(), stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        if result.returncode:
            raise ValueError("approval signature is not authorized")


def check_acceptance(artifact: str, ledger: Path, claim_id: str = "") -> None:
    view = read_view(artifact, ledger)
    if view.named and not claim_id:
        if view.errors or not view.ids:
            raise ValueError("invalid multi-claim ledger")
        for selected in view.ids:
            check_acceptance(artifact, ledger, selected)
        return
    acceptance = claim_acceptance_path(ledger, claim_id) if view.named else checked_path(ledger.with_suffix(".acceptance.json"))
    envelope = parse_json(acceptance.read_text())
    validate(artifact, ledger, envelope["approval"], envelope["signature"], claim_id)


def main() -> None:
    action, artifact, *args = sys.argv[1:]
    if action == "snapshot":
        print(json.dumps(snapshot(artifact, args[0])))
        return
    ledger = checked_path(Path(args[0]))
    acceptance = checked_path(ledger.with_suffix(".acceptance.json"))
    if action == "claim-set":
        declare_claims(artifact, ledger, args[1:])
    elif action == "select":
        claim_id = args[1] if len(args) > 1 else ""
        view = read_view(artifact, ledger)
        if view.named or claim_id:
            selected_record(view, claim_id, missing=True)
            if view.named:
                claim_acceptance_path(ledger, claim_id)
    elif action in ("status", "metadata", "report"):
        view = read_view(artifact, ledger)
        result = report(artifact, ledger, args[1] if action == "report" and len(args) > 1 else "unspecified")
        if action == "status":
            print(result["status"])
        elif action == "metadata":
            print(json.dumps({"artifact": view.artifact, "status": result["status"],
                              **({"claims": result["claims"]} if "claims" in result else {})}))
        else:
            print(json.dumps(result))
    elif action == "record":
        record_write(artifact, ledger, args[1], args[2], args[3] if len(args) > 3 else "")
    elif action == "payload":
        selected_binding = binding(artifact, ledger, args[1] if len(args) > 1 else "")
        payload = {"schema": "cbc-acceptance/v2" if "claim_id" in selected_binding else "cbc-acceptance/v1",
                   **selected_binding, "accepted_by": "", "approved_at": "", "approval_ref": ""}
        print(json.dumps(payload, indent=2))
    elif action == "accept":
        claim_id = args[3] if len(args) > 3 else ""
        with ledger_lock(ledger):
            view = read_view(artifact, ledger)
            acceptance = claim_acceptance_path(ledger, claim_id) if view.named else acceptance
            approval = Path(args[1]).read_bytes().decode("utf-8")
            signature = Path(args[2]).read_text()
            validate(artifact, ledger, approval, signature, claim_id)
            replace(acceptance, canonical({"approval": approval, "signature": signature}) + b"\n")
        print(f"cbc accept: {artifact} -> accepted")
    elif action == "check":
        check_acceptance(artifact, ledger, args[1] if len(args) > 1 else "")
    else:
        raise ValueError("unknown acceptance action")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, TypeError, IndexError, AttributeError,
            subprocess.SubprocessError) as error:
        print(f"cbc acceptance: {error}", file=sys.stderr)
        sys.exit(4)
