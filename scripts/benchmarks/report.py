"""Versioned benchmark summaries and conservative, environment-matched comparisons.

Only finite raw samples are pooled, and only within the same session, metric,
unit, scope and direction. Comparisons with fewer than 20 observations or three
valid runs are informational (suspected regressions), never gating regressions.
"""
from __future__ import annotations

import json
import math
from collections import Counter
from numbers import Real
from statistics import fmean
from typing import Any, Iterable, Mapping

SCHEMA_VERSION = 1
MIN_SAMPLES = 20
MIN_RUNS = 3


def summarize(samples: Iterable[float]) -> dict[str, Any]:
    """Return nearest-rank p50/p95 and arithmetic mean of finite numeric samples.

    An empty sequence has n=0 and null statistics. Booleans, strings, NaN and
    infinities are invalid; they must not silently disappear from a baseline.
    """
    values = list(samples)
    if any(isinstance(v, bool) or not isinstance(v, Real) or not math.isfinite(v) for v in values):
        raise ValueError("benchmark samples must be finite numbers")
    values = sorted(float(v) for v in values)
    if not values:
        return dict(n=0, p50=None, p95=None, min=None, max=None, mean=None)
    rank = lambda p: values[max(0, math.ceil(len(values) * p) - 1)]
    return dict(n=len(values), p50=rank(.5), p95=rank(.95), min=values[0], max=values[-1], mean=fmean(values))


def _direction(metric: Mapping[str, Any]) -> str:
    return metric.get("direction", "higher" if metric.get("unit") in {"fps", "ops/s", "ops_per_s", "rows/s"} else "lower")


def _quality_invalid(run: Mapping[str, Any]) -> bool:
    quality = run.get("quality", {})
    return isinstance(quality, Mapping) and (quality.get("valid") is False or quality.get("comparable") is False or quality.get("status") in {"failed", "invalid"})


def _identity(row: Mapping[str, Any]) -> tuple[str, ...]:
    return tuple(str(row.get(k, "unknown")) for k in ("session_size", "id", "unit", "scope", "direction"))


def build_summary(report: Mapping[str, Any]) -> dict[str, Any]:
    """Group raw observations, retaining pooled statistics and per-run medians.

    Failed/explicitly invalid runs and unavailable samples do not enter pooled
    values. A required metric absent in even one same-session run is partial.
    """
    runs = report.get("runs", [])
    if not isinstance(runs, list):
        raise ValueError("report.runs must be a list")
    sessions = Counter(str(run.get("session_size", "unknown")) for run in runs)
    buckets: dict[tuple[str, ...], dict[str, Any]] = {}
    issues = []
    if not runs:
        issues.append("report has no runs")
    for index, run in enumerate(runs):
        session = str(run.get("session_size", "unknown"))
        for metric in run.get("metrics", []):
            row = dict(metric, session_size=session, direction=_direction(metric))
            key = _identity(row)
            bucket = buckets.setdefault(key, dict(session_size=key[0], id=key[1], unit=key[2], scope=key[3], direction=key[4], required=False, by_run={}, reasons=[]))
            bucket["required"] |= metric.get("required", True)
            reason = None
            if any(not isinstance(metric.get(field), str) or not metric[field] for field in ("id", "unit", "scope")):
                reason = "metric id, unit and scope must be non-empty strings"
            elif run.get("status", "passed") != "passed" or _quality_invalid(run):
                reason = "run failed or quality was explicitly invalid"
            elif metric.get("status", "available") != "available":
                reason = metric.get("reason", "metric unavailable")
            elif key[4] not in {"lower", "higher"}:
                reason = "invalid metric direction"
            else:
                try:
                    stats = summarize(metric.get("samples", []))
                except (TypeError, ValueError, OverflowError) as error:
                    reason = str(error)
                else:
                    if not stats["n"]:
                        reason = "no raw samples (aggregate-only values are not pooled)"
            if reason:
                bucket["reasons"].append(f"run {run.get('repeat', index + 1)}: {reason}")
            else:
                item = bucket["by_run"].setdefault(index, {"repeat": run.get("repeat", index + 1), "samples": []})
                item["samples"].extend(metric["samples"])
    if runs and not buckets:
        issues.append("report contains no measurement entries")
    rows = []
    for key, bucket in sorted(buckets.items()):
        by_run = bucket.pop("by_run")
        run_summaries = [dict(repeat=item["repeat"], **summarize(item["samples"])) for item in by_run.values()]
        stats = summarize(v for item in by_run.values() for v in item["samples"])
        available = len(by_run)
        total = sessions[bucket["session_size"]]
        if available < total:
            bucket["reasons"].append(f"metric missing or unavailable in {total - available} of {total} runs")
        status = "available" if available == total and not bucket["reasons"] else "partial" if available else "unavailable"
        confidence = []
        if stats["n"] < MIN_SAMPLES:
            confidence.append(f"fewer than {MIN_SAMPLES} raw samples")
        if available < MIN_RUNS:
            confidence.append(f"fewer than {MIN_RUNS} valid runs")
        if status != "available":
            confidence.append("incomplete observations")
        rows.append(dict(bucket, status=status, summary=stats, run_medians=[r["p50"] for r in run_summaries], run_summaries=run_summaries, run_count=available, available_run_count=available, total_run_count=total, confidence={"sufficient": not confidence, "reasons": confidence}))
    return {"schema_version": SCHEMA_VERSION, "metrics": rows, "issues": issues, "quality": [{"session_size": run.get("session_size"), "repeat": run.get("repeat"), "status": run.get("status"), "quality": run.get("quality", {})} for run in runs]}


def _incompatibilities(baseline: Mapping[str, Any], candidate: Mapping[str, Any]) -> list[str]:
    reasons = []
    for key in ("schema_version", "suite_version"):
        versions = (baseline.get(key), candidate.get(key))
        if any(type(value) is not int or value < 1 for value in versions) or versions[0] != versions[1]:
            reasons.append(f"{key} differs or is missing")
    if baseline.get("schema_version") != SCHEMA_VERSION:
        reasons.append("unsupported report schema_version")
    before, after = baseline.get("metadata", {}), candidate.get("metadata", {})
    for key in ("hardware", "configuration", "fixture_sha256", "benchmark_sha256"):
        if not before.get(key) or not after.get(key):
            reasons.append(f"metadata.{key} missing")
        elif before[key] != after[key]:
            reasons.append(f"metadata.{key} differs")
    for key in ("model", "os_version", "architecture", "logical_cpus", "memory_bytes"):
        if before.get("hardware", {}).get(key) in (None, "") or after.get("hardware", {}).get(key) in (None, ""):
            reasons.append(f"hardware.{key} missing")
    if before.get("conditions", {}) != after.get("conditions", {}):
        reasons.append("metadata.conditions differs")
    return reasons


def compare_reports(baseline: Mapping[str, Any], candidate: Mapping[str, Any], relative_threshold: float = .1, absolute_threshold_ms: float = 1.) -> dict[str, Any]:
    """Compare p50 and p95 only when provenance and measurement definitions match.

    A millisecond regression must exceed BOTH thresholds, other units use the
    relative threshold only. Low-confidence changes are suspected/non-gating.
    Missing required observations or failed reports make the overall result
    incomplete, while preserving known per-metric regression findings.
    """
    for value in (relative_threshold, absolute_threshold_ms):
        if isinstance(value, bool) or not isinstance(value, Real) or not math.isfinite(value) or value < 0:
            raise ValueError("comparison thresholds must be finite non-negative numbers")
    reasons = _incompatibilities(baseline, candidate)
    before = {_identity(row): row for row in build_summary(baseline)["metrics"]}
    after = {_identity(row): row for row in build_summary(candidate)["metrics"]}
    signatures = lambda rows, identity: {key[2:] for key in rows if key[:2] == identity}
    for identity in {key[:2] for key in before} & {key[:2] for key in after}:
        if signatures(before, identity) != signatures(after, identity):
            reasons.append(f"unit/scope/direction differs for {'/'.join(identity)}")
    result = {"schema_version": SCHEMA_VERSION, "compatible": not reasons, "reasons": reasons, "status": "incompatible" if reasons else "unchanged", "incomplete": False, "comparisons": [], "regressions": [], "suspected_regressions": [], "thresholds": {"relative": relative_threshold, "absolute_ms": absolute_threshold_ms}, "provenance": {"baseline_binary_sha256": baseline.get("metadata", {}).get("binary_sha256"), "candidate_binary_sha256": candidate.get("metadata", {}).get("binary_sha256")}}
    result["quality"] = {label: build_summary(report)["quality"] for label, report in (("baseline", baseline), ("candidate", candidate))}
    result["confidence"] = {"sufficient": False, "low_confidence_metrics": []}
    if reasons:
        return result
    if not before and not after:
        result["incomplete"] = True
        result["reasons"].append("neither report contains measurements")
    for label, report in (("baseline", baseline), ("candidate", candidate)):
        if report.get("status") != "passed" or any(run.get("status", "passed") != "passed" or _quality_invalid(run) for run in report.get("runs", [])):
            result["incomplete"] = True
            result["reasons"].append(f"{label} report contains failed or invalid runs")
    for key in sorted(before.keys() | after.keys()):
        old, new = before.get(key), after.get(key)
        row = dict(zip(("session_size", "id", "unit", "scope", "direction"), key))
        row.update(required=bool((old or {}).get("required") or (new or {}).get("required")), confidence={"sufficient": False}, status="unavailable")
        if old is None or new is None or old["status"] != "available" or new["status"] != "available":
            row["reason"] = "baseline or candidate metric missing/unavailable/partial"
            if row["required"]:
                result["incomplete"] = True
        else:
            sufficient = old["confidence"]["sufficient"] and new["confidence"]["sufficient"]
            row["confidence"] = {"sufficient": sufficient, "reasons": sorted(set(old["confidence"]["reasons"] + new["confidence"]["reasons"]))}
            row["baseline"], row["candidate"] = old["summary"], new["summary"]
            changes = {}
            for percentile in ("p50", "p95"):
                first, second = old["summary"][percentile], new["summary"][percentile]
                delta = second - first
                bad_delta = delta if key[4] == "lower" else -delta
                limit = max(abs(first) * relative_threshold, absolute_threshold_ms if key[2] == "ms" else 0.)
                changes[percentile] = {"delta": delta, "percent": delta / abs(first) * 100 if first else None, "regressed": bad_delta > limit, "improved": -bad_delta > limit}
            row["changes"] = changes
            if any(change["regressed"] for change in changes.values()):
                row["status"] = "regression" if sufficient else "suspected_regression"
                result["regressions" if sufficient else "suspected_regressions"].append(f"{key[0]}/{key[1]}")
            elif any(change["improved"] for change in changes.values()):
                row["status"] = "improved" if sufficient else "suspected_improvement"
            else:
                row["status"] = "unchanged"
        result["comparisons"].append(row)
    compared = [row for row in result["comparisons"] if "changes" in row]
    if not compared:
        result["incomplete"] = True
        result["reasons"].append("no available comparable measurements")
    result["confidence"] = {"sufficient": bool(compared) and all(row["confidence"]["sufficient"] for row in compared) and not result["incomplete"], "low_confidence_metrics": [f'{row["session_size"]}/{row["id"]}' for row in compared if not row["confidence"]["sufficient"]]}
    states = {row["status"] for row in result["comparisons"]}
    result["status"] = ("incomplete" if result["incomplete"] else "regression" if "regression" in states else "suspected_regression" if "suspected_regression" in states else "improved" if states & {"improved", "suspected_improvement"} else "unchanged")
    return result


def _cell(value: Any) -> str:
    return str(value).replace("|", "\\|").replace("\n", " ").replace("\r", " ")


def _number(value: Any) -> str:
    return "—" if value is None else f"{value:.3f}"


def _json(value: Any) -> str:
    return "```json\n" + json.dumps(value, indent=2, ensure_ascii=True, allow_nan=False) + "\n```"


def render_markdown(report: Mapping[str, Any]) -> str:
    """Render measurements, missing capabilities, provenance and scoped energy."""
    summary = build_summary(report)
    lines = ["# Axolotl benchmark", "", f"Status: **{_cell(report.get('status', 'unknown'))}** · schema {report.get('schema_version', 'unknown')} · suite {report.get('suite_version', 'unknown')}", "", "All figures retain the producer's units and measurement scope. Draw-submission timings are not mouse-to-photon latency.", "", "## Measurements", "", "| Session | Metric | Unit / scope | p50 | p95 | Max | Samples / runs | Status |", "| --- | --- | --- | ---: | ---: | ---: | ---: | --- |"]
    for row in summary["metrics"]:
        stats = row["summary"]
        status = row["status"] + ("; low confidence" if not row["confidence"]["sufficient"] else "")
        lines.append(f"| {_cell(row['session_size'])} | {_cell(row['id'])} | {_cell(row['unit'])} / {_cell(row['scope'])} | {_number(stats['p50'])} | {_number(stats['p95'])} | {_number(stats['max'])} | {stats['n']} / {row['run_count']} | {_cell(status)} |")
        if row["reasons"]:
            lines.extend(["", f"{_cell(row['session_size'])}/{_cell(row['id'])}: " + "; ".join(_cell(r) for r in row["reasons"]), ""])
    lines.extend(["", "Pooled percentiles use only raw samples with identical units and scope. Per-run medians are retained in report.json. Fewer than 20 samples or three valid runs is insufficient for regression gating.", "", "## Run quality, resources and energy", "", "Energy readings retain their recorded scope and availability. CPU usage and Energy Impact are not converted into battery energy. Whole-device battery changes include other applications and system activity."])
    for run in report.get("runs", []):
        lines.extend(["", f"### {_cell(run.get('session_size', 'unknown'))} / run {_cell(run.get('repeat', '?'))}", "", _json({key: run.get(key) for key in ("status", "quality", "resources", "energy") if key in run})])
    if summary["issues"]:
        lines.extend(["", "## Issues", ""] + [f"- {_cell(issue)}" for issue in summary["issues"]])
    lines.extend(["", "## Provenance", "", _json(report.get("metadata", {})), ""])
    return "\n".join(lines)


def render_comparison(result: Mapping[str, Any]) -> str:
    """Render compare_reports output without presenting unavailable data as green."""
    lines = ["# Axolotl benchmark comparison", "", f"Status: **{_cell(result.get('status', 'unknown'))}**", "", "Compatibility must match hardware, OS, fixtures, benchmark code, configuration and recorded conditions. Low-confidence changes are informational; suspected regressions do not establish regressions.", ""]
    lines.extend(f"- {_cell(reason)}" for reason in result.get("reasons", []))
    lines.extend(["", "| Session | Metric | Unit | Before p50 / p95 | After p50 / p95 | Status |", "| --- | --- | --- | ---: | ---: | --- |"])
    for row in result.get("comparisons", []):
        first, second = row.get("baseline", {}), row.get("candidate", {})
        lines.append(f"| {_cell(row['session_size'])} | {_cell(row['id'])} | {_cell(row['unit'])} | {_number(first.get('p50'))} / {_number(first.get('p95'))} | {_number(second.get('p50'))} / {_number(second.get('p95'))} | {_cell(row['status'])} |")
        if row.get("reason"):
            lines.extend(["", f"{_cell(row['id'])}: {_cell(row['reason'])}", ""])
    return "\n".join(lines) + "\n"
