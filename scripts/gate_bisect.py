#!/usr/bin/env python3
"""Bisection driver for `rholang/tests/stack_depth_gate.rs`'s child probes.

The gate's own ladders are assertions with fixed endpoints. This driver answers
the two *measurement* questions the assertions do not:

  * ``stack``  — what is the smallest native stack on which subject ``S``
    survives nesting depth ``N``?  (bisect STACK at fixed DEPTH)
  * ``depth``  — what is the greatest nesting depth subject ``S`` survives on a
    stack of ``B`` bytes?  (bisect DEPTH at fixed STACK)

Both re-exec the gate's ``gate_child`` test once per probe point, because a
native-stack overflow is a ``SIGSEGV`` turned into ``abort()`` by Rust's
guard-page handler — signal 6, shell status 134 — and is therefore not
containable in-process.

★ Every number this prints is accompanied by the BRACKETING evidence (the last
surviving point and the first failing point, with the failing point's exit
status), so a reader can re-run either point by hand.  Parsing and arithmetic are
done here rather than in a shell pipeline: four false zeros in this campaign came
from malformed shell patterns.

Usage
-----
    scripts/gate_bisect.py stack  --bin <gate-binary> --subject clone --depth 128
    scripts/gate_bisect.py depth  --bin <gate-binary> --subject clone --stack 2097152
    scripts/gate_bisect.py ladder --bin <gate-binary> --subject clone --lo 256 --hi 4096
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys

RESOLUTION = 4096  # bytes; matches the gate's own bisection granularity


def run_point(binary: str, subject: str, depth: int, stack: int) -> int:
    """Run one probe point.  Returns the child's exit code (negative = signal)."""
    env = dict(os.environ)
    env["GATE_SUBJECT"] = subject
    env["GATE_DEPTH"] = str(depth)
    env["GATE_STACK"] = str(stack)
    proc = subprocess.run(
        [binary, "--ignored", "--exact", "gate_child"],
        env=env,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    return proc.returncode


def survives(binary: str, subject: str, depth: int, stack: int) -> bool:
    return run_point(binary, subject, depth, stack) == 0


def describe_exit(code: int) -> str:
    if code == 0:
        return "0 (survived)"
    if code < 0:
        return f"{code} (killed by signal {-code})"
    if code == 134:
        return "134 (SIGABRT — `fatal runtime error: stack overflow`)"
    if code == 101:
        return "101 (Rust test panic — NOT a stack overflow)"
    return str(code)


def min_stack_for(binary: str, subject: str, depth: int, cap: int = 512 * 1024 * 1024) -> dict:
    """Smallest stack (to RESOLUTION) on which `subject` survives `depth`."""
    hi = 16 * 1024
    while hi <= cap and not survives(binary, subject, depth, hi):
        hi *= 2
    if hi > cap:
        raise SystemExit(f"`{subject}` needed more than {cap} bytes at depth {depth}")
    lo = hi // 2
    while hi - lo > RESOLUTION:
        mid = (lo + hi) // 2
        if survives(binary, subject, depth, mid):
            hi = mid
        else:
            lo = mid
    return {
        "subject": subject,
        "depth": depth,
        "min_stack": hi,
        "last_failing_stack": lo,
        "last_failing_exit": describe_exit(run_point(binary, subject, depth, lo)),
    }


def max_depth_for(binary: str, subject: str, stack: int, cap: int = 1 << 20) -> dict:
    """Greatest depth `subject` survives on a `stack`-byte thread."""
    if not survives(binary, subject, 1, stack):
        raise SystemExit(
            f"HARNESS FAILURE: `{subject}` does not run at depth 1 on {stack} bytes"
        )
    lo, hi = 1, 2
    while hi <= cap and survives(binary, subject, hi, stack):
        lo, hi = hi, hi * 2
    if hi > cap:
        return {"subject": subject, "stack": stack, "max_depth": f">={lo}", "capped": True}
    while hi - lo > 1:
        mid = lo + (hi - lo) // 2
        if survives(binary, subject, mid, stack):
            lo = mid
        else:
            hi = mid
    return {
        "subject": subject,
        "stack": stack,
        "max_depth": lo,
        "first_failing_depth": lo + 1,
        "first_failing_exit": describe_exit(run_point(binary, subject, lo + 1, stack)),
        "capped": False,
    }


def ladder(binary: str, subject: str, lo_param: int, hi_param: int) -> dict:
    a = min_stack_for(binary, subject, lo_param)
    b = min_stack_for(binary, subject, hi_param)
    growth = max(0, b["min_stack"] - a["min_stack"])
    return {
        "subject": subject,
        "lo_param": lo_param,
        "lo_stack": a["min_stack"],
        "hi_param": hi_param,
        "hi_stack": b["min_stack"],
        "growth": growth,
        "bytes_per_level": growth / (hi_param - lo_param),
    }


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("mode", choices=["stack", "depth", "ladder"])
    p.add_argument("--bin", required=True)
    p.add_argument("--subject", required=True)
    p.add_argument("--depth", type=int, default=256)
    p.add_argument("--stack", type=int, default=2 * 1024 * 1024)
    p.add_argument("--lo", type=int, default=256)
    p.add_argument("--hi", type=int, default=4096)
    args = p.parse_args()

    if args.mode == "stack":
        out = min_stack_for(args.bin, args.subject, args.depth)
    elif args.mode == "depth":
        out = max_depth_for(args.bin, args.subject, args.stack)
    else:
        out = ladder(args.bin, args.subject, args.lo, args.hi)

    json.dump(out, sys.stdout, indent=2)
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
