"""Whole-device battery observations and optional estimated SoC power on macOS.

No metric here attributes battery use to Axolotl. Apple capacity values are
percent (IOKit/ps/IOPSKeys.h); powermetrics calls its rail power estimates, not
metered app joules. CPU time and Energy Impact are never converted into energy.
"""
from __future__ import annotations

import datetime as dt
import json
import math
from pathlib import Path
import re
import subprocess
import threading
import time


MAX_SESSION_SECONDS = 3600


class EnergyPrerequisiteError(RuntimeError):
    """Requested battery protocol cannot start in the current power state."""


def parse_battery(text: str) -> dict:
    source = re.search(r"Now drawing from '([^']+)'", text)
    charge = re.search(r"\b(\d+(?:\.\d+)?)%;\s*([^;\n]+)", text)
    power = {"AC Power": "ac", "Battery Power": "battery"}.get(
        source.group(1) if source else "", "unknown")
    percent = float(charge.group(1)) if charge else None
    if percent is not None and not 0 <= percent <= 100:
        percent = None
    state = charge.group(2).strip().lower() if charge else "unknown"
    return {"source": power, "charge_percent": percent, "state": state,
            "discharging": power == "battery" and state == "discharging",
            "available": percent is not None and power != "unknown"}


def read_battery() -> dict:
    try:
        result = subprocess.run(["/usr/bin/pmset", "-g", "batt"], check=True,
                                capture_output=True, text=True, timeout=3)
        return parse_battery(result.stdout)
    except (OSError, subprocess.SubprocessError):
        return {"source": "unknown", "charge_percent": None, "state": "unknown",
                "discharging": False, "available": False,
                "error": "pmset battery telemetry unavailable"}


_POWER = re.compile(r"^(CPU Power|GPU Power|ANE Power|Combined Power \(CPU \+ GPU \+ ANE\)):\s*([\d.]+)\s*(mW|W)\s*$", re.M)
_NAMES = {"CPU Power": "cpu", "GPU Power": "gpu", "ANE Power": "ane",
          "Combined Power (CPU + GPU + ANE)": "combined_cpu_gpu_ane"}


def parse_power(text: str) -> dict:
    """Parse one finite text sample, requiring the documented elapsed interval.

    Combined and individual rails are separate series; they must not be summed.
    Unknown/unsupported output stays unavailable rather than becoming zero.
    """
    elapsed = re.search(r"\(([\d.]+)ms elapsed\)", text)
    if not elapsed:
        return {"status": "unavailable", "reason": "missing elapsed sample interval"}
    seconds = float(elapsed.group(1)) / 1000
    if not math.isfinite(seconds) or seconds <= 0:
        return {"status": "unavailable", "reason": "invalid elapsed sample interval"}
    rails = {}
    for label, value, unit in _POWER.findall(text):
        watts = float(value) / (1000 if unit == "mW" else 1)
        if math.isfinite(watts) and watts >= 0:
            rails[_NAMES[label]] = watts
    if not rails:
        return {"status": "unavailable", "reason": "no supported SoC power estimates"}
    return {"status": "estimated", "interval_seconds": seconds, "watts": rails}


def summarize_battery(samples: list[dict], duration: float, requested: bool) -> dict:
    samples = sorted(samples, key=lambda sample: sample["elapsed_seconds"])
    result = {"status": "not_requested" if not requested else "unavailable",
              "scope": "whole_device", "duration_seconds": duration,
              "sample_count": len(samples), "resolution_percentage_points": 1,
              "charge_change_percentage_points": None,
              "percent_points_per_hour": None, "raw_file": "battery.jsonl"}
    transitions = []
    for sample in samples:
        state = (sample["source"], sample["state"])
        if not transitions or state != (transitions[-1]["source"], transitions[-1]["state"]):
            transitions.append({k: sample[k] for k in ["elapsed_seconds", "source", "state"]})
    result["power_states"] = transitions
    if not requested:
        result["reason"] = "battery-discharge measurement not requested"
        return result
    if len(samples) < 2 or any(not s["available"] for s in samples):
        result["reason"] = "incomplete battery telemetry"
        return result
    result["start_charge_percent"] = samples[0]["charge_percent"]
    result["end_charge_percent"] = samples[-1]["charge_percent"]
    if any(not s["discharging"] for s in samples):
        result.update(status="invalid", reason="battery protocol requires unplugged, discharging power for the entire interval")
        return result
    loss = samples[0]["charge_percent"] - samples[-1]["charge_percent"]
    if loss < 0:
        result.update(status="invalid", reason="battery gauge increased while discharging")
        return result
    result.update(status="measured", charge_change_percentage_points=loss,
                  below_resolution=loss < 1)
    observed_seconds = samples[-1]["elapsed_seconds"] - samples[0]["elapsed_seconds"]
    result["observation_interval_seconds"] = observed_seconds
    if observed_seconds >= 300 and loss >= 1:
        result["percent_points_per_hour"] = loss * 3600 / observed_seconds
    else:
        result["rate_unavailable_reason"] = "need at least 300 seconds and a 1 percentage-point drop for this coarse observed rate"
    result["attribution"] = "whole-device gauge change; not per-app consumption and not watt-hours"
    return result


def summarize_power(samples: list[dict], duration: float) -> dict:
    rails = {}
    for sample in samples:
        for rail, watts in sample["watts"].items():
            total = rails.setdefault(rail, {"estimated_joules": 0., "covered_seconds": 0., "sample_count": 0})
            total["estimated_joules"] += watts * sample["interval_seconds"]
            total["covered_seconds"] += sample["interval_seconds"]
            total["sample_count"] += 1
    for value in rails.values():
        value["mean_milliwatts"] = value["estimated_joules"] / value["covered_seconds"] * 1000
    coverage = max((v["covered_seconds"] for v in rails.values()), default=0)
    return {"status": "estimated" if rails else "unavailable", "scope": "whole_soc",
            "source": "powermetrics", "components": rails,
            "sample_count": len(samples), "covered_seconds": coverage,
            "session_seconds": duration,
            "coverage_fraction": min(1., coverage / duration) if duration > 0 else 0.,
            "raw_file": "powermetrics.txt", "intervals_file": "power-intervals.jsonl",
            "attribution": "estimated CPU/GPU/ANE rail energy for the device; not Axolotl-only, battery energy, or display/network energy",
            "aggregation": "combined_cpu_gpu_ane is already a total; do not add it to individual rails"}


class EnergySession:
    """Single-use collector; stop() is idempotent and returns a JSON-safe report.

    --battery rejects AC/unknown state at start. --power never prompts for sudo;
    missing privilege is reported as unavailable. Finite one-sample invocations
    avoid leaving an indefinitely running privileged sampler behind on stop.
    Only complete power intervals inside the session are integrated, with gaps
    disclosed as coverage. Collector overhead is part of whole-device readings.
    """
    def __init__(self, output_dir: Path, battery: bool = False, power: bool = False):
        self.output_dir = Path(output_dir)
        self.battery = battery
        self.power = power
        self._stop = threading.Event()
        self._threads = []
        self._battery_samples = []
        self._power_samples = []
        self._power_reason = None
        self._limit_reached = False
        self._started = None
        self._ended = None
        self._report = None

    def _observe_battery(self):
        sample = read_battery()
        sample.update(elapsed_seconds=time.monotonic() - self._started,
                      timestamp=dt.datetime.now(dt.timezone.utc).isoformat())
        self._battery_samples.append(sample)
        with (self.output_dir / "battery.jsonl").open("a") as stream:
            stream.write(json.dumps(sample) + "\n")
        return sample

    def start(self):
        if self._started is not None:
            raise RuntimeError("EnergySession is single-use")
        self.output_dir.mkdir(parents=True, exist_ok=True)
        self._started = time.monotonic()
        initial = self._observe_battery()
        if self.battery and not initial["discharging"]:
            raise EnergyPrerequisiteError("battery benchmark requires an unplugged Mac that reports discharging; current power source is " + initial["source"])
        if self.battery or self.power:
            self._threads.append(threading.Thread(target=self._battery_loop, daemon=True))
        if self.power:
            self._threads.append(threading.Thread(target=self._power_loop, daemon=True))
        for thread in self._threads:
            thread.start()
        return {"battery_requested": self.battery, "power_requested": self.power,
                "initial_power_source": initial["source"],
                "initial_charge_percent": initial["charge_percent"]}

    def _battery_loop(self):
        while not self._stop.wait(1):
            if time.monotonic() - self._started >= MAX_SESSION_SECONDS:
                self._limit_reached = True
                return
            self._observe_battery()

    def _power_loop(self):
        command = ["/usr/bin/sudo", "-n", "/usr/bin/powermetrics", "--samplers",
                   "cpu_power,gpu_power,ane_power", "--format", "text", "--sample-rate",
                   "1000", "--sample-count", "1"]
        # The external sampler is explicitly finite. Collection also has a hard
        # one-hour bound even if a caller forgets to stop its session.
        for _ in range(3600):
            if self._stop.is_set():
                return
            if time.monotonic() - self._started >= MAX_SESSION_SECONDS:
                self._limit_reached = True
                self._power_reason = "one-hour collection limit reached"
                return
            began = time.monotonic()
            try:
                process = subprocess.run(command, capture_output=True, text=True, timeout=8)
            except (OSError, subprocess.SubprocessError) as error:
                self._power_reason = f"power sampler unavailable: {type(error).__name__}"
                return
            ended = time.monotonic()
            with (self.output_dir / "powermetrics.txt").open("a") as stream:
                stream.write(process.stdout)
            if process.stderr:
                with (self.output_dir / "powermetrics.stderr.txt").open("a") as stream:
                    stream.write(process.stderr)
            if process.returncode != 0:
                self._power_reason = "powermetrics requires supported samplers and existing noninteractive sudo authorization; see raw stderr"
                return
            parsed = parse_power(process.stdout)
            if parsed["status"] != "estimated":
                self._power_reason = parsed["reason"]
                return
            if parsed["interval_seconds"] > ended - began + 0.1:
                self._power_reason = "sampler interval exceeds invocation wall duration"
                return
            parsed.update(began_monotonic=began, ended_monotonic=ended,
                          elapsed_end_seconds=ended - self._started)
            self._power_samples.append(parsed)
            with (self.output_dir / "power-intervals.jsonl").open("a") as stream:
                stream.write(json.dumps(parsed) + "\n")

    def stop(self) -> dict:
        if self._started is None:
            raise RuntimeError("start() must precede stop()")
        if self._report is not None:
            return self._report
        self._stop.set()
        # Take the battery endpoint before waiting for a finite power sample to
        # finish. Its timestamp is the actual observation, never backdated.
        final = self._observe_battery()
        duration = final["elapsed_seconds"]
        self._ended = self._started + duration
        for thread in self._threads:
            thread.join(timeout=10)
        if any(thread.is_alive() for thread in self._threads):
            raise RuntimeError("energy collector did not stop within its finite sample timeout")
        observed = [s for s in self._battery_samples if s["elapsed_seconds"] <= duration]
        complete = [s for s in self._power_samples if s["ended_monotonic"] <= self._ended]
        power = summarize_power(complete, duration) if self.power else {"status": "not_requested", "scope": "whole_soc"}
        if self.power and not complete:
            power["reason"] = self._power_reason or "no complete supported power intervals inside the session"
        elif self._power_reason:
            power["warning"] = self._power_reason
        battery = summarize_battery(observed, duration, self.battery)
        if self._limit_reached and self.battery:
            battery.update(status="invalid", charge_change_percentage_points=None,
                           percent_points_per_hour=None, reason="one-hour collection limit reached; power transitions after that point were not monitored")
        self._report = {"protocol_version": 1,
                        "battery": battery,
                        "soc_power": power,
                        "collector_overhead": "pmset polling and finite powermetrics invocations are included in whole-device readings; keep the protocol identical between runs"}
        return self._report
