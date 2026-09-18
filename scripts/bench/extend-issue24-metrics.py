"""Extend the system-integration harness metrics module with the issue 24 diagnostics.

This repository ships Rust and bash. Python is permitted here as a scoped
exception, because this tool edits the harness's own Python module and the
injected runtime block becomes part of that module. The AST checks keep the
edit structural. The tool runs only where the Python harness runs, so python3
is present. Do not add Python elsewhere under this exception.
"""
import ast
import sys
from pathlib import Path


RUNTIME = '''
def _issue24_metric_deltas(before, after, result):
    import math

    for name in _ISSUE24_HISTOGRAMS:
        result.pop(name, None)
        result.pop(name + ".count", None)
        keys = (name + "_sum", name + "_count")
        if not all(key in sample and math.isfinite(sample[key]) for sample in (before, after) for key in keys):
            continue
        total, count = (after[key] - before[key] for key in keys)
        if total < 0 or count < 0 or not math.isfinite(total) or not math.isfinite(count):
            continue
        result[name + ".count"] = count
        if count > 0:
            mean = total / count
            if math.isfinite(mean):
                result[name] = mean
    for name in _ISSUE24_COUNTERS:
        result.pop(name + ".count", None)
        if not all(name in sample and math.isfinite(sample[name]) for sample in (before, after)):
            continue
        delta = after[name] - before[name]
        if math.isfinite(delta) and delta >= 0:
            result[name + ".count"] = delta
    return result


def _issue24_metric_report(metrics):
    import json

    histograms = {}
    for name in _ISSUE24_HISTOGRAMS:
        if name.endswith("_time"):
            unit = "seconds"
        elif name.endswith("_bytes"):
            unit = "bytes"
        elif name == "block_arrival_depth":
            unit = "blocks"
        else:
            unit = "items"
        histograms[name] = {
            "mean": metrics.get(name),
            "samples": metrics.get(name + ".count"),
            "unit": unit,
        }
    counters = {
        name: {
            "delta": metrics.get(name + ".count"),
            "unit": "nanoseconds" if name.endswith("_ns") else "events",
        }
        for name in _ISSUE24_COUNTERS
    }
    return "ISSUE24_METRICS " + json.dumps(
        {"histograms": histograms, "counters": counters}, sort_keys=True, allow_nan=False,
    )
'''


def extend(path, histograms, counters):
    source = path.read_text()
    tree = ast.parse(source)
    functions = {node.name: node for node in tree.body if isinstance(node, ast.FunctionDef)}
    required = {"compute_metric_deltas", "format_node_metrics"}
    if not required <= functions.keys():
        raise ValueError("The metrics module does not have the required report functions.")
    extensions = {"_issue24_metric_deltas", "_issue24_metric_report"}
    if extensions & functions.keys():
        if not extensions <= functions.keys():
            raise ValueError("The metrics module has an incomplete diagnostic extension.")
        assignments = {
            node.targets[0].id: ast.literal_eval(node.value)
            for node in tree.body
            if isinstance(node, ast.Assign) and isinstance(node.targets[0], ast.Name)
            and node.targets[0].id in {"_ISSUE24_HISTOGRAMS", "_ISSUE24_COUNTERS"}
        }
        if assignments != {"_ISSUE24_HISTOGRAMS": histograms, "_ISSUE24_COUNTERS": counters}:
            raise ValueError("The diagnostic registry does not match the extension.")
        return
    changes = []
    for name, statement in [
        ("compute_metric_deltas", "    result = _issue24_metric_deltas(before, after, result)\n"),
        ("format_node_metrics", "    lines.append(_issue24_metric_report(metrics))\n"),
    ]:
        function = functions[name]
        expected_args = ["before", "after"] if name == "compute_metric_deltas" else ["metrics"]
        if [arg.arg for arg in function.args.args] != expected_args:
            raise ValueError(f"The function {name} has different parameters.")
        final = function.body[-1]
        if not isinstance(final, ast.Return) or final.value is None:
            raise ValueError(f"The function {name} does not have a final return statement.")
        if name == "compute_metric_deltas" and not (isinstance(final.value, ast.Name) and final.value.id == "result"):
            raise ValueError("The metric delta return contract changed.")
        if name == "format_node_metrics" and ast.dump(final.value) != ast.dump(ast.parse("'\\n'.join(lines)", mode="eval").body):
            raise ValueError("The metric report return contract changed.")
        changes.append((final.lineno - 1, statement))
        if name == "format_node_metrics":
            for node in ast.walk(function):
                if isinstance(node, ast.Return) and isinstance(node.value, ast.Constant) and isinstance(node.value.value, str):
                    replacement = " " * node.col_offset
                    replacement += f"return {node.value.value!r} + '\\n' + _issue24_metric_report(metrics)\n"
                    changes.append((node.lineno - 1, replacement, node.end_lineno))
    lines = source.splitlines(keepends=True)
    for change in sorted(changes, reverse=True):
        start, replacement, *end = change
        lines[start:end[0] if end else start] = [replacement]
    source = "".join(lines)
    source += "\n_ISSUE24_HISTOGRAMS = " + repr(histograms) + "\n"
    source += "_ISSUE24_COUNTERS = " + repr(counters) + "\n" + RUNTIME
    ast.parse(source)
    path.write_text(source)


if __name__ == "__main__":
    extend(Path(sys.argv[1]), sys.argv[2].split("|"), sys.argv[3].split("|"))
