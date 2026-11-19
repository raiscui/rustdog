#!/usr/bin/env python3
"""Prepare replay bindings locally; execution requires current caller authorization."""
import argparse
import hashlib
import json
import os
import re
import select
import stat
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path

LIMIT = 1_048_576
CHECKER_HASH = "e70200afbd48b0f9c21b2472c54c00e492a0f1712f8daff3a5b0b2e7efe8e8fe"
CHECKER = Path(__file__).with_name("check-replay-plan.py")
RUN_TIMEOUT = 120
RDOG_HASH = "6a71e4e6e7b8b6cf97f3de2e832989a9c4970d29769c4fee5b62e1579140c305"
SAFE_ENV = {"PATH": "/usr/bin:/bin", "HOME": str(Path.home())}


class Invalid(Exception):
    pass


class Parser(argparse.ArgumentParser):
    def error(self, message):
        raise Invalid()


def require(condition):
    if not condition:
        raise Invalid()


def unique(pairs):
    result = {}
    for key, entry in pairs:
        require(key not in result)
        result[key] = entry
    return result


def reject_constant(_):
    raise Invalid()

def parse(raw):
    try:
        parsed = json.loads(raw.decode("utf-8"), object_pairs_hook=unique, parse_constant=reject_constant, parse_float=reject_constant)
        pending = [parsed]
        while pending:
            entry = pending.pop()
            if type(entry) is str:
                entry.encode("utf-8")
            elif type(entry) is dict:
                pending.extend(entry.keys())
                pending.extend(entry.values())
            elif type(entry) is list:
                pending.extend(entry)
        return parsed
    except (ValueError, UnicodeError, RecursionError):
        raise Invalid() from None


def exact(entry, keys):
    require(type(entry) is dict and set(entry) == set(keys))


def name(entry):
    require(type(entry) is str and bool(re.fullmatch(r"[A-Za-z][A-Za-z0-9_.-]{0,127}", entry)))
    return entry


def integer(entry, minimum, maximum):
    require(type(entry) is int and minimum <= entry <= maximum)
    return entry


def read_snapshot(path, maximum, deadline):
    require(hasattr(os, "O_NOFOLLOW") and time.monotonic() < deadline)
    fd = os.open(path, os.O_RDONLY | os.O_NONBLOCK | os.O_NOFOLLOW)
    try:
        metadata = os.fstat(fd)
        require(stat.S_ISREG(metadata.st_mode) and metadata.st_size <= maximum)
        chunks = bytearray()
        while True:
            require(time.monotonic() < deadline)
            chunk = os.read(fd, min(65536, maximum + 1 - len(chunks)))
            if not chunk:
                return bytes(chunks)
            chunks.extend(chunk)
            require(len(chunks) <= maximum)
    finally:
        os.close(fd)


def read_file(path, deadline):
    return read_snapshot(path, LIMIT, deadline)


def file_digest(path, maximum, deadline):
    require(hasattr(os, "O_NOFOLLOW") and time.monotonic() < deadline)
    fd = os.open(path, os.O_RDONLY | os.O_NONBLOCK | os.O_NOFOLLOW)
    try:
        metadata = os.fstat(fd)
        require(stat.S_ISREG(metadata.st_mode) and metadata.st_size <= maximum)
        digest = hashlib.sha256()
        total = 0
        while True:
            require(time.monotonic() < deadline)
            chunk = os.read(fd, 65536)
            if not chunk:
                return digest.digest()
            total += len(chunk)
            require(total <= maximum)
            digest.update(chunk)
    finally:
        os.close(fd)


@dataclass(frozen=True)
class Entry:
    parameter: str
    kind: str
    target: str
    reference: str


@dataclass(frozen=True)
class BindingTemplate:
    entries: tuple[Entry, ...]


def parse_template(raw):
    template = parse(raw)
    exact(template, ("schema", "entries"))
    require(template["schema"] == "rdog.binding-template.v1")
    require(type(template["entries"]) is list and bool(template["entries"]))
    entries = []
    for entry in template["entries"]:
        exact(entry, ("parameter", "kind", "target", "reference"))
        require(entry["kind"] in ("text", "pointer"))
        entries.append(Entry(name(entry["parameter"]), entry["kind"], name(entry["target"]), name(entry["reference"])))
    require(len({entry.parameter for entry in entries}) == len(entries))
    return BindingTemplate(tuple(entries))


def manifest(template):
    return {"bindings": sorted(e.parameter for e in template.entries if e.kind == "text"),
            "pointer_bindings": sorted(e.parameter for e in template.entries if e.kind == "pointer")}


def validate_target(target):
    require(type(target) is dict and target.get("kind") in ("native_control", "focused_control"))
    if target["kind"] == "native_control":
        exact(target, ("kind", "hwnd"))
        integer(target["hwnd"], 1, 2**64 - 1)
    else:
        exact(target, ("kind", "control_type"))
        integer(target["control_type"], 1, 2**31 - 1)


def validate_verify(verify):
    require(type(verify) is dict and verify.get("kind") in ("position", "focus", "selection", "scroll"))
    kind = verify["kind"]
    keys = {"position": ("kind", "control_hwnd"), "focus": ("kind", "control_hwnd"),
            "selection": ("kind", "control_hwnd", "start", "end"),
            "scroll": ("kind", "control_hwnd", "vertical", "increase")}
    exact(verify, keys[kind])
    integer(verify["control_hwnd"], 1, 2**64 - 1)
    if kind == "selection":
        integer(verify["start"], 0, 65535)
        integer(verify["end"], 0, 65535)
    if kind == "scroll":
        require(type(verify["vertical"]) is bool and type(verify["increase"]) is bool)


def parse_observation(raw, template):
    observation = parse(raw)
    exact(observation, ("schema", "targets", "pointers"))
    require(observation["schema"] == "rdog.binding-observation.v1")
    targets, pointers = observation["targets"], observation["pointers"]
    require(type(targets) is dict and type(pointers) is dict)
    require(set(targets) == {e.target for e in template.entries})
    require(set(pointers) == {e.reference for e in template.entries if e.kind == "pointer"})
    for target in targets.values():
        exact(target, ("pid", "root_hwnd", "text_target"))
        integer(target["pid"], 1, 2**31 - 1)
        integer(target["root_hwnd"], 1, 2**64 - 1)
        if target["text_target"] is not None:
            validate_target(target["text_target"])
    for pointer in pointers.values():
        exact(pointer, ("target", "points", "verify"))
        require(pointer["target"] in targets and type(pointer["points"]) is list and bool(pointer["points"]))
        for point in pointer["points"]:
            exact(point, ("x", "y", "offset_ns"))
            integer(point["x"], -(2**31), 2**31 - 1)
            integer(point["y"], -(2**31), 2**31 - 1)
            integer(point["offset_ns"], 0, 2**64 - 1)
        validate_verify(pointer["verify"])
    return observation


def parse_values(raw, template):
    values = parse(raw)
    require(type(values) is dict and set(values) == {e.reference for e in template.entries if e.kind == "text"})
    for value in values.values():
        require(type(value) is str and "\0" not in value and len(value.encode("utf-8")) <= 262144)
    return values


def runtime(template, observation, values):
    bindings, pointers = {}, {}
    for entry in template.entries:
        target = observation["targets"][entry.target]
        binding = {"pid": target["pid"], "root_hwnd": target["root_hwnd"]}
        if entry.kind == "text":
            require(target["text_target"] is not None)
            binding.update(target=target["text_target"], value=values[entry.reference])
            bindings[entry.parameter] = binding
        else:
            pointer = observation["pointers"][entry.reference]
            require(pointer["target"] == entry.target)
            binding.update(points=pointer["points"], verify=pointer["verify"])
            pointers[entry.parameter] = binding
    return {"bindings": bindings, "pointer_bindings": pointers}


def validate_intents(template, observation, raw_plan):
    plan = parse(raw_plan)
    require(type(plan) is dict and type(plan.get("steps")) is list)
    entries = {entry.parameter: entry for entry in template.entries}
    allowed = {"position_pointer": "position", "focus_control": "focus",
               "confirmed_click": "selection", "confirmed_drag": "selection", "confirmed_wheel": "scroll"}
    for step in plan["steps"]:
        require(type(step) is dict and step.get("parameter") in entries)
        entry = entries[step["parameter"]]
        if entry.kind == "text":
            require(step.get("intent") == "confirmed_text")
        else:
            pointer = observation["pointers"][entry.reference]
            require(step.get("intent") in allowed and pointer["verify"]["kind"] == allowed[step["intent"]])


def read_pipe(fd, deadline):
    require(not os.isatty(fd) and stat.S_ISFIFO(os.fstat(fd).st_mode))
    duplicate = os.dup(fd)
    blocking = os.get_blocking(duplicate)
    try:
        os.set_blocking(duplicate, False)
        chunks = bytearray()
        while True:
            remaining = deadline - time.monotonic()
            require(remaining > 0 and bool(select.select([duplicate], [], [], remaining)[0]))
            try:
                chunk = os.read(duplicate, min(65536, LIMIT + 1 - len(chunks)))
            except BlockingIOError:
                continue
            if not chunk:
                return bytes(chunks)
            chunks.extend(chunk)
            require(len(chunks) <= LIMIT)
    finally:
        os.set_blocking(duplicate, blocking)
        os.close(duplicate)


def snapshot(paths, deadline, executable):
    return {path: file_digest(path, 256 * LIMIT if path == executable else 64 * LIMIT, deadline) for path in paths}


def check_plan(args, deadline, before):
    require(before[str(CHECKER)].hex() == CHECKER_HASH and before[args.rdog].hex() == RDOG_HASH)
    require(time.monotonic() < deadline)
    command = [sys.executable, str(CHECKER), "--rdog", args.rdog, "--plan", args.plan,
               "--parameter-names", args.names, *args.journals]
    child = subprocess.Popen(command, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                             stderr=subprocess.DEVNULL, close_fds=True, env=SAFE_ENV)
    try:
        budget = max(0, deadline - time.monotonic())
        work_deadline = deadline - min(0.5, budget / 2)
        raw = read_pipe(child.stdout.fileno(), work_deadline)
        remaining = work_deadline - time.monotonic()
        require(remaining > 0 and child.wait(timeout=remaining) == 0)
        receipt = parse(raw)
        exact(receipt, ("stage", "runtime_validated", "live_validated", "plan_path", "plan_sha256", "source_sha256"))
        source_hashes = receipt["source_sha256"]
        expected = [before[path].hex() for path in args.journals]
        require(receipt["stage"] == "plan_and_parameter_names_validated"
                and receipt["runtime_validated"] is False and receipt["live_validated"] is False
                and receipt["plan_path"] == str(Path(args.plan).absolute())
                and receipt["plan_sha256"] == before[args.plan].hex()
                and type(source_hashes) is list and len(source_hashes) == len(expected)
                and all(type(digest) is str for digest in source_hashes)
                and len(set(source_hashes)) == len(source_hashes) and set(source_hashes) == set(expected))
    except (Invalid, subprocess.TimeoutExpired, OSError, ValueError, TypeError, KeyError, RecursionError):
        try:
            child.kill()
        except OSError:
            pass
        try:
            child.wait(timeout=max(0, deadline - time.monotonic()))
        except (subprocess.TimeoutExpired, OSError):
            pass
        raise Invalid() from None
    finally:
        child.stdout.close()


def prepare(args, deadline, extra=()):
    paths = [args.template, args.names, args.plan, args.rdog, str(CHECKER), *args.journals, *extra]
    before = snapshot(paths, deadline, args.rdog)
    template = parse_template(read_file(args.template, deadline))
    require(parse(read_file(args.names, deadline)) == manifest(template))
    check_plan(args, deadline, before)
    require(snapshot(paths, deadline, args.rdog) == before)
    return template, before

def dispatch(command, payload, deadline):
    if deadline - time.monotonic() <= 0.1:
        return "not_dispatched"
    try:
        child = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.DEVNULL,
                                 stderr=subprocess.DEVNULL, close_fds=True, env=SAFE_ENV)
    except OSError:
        return "not_dispatched"
    try:
        budget = max(0, deadline - time.monotonic())
        remaining = budget - min(0.5, budget / 2)
        require(remaining > 0)
        child.communicate(input=payload, timeout=remaining)
        return "runner_exit_zero" if child.returncode == 0 else "outcome_unknown_do_not_retry"
    except (Invalid, subprocess.TimeoutExpired, OSError):
        try:
            child.kill()
        except OSError:
            pass
        try:
            child.wait(timeout=max(0, deadline - time.monotonic()))
        except (subprocess.TimeoutExpired, OSError):
            pass
        return "outcome_unknown_do_not_retry"
    finally:
        child.stdin.close()


def parser():
    root = Parser(add_help=False)
    commands = root.add_subparsers(dest="command", required=True, parser_class=Parser)
    names = commands.add_parser("names", add_help=False)
    names.add_argument("--template", required=True)
    names.add_argument("--output", required=True)
    for command in ("validate", "execute"):
        sub = commands.add_parser(command, add_help=False)
        for option in ("template", "names", "plan", "rdog"):
            sub.add_argument("--" + option, required=True)
        if command == "execute":
            for option in ("observation", "operation-approval", "target-name", "entry-point"):
                sub.add_argument("--" + option, required=True)
            sub.add_argument("--namespace")
            sub.add_argument("--parameter-fd", type=int, required=True)
        sub.add_argument("journals", nargs="+")
    return root


def run(args):
    deadline = time.monotonic() + RUN_TIMEOUT
    if args.command == "names":
        encoded = json.dumps(manifest(parse_template(read_file(args.template, deadline))), separators=(",", ":")).encode()
        with open(args.output, "xb") as stream:
            stream.write(encoded + b"\n")
        return "names_prepared"
    if args.command == "validate":
        prepare(args, deadline)
        return "plan_and_parameter_names_validated"
    name(args.operation_approval)
    template, before = prepare(args, deadline, (args.observation,))
    observation = parse_observation(read_file(args.observation, deadline), template)
    validate_intents(template, observation, read_file(args.plan, deadline))
    values = parse_values(read_pipe(args.parameter_fd, deadline), template)
    bound = runtime(template, observation, values)
    payload = json.dumps(bound, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
    require(len(payload) <= LIMIT and snapshot(before, deadline, args.rdog) == before)
    command = [args.rdog, "replay", "--plan", args.plan, "--target-name", args.target_name,
               "--entry-point", args.entry_point]
    if args.namespace is not None:
        command += ["--namespace", args.namespace]
    return dispatch(command + args.journals, payload, deadline)


def main():
    try:
        stage = run(parser().parse_args())
    except (Invalid, OSError, ValueError, TypeError, KeyError, IndexError, RecursionError):
        stage = "not_dispatched"
    print(json.dumps({"stage": stage, "runtime_validated": False, "live_validated": False}, separators=(",", ":")))
    return 0 if stage in ("names_prepared", "plan_and_parameter_names_validated", "runner_exit_zero") else 1


if __name__ == "__main__":
    sys.exit(main())
