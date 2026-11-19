import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from unittest import mock

SCRIPT = Path(__file__).resolve().parents[1] / ".codex/skills/rdog-control/scripts/prepare-replay.py"
spec = importlib.util.spec_from_file_location("prepare_replay", SCRIPT)
prepare = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = prepare
spec.loader.exec_module(prepare)


def encoded(value):
    return json.dumps(value).encode()


def template():
    return {"schema": "rdog.binding-template.v1", "entries": [
        {"parameter": "textA", "kind": "text", "target": "windowA", "reference": "valueA"},
        {"parameter": "moveA", "kind": "pointer", "target": "windowA", "reference": "pointA"}]}


def observation():
    return {"schema": "rdog.binding-observation.v1", "targets": {
        "windowA": {"pid": 321, "root_hwnd": 654, "text_target": {"kind": "native_control", "hwnd": 987}}},
        "pointers": {"pointA": {"target": "windowA", "points": [{"x": 11, "y": 22, "offset_ns": 0}],
                                "verify": {"kind": "position", "control_hwnd": 987}}}}


class PreparationTests(unittest.TestCase):
    def test_strict_json_and_schema(self):
        for raw in (b'{"a":1,"a":2}', b'"\\ud800"', b'NaN', b'1e999', b'\xff'):
            with self.subTest(raw=raw), self.assertRaises(prepare.Invalid):
                prepare.parse(raw)
        for mutate in (lambda t: t["entries"].append(t["entries"][0]),
                       lambda t: t.update(extra=1)):
            doc = template()
            mutate(doc)
            with self.assertRaises(prepare.Invalid):
                prepare.parse_template(encoded(doc))
        doc = observation()
        doc["targets"]["windowA"]["pid"] = True
        with self.assertRaises(prepare.Invalid):
            prepare.parse_observation(encoded(doc), prepare.parse_template(encoded(template())))

    def test_pipe_and_runtime_reach_sink_without_echo(self):
        parsed = prepare.parse_template(encoded(template()))
        observed = prepare.parse_observation(encoded(observation()), parsed)
        read_fd, write_fd = os.pipe()
        try:
            os.write(write_fd, encoded({"valueA": "synthetic-marker"}))
            os.close(write_fd)
            write_fd = -1
            values = prepare.parse_values(prepare.read_pipe(read_fd, time.monotonic() + 3), parsed)
        finally:
            os.close(read_fd)
            if write_fd != -1:
                os.close(write_fd)
        bound = prepare.runtime(parsed, observed, values)
        with tempfile.TemporaryDirectory() as directory:
            sink = Path(directory) / "sink.py"
            result = Path(directory) / "result.json"
            sink.write_text("import json,os,sys\n"
                            "v=json.load(sys.stdin)\n"
                            "good=(set(v)=={'bindings','pointer_bindings'} and "
                            "set(v['bindings'])=={'textA'} and set(v['pointer_bindings'])=={'moveA'} and "
                            "v['bindings']['textA']['value']=='synthetic-marker' and "
                            "set(v['bindings']['textA'])=={'pid','root_hwnd','target','value'} and "
                            "v['bindings']['textA']['pid']==321 and v['bindings']['textA']['root_hwnd']==654 and "
                            "v['bindings']['textA']['target']==dict(kind='native_control',hwnd=987) and "
                            "set(v['pointer_bindings']['moveA'])=={'pid','root_hwnd','points','verify'} and "
                            "v['pointer_bindings']['moveA']['pid']==321 and v['pointer_bindings']['moveA']['root_hwnd']==654 and "
                            "v['pointer_bindings']['moveA']['points']==[dict(x=11,y=22,offset_ns=0)] and "
                            "v['pointer_bindings']['moveA']['verify']==dict(kind='position',control_hwnd=987))\n"
                            "open(sys.argv[1],'w').write(json.dumps({'equivalent':good,'stdin_isatty':os.isatty(0)}))\n"
                            "print(json.dumps(v));print(json.dumps(v),file=sys.stderr)\n")
            stage = prepare.dispatch([sys.executable, str(sink), str(result)], encoded(bound), time.monotonic() + 3)
            self.assertEqual(stage, "runner_exit_zero")
            self.assertEqual(json.loads(result.read_text()), {"equivalent": True, "stdin_isatty": False})
    def test_fd_rejects_regular_file_and_open_pipe_times_out(self):
        with tempfile.TemporaryFile() as regular:
            with self.assertRaises(prepare.Invalid):
                prepare.read_pipe(regular.fileno(), time.monotonic() + 1)
        read_fd, write_fd = os.pipe()
        try:
            with self.assertRaises(prepare.Invalid):
                prepare.read_pipe(read_fd, time.monotonic() + 0.01)
        finally:
            os.close(read_fd)
            os.close(write_fd)

    def test_failure_and_timeout_are_unknown(self):
        self.assertEqual(prepare.dispatch([sys.executable, "-c", "import sys; sys.exit(1)"], b"{}", time.monotonic() + 3), "outcome_unknown_do_not_retry")
        self.assertEqual(prepare.dispatch([sys.executable, "-c", "import time; time.sleep(10)"], b"{}", time.monotonic() + 0.3), "outcome_unknown_do_not_retry")
        self.assertEqual(prepare.dispatch(["/nonexistent-replay-executable"], b"{}", time.monotonic() + 3), "not_dispatched")

    def test_expired_deadline_never_spawns(self):
        with tempfile.TemporaryDirectory() as directory:
            marker = Path(directory) / "started"
            command = [sys.executable, "-c", "import pathlib,sys;pathlib.Path(sys.argv[1]).touch()", str(marker)]
            with mock.patch.object(prepare.subprocess, "Popen") as spawn:
                self.assertEqual(prepare.dispatch(command, b"{}", time.monotonic() - 1), "not_dispatched")
                spawn.assert_not_called()
            self.assertEqual(prepare.dispatch(command, b"{}", time.monotonic() - 1), "not_dispatched")
            self.assertFalse(marker.exists())

    def test_paths_reject_fifo_symlink_and_directory(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fifo = root / "fifo"
            os.mkfifo(fifo)
            link = root / "link"
            link.symlink_to(fifo)
            for path in (root, fifo, link):
                with self.subTest(path=path), self.assertRaises((prepare.Invalid, OSError)):
                    prepare.read_file(path, time.monotonic() + 1)

    def test_checker_rejects_bad_receipt_and_cleans_read_error(self):
        args = type("Args", (), {"rdog": "/fixture/rdog", "plan": "/fixture/plan",
                                  "names": "/fixture/names", "journals": ["/fixture/journal"]})()
        before = {str(prepare.CHECKER): bytes.fromhex(prepare.CHECKER_HASH),
                  args.rdog: bytes.fromhex(prepare.RDOG_HASH), args.plan: bytes(32),
                  args.journals[0]: bytes([1]) * 32}
        child = mock.Mock()
        child.stdout.fileno.return_value = 10
        child.wait.return_value = 0
        receipt = {"stage": "wrong", "runtime_validated": False, "live_validated": False,
                   "plan_path": args.plan, "plan_sha256": bytes(32).hex(),
                   "source_sha256": [before[args.journals[0]].hex()]}
        for outcome in (encoded(receipt), OSError("private")):
            child.reset_mock()
            with mock.patch.object(prepare.subprocess, "Popen", return_value=child):
                with mock.patch.object(prepare, "read_pipe", side_effect=outcome if isinstance(outcome, OSError) else None,
                                       return_value=outcome if isinstance(outcome, bytes) else None):
                    with self.assertRaises(prepare.Invalid):
                        prepare.check_plan(args, time.monotonic() + 3, before)
            child.kill.assert_called_once()
            child.wait.assert_called()
            child.stdout.close.assert_called_once()

    def test_rejects_invalid_values_targets_and_intents(self):
        parsed = prepare.parse_template(encoded(template()))
        with self.assertRaises(prepare.Invalid):
            prepare.parse_values(encoded({"valueA": "x" * 262145}), parsed)
        with self.assertRaises(prepare.Invalid):
            prepare.parse_values(encoded({}), parsed)
        observed = observation()
        observed["pointers"]["pointA"]["target"] = "other"
        with self.assertRaises(prepare.Invalid):
            prepare.parse_observation(encoded(observed), parsed)
        observed = prepare.parse_observation(encoded(observation()), parsed)
        plan = {"steps": [{"parameter": "moveA", "intent": "confirmed_click"}]}
        with self.assertRaises(prepare.Invalid):
            prepare.validate_intents(parsed, observed, encoded(plan))

    def test_missing_approval_and_changed_inputs_never_dispatch(self):
        completed = subprocess.run([sys.executable, str(SCRIPT), "execute"], capture_output=True)
        self.assertEqual(json.loads(completed.stdout)["stage"], "not_dispatched")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for filename, content in (("template", encoded(template())), ("names", encoded(prepare.manifest(prepare.parse_template(encoded(template()))))),
                                      ("plan", b"plan"), ("rdog", b"runner"), ("journal", b"journal")):
                (root / filename).write_bytes(content)
            args = type("Args", (), {key: str(root / key) for key in ("template", "names", "plan", "rdog")})()
            args.journals = [str(root / "journal")]
            def mutate(*_):
                (root / "journal").write_bytes(b"changed")
            with mock.patch.object(prepare, "check_plan", side_effect=mutate):
                with self.assertRaises(prepare.Invalid):
                    prepare.prepare(args, time.monotonic() + 3)

    def test_cli_names_and_unknown_arguments_never_echo(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "template.json"
            names = Path(directory) / "names.json"
            source.write_bytes(encoded(template()))
            completed = subprocess.run([sys.executable, str(SCRIPT), "names", "--template", str(source), "--output", str(names)], capture_output=True)
            self.assertEqual(completed.returncode, 0)
            self.assertEqual(json.loads(names.read_bytes()), {"bindings": ["textA"], "pointer_bindings": ["moveA"]})
            completed = subprocess.run([sys.executable, str(SCRIPT), "--malicious-private-argument"], capture_output=True)
            self.assertNotIn(b"malicious-private-argument", completed.stdout + completed.stderr)
            self.assertEqual(json.loads(completed.stdout)["stage"], "not_dispatched")


if __name__ == "__main__":
    unittest.main()
