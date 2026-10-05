import hashlib
import json
from pathlib import Path
import sys
import tomllib


ROOT = Path(__file__).resolve().parents[1]
CORDIAL = ROOT / "consensus" / "cordial"
FORBIDDEN = {
    "casper", "models", "node", "comm", "block-storage", "rholang",
    "rho-pure-eval", "rspace_rust", "rspace_plus_plus", "rspace++",
    "cordial-por",
}


def main():
    inventory = json.loads((CORDIAL / "upstream-files.json").read_text())
    patches = json.loads((CORDIAL / "local-patches.json").read_text())
    errors = []
    unchanged = 0
    patched = 0
    manifests = 0
    imported_paths = {entry["path"] for entry in inventory["files"]}
    for path in patches:
        if path not in imported_paths:
            errors.append(f"Patch does not refer to an imported file: {path}")
    for entry in inventory["files"]:
        path = CORDIAL / entry["path"]
        if not path.is_file():
            errors.append(f"Missing imported file: {entry['path']}")
            continue
        data = path.read_bytes()
        blob = hashlib.sha1(b"blob " + str(len(data)).encode() + b"\0" + data).hexdigest()
        patch = patches.get(entry["path"])
        if patch:
            if hashlib.sha256(data).hexdigest() != patch["sha256"] or not patch["reason"]:
                errors.append(f"Local patch differs from its recorded revision: {entry['path']}")
            else:
                patched += 1
        elif blob == entry["blob"]:
            unchanged += 1
        elif entry["adapted"] and path.name == "Cargo.toml":
            manifests += 1
        else:
            errors.append(f"Unrecorded source change: {entry['path']}")

    workspace = tomllib.loads((ROOT / "Cargo.toml").read_text())
    members = set(workspace["workspace"]["members"])
    if "consensus/cordial/cordial-por" in members or (CORDIAL / "cordial-por" / "Cargo.toml").exists():
        errors.append("PoR integration is deferred and must not be included")
    lockfile = tomllib.loads((ROOT / "Cargo.lock").read_text())
    if any(package["name"] == "cordial-por" for package in lockfile["package"]):
        errors.append("PoR must not remain in the resolved dependency graph")
    for crate in ("cordial-miners-core", "cordial-app-runtime", "cordial-f1r3node-adapter"):
        if f"consensus/cordial/{crate}" not in members:
            errors.append(f"Missing workspace member: {crate}")
        manifest = tomllib.loads((CORDIAL / crate / "Cargo.toml").read_text())
        for section in ("dependencies", "dev-dependencies", "build-dependencies"):
            for name, spec in manifest.get(section, {}).items():
                package = spec.get("package", name) if isinstance(spec, dict) else name
                if package in FORBIDDEN:
                    errors.append(f"Forbidden native dependency: {crate} -> {package}")
                if isinstance(spec, dict) and "path" in spec:
                    dependency = (CORDIAL / crate / spec["path"]).resolve()
                    if not dependency.is_relative_to(CORDIAL):
                        errors.append(f"Dependency leaves imported native modules: {crate} -> {dependency}")

    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    print(f"Verified {unchanged} unchanged files, {patched} recorded native patches, and {manifests} adapted manifests at {inventory['revision']}.")
    print("Native manifests have no direct Casper, node, transport, or VM dependencies.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
