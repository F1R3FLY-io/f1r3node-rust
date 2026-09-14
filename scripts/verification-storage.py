#!/usr/bin/env python3
import argparse
import contextlib
import fcntl
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import shutil
import stat
import subprocess
import sys
import tempfile
import zipfile


CHUNK = 1024 * 1024
GENERATED = {"target", "node_modules", "__pycache__", ".venv", ".git"}


def digest_file(stream, limit=None):
    digest = hashlib.sha256()
    total = 0
    while chunk := stream.read(CHUNK if limit is None else min(CHUNK, limit - total + 1)):
        total += len(chunk)
        if limit is not None and total > limit:
            raise ValueError("An input grew beyond its byte limit.")
        digest.update(chunk)
    return digest.hexdigest()


def relative_name(name):
    path = PurePosixPath(name)
    if (not name or name == "." or path.is_absolute() or str(path) != name
            or any(part in (".", "..") for part in path.parts)
            or any(ord(char) < 32 for char in name) or "\\" in name):
        raise ValueError("A relative path is invalid.")
    return name


def generated(name):
    parts = PurePosixPath(name).parts
    return (bool(GENERATED.intersection(parts)) or name.endswith(".pyc")
            or (parts[:2] == ("formal", "tlaplus") and len(parts) > 3 and parts[3] == "states"))


def stamp(status):
    return (status.st_dev, status.st_ino, status.st_mode, status.st_size,
            status.st_mtime_ns, status.st_ctime_ns)


@contextlib.contextmanager
def open_source(root, name):
    directory = os.open(root, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        parts = PurePosixPath(relative_name(name)).parts
        for part in parts[:-1]:
            child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=directory)
            os.close(directory)
            directory = child
        descriptor = os.open(parts[-1], os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=directory)
        with os.fdopen(descriptor, "rb") as stream:
            if not stat.S_ISREG(os.fstat(stream.fileno()).st_mode):
                raise ValueError("A selected input is not a regular file.")
            yield stream
    finally:
        os.close(directory)


def git(root, *arguments, required=True):
    result = subprocess.run(["git", "-c", "core.fsmonitor=false", "-C", str(root), *arguments],
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=30)
    if result.returncode and required:
        raise ValueError("The Git input query failed.")
    return result.stdout if result.returncode == 0 else b""


def selected_index(root, selections):
    selected = {}
    for entry in git(root, "ls-files", "--stage", "-z").split(b"\0"):
        if not entry:
            continue
        metadata, raw_name = entry.split(b"\t", 1)
        name = raw_name.decode("utf-8")
        if not any(prefix == "." or name == prefix or name.startswith(prefix + "/") for prefix in selections):
            continue
        relative_name(name)
        mode, oid, stage = metadata.decode("ascii").split()
        if stage != "0":
            raise ValueError("A selected Git input has unresolved stages.")
        selected[name] = (mode, oid)
    return selected


def inventory(root):
    if root.is_symlink() or not root.is_dir():
        raise ValueError("The inventory root must be a directory without a symbolic link.")
    root = root.resolve(strict=True)
    records = {}
    allocated = 0
    seen = set()
    for directory, folders, files in os.walk(root, followlinks=False, onerror=lambda error: (_ for _ in ()).throw(error)):
        for name in folders + files:
            path = Path(directory) / name
            status = path.lstat()
            if stat.S_ISLNK(status.st_mode):
                raise ValueError("The inventory contains a symbolic link.")
            if stat.S_ISREG(status.st_mode):
                records[path.relative_to(root).as_posix()] = status
            elif not stat.S_ISDIR(status.st_mode):
                raise ValueError("The inventory contains a special file.")
            identity = (status.st_dev, status.st_ino)
            if identity not in seen:
                allocated += status.st_blocks * 512
                seen.add(identity)
    return records, allocated


def verify_object(target, entries):
    if target.is_symlink() or {path.name for path in target.iterdir()} != {"source", "manifest.json"}:
        raise ValueError("The stored snapshot has unexpected entries.")
    if (target / "manifest.json").is_symlink():
        raise ValueError("The stored manifest is a symbolic link.")
    records, _ = inventory(target / "source")
    if set(records) != {entry["path"] for entry in entries}:
        raise ValueError("The stored snapshot has a different file set.")
    source = target / "source"
    expected_directories = {source / str(parent) for entry in entries for parent in PurePosixPath(entry["path"]).parents}
    actual_directories = {Path(directory) for directory, _, _ in os.walk(source)}
    if (actual_directories != expected_directories
            or any(stat.S_IMODE(path.stat().st_mode) != 0o555 for path in actual_directories)):
        raise ValueError("The stored snapshot has different directories or directory permissions.")
    for entry in entries:
        with open_source(target / "source", entry["path"]) as stream:
            status = os.fstat(stream.fileno())
            if (status.st_size != entry["bytes"] or stat.S_IMODE(status.st_mode) != (entry["mode"] & ~0o222)
                    or digest_file(stream, entry["bytes"]) != entry["sha256"]):
                raise ValueError("The stored snapshot failed its integrity check.")


def snapshot(args):
    root = args.source.resolve(strict=True)
    top = git(root, "rev-parse", "--show-toplevel").decode().strip()
    if Path(top).resolve() != root:
        raise ValueError("SOURCE must be the Git repository root.")
    store = args.store.resolve()
    if store == root or root in store.parents:
        raise ValueError("The source store must be outside the source repository.")
    selections = [name if name == "." else relative_name(name) for name in args.paths]
    extras = [relative_name(name) for name in args.extra_file]
    head = git(root, "rev-parse", "--verify", "HEAD", required=False).decode().strip() or None
    before_index = selected_index(root, selections)
    requested = set(before_index) | set(extras)
    if any(not any(prefix == "." or name == prefix or name.startswith(prefix + "/") for name in requested)
           for prefix in selections):
        raise ValueError("A requested source selection has no inputs.")
    excluded = sorted(name for name in before_index if generated(name))
    if any(generated(name) for name in extras):
        raise ValueError("An extra input is generated output.")
    names = sorted((set(before_index) - set(excluded)) | set(extras))
    if not names or len(names) > args.max_files:
        raise ValueError("The snapshot file count is outside its limit.")
    if any(before_index[name][0] not in ("100644", "100755") for name in names if name in before_index):
        raise ValueError("Selected Git links and symbolic links require separate capture.")
    entries = []
    stamps = {}
    total = 0
    for name in names:
        with open_source(root, name) as stream:
            status = os.fstat(stream.fileno())
            total += status.st_size
            if total > args.max_bytes:
                raise ValueError("The snapshot exceeds its byte limit.")
            if status.st_mode & 0o7000:
                raise ValueError("A source input has unsupported special permission bits.")
            value = digest_file(stream, status.st_size)
            if stamp(status) != stamp(os.fstat(stream.fileno())):
                raise ValueError("A source input changed during hashing.")
            stamps[name] = stamp(status)
            entries.append({"path": name, "bytes": status.st_size,
                            "mode": stat.S_IMODE(status.st_mode), "sha256": value})
    encoded = (json.dumps({"schema_version": 1, "entries": entries}, sort_keys=True, separators=(",", ":")) + "\n").encode()
    identity = hashlib.sha256(encoded).hexdigest()
    parent = store
    while not parent.exists():
        parent = parent.parent
    estimate = total + len(encoded) + sum(4096 * (len(PurePosixPath(name).parts) + 1) for name in names)
    if shutil.disk_usage(parent).free < args.min_free_bytes + estimate:
        raise ValueError("The source store has insufficient free space.")
    store.mkdir(parents=True, exist_ok=True, mode=0o700)
    with (store / ".lock").open("a+b") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        target = store / identity
        reused = target.exists()
        if reused:
            if (target / "manifest.json").read_bytes() != encoded:
                raise ValueError("The stored manifest does not match its identity.")
            verify_object(target, entries)
        else:
            _, allocation = inventory(store)
            if allocation + estimate > args.max_store_bytes:
                raise ValueError("The source store exceeds its admission budget.")
            with tempfile.TemporaryDirectory(prefix=".capture-", dir=store) as temporary:
                stage = Path(temporary)
                for entry in entries:
                    name = entry["path"]
                    output = stage / "source" / name
                    output.parent.mkdir(parents=True, exist_ok=True)
                    with open_source(root, name) as stream, output.open("xb") as destination:
                        if stamp(os.fstat(stream.fileno())) != stamps[name]:
                            raise ValueError("A source input changed before copying.")
                        remaining = entry["bytes"]
                        while remaining:
                            chunk = stream.read(min(CHUNK, remaining))
                            if not chunk:
                                raise ValueError("A source input became shorter during copying.")
                            destination.write(chunk)
                            remaining -= len(chunk)
                        if stream.read(1):
                            raise ValueError("A source input grew during copying.")
                    output.chmod(entry["mode"] & ~0o222)
                (stage / "manifest.json").write_bytes(encoded)
                verify_source(root, names, stamps, before_index, selections, head)
                for directory, _, _ in os.walk(stage / "source", topdown=False):
                    Path(directory).chmod(0o555)
                (stage / "manifest.json").chmod(0o444)
                verify_object(stage, entries)
                try:
                    os.rename(stage, target)
                except OSError as error:
                    raise ValueError("The source snapshot could not be published.") from error
        verify_source(root, names, stamps, before_index, selections, head)
    return {"status": "captured", "source_state": "worktree", "source_commit": head,
            "snapshot": str(target), "manifest_sha256": identity, "files": len(entries), "source_bytes": total,
            "selections": selections, "excluded_tracked_paths": excluded, "extra_files": extras, "reused": reused}


def verify_source(root, names, stamps, before_index, selections, head):
    current_head = git(root, "rev-parse", "--verify", "HEAD", required=False).decode().strip() or None
    if head != current_head:
        raise ValueError("The source commit changed during capture.")
    if before_index != selected_index(root, selections):
        raise ValueError("The selected Git index changed during capture.")
    for name in names:
        with open_source(root, name) as stream:
            if stamp(os.fstat(stream.fileno())) != stamps[name]:
                raise ValueError("A source input changed during capture.")


def verify_zip(args):
    expected = args.sha256.lower()
    if len(expected) != 64 or any(char not in "0123456789abcdef" for char in expected):
        raise ValueError("The archive digest is invalid.")
    before, allocated = inventory(args.extracted)
    root = args.extracted.resolve(strict=True)
    with args.archive.open("rb") as stream:
        archive_stamp = stamp(os.fstat(stream.fileno()))
        if digest_file(stream) != expected:
            raise ValueError("The archive digest does not match.")
        stream.seek(0)
        with zipfile.ZipFile(stream) as archive:
            infos = archive.infolist()
            if len(infos) > args.max_files or sum(info.file_size for info in infos) > args.max_bytes:
                raise ValueError("The archive exceeds its declared limits.")
            names = set()
            allowed_directories = set()
            files = {}
            for info in infos:
                name = relative_name(info.filename.rstrip("/") if info.is_dir() else info.filename)
                if name in names:
                    raise ValueError("The archive contains duplicate paths.")
                names.add(name)
                allowed_directories.update(str(parent) for parent in PurePosixPath(name).parents if str(parent) != ".")
                if info.is_dir():
                    allowed_directories.add(name)
                kind = stat.S_IFMT(info.external_attr >> 16)
                if kind not in (0, stat.S_IFDIR if info.is_dir() else stat.S_IFREG):
                    raise ValueError("The archive contains a symbolic link or special file.")
                if not info.is_dir():
                    files[name] = info
            if set(files) != set(before):
                raise ValueError("The archive and extraction have different file sets.")
            actual_directories = {path.relative_to(root).as_posix() for path in root.rglob("*") if path.is_dir()}
            if actual_directories != allowed_directories:
                raise ValueError("The archive and extraction have different directory sets.")
            for name, info in files.items():
                with open_source(root, name) as extracted, archive.open(info) as member:
                    if (stamp(os.fstat(extracted.fileno())) != stamp(before[name])
                            or before[name].st_size != info.file_size
                            or digest_file(extracted, info.file_size) != digest_file(member, info.file_size)):
                        raise ValueError("An extracted file differs from the archive.")
        if stamp(os.fstat(stream.fileno())) != archive_stamp:
            raise ValueError("The archive changed during verification.")
    after, _ = inventory(root)
    if {name: stamp(value) for name, value in before.items()} != {name: stamp(value) for name, value in after.items()}:
        raise ValueError("The extraction changed during verification.")
    return {"status": "verified", "scope": "regular-file paths and bytes", "archive_sha256": expected,
            "files": len(before), "expanded_bytes": sum(value.st_size for value in before.values()),
            "allocated_bytes": allocated, "deleted": False}


def audit(args):
    results = []
    for path in args.paths:
        files, allocated = inventory(path)
        space = shutil.disk_usage(path)
        results.append({"path": str(path.resolve()), "files": len(files), "logical_bytes": sum(value.st_size for value in files.values()),
                        "allocated_bytes": allocated, "filesystem_free_bytes": space.free, "deletion_authorized": False})
    return {"roots": results, "note": "Overlapping roots and shared inodes must not be added across rows."}


def positive(value):
    try:
        result = int(value)
    except ValueError as error:
        raise argparse.ArgumentTypeError("The limit must be a positive integer.") from error
    if result <= 0:
        raise argparse.ArgumentTypeError("The limit must be positive.")
    return result


def main():
    parser = argparse.ArgumentParser(description="Capture bounded source snapshots and audit retained storage.")
    commands = parser.add_subparsers(dest="command", required=True)
    capture = commands.add_parser("snapshot")
    capture.add_argument("source", type=Path)
    capture.add_argument("store", type=Path)
    capture.add_argument("paths", nargs="+")
    capture.add_argument("--extra-file", action="append", default=[])
    capture.add_argument("--max-bytes", type=positive, default=64 * 1024**2)
    capture.add_argument("--max-files", type=positive, default=20000)
    capture.add_argument("--max-store-bytes", type=positive, default=1024**3)
    capture.add_argument("--min-free-bytes", type=positive, default=10 * 1024**3)
    capture.set_defaults(action=snapshot)
    inspect = commands.add_parser("audit")
    inspect.add_argument("paths", type=Path, nargs="+")
    inspect.set_defaults(action=audit)
    archive = commands.add_parser("verify-zip")
    archive.add_argument("archive", type=Path)
    archive.add_argument("extracted", type=Path)
    archive.add_argument("--sha256", required=True)
    archive.add_argument("--max-bytes", type=positive, default=32 * 1024**3)
    archive.add_argument("--max-files", type=positive, default=20000)
    archive.set_defaults(action=verify_zip)
    args = parser.parse_args()
    try:
        print(json.dumps(args.action(args), sort_keys=True, indent=2))
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError, zipfile.BadZipFile) as error:
        print(f"FAIL: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
