#!/usr/bin/env python3
"""Opt-in, retained-fixture OpenBao/MCP regression. Never prints backend bodies."""

from __future__ import annotations

import argparse
import base64
import concurrent.futures
import datetime as dt
import json
import os
from pathlib import Path
import secrets
import shlex
import shutil
import socket
import ssl
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request


ROOT = Path(__file__).resolve().parents[1]
APP = ROOT
OUTPUT = APP / "test-output"
TIMEOUT = 20
LIMIT = 8 * 1024 * 1024
DISCOVERY_POLICY = '''
path "auth/token/lookup-self" { capabilities = ["read"] }
path "sys/capabilities-self" { capabilities = ["update"] }
path "sys/internal/ui/resultant-acl" { capabilities = ["read"] }
path "sys/internal/ui/mounts" { capabilities = ["read"] }
'''
ADMIN_POLICY = 'path "*" { capabilities = ["create", "read", "update", "delete", "list", "sudo", "patch", "scan"] }'
TOOLS = {
    "bao_request", "api_help", "cache_refresh", "vault_create", "kv_read",
    "kv_write", "kv_manage", "ssh_ca", "ssh_roles", "ssh_issue", "pki_ca",
    "pki_roles", "pki_certificates", "pki_config", "transit_keys", "transit_crypto",
    "database_connections", "database_roles", "database_credentials", "totp_keys",
    "totp_code", "cubbyhole", "identity", "auth", "token", "policy", "leases",
    "mounts", "namespaces", "audit", "system", "kubernetes", "ldap", "rabbitmq",
}


class Blocked(Exception):
    """A known prerequisite is absent. Messages are fixed, nonsecret text."""


class Failed(Exception):
    """A check failed. Messages are fixed, nonsecret text."""


class ApiError(Exception):
    def __init__(self, status: int, code: str = "request_failed"):
        self.status = status
        self.code = code
        super().__init__(f"HTTP {status}; {code}")


def expect(condition: bool, message: str) -> None:
    if not condition:
        raise Failed(message)


def private_json(path: Path, value: object) -> None:
    """Atomically persist secrets without ever exposing them to stdout."""
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    os.chmod(path.parent, 0o700)
    temporary = path.with_name(path.name + ".tmp")
    flags = os.O_WRONLY | os.O_CREAT | os.O_TRUNC | getattr(os, "O_NOFOLLOW", 0)
    fd = os.open(temporary, flags, 0o600)
    os.fchmod(fd, 0o600)
    with os.fdopen(fd, "w", encoding="utf-8") as file:
        json.dump(value, file, indent=2, sort_keys=True)
        file.write("\n")
    os.replace(temporary, path)


def dotenv_data(path: Path) -> dict[str, str]:
    """Read simple assignments as data. No shell, expansion, source or eval."""
    if not path.is_file():
        return {}
    values = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        try:
            words = shlex.split(line, comments=True, posix=True)
        except ValueError:
            continue
        if words[:1] == ["export"]:
            words = words[1:]
        if len(words) != 1 or "=" not in words[0]:
            continue
        name, value = words[0].split("=", 1)
        if name in {"BAO_ADDR", "BAO_TOKEN", "BAO_NAMESPACE", "BAO_CACERT", "OPENBAO_URL", "OPENBAO_TOKEN", "OPENBAO_NAMESPACE", "OPENBAO_CA_FILE"}:
            values[name] = value
    return values


def configured_value(values: dict[str, str], *names: str) -> str | None:
    for source in (os.environ, values):
        for name in names:
            if source.get(name):
                return source[name]
    return None


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def decode_response(raw: bytes, content_type: str) -> object:
    if not raw:
        return {}
    if "text/event-stream" in content_type:
        # Streamable HTTP may answer a POST with one or more SSE data frames.
        for event in raw.decode("utf-8").split("\n\n"):
            data = "\n".join(line[5:].lstrip() for line in event.splitlines() if line.startswith("data:"))
            if data:
                value = json.loads(data)
                if isinstance(value, dict) and ("result" in value or "error" in value):
                    return value
        raise ApiError(502, "invalid_mcp_response")
    try:
        return json.loads(raw)
    except (ValueError, UnicodeError):
        raise ApiError(502, "invalid_json_response") from None


class Http:
    def __init__(self, ca: str | None = None):
        context = ssl.create_default_context(cafile=ca or None)
        self.opener = urllib.request.build_opener(NoRedirect(), urllib.request.HTTPSHandler(context=context))

    def request(self, url: str, method: str, headers: dict, data: object = None):
        body = None if data is None else json.dumps(data).encode()
        headers = {**headers, "Accept": "application/json, text/event-stream"}
        if body is not None:
            headers["Content-Type"] = "application/json"
        request = urllib.request.Request(url, data=body, headers=headers, method=method)
        try:
            with self.opener.open(request, timeout=TIMEOUT) as response:
                raw = response.read(LIMIT + 1)
                if len(raw) > LIMIT:
                    raise ApiError(502, "response_limit")
                return response.status, dict(response.headers), decode_response(raw, response.headers.get("Content-Type", ""))
        except urllib.error.HTTPError as error:
            # Backend error bodies can contain credentials or private keys.
            # Discard the body; neither logs nor results retain it.
            status = error.code
            error.close()
            raise ApiError(status) from None
        except (urllib.error.URLError, TimeoutError, OSError):
            raise ApiError(0, "transport_unavailable") from None


class Bao:
    def __init__(self, http: Http, address: str, namespace: str, token: str):
        self.http, self.address, self.namespace, self.token = http, address.rstrip("/"), namespace, token

    def call(self, method: str, path: str, data: object = None):
        headers = {"X-Vault-Token": self.token}
        if self.namespace:
            headers["X-Vault-Namespace"] = self.namespace
        return self.http.request(self.address + "/v1/" + path, method, headers, data)[2]


class Mcp:
    def __init__(self, url: str, token: str | None):
        self.http, self.url, self.token = Http(), url, token
        self.session, self.protocol, self.sequence = None, "2025-03-26", 0

    def rpc(self, method: str, params: object = None, notification: bool = False):
        self.sequence += 1
        payload = {"jsonrpc": "2.0", "method": method}
        if not notification:
            payload["id"] = self.sequence
        if params is not None:
            payload["params"] = params
        headers = {"MCP-Protocol-Version": self.protocol}
        if self.token is not None:
            headers["Authorization"] = "Bearer " + self.token
        if self.session:
            headers["Mcp-Session-Id"] = self.session
        _, response_headers, value = self.http.request(self.url, "POST", headers, payload)
        for key, item in response_headers.items():
            if key.lower() == "mcp-session-id":
                self.session = item
        if notification:
            return value
        if not isinstance(value, dict) or "result" not in value:
            # Do not echo JSON-RPC error data, which is arbitrary backend data.
            raise ApiError(502, "mcp_rpc_error")
        return value["result"]

    def initialize(self):
        result = self.rpc("initialize", {"protocolVersion": self.protocol, "capabilities": {}, "clientInfo": {"name": "bao-mcp-live", "version": "1"}})
        self.protocol = result.get("protocolVersion", self.protocol)
        self.rpc("notifications/initialized", notification=True)
        return self

    def tools(self) -> set[str]:
        result = self.rpc("tools/list", {})
        expect("nextCursor" not in result, "unexpected tool catalog pagination")
        return {tool["name"] for tool in result.get("tools", [])}

    def tool(self, name: str, args: dict, *, denied: bool = False):
        result = self.rpc("tools/call", {"name": name, "arguments": args})
        value = {}
        for item in result.get("content", []):
            if item.get("type") == "text":
                try:
                    value = json.loads(item["text"])
                except ValueError:
                    raise ApiError(502, "invalid_tool_json") from None
                break
        error_status = (value.get("error") or {}).get("status", 0) if isinstance(value, dict) else 0
        if denied:
            expect(result.get("isError") is True and error_status in {401, 403}, "a denied native operation did not reject this caller")
            return value
        if result.get("isError"):
            raise ApiError(error_status, "mcp_tool_failed")
        return value


class Run:
    def __init__(self, args):
        self.args, self.process, self.log_file = args, None, None
        self.clients = {}
        self.results = {}
        self.artifacts = {}
        self.selected = []
        if args.state:
            self.state_file = Path(args.state).expanduser().resolve()
            self.state = json.loads(self.state_file.read_text())
            self.directory = self.state_file.parent
            if not args.only:
                raise Blocked("retained state requires --only; whole-suite replay is disabled")
            if self.state.get("format") != 1:
                raise Blocked("unsupported retained-state format")
            old_results = self.directory / "results.json"
            if old_results.is_file():
                self.results = json.loads(old_results.read_text()).get("checks", {})
            old_artifacts = self.directory / "private-artifacts.json"
            if old_artifacts.is_file():
                self.artifacts = json.loads(old_artifacts.read_text())
        else:
            if args.only:
                raise Blocked("--only requires --state pointing to retained fixtures")
            env = dotenv_data(ROOT / ".env")
            address = configured_value(env, "BAO_ADDR", "OPENBAO_URL")
            token = configured_value(env, "BAO_TOKEN", "OPENBAO_TOKEN")
            namespace = configured_value(env, "BAO_NAMESPACE", "OPENBAO_NAMESPACE") or "agent"
            ca = configured_value(env, "BAO_CACERT", "OPENBAO_CA_FILE")
            if not address or not token:
                raise Blocked("BAO_ADDR and BAO_TOKEN (or their OPENBAO aliases) are required")
            parsed = urllib.parse.urlsplit(address)
            if parsed.scheme not in {"http", "https"} or not parsed.hostname or parsed.username or parsed.password or parsed.query or parsed.fragment:
                raise Blocked("the OpenBao address must be an HTTP(S) base URL without embedded credentials")
            if ca:
                ca_path = Path(ca).expanduser()
                ca = str(ca_path if ca_path.is_absolute() else ROOT / ca_path)
            stamp = dt.datetime.now(dt.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
            identifier = "mcp-live-" + stamp.lower() + "-" + secrets.token_hex(4)
            self.directory = OUTPUT / identifier
            self.directory.mkdir(parents=True, mode=0o700)
            self.state_file = self.directory / "state.json"
            namespace = namespace.strip("/")
            self.state = {"format": 1, "created_at": self.now(), "address": address.rstrip("/"), "source_namespace": namespace, "namespace": "/".join(filter(None, [namespace, identifier])), "child": identifier, "ca": ca, "source_token": token, "tokens": {}, "vault": "live", "transit": "live-transit"}
            self.save_state()
        os.chmod(self.directory, 0o700)
        os.chmod(self.state_file, 0o600)
        self.http = Http(self.state.get("ca"))
        self.parent = Bao(self.http, self.state["address"], self.state["source_namespace"], self.state["source_token"])
        self.native = Bao(self.http, self.state["address"], self.state["namespace"], self.state["source_token"])

    @staticmethod
    def now():
        return dt.datetime.now(dt.timezone.utc).isoformat()

    def save_state(self):
        private_json(self.state_file, self.state)

    def retain(self, name: str, value: object):
        self.artifacts[name] = value
        private_json(self.directory / "private-artifacts.json", self.artifacts)

    def report(self):
        report = {"format": 1, "updated_at": self.now(), "namespace": self.state["namespace"], "remote_resources_retained": True, "selected_checks": self.selected, "backend_paths": [self.state["vault"], "ssh-ca-" + self.state["vault"], "_sys_pki_root", "_sys_pki_int", self.state["transit"], "sys/policies/acl/live-*", "auth/token"], "checks": self.results}
        private_json(self.directory / "results.json", report)

    def check(self, name: str, function):
        start = time.monotonic()
        try:
            details = function() or "verified"
            status = "passed"
        except Blocked as error:
            status, details = "blocked", str(error)
        except Failed as error:
            status, details = "failed", str(error)
        except ApiError as error:
            status, details = "failed", f"request failed (HTTP {error.status}; {error.code}); response body suppressed"
        except Exception as error:
            status, details = "failed", "unexpected " + type(error).__name__ + "; exception details suppressed"
        self.results[name] = {"status": status, "detail": details, "at": self.now(), "duration_seconds": round(time.monotonic() - start, 3)}
        self.report()
        print(f"{status.upper()} {name}: {details}", flush=True)
        return status == "passed"

    def policy(self, name: str, body: str):
        self.native.call("PUT", "sys/policies/acl/" + name, {"policy": DISCOVERY_POLICY + body})

    def token(self, name: str, policy: str, body: str, *, fresh: bool = False) -> str:
        self.policy(policy, body)
        entry = self.state["tokens"].get(name)
        if fresh or not entry or entry.get("revoked"):
            response = self.native.call("POST", "auth/token/create", {"policies": [policy], "no_default_policy": True, "ttl": "24h", "display_name": "bao-mcp-live-" + name})
            auth = response["auth"]
            entry = {"token": auth["client_token"], "accessor": auth.get("accessor"), "policy": policy, "revoked": False}
            self.state["tokens"][name] = entry
            self.save_state()
        return entry["token"]

    def setup(self):
        if not self.state.get("namespace_created"):
            try:
                self.parent.call("PUT", "sys/namespaces/" + self.state["child"], {})
            except ApiError as error:
                if error.status in {401, 403, 404}:
                    raise Blocked("source token cannot create a child namespace; no fallback or existing namespace modification was attempted") from None
                raise
            self.state["namespace_created"] = True
            self.save_state()
        admin = self.token("admin", "live-admin", ADMIN_POLICY)
        self.native = Bao(self.http, self.state["address"], self.state["namespace"], admin)
        return "one isolated child namespace created/reused; fixtures retained"

    def start(self):
        binary = Path(self.args.binary).expanduser().resolve()
        if not binary.is_file() or not os.access(binary, os.X_OK):
            raise Blocked("build bao-mcp/target/debug/bao-mcp before the live run")
        # No Bao/Vault/service token is inherited by the gateway process.
        env = {key: os.environ[key] for key in ["PATH", "TMPDIR", "SYSTEMROOT"] if key in os.environ}
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            port = sock.getsockname()[1]
        env.update({"BAO_ADDR": self.state["address"], "BAO_NAMESPACE": self.state["namespace"], "MCP_LISTEN": f"127.0.0.1:{port}", "CACHE_PATH": str(self.directory / "cache.db"), "RUST_LOG": "info"})
        if self.state.get("ca"):
            env["BAO_CACERT"] = self.state["ca"]
        log_path = self.directory / "server.log"
        fd = os.open(log_path, os.O_WRONLY | os.O_CREAT | os.O_APPEND | getattr(os, "O_NOFOLLOW", 0), 0o600)
        os.fchmod(fd, 0o600)
        self.log_file = os.fdopen(fd, "ab", buffering=0)
        self.process = subprocess.Popen([str(binary)], cwd=APP, env=env, stdin=subprocess.DEVNULL, stdout=self.log_file, stderr=subprocess.STDOUT)
        self.url = f"http://127.0.0.1:{port}/mcp"
        deadline = time.monotonic() + 12
        while time.monotonic() < deadline:
            if self.process.poll() is not None:
                raise Blocked("gateway exited during startup; private server.log retained")
            try:
                Http().request(f"http://127.0.0.1:{port}/healthz", "GET", {})
                return "gateway started without a service token environment variable"
            except ApiError:
                time.sleep(0.1)
        raise Blocked("gateway health endpoint did not become ready within 12 seconds")

    def client(self, name: str = "admin") -> Mcp:
        if name not in self.clients:
            token = self.state["tokens"].get(name, {}).get("token")
            if not token:
                raise Blocked("required fixture token is unavailable")
            self.clients[name] = Mcp(self.url, token).initialize()
        return self.clients[name]

    def require_bundle(self):
        if not self.state.get("bundle_ready"):
            raise Blocked("vault.bundle must pass before this check")

    def seed_scopes(self):
        self.require_bundle()
        mount = self.state["vault"]
        for name in ["a", "b"]:
            value = self.state.setdefault("synthetic_values", {}).setdefault(name, secrets.token_urlsafe(24))
            self.native.call("PUT", f"{mount}/data/scope-{name}", {"data": {"value": value}})
            self.token(name, "live-scope-" + name, f'path "{mount}/data/scope-{name}" {{ capabilities = ["read"] }}')
        self.save_state()

    def anonymous(self):
        for token in [None, "synthetic-invalid-" + secrets.token_hex(8)]:
            try:
                Mcp(self.url, token).initialize()
            except ApiError as error:
                expect(error.status in {401, 403}, "anonymous/invalid-token rejection used an unexpected status")
            else:
                raise Failed("anonymous or invalid-token MCP request was accepted")
        return "anonymous and invalid bearer requests rejected"

    def catalog(self):
        tools = self.client().tools()
        expect(tools <= TOOLS, "discovery advertised a tool outside the native catalog")
        essential = {"bao_request", "api_help", "cache_refresh", "vault_create", "mounts", "policy", "token", "namespaces", "system"}
        expect(essential <= tools, "namespace admin discovery omitted native management tools")
        return f"{len(tools)} permitted tools discovered from a {len(TOOLS)}-tool catalog; unmounted engines are not presumed available"

    def bundle(self):
        vault = self.state["vault"]
        client = self.client()
        response = client.tool("vault_create", {"vault_id": vault, "description": "retained synthetic live fixture"})
        expect(response.get("complete") is True, "vault allocation returned a partial result")
        expected = {vault: "kv", "ssh-ca-" + vault: "ssh", "_sys_pki_root": "pki", "_sys_pki_int": "pki"}
        before = {}
        for mount, kind in expected.items():
            value = self.native.call("GET", "sys/mounts/" + mount)["data"]
            expect(value.get("type") == kind, "vault allocation enabled the wrong engine type")
            if kind == "kv":
                expect(value.get("options", {}).get("version") == "2", "vault allocation did not enable KV v2")
            before[mount] = value
        client.tool("mounts", {"operation": "write", "path": "mounts/" + vault + "/tune", "data": {"description": "retained user setting", "default_lease_ttl": "900s"}})
        before = {mount: self.native.call("GET", "sys/mounts/" + mount)["data"] for mount in expected}
        again = client.tool("vault_create", {"vault_id": vault, "description": "must not replace existing settings"})
        expect(again.get("complete") is True, "idempotent allocation failed")
        for mount in expected:
            after = self.native.call("GET", "sys/mounts/" + mount)["data"]
            for field in ["type", "description", "options", "config", "uuid"]:
                expect(before[mount].get(field) == after.get(field), "idempotent allocation changed an existing mount setting")
        self.state["bundle_ready"] = True
        self.save_state()
        return "KV/SSH/shared PKI mounts allocated; second allocation preserved native settings"

    def discovery(self):
        self.seed_scopes()
        client = self.client("a")
        expect("kv_read" in client.tools(), "an exact KV-only ACL grant did not expose the KV family")
        value = client.tool("kv_read", {"mount": self.state["vault"], "path": "scope-a"})
        expect(value["data"]["data"]["value"] == self.state["synthetic_values"]["a"], "exact-path KV read returned the wrong synthetic value")
        client.tool("kv_read", {"mount": self.state["vault"], "path": "scope-b"}, denied=True)
        return "exact KV data-path grant discovered and enforced without metadata/list grants"

    def isolation(self):
        self.seed_scopes()
        clients = {name: self.client(name) for name in ["a", "b"]}
        mount = self.state["vault"]

        def own(name):
            other = "b" if name == "a" else "a"
            value = clients[name].tool("kv_read", {"mount": mount, "path": "scope-" + name})
            expect(value["data"]["data"]["value"] == self.state["synthetic_values"][name], "caller received another caller's value")
            clients[name].tool("kv_read", {"mount": mount, "path": "scope-" + other}, denied=True)

        with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
            futures = [pool.submit(own, name) for name in ["a", "b"]]
            for future in futures:
                future.result()
        # A native auth/token operation must also resolve the request's own token.
        for name in ["b", "a"]:
            value = clients[name].tool("token", {"operation": "read", "path": "lookup-self"})
            expect(value["data"]["id"] == self.state["tokens"][name]["token"], "token lookup used a different caller's credentials")
        return "concurrent clients read only their granted key; native identity stayed request scoped"

    def kv(self):
        self.require_bundle()
        client, mount = self.client(), self.state["vault"]
        key = "versions-" + secrets.token_hex(4)
        self.state.setdefault("kv_paths", []).append(mount + "/data/" + key)
        self.save_state()
        first = client.tool("kv_write", {"mount": mount, "path": key, "data": {"data": {"value": "synthetic-v1"}, "options": {"cas": 0}}})
        expect(first["data"]["version"] == 1, "initial KV CAS write did not create version one")
        client.tool("kv_write", {"mount": mount, "path": key, "data": {"data": {"value": "synthetic-v2"}, "options": {"cas": 1}}})
        historic = client.tool("kv_read", {"mount": mount, "path": key, "version": 1})
        expect(historic["data"]["data"]["value"] == "synthetic-v1", "historical KV version was not forwarded")
        try:
            client.tool("kv_write", {"mount": mount, "path": key, "data": {"data": {"value": "must-not-be-written"}, "options": {"cas": 1}}})
        except ApiError as error:
            expect(error.status == 400, "stale KV CAS used an unexpected status")
        else:
            raise Failed("stale KV CAS was accepted")
        client.tool("kv_manage", {"mount": mount, "operation": "write", "path": "delete/" + key, "data": {"versions": [2]}})
        metadata = self.native.call("GET", mount + "/metadata/" + key)["data"]
        expect(bool(metadata["versions"]["2"]["deletion_time"]), "native KV version deletion was not recorded")
        client.tool("kv_manage", {"mount": mount, "operation": "write", "path": "undelete/" + key, "data": {"versions": [2]}})
        native = self.native.call("GET", mount + "/data/" + key)
        expect(native["data"]["data"]["value"] == "synthetic-v2", "native KV data path did not retain version two")
        self.retain("kv_last", {"path": mount + "/data/" + key, "response": native})
        return "native data/metadata/delete/undelete paths, historical reads and CAS verified; versions retained"

    def has_issuers(self, mount):
        try:
            value = self.native.call("GET", mount + "/issuers?list=true")
            return bool(value.get("data", {}).get("keys", []))
        except ApiError as error:
            if error.status == 404:
                return False
            raise

    def ssh(self):
        self.require_bundle()
        executable = shutil.which("ssh-keygen")
        if not executable:
            raise Blocked("ssh-keygen is unavailable; no synthetic SSH key was generated")
        key = self.directory / "synthetic-ssh-key"
        if not key.is_file():
            completed = subprocess.run([executable, "-q", "-t", "ed25519", "-N", "", "-C", "bao-mcp-live", "-f", str(key)], stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=15)
            expect(completed.returncode == 0, "synthetic SSH key generation failed")
        os.chmod(key, 0o600)
        mount, client = "ssh-ca-" + self.state["vault"], self.client()
        if not self.has_issuers(mount):
            client.tool("ssh_ca", {"mount": mount, "operation": "write", "path": "config/ca", "data": {"generate_signing_key": True}})
        client.tool("ssh_roles", {"mount": mount, "operation": "write", "path": "roles/live", "data": {"key_type": "ca", "allow_user_certificates": True, "allowed_users": "live-test", "default_user": "live-test", "ttl": "15m", "max_ttl": "1h"}})
        signed = client.tool("ssh_issue", {"mount": mount, "operation": "write", "path": "sign/live", "data": {"public_key": key.with_suffix(".pub").read_text(), "valid_principals": "live-test", "cert_type": "user", "ttl": "15m"}})
        expect("-cert-v01@openssh.com" in signed["data"]["signed_key"], "SSH signing did not return an OpenSSH certificate")
        self.retain("ssh_signed_certificate", signed)
        certificate = self.directory / "synthetic-ssh-key-cert.pub"
        certificate.write_text(signed["data"]["signed_key"])
        os.chmod(certificate, 0o600)
        return "native SSH CA generation, role configuration and synthetic public-key signing verified"

    def pki(self):
        self.require_bundle()
        client = self.client()
        if not self.has_issuers("_sys_pki_root"):
            root = client.tool("pki_ca", {"mount": "_sys_pki_root", "operation": "write", "path": "root/generate/internal", "data": {"common_name": "bao-mcp synthetic live root", "ttl": "72h", "key_type": "ec", "key_bits": 256}})
            self.retain("pki_root", root)
        if not self.has_issuers("_sys_pki_int"):
            csr = client.tool("pki_ca", {"mount": "_sys_pki_int", "operation": "write", "path": "intermediate/generate/internal", "data": {"common_name": "bao-mcp synthetic live intermediate", "key_type": "ec", "key_bits": 256}})
            self.retain("pki_intermediate_csr", csr)
            signed = client.tool("pki_ca", {"mount": "_sys_pki_root", "operation": "write", "path": "root/sign-intermediate", "data": {"csr": csr["data"]["csr"], "format": "pem_bundle", "ttl": "48h"}})
            self.retain("pki_intermediate_signed", signed)
            client.tool("pki_ca", {"mount": "_sys_pki_int", "operation": "write", "path": "intermediate/set-signed", "data": {"certificate": signed["data"]["certificate"]}})
        client.tool("pki_roles", {"mount": "_sys_pki_int", "operation": "write", "path": "roles/live", "data": {"allowed_domains": ["live.invalid"], "allow_subdomains": True, "max_ttl": "1h", "key_type": "ec", "key_bits": 256}})
        issued = client.tool("pki_certificates", {"mount": "_sys_pki_int", "operation": "write", "path": "issue/live", "data": {"common_name": "app.live.invalid", "ttl": "15m"}})
        expect("BEGIN CERTIFICATE" in issued["data"]["certificate"] and "PRIVATE KEY" in issued["data"]["private_key"], "PKI issuance did not return a certificate/private key")
        self.retain("pki_leaf", issued)
        return "root/internal intermediate signing and synthetic leaf issuance verified; keys/certificates retained privately"

    def transit(self):
        self.require_bundle()
        mount, client = self.state["transit"], self.client()
        if not self.state.get("transit_ready"):
            native_mounts = self.native.call("GET", "sys/mounts")["data"]
            if mount + "/" not in native_mounts:
                client.tool("mounts", {"operation": "write", "path": "mounts/" + mount, "data": {"type": "transit", "description": "retained synthetic live fixture"}})
            self.state["transit_ready"] = True
            self.save_state()
        for name, kind in [("enc", "aes256-gcm96"), ("sign", "ed25519")]:
            client.tool("transit_keys", {"mount": mount, "operation": "write", "path": "keys/" + name, "data": {"type": kind}})
        plaintext = base64.b64encode(secrets.token_bytes(24)).decode()
        encrypted = client.tool("transit_crypto", {"mount": mount, "operation": "write", "path": "encrypt/enc", "data": {"plaintext": plaintext}})
        decrypted = client.tool("transit_crypto", {"mount": mount, "operation": "write", "path": "decrypt/enc", "data": {"ciphertext": encrypted["data"]["ciphertext"]}})
        expect(decrypted["data"]["plaintext"] == plaintext, "Transit decrypt did not recover synthetic plaintext")
        signed = client.tool("transit_crypto", {"mount": mount, "operation": "write", "path": "sign/sign", "data": {"input": plaintext}})
        verified = client.tool("transit_crypto", {"mount": mount, "operation": "write", "path": "verify/sign", "data": {"input": plaintext, "signature": signed["data"]["signature"]}})
        expect(verified["data"]["valid"] is True, "Transit signature verification failed")
        self.token("encrypt-only", "live-encrypt-only", f'path "{mount}/encrypt/enc" {{ capabilities = ["update"] }}')
        limited = self.client("encrypt-only")
        limited.tool("transit_crypto", {"mount": mount, "operation": "write", "path": "encrypt/enc", "data": {"plaintext": plaintext}})
        limited.tool("transit_crypto", {"mount": mount, "operation": "write", "path": "decrypt/enc", "data": {"ciphertext": encrypted["data"]["ciphertext"]}}, denied=True)
        self.retain("transit_roundtrip", {"plaintext": plaintext, "encrypted": encrypted, "signed": signed})
        return "Transit key creation/encrypt/decrypt/sign/verify and native decrypt denial verified"

    def policies_tokens(self):
        self.require_bundle()
        client = self.client()
        policy = DISCOVERY_POLICY + f'path "{self.state["vault"]}/data/policy-token" {{ capabilities = ["read"] }}'
        client.tool("policy", {"operation": "write", "path": "policies/acl/live-native-policy", "data": {"policy": policy}})
        read = client.tool("policy", {"operation": "read", "path": "policies/acl/live-native-policy"})
        expect("policy-token" in read["data"]["policy"], "native policy write/read lost the policy body")
        created = client.tool("token", {"operation": "write", "path": "create", "data": {"policies": ["live-native-policy"], "no_default_policy": True, "ttl": "1h", "display_name": "bao-mcp-live-native"}})
        auth = created["auth"]
        self.state["tokens"]["native-created"] = {"token": auth["client_token"], "accessor": auth.get("accessor"), "policy": "live-native-policy", "revoked": False}
        self.save_state()
        self.clients.pop("native-created", None)
        lookup = self.client("native-created").tool("token", {"operation": "read", "path": "lookup-self"})
        expect(lookup["data"]["id"] == auth["client_token"] and "live-native-policy" in lookup["data"]["policies"], "native token creation/lookup did not preserve caller identity")
        return "native policy write/read and limited token create/lookup verified; issued token retained privately"

    def refresh(self):
        self.require_bundle()
        if not self.state.get("transit_ready"):
            raise Blocked("transit.roundtrip_and_denial must pass before this check")
        mount, transit = self.state["vault"], self.state["transit"]
        base = f'path "{mount}/data/scope-a" {{ capabilities = ["read"] }}'
        self.token("mutable", "live-mutable", base, fresh=True)
        self.clients.pop("mutable", None)
        client = self.client("mutable")
        client.tools()
        added_path = transit + "/encrypt/enc"
        self.policy("live-mutable", base + f'\npath "{added_path}" {{ capabilities = ["update"] }}')
        # Execution must already honor Bao's live ACL, before discovery refresh.
        client.tool("transit_crypto", {"mount": transit, "operation": "write", "path": "encrypt/enc", "data": {"plaintext": base64.b64encode(b"synthetic refresh probe").decode()}})
        time.sleep(5.1)  # Explicit refresh is intentionally limited to one per five seconds.
        refreshed = client.tool("cache_refresh", {})
        expect("transit_crypto" in refreshed.get("tools", []), "cache refresh did not expose the new Transit grant")
        capabilities = refreshed.get("capabilities", {}).get("paths", {})
        expect("update" in capabilities.get(added_path, []), "cache refresh did not retrieve the new concrete path capability")
        # A second actor's cached metadata must keep its own exact scope.
        a = self.client("a") if "a" in self.state["tokens"] else None
        if a:
            a.tool("transit_crypto", {"mount": transit, "operation": "write", "path": "encrypt/enc", "data": {"plaintext": base64.b64encode(b"denied").decode()}}, denied=True)
        self.native.call("POST", "auth/token/revoke", {"token": self.state["tokens"]["mutable"]["token"]})
        self.state["tokens"]["mutable"]["revoked"] = True
        self.save_state()
        try:
            client.tool("kv_read", {"mount": mount, "path": "scope-a"}, denied=True)
        except ApiError as error:
            expect(error.status in {401, 403}, "revoked token rejection used an unexpected status")
        return "new policy granted execution before refresh; caller-only refresh updated concrete scopes; revoked token rejected"

    def persistence(self):
        tokens = [self.state["source_token"]] + [entry["token"] for entry in self.state["tokens"].values()]
        probes = [token.encode() for token in tokens if token]
        probes += [value.encode() for value in self.state.get("synthetic_values", {}).values()]
        files = [self.directory / "server.log"] + list(self.directory.glob("cache.db*"))
        expect(any(path.name == "cache.db" for path in files), "gateway cache database was not created")
        for path in files:
            if path.is_file():
                raw = path.read_bytes()
                expect(not any(probe in raw for probe in probes), "plaintext caller token or synthetic KV value found in gateway cache/log")
        for path in self.directory.iterdir():
            if path.is_file():
                os.chmod(path, 0o600)
        return "known source/caller tokens and synthetic KV values absent from cache database/sidecars and gateway log"

    def external(self):
        raise Blocked("database/cloud/directory/broker/Kubernetes integrations require external fixtures; native adapter routing is covered by offline tests")

    def stop(self):
        if self.process and self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=5)
        if self.log_file:
            self.log_file.close()
        # Remote namespaces, policies, tokens, keys, versions and certificates remain.
        if hasattr(self, "directory"):
            for path in self.directory.iterdir():
                if path.is_file():
                    os.chmod(path, 0o600)


CHECKS = [
    ("auth.reject_anonymous_and_wrong_token", "anonymous"),
    ("catalog.native_families", "catalog"),
    ("vault.bundle", "bundle"),
    ("discovery.exact_kv_path", "discovery"),
    ("isolation.per_caller", "isolation"),
    ("kv.native_version_lifecycle", "kv"),
    ("ssh.ca_role_sign", "ssh"),
    ("pki.root_intermediate_issue", "pki"),
    ("transit.roundtrip_and_denial", "transit"),
    ("native.policy_token", "policies_tokens"),
    ("cache.refresh_scope_and_revocation", "refresh"),
    ("cache.no_plaintext_credentials", "persistence"),
    ("external_engines.requires_fixtures", "external"),
]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--live", action="store_true", help="explicitly authorize this retained-fixture live run")
    parser.add_argument("--state", help="private state.json from the original run; requires --only")
    parser.add_argument("--only", action="append", default=[], help="check/family name; repeat or comma-separate to rerun only selected checks")
    parser.add_argument("--binary", default=str(APP / "target/debug/bao-mcp"), help="already-built gateway executable")
    parser.add_argument("--list", action="store_true", help="list check names without reading credentials or contacting services")
    args = parser.parse_args()
    if args.list:
        print("\n".join(name for name, _ in CHECKS))
        return 0
    if not args.live:
        parser.error("live execution is opt-in; pass --live (remote resources are retained)")
    selectors = [item.strip() for value in args.only for item in value.split(",") if item.strip()]
    selected = [(name, method) for name, method in CHECKS if not selectors or any(name == item or name.startswith(item + ".") for item in selectors)]
    if not selected or any(not any(name == item or name.startswith(item + ".") for name, _ in CHECKS) for item in selectors):
        parser.error("--only contains an unknown check/family; use --list")
    run = None
    try:
        run = Run(args)
        run.selected = [name for name, _ in selected]
        print("Private retained output: " + str(run.directory), flush=True)
        if not run.check("setup.namespace", run.setup):
            for name, _ in selected:
                run.check(name, lambda: (_ for _ in ()).throw(Blocked("child namespace setup did not pass")))
            return 2
        if not run.check("setup.gateway", run.start):
            for name, _ in selected:
                run.check(name, lambda: (_ for _ in ()).throw(Blocked("gateway startup did not pass")))
            return 2
        for name, method in selected:
            run.check(name, getattr(run, method))
        statuses = [run.results[name]["status"] for name, _ in selected]
        # Expected external-fixture gaps are visible but do not mask actual failures.
        if "failed" in statuses:
            return 1
        return 2 if any(run.results[name]["status"] == "blocked" and not name.startswith("external_engines.") for name, _ in selected) else 0
    except Blocked as error:
        print("BLOCKED: " + str(error), file=sys.stderr)
        return 2
    except KeyboardInterrupt:
        print("INTERRUPTED: remote fixtures and private output retained", file=sys.stderr)
        return 130
    except Exception as error:
        print("FAILED: unexpected " + type(error).__name__ + "; details suppressed to protect credentials", file=sys.stderr)
        return 1
    finally:
        if run:
            run.stop()


if __name__ == "__main__":
    sys.exit(main())
