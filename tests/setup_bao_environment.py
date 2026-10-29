#!/usr/bin/env python3
"""Mock CLI checks for setup-bao-environment.sh; no OpenBao server is contacted."""
import copy
import errno
import json
import os
from pathlib import Path
import pty
import select
import stat
import subprocess
import tempfile
import time
import unittest


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "setup-bao-environment.sh"
POLICY = ROOT / "scripts" / "bao" / "policies" / "admin.hcl"
SETUP_TOKEN = "mock-setup-token-private"
ISSUED_TOKEN = "mock-issued-token-private"
DIAGNOSTIC = "mock-server-secret-private"
MOCK = r'''#!/usr/bin/env python3
import json
import os
from pathlib import Path
import sys

state_path = Path(os.environ["MOCK_BAO_STATE"])
state = json.loads(state_path.read_text())
args = sys.argv[1:]
state["calls"].append(args)
state_path.write_text(json.dumps(state))
namespace_flags = [arg for arg in args if arg.startswith("-namespace=")]
assert len(namespace_flags) == 1
namespace = namespace_flags[0].split("=", 1)[1]
command_depth = 2 if args[0] in {"token", "namespace", "policy"} else 1
assert args[command_depth].startswith("-namespace=")
assert not any(name in os.environ for name in ("BAO_NAMESPACE", "VAULT_NAMESPACE", "BAO_WRAP_TTL", "VAULT_WRAP_TTL"))
assert "mock-setup-token-private" not in " ".join(args)
assert "mock-issued-token-private" not in " ".join(args)
assert os.environ.get("BAO_TOKEN") in ("mock-setup-token-private", "mock-issued-token-private")
command = tuple(args[:command_depth])
remaining = [arg for arg in args[command_depth:] if not arg.startswith("-")]


def save():
    state_path.write_text(json.dumps(state))


def emit(value):
    print(json.dumps(value))
    sys.exit(0)


def fail(code=403):
    print("Error reading server state: mock-server-secret-private", file=sys.stderr)
    print("URL: GET https://example.invalid/v1/state", file=sys.stderr)
    print("Code: %s. Errors:" % code, file=sys.stderr)
    if code == 403:
        print("* Code: 404. Errors: mock-server-secret-private", file=sys.stderr)
    sys.exit(2)


def absent(path):
    if state.get("absence") == "http":
        fail(404)
    print("No value found at " + path, file=sys.stderr)
    sys.exit(2)


def injected_failure(phase):
    if state.get("failure_phase") != phase:
        return
    error = state.get("error")
    if error is None:
        fail(403)
    sys.stdout.write(error.get("stdout", ""))
    sys.stderr.write(error.get("stderr", ""))
    sys.exit(error.get("returncode", 2))


failure = state.get("failure")
if command == ("token", "lookup") and os.environ["BAO_TOKEN"] == "mock-setup-token-private":
    assert namespace == ""
    emit({"data": {"id": "mock-setup-token-private", "policies": state.get("setup_policies", ["super-admin"]), "namespace_path": ""}})
if command == ("read",):
    path = remaining[0]
    if path.startswith("sys/namespaces/"):
        assert namespace == ""
        injected_failure("namespace_read")
        if failure == "namespace_network":
            print("mock-server-secret-private connection failed", file=sys.stderr)
            sys.exit(2)
        if failure == "namespace_forbidden":
            fail(403)
        if failure == "namespace_bad_request":
            fail(400)
        if failure == "namespace_malformed":
            print("mock-server-secret-private invalid json")
            sys.exit(0)
        if not state["namespace"]:
            absent(path)
        emit({"data": {"path": path.rsplit("/", 1)[1] + "/", "custom_metadata": state["metadata"]}})
    assert path == "sys/policies/acl/admin"
    assert namespace == state["name"]
    injected_failure("policy_read")
    if failure == "policy_forbidden":
        fail(403)
    if failure == "policy_malformed":
        emit({"data": {"policy": 42}})
    if state["policy"] is None:
        absent(path)
    emit({"data": {"policy": state["policy"]}})
if command == ("namespace", "create"):
    assert namespace == ""
    injected_failure("namespace_create")
    state["namespace"] = True
    state["name"] = remaining[0]
    state["metadata"] = {}
    save()
    emit({"path": state["name"] + "/"})
if command == ("policy", "write"):
    assert namespace == state["name"]
    assert remaining[0] == "admin"
    injected_failure("policy_write")
    state["policy"] = Path(remaining[1]).read_text()
    save()
    print("Success! Uploaded policy: admin")
    sys.exit(0)
if command == ("token", "create"):
    assert namespace == state["name"]
    injected_failure("token_create")
    assert "-policy=admin" in args and "-orphan" in args and "-type=service" in args
    assert "-display-name=bao-mcp" in args and "-no-default-policy" not in args
    assert [arg.split("=", 1)[1] for arg in args if arg.startswith("-ttl=")] == [arg.split("=", 1)[1] for arg in args if arg.startswith("-explicit-max-ttl=")]
    state["issued"] = True
    save()
    if failure == "issuance":
        fail(500)
    if failure == "token_malformed":
        print("mock-server-secret-private invalid json")
        sys.exit(0)
    auth = {"client_token": "mock-issued-token-private", "token_type": "service", "orphan": True, "policies": ["admin", "default"], "token_policies": ["admin", "default"], "lease_duration": state.get("issued_ttl", 3600)}
    auth.update(state.get("auth_override", {}))
    if state.get("omit_create_token_type"):
        auth.pop("token_type")
    emit({"auth": auth})
if command == ("token", "lookup"):
    assert os.environ["BAO_TOKEN"] == "mock-issued-token-private"
    assert namespace == state["name"]
    injected_failure("token_lookup")
    if failure == "verify":
        fail(403)
    data = {"id": "mock-issued-token-private", "type": "service", "orphan": True, "policies": ["admin", "default"], "ttl": state.get("issued_ttl", 3600), "explicit_max_ttl": state.get("issued_ttl", 3600), "namespace_path": state["name"] + "/"}
    data.update(state.get("lookup_override", {}))
    if state.get("race_file"):
        Path(state["race_file"]).write_text("preserve raced file")
    if state.get("race_symlink"):
        Path(state["race_symlink"]).symlink_to(state["race_target"])
    emit({"data": data})
raise AssertionError("Unexpected mock command")
'''


class SetupBaoEnvironment(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)
        self.bin = self.directory / "bin"
        self.bin.mkdir()
        self.mock = self.bin / "bao"
        self.mock.write_text(MOCK)
        self.mock.chmod(0o755)
        self.state_path = self.directory / "state.json"
        self.target = self.directory / "agent-admin.token"
        self.environment = os.environ.copy()
        self.environment.update({
            "PATH": str(self.bin) + os.pathsep + os.environ["PATH"],
            "MOCK_BAO_STATE": str(self.state_path),
            "BAO_ADDR": "http://example.invalid:8200",
            "BAO_TOKEN": SETUP_TOKEN,
            "BAO_NAMESPACE": "inherited/wrong",
            "VAULT_NAMESPACE": "inherited/wrong",
            "BAO_WRAP_TTL": "1h",
            "VAULT_WRAP_TTL": "1h",
        })
        self.initial = {"namespace": False, "name": "agent", "metadata": {}, "policy": None, "calls": [], "issued": False}
        self.write_state()

    def write_state(self, **updates):
        state = copy.deepcopy(self.initial)
        state.update(updates)
        self.state_path.write_text(json.dumps(state))

    def state(self):
        return json.loads(self.state_path.read_text())

    def run_setup(self, *extra, environment=None, target=None):
        result = subprocess.run([str(SCRIPT), "--token-file", str(target or self.target), *extra], env=environment or self.environment, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=15, text=True)
        self.assert_secret_safe(result.stdout + result.stderr)
        return result

    def assert_secret_safe(self, output):
        for secret in (SETUP_TOKEN, ISSUED_TOKEN, DIAGNOSTIC):
            self.assertNotIn(secret, output)

    def assert_failure(self, result):
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "")
        self.assertTrue(result.stderr.startswith("Error: "), result.stderr)
        self.assertFalse(self.target.exists())

    def test_fresh_setup_and_private_publication(self):
        result = self.run_setup()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.target.read_text(), ISSUED_TOKEN + "\n")
        self.assertEqual(stat.S_IMODE(self.target.stat().st_mode), 0o600)
        self.assertEqual(result.stdout, "Namespace: agent\nPolicy: admin\nToken file: " + str(self.target) + "\nToken TTL: 3600 seconds\n")
        self.assertEqual(result.stderr, "")
        state = self.state()
        self.assertEqual(state["policy"], POLICY.read_text())
        self.assertEqual(sum(call[:2] == ["namespace", "create"] for call in state["calls"]), 1)
        self.assertEqual(sum(call[:2] == ["policy", "write"] for call in state["calls"]), 1)
        self.assertFalse(list(self.directory.glob(".bao-setup-*")))

    def test_cli_omits_create_token_type_but_lookup_must_confirm_service(self):
        self.write_state(omit_create_token_type=True)
        result = self.run_setup()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.target.unlink()
        self.write_state(omit_create_token_type=True, lookup_override={"type": "batch"})
        self.assert_failure(self.run_setup())

    def test_existing_namespace_metadata_and_policy_are_preserved(self):
        metadata = {"owner": "keep-this", "purpose": "existing"}
        self.write_state(namespace=True, metadata=metadata, policy=POLICY.read_text())
        result = self.run_setup()
        self.assertEqual(result.returncode, 0, result.stderr)
        state = self.state()
        self.assertEqual(state["metadata"], metadata)
        self.assertFalse(any(call[:2] in (["namespace", "create"], ["policy", "write"]) for call in state["calls"]))

    def test_policy_conflict_stops_without_overwrite_or_issuance(self):
        self.write_state(namespace=True, metadata={"keep": "yes"}, policy='path "*" { capabilities = ["read"] }')
        self.assert_failure(self.run_setup())
        state = self.state()
        self.assertFalse(state["issued"])
        self.assertIn('capabilities = ["read"]', state["policy"])
        self.assertFalse(any(call[:2] == ["policy", "write"] for call in state["calls"]))

    def test_confirmed_http_404_is_missing(self):
        self.write_state(absence="http")
        result = self.run_setup()
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_state_failures_never_count_as_absence(self):
        for failure in ("namespace_network", "namespace_forbidden", "namespace_bad_request", "namespace_malformed", "policy_forbidden", "policy_malformed"):
            with self.subTest(failure=failure):
                self.write_state(namespace=True, policy=POLICY.read_text(), failure=failure)
                self.assert_failure(self.run_setup())
                self.assertFalse(self.state()["issued"])
                self.assertFalse(any(call[:2] in (["namespace", "create"], ["policy", "write"]) for call in self.state()["calls"]))

    def test_non_root_setup_policy_needs_no_setup_token_lookup(self):
        self.write_state(setup_policies=["super-admin"])
        result = self.run_setup()
        self.assertEqual(result.returncode, 0, result.stderr)
        calls = self.state()["calls"]
        self.assertEqual(calls[0], ["read", "-namespace=", "-format=json", "sys/namespaces/agent"])
        lookups = [call for call in calls if call[:2] == ["token", "lookup"]]
        self.assertEqual(lookups, [["token", "lookup", "-namespace=agent", "-format=json"]])
        self.assertFalse(any("capabilities" in call for call in calls))

    def test_actual_namespace_api_denial_precedes_mutation(self):
        self.write_state(failure_phase="namespace_read")
        result = self.run_setup()
        self.assert_failure(result)
        self.assertEqual(result.stderr, "Error: Namespace read failed (namespace: root, path: sys/namespaces/agent): HTTP 403, permission denied.\n")
        self.assertEqual(len(self.state()["calls"]), 1)
        self.assertFalse(self.state()["namespace"])

    def test_later_api_failures_retain_operation_context_and_status(self):
        cases = (
            ("namespace_create", "Namespace creation", "root", "sys/namespaces/agent", 403),
            ("policy_read", "Admin policy read", "agent", "sys/policies/acl/admin", 403),
            ("policy_write", "Admin policy installation", "agent", "sys/policies/acl/admin", 403),
            ("token_create", "Token issuance", "agent", "auth/token/create", 500),
            ("token_lookup", "Issued-token verification", "agent", "auth/token/lookup-self", 401),
        )
        for phase, label, namespace, path, status in cases:
            with self.subTest(phase=phase):
                self.write_state(failure_phase=phase, error={"stderr": DIAGNOSTIC + "\nCode: " + str(status) + ". Errors:\nAuthorization: " + SETUP_TOKEN})
                result = self.run_setup()
                self.assert_failure(result)
                self.assertIn(label + " failed (namespace: " + namespace + ", path: " + path + "): HTTP " + str(status), result.stderr)
                self.assertEqual(self.state()["namespace"], phase != "namespace_create")
                if phase in {"token_create", "token_lookup"}:
                    self.assertEqual(self.state()["policy"], POLICY.read_text())

    def test_cli_error_diagnostics_are_allowlisted_and_never_exposed(self):
        cases = (
            ("Code: 401. Errors:\n", "HTTP 401, authentication failed"),
            ("Code: 403. Errors:\nCode: 404. Errors:\n", "HTTP 403, permission denied"),
            ("Code: 503. Errors:\nserver is sealed\n", "HTTP 503, server sealed"),
            ("Code: 503. Errors:\n", "HTTP 503, server unavailable"),
            ("x509: certificate signed by unknown authority\n", "CLI exit 7, TLS error"),
            ("remote error: tls: bad certificate\n", "CLI exit 7, TLS error"),
            ("dial tcp: lookup example.invalid: no such host\n", "CLI exit 7, DNS resolution failed"),
            ("dial tcp: connect: connection refused\n", "CLI exit 7, connection failed"),
            ("context deadline exceeded\n", "CLI exit 7, connection timed out"),
            ("server is sealed\n", "CLI exit 7, server sealed"),
            ("service unavailable\n", "CLI exit 7, server unavailable"),
            ("unknown failure\n", "CLI exit 7, unclassified CLI error"),
        )
        for diagnostic, expected in cases:
            with self.subTest(expected=expected, diagnostic=diagnostic):
                self.write_state(failure_phase="namespace_read", error={
                    "returncode": 7,
                    "stdout": "X-Vault-Token: " + ISSUED_TOKEN,
                    "stderr": DIAGNOSTIC + "\nURL: GET https://example.invalid/private\n" + diagnostic + "Authorization: " + SETUP_TOKEN,
                })
                result = self.run_setup()
                self.assert_failure(result)
                self.assertEqual(result.stderr, "Error: Namespace read failed (namespace: root, path: sys/namespaces/agent): " + expected + ".\n")
                self.assertEqual(len(self.state()["calls"]), 1)

    def test_absence_requires_exit_two_and_confirmed_404_or_exact_no_value(self):
        cases = (
            {"returncode": 1, "stderr": "Code: 404. Errors:\n"},
            {"returncode": 2, "stderr": "* Code: 404. Errors:\n"},
            {"returncode": 2, "stderr": DIAGNOSTIC + "\nNo value found at sys/namespaces/agent\n"},
        )
        for error in cases:
            with self.subTest(error=error):
                self.write_state(failure_phase="namespace_read", error=error)
                self.assert_failure(self.run_setup())
                self.assertEqual(len(self.state()["calls"]), 1)
                self.assertFalse(self.state()["namespace"])

    def test_cli_process_launch_error_is_distinct_from_timeout(self):
        custom = self.directory / "python-customization"
        custom.mkdir()
        (custom / "sitecustomize.py").write_text(
            "import os, subprocess\n"
            "original = subprocess.run\n"
            "def run(command, *args, **kwargs):\n"
            "    if command[0] == 'bao':\n"
            "        if os.environ['MOCK_BAO_PROCESS_FAILURE'] == 'launch':\n"
            "            raise OSError('mock-server-secret-private')\n"
            "        raise subprocess.TimeoutExpired(command, 60, output=b'mock-issued-token-private', stderr=b'mock-server-secret-private')\n"
            "    return original(command, *args, **kwargs)\n"
            "subprocess.run = run\n"
        )
        for mode, diagnostic in (("launch", "CLI process launch error"), ("timeout", "CLI timed out after 60 seconds")):
            with self.subTest(mode=mode):
                environment = self.environment.copy()
                environment["PYTHONPATH"] = str(custom)
                environment["MOCK_BAO_PROCESS_FAILURE"] = mode
                result = self.run_setup(environment=environment)
                self.assert_failure(result)
                self.assertEqual(result.stderr, "Error: Namespace read failed (namespace: root, path: sys/namespaces/agent): " + diagnostic + ".\n")
                self.assertEqual(self.state()["calls"], [])

    def test_existing_file_is_not_overwritten(self):
        self.target.write_text("keep existing content")
        result = self.run_setup()
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.target.read_text(), "keep existing content")
        self.assertEqual(self.state()["calls"], [])

    def test_existing_and_dangling_symlinks_are_rejected(self):
        for exists in (False, True):
            with self.subTest(existing_target=exists):
                linked = self.directory / "linked"
                if exists:
                    linked.write_text("preserve")
                self.target.symlink_to(linked)
                result = self.run_setup()
                self.assertNotEqual(result.returncode, 0)
                self.assertTrue(self.target.is_symlink())
                self.assertEqual(self.state()["calls"], [])
                self.target.unlink()

    def test_invalid_options_namespace_ttl_address_and_parent_preflight(self):
        cases = [("--namespace", value) for value in ("root", "auth", "sys", "audit", "cubbyhole", "identity", "../bad", "a/b", "/agent", ".", "-agent")]
        cases += [("--ttl", value) for value in ("0s", "-1h", "365", "1y", "1M", "1hjunk", "0.5s", "1825d1s", "NaN", "1e9s")]
        cases += [("--unknown", "value")]
        for arguments in cases:
            with self.subTest(arguments=arguments):
                self.assert_failure(self.run_setup(*arguments))
                self.assertEqual(self.state()["calls"], [])
        missing_address = self.environment.copy()
        missing_address.pop("BAO_ADDR")
        self.assert_failure(self.run_setup(environment=missing_address))
        self.assertEqual(self.state()["calls"], [])
        self.assert_failure(self.run_setup(target=self.directory / "missing" / "token"))
        self.assertEqual(self.state()["calls"], [])
        locked = self.directory / "locked"
        locked.mkdir()
        locked.chmod(0o500)
        self.addCleanup(locked.chmod, 0o700)
        self.assert_failure(self.run_setup(target=locked / "token"))
        self.assertEqual(self.state()["calls"], [])

    def test_compound_and_maximum_ttl_are_forwarded_as_explicit_max(self):
        for index, ttl in enumerate(("1h30m", "1825d", "0.5h30m", "1s")):
            with self.subTest(ttl=ttl):
                self.write_state(issued_ttl=1)
                result = self.run_setup("--ttl", ttl, target=self.directory / (str(index) + ".token"))
                self.assertEqual(result.returncode, 0, result.stderr)
                creation = next(call for call in self.state()["calls"] if call[:2] == ["token", "create"])
                self.assertIn("-ttl=" + ttl, creation)
                self.assertIn("-explicit-max-ttl=" + ttl, creation)
                self.assertNotIn("-no-default-policy", creation)

    def test_custom_namespace_and_server_clamped_ttl(self):
        self.write_state(issued_ttl=120)
        result = self.run_setup("--namespace", "project-a", "--ttl", "1h")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("Namespace: project-a\n", result.stdout)
        self.assertIn("Token TTL: 120 seconds\n", result.stdout)

    def test_malformed_or_failed_issuance_and_lookup_stay_private(self):
        for failure in ("issuance", "token_malformed", "verify"):
            with self.subTest(failure=failure):
                self.write_state(failure=failure)
                self.assert_failure(self.run_setup())
                self.assertTrue(self.state()["namespace"])
                self.assertEqual(self.state()["policy"], POLICY.read_text())

    def test_issued_token_type_orphan_ttl_and_policies_are_checked(self):
        for override in ({"token_type": "batch"}, {"orphan": False}, {"lease_duration": 0}, {"lease_duration": 86401}, {"policies": ["root", "admin", "default"]}, {"policies": ["admin"]}, {"token_policies": ["root"]}, {"client_token": ""}):
            with self.subTest(override=override):
                self.write_state(auth_override=override)
                self.assert_failure(self.run_setup())

    def test_lookup_namespace_type_orphan_explicit_ttl_and_policies_are_checked(self):
        for override in ({"namespace_path": "wrong/"}, {"type": "batch"}, {"orphan": False}, {"ttl": 0}, {"ttl": 86401}, {"explicit_max_ttl": 0}, {"explicit_max_ttl": 86401}, {"policies": ["root", "admin"]}, {"id": "another-private-token"}):
            with self.subTest(override=override):
                self.write_state(lookup_override=override)
                self.assert_failure(self.run_setup())

    def test_raced_file_or_symlink_is_not_overwritten(self):
        for symlink in (False, True):
            with self.subTest(symlink=symlink):
                linked = self.directory / "race-target"
                if symlink:
                    linked.write_text("preserve linked file")
                    self.write_state(race_symlink=str(self.target), race_target=str(linked))
                else:
                    self.write_state(race_file=str(self.target))
                result = self.run_setup()
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(self.target.read_text(), "preserve linked file" if symlink else "preserve raced file")
                self.assertEqual(self.target.is_symlink(), symlink)
                self.assertFalse(list(self.directory.glob(".bao-setup-*")))
                self.target.unlink()

    def test_missing_noninteractive_token_does_not_fallback_to_cli_helper(self):
        environment = self.environment.copy()
        environment.pop("BAO_TOKEN")
        result = self.run_setup(environment=environment)
        self.assert_failure(result)
        self.assertIn("Set BAO_TOKEN", result.stderr)
        self.assertEqual(self.state()["calls"], [])

    def test_hidden_interactive_setup_token(self):
        environment = self.environment.copy()
        environment.pop("BAO_TOKEN")
        master, slave = pty.openpty()
        process = subprocess.Popen([str(SCRIPT), "--token-file", str(self.target)], env=environment, stdin=slave, stderr=slave, stdout=subprocess.PIPE)
        os.close(slave)
        captured = b""
        try:
            deadline = time.monotonic() + 8
            while b"OpenBao setup/admin token: " not in captured and time.monotonic() < deadline:
                ready, _, _ = select.select([master], [], [], 0.1)
                if ready:
                    captured += os.read(master, 4096)
            self.assertIn(b"OpenBao setup/admin token: ", captured)
            os.write(master, (SETUP_TOKEN + "\n").encode())
            stdout, _ = process.communicate(timeout=15)
            while select.select([master], [], [], 0)[0]:
                try:
                    chunk = os.read(master, 4096)
                except OSError as error:
                    if error.errno == errno.EIO:
                        break
                    raise
                if not chunk:
                    break
                captured += chunk
            self.assert_secret_safe((stdout + captured).decode())
            self.assertEqual(process.returncode, 0, captured.decode())
            self.assertEqual(self.target.read_text(), ISSUED_TOKEN + "\n")
        finally:
            if process.poll() is None:
                process.kill()
                process.communicate()
            os.close(master)

    def test_help_is_available_without_address_or_token(self):
        environment = self.environment.copy()
        environment.pop("BAO_ADDR")
        environment.pop("BAO_TOKEN")
        result = subprocess.run([str(SCRIPT), "--help"], env=environment, stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=15)
        self.assertEqual(result.returncode, 0, result.stderr)
        for option in ("--token-file", "--namespace", "--ttl", "BAO_TOKEN", "BAO_ADDR", "1825d"):
            self.assertIn(option, result.stdout)
        self.assertEqual(self.state()["calls"], [])
        self.assert_secret_safe(result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main(verbosity=2)
