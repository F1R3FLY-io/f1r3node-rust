import argparse
from datetime import datetime, timezone
import hashlib
import json
import math
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import time


ROOT = Path(__file__).resolve().parents[2]
PLAN = ROOT / "formal/tlaplus/casper_soak/verification-plan.jsonc"
CLEAN_MARKER = "Model checking completed. No error has been found."


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def classify(returncode, output, expected_property=None):
    violations = re.findall(r"Invariant ([A-Za-z][A-Za-z0-9_]*) is violated\.", output)
    if expected_property is None:
        if returncode == 0 and CLEAN_MARKER in output and not violations and "Error:" not in output:
            return "passed"
    elif (
        returncode == 12
        and violations == [expected_property]
        and re.search(r"The behavior up to this point is:\s+State 1:", output)
        and CLEAN_MARKER not in output
    ):
        return "passed"
    return "unexpected_result"


def validate_configuration(control, text, knobs):
    required = control.get("properties", [control.get("property")])
    invariants = re.findall(r"^INVARIANT\s+(\w+)\s*$", text, re.MULTILINE)
    if set(invariants) != {"TypeOK", *required}:
        raise ValueError("Configuration properties do not match the registry.")
    if len(invariants) != len(set(invariants)):
        raise ValueError("Duplicate invariant registration.")
    for knob in knobs:
        values = re.findall(rf"^\s*{re.escape(knob)}\s*=\s*(TRUE|FALSE)\s*$", text, re.MULTILINE)
        expected = "TRUE" if control.get("knob") == knob else "FALSE"
        if values != [expected]:
            raise ValueError("Configuration defect knobs do not match the registry.")


def output_text(value):
    return value.decode("utf-8", errors="replace") if isinstance(value, bytes) else (value or "")


def run_control(model, config, control, knobs, java, jar, timeout, output_dir):
    name = config.stem
    result = {
        "configuration": config.name,
        "expected_property": control.get("property"),
        "expected_exit": control["expected_exit"],
        "exit": None,
        "outcome": "tool_error",
        "log": name + ".log",
    }
    log = ""
    started = time.monotonic()
    try:
        config_text = config.read_text()
        validate_configuration(control, config_text, knobs)
        result["constant_assignments"] = re.findall(r"^\s*(\w+)\s*=\s*(.*?)\s*$", config_text, re.MULTILINE)
        inputs = {"model_sha256": digest(model), "configuration_sha256": digest(config), "tlc_sha256": digest(jar)}
        result.update(inputs)
        with tempfile.TemporaryDirectory(prefix="casper-soak-tlc-") as metadata:
            command = [
                java, "-Xmx512m", "-XX:+UseParallelGC", "-cp", str(jar), "tlc2.TLC",
                "-workers", "1", "-seed", "1", "-metadir", metadata,
                "-config", str(config), str(model),
            ]
            completed = subprocess.run(
                command, cwd=model.parent, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                timeout=timeout, check=False,
            )
        log = output_text(completed.stdout)
        result["exit"] = completed.returncode
        result["outcome"] = classify(completed.returncode, log, control.get("property"))
        if inputs != {"model_sha256": digest(model), "configuration_sha256": digest(config), "tlc_sha256": digest(jar)}:
            result["outcome"] = "input_changed"
        version = re.search(r"^TLC2 Version .+$", log, re.MULTILINE)
        result["tlc_version"] = version.group(0) if version else None
        counts = re.search(r"([\d,]+) states generated, ([\d,]+) distinct states found", log)
        if counts:
            result["states_generated"] = int(counts.group(1).replace(",", ""))
            result["distinct_states"] = int(counts.group(2).replace(",", ""))
    except subprocess.TimeoutExpired as error:
        log = output_text(error.output)
        result["outcome"] = "timeout"
    except KeyboardInterrupt:
        log = "Control execution cancelled. No complete verifier output is available."
        result["outcome"] = "cancelled"
    except (OSError, ValueError) as error:
        log = str(error)
        result["outcome"] = "tool_error"
    result["elapsed_seconds"] = round(time.monotonic() - started, 3)
    log_path = output_dir / result["log"]
    log_path.write_text(log)
    result["log_sha256"] = digest(log_path)
    return result


def registered_controls(plan):
    controls = [plan["positive_control"], *plan["negative_controls"]]
    names = [c["configuration"] for c in controls]
    if len(set(names)) != len(names):
        raise ValueError("Duplicate configuration registration.")
    for name in names:
        if Path(name).name != name or not name.endswith(".cfg"):
            raise ValueError("Configuration must be a local .cfg filename.")
    if controls[0]["expected_exit"] != 0:
        raise ValueError("The positive control must require exit 0.")
    if any(c["expected_exit"] != 12 for c in controls[1:]):
        raise ValueError("Negative controls must require exit 12.")
    properties = [c["property"] for c in controls[1:]]
    knobs = [c["knob"] for c in controls[1:]]
    if len(set(properties)) != len(properties) or len(set(knobs)) != len(knobs):
        raise ValueError("Duplicate negative property or defect knob.")
    if set(properties) != set(controls[0]["properties"]):
        raise ValueError("Positive and negative property registries differ.")
    return controls, knobs


def main(argv=None):
    parser = argparse.ArgumentParser()
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--java", default=os.environ.get("JAVA", "java"))
    parser.add_argument("--jar", type=Path, default=Path(os.environ.get("TLA_TOOLS_JAR", str(Path.home() / ".tla/tla2tools.jar"))))
    parser.add_argument("--timeout", type=float, default=120)
    args = parser.parse_args(argv)
    if args.timeout <= 0 or not math.isfinite(args.timeout):
        parser.error("--timeout must be finite and positive")
    output_dir = args.output_dir.resolve()
    try:
        output_dir.mkdir(parents=True, exist_ok=False)
    except OSError as error:
        parser.error(str(error))
    report = {
        "scope": "bounded-harness-refutation-only", "status": "tool_error",
        "construction": "not-applicable", "driver_binding": "pending",
        "profile_verification": "pending", "seed": 1, "workers": 1,
        "heap_limit_mb": 512, "timeout_seconds": args.timeout, "results": [],
        "run_id": output_dir.name, "started_at": datetime.now(timezone.utc).isoformat(),
        "runner_sha256": digest(Path(__file__)), "python_version": sys.version,
    }
    try:
        plan = json.loads(PLAN.read_text())
        report["plan_sha256"] = digest(PLAN)
        controls, knobs = registered_controls(plan)
        model = (ROOT / plan["model"]).resolve()
        model.relative_to(PLAN.parent)
        report["model"] = plan["model"]
        report["bounds_source"] = "Per-control constant_assignments and configuration digests."
        for control in controls:
            result = run_control(model, PLAN.parent / control["configuration"], control, knobs,
                                 args.java, args.jar.resolve(), args.timeout, output_dir)
            report["results"].append(result)
            print(f'{result["configuration"]}: {result["outcome"]} (exit {result["exit"]})')
            if result["outcome"] == "cancelled":
                break
        report["status"] = "passed" if all(r["outcome"] == "passed" for r in report["results"]) else "failed"
        if report["plan_sha256"] != digest(PLAN):
            report["status"] = "input_changed"
    except (OSError, ValueError, KeyError, TypeError) as error:
        report["error"] = str(error)
    (output_dir / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    return 0 if report["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
