#!/usr/bin/env bash
set +x
set -euo pipefail
umask 077
unset BAO_WRAP_TTL VAULT_WRAP_TTL

command -v python3 >/dev/null 2>&1 || { printf 'Error: python3 is required.\n' >&2; exit 1; }
# Keep the original input available for a hidden prompt while Python reads this program.
exec 3<&0
export BAO_SETUP_SCRIPT_DIR
BAO_SETUP_SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
python3 - "$@" <<'PY'
import argparse
import decimal
import json
import math
import os
import re
import secrets
import shutil
import stat
import subprocess
import sys
import termios
from pathlib import Path


class SetupError(Exception):
    pass


class Parser(argparse.ArgumentParser):
    def error(self, message):
        raise SetupError("Invalid options. Run with --help for usage")


def duration(value):
    parts = list(re.finditer(r"(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)[smhd]", value))
    if not parts or "".join(part.group() for part in parts) != value:
        raise SetupError("TTL must be a positive duration using s, m, h, or d; maximum 1825d")
    units = {"s": 1, "m": 60, "h": 3600, "d": 86400}
    try:
        seconds = sum(decimal.Decimal(part.group()[:-1]) * units[part.group()[-1]] for part in parts)
    except decimal.DecimalException:
        raise SetupError("TTL must be a positive duration using s, m, h, or d; maximum 1825d")
    if not seconds.is_finite() or seconds < 1 or seconds > 1825 * 86400:
        raise SetupError("TTL must be at least 1s and no more than 1825d")
    return seconds


def json_object(raw, message):
    try:
        value = json.loads(raw)
    except (ValueError, UnicodeError):
        raise SetupError(message)
    if not isinstance(value, dict):
        raise SetupError(message)
    return value


def positive_integer(value):
    return type(value) is int and value > 0


def policies_valid(value):
    return isinstance(value, list) and all(isinstance(item, str) for item in value) and set(value) == {"admin", "default"}


def setup_token():
    token = os.environ.get("BAO_TOKEN", "")
    if not token:
        if not os.isatty(3):
            raise SetupError("Set BAO_TOKEN to a setup/admin token, or run interactively to enter it")
        settings = termios.tcgetattr(3)
        hidden = settings.copy()
        hidden[3] &= ~termios.ECHO
        try:
            termios.tcsetattr(3, termios.TCSAFLUSH, hidden)
            sys.stderr.write("OpenBao setup/admin token: ")
            sys.stderr.flush()
            with os.fdopen(os.dup(3), "r") as terminal:
                token = terminal.readline(1048577).rstrip("\r\n")
        finally:
            termios.tcsetattr(3, termios.TCSAFLUSH, settings)
            sys.stderr.write("\n")
            sys.stderr.flush()
    if not token or len(token) > 1048576 or any(char.isspace() or ord(char) < 32 for char in token):
        raise SetupError("A nonempty setup token is required")
    return token


class Bao:
    def __init__(self, token):
        self.environment = os.environ.copy()
        self.environment["BAO_TOKEN"] = token
        for key in ("BAO_NAMESPACE", "VAULT_NAMESPACE", "BAO_WRAP_TTL", "VAULT_WRAP_TTL", "BAO_SETUP_SCRIPT_DIR"):
            self.environment.pop(key, None)

    @staticmethod
    def context(phase, namespace, path):
        return phase + " failed (namespace: " + (namespace or "root") + ", path: " + path + ")"

    @staticmethod
    def http_status(result):
        # CLI HTTP diagnostics use their own line; error-body lookalikes are not statuses.
        diagnostic = result.stderr + b"\n" + result.stdout
        match = re.search(rb"(?m)^Code: ([1-5][0-9]{2})\.", diagnostic)
        return int(match.group(1)) if match else None

    def failure(self, result, phase, namespace, path):
        status = self.http_status(result)
        diagnostic = (result.stderr + b"\n" + result.stdout).lower()
        if status in (401, 403):
            category = "authentication failed" if status == 401 else "permission denied"
        elif status == 503:
            category = "server sealed" if b"sealed" in diagnostic else "server unavailable"
        elif status is not None:
            category = "server error" if status >= 500 else "request failed"
        elif any(marker in diagnostic for marker in (b"x509:", b"tls:", b"certificate verify failed", b"certificate validation failed", b"tls handshake")):
            category = "TLS error"
        elif any(marker in diagnostic for marker in (b"no such host", b"temporary failure in name resolution", b"server misbehaving")):
            category = "DNS resolution failed"
        elif any(marker in diagnostic for marker in (b"timeout", b"timed out", b"context deadline exceeded")):
            category = "connection timed out"
        elif any(marker in diagnostic for marker in (b"connection refused", b"connection reset", b"connection failed", b"failed to connect", b"network is unreachable", b"no route to host")):
            category = "connection failed"
        elif any(marker in diagnostic for marker in (b"server is sealed", b"vault is sealed", b"bao is sealed")):
            category = "server sealed"
        elif b"service unavailable" in diagnostic:
            category = "server unavailable"
        else:
            category = "unclassified CLI error"
        code = "HTTP " + str(status) if status is not None else "CLI exit " + str(result.returncode)
        # Only fixed context, a numeric status, and allowlisted categories reach the user.
        return SetupError(self.context(phase, namespace, path) + ": " + code + ", " + category)

    def run(self, arguments, namespace, token=None, *, phase, path):
        environment = self.environment.copy()
        if token is not None:
            environment["BAO_TOKEN"] = token
        depth = 2 if arguments[0] in {"token", "namespace", "policy"} else 1
        try:
            return subprocess.run(
                ["bao", *arguments[:depth], "-namespace=" + namespace, *arguments[depth:]],
                stdin=subprocess.DEVNULL,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                env=environment,
                timeout=60,
                check=False,
            )
        except subprocess.TimeoutExpired:
            raise SetupError(self.context(phase, namespace, path) + ": CLI timed out after 60 seconds")
        except OSError:
            raise SetupError(self.context(phase, namespace, path) + ": CLI process launch error")

    def read(self, path, namespace, phase):
        result = self.run(["read", "-format=json", path], namespace, phase=phase, path=path)
        if result.returncode == 0:
            value = json_object(result.stdout, "OpenBao returned an invalid read response")
            if not isinstance(value.get("data"), dict):
                raise SetupError("OpenBao returned an invalid read response")
            return value["data"]
        # Only a confirmed CLI absence or its first HTTP status establishes absence.
        status = self.http_status(result)
        missing = status == 404
        if status is None:
            diagnostic = result.stdout + b"\n" + result.stderr
            missing = diagnostic.strip() == ("No value found at " + path).encode()
        if result.returncode == 2 and missing:
            return None
        raise self.failure(result, phase, namespace, path)


def policy_matches(data, expected):
    return isinstance(data.get("policy"), str) and data["policy"].rstrip("\n") == expected.rstrip("\n")


def ensure_namespace(data, namespace):
    if not isinstance(data, dict) or data.get("path") not in (namespace, namespace + "/"):
        raise SetupError("OpenBao returned an invalid namespace response")


def publish(directory, filename, token):
    temporary = ".bao-setup-" + secrets.token_hex(16)
    descriptor = None
    created = False
    try:
        descriptor = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600, dir_fd=directory)
        created = True
        os.fchmod(descriptor, 0o600)
        with os.fdopen(descriptor, "w") as output:
            descriptor = None
            output.write(token + "\n")
            output.flush()
            os.fsync(output.fileno())
        # Linking publishes a complete private file and atomically refuses any existing leaf.
        os.link(temporary, filename, src_dir_fd=directory, dst_dir_fd=directory, follow_symlinks=False)
    except OSError:
        raise SetupError("A token was issued but could not be saved. Choose a new writable token-file path and retry")
    finally:
        if descriptor is not None:
            os.close(descriptor)
        if created:
            os.unlink(temporary, dir_fd=directory)


def main():
    parser = Parser(
        prog="scripts/setup-bao-environment.sh",
        description="Prepare an agent namespace and save a finite-lived orphan admin service token.",
        epilog="Requires BAO_ADDR, bao, and Python 3. Uses BAO_TOKEN or a hidden interactive setup/admin-token prompt; native API permissions authorize each operation. Existing namespace metadata and matching policy are preserved; a different admin policy stops setup. Token files are never overwritten.",
    )
    parser.add_argument("--token-file", required=True, help="New token file in an existing writable directory (mode 0600)")
    parser.add_argument("--namespace", default="agent", help="Single namespace segment (default: agent)")
    parser.add_argument("--ttl", default="24h", help="Duration using s/m/h/d, including 1h30m (default: 24h; range: 1s to 1825d)")
    arguments = parser.parse_args()
    namespace = arguments.namespace
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]*", namespace) or namespace.lower() in {"root", "sys", "audit", "auth", "cubbyhole", "identity"}:
        raise SetupError("Namespace must be one nonreserved segment using letters, digits, underscores, dots, or hyphens")
    seconds = duration(arguments.ttl)
    if not os.environ.get("BAO_ADDR", "").strip():
        raise SetupError("Set BAO_ADDR to the initialized, unsealed OpenBao server address")
    if shutil.which("bao") is None:
        raise SetupError("bao is required")
    if not arguments.token_file or any(ord(char) < 32 for char in arguments.token_file):
        raise SetupError("Provide a valid token-file path")
    destination = Path(os.path.abspath(arguments.token_file))
    try:
        directory = os.open(destination.parent, os.O_RDONLY | os.O_DIRECTORY)
    except OSError:
        raise SetupError("The token-file parent must be an existing writable directory")
    try:
        if not os.fstat(directory).st_mode & (stat.S_IWUSR | stat.S_IWGRP | stat.S_IWOTH) or not os.access(destination.parent, os.W_OK | os.X_OK):
            raise SetupError("The token-file parent must be an existing writable directory")
        try:
            os.stat(destination.name, dir_fd=directory, follow_symlinks=False)
        except FileNotFoundError:
            pass
        else:
            raise SetupError("The token file already exists. Choose a new token-file path")
        try:
            expected = (Path(os.environ["BAO_SETUP_SCRIPT_DIR"]) / "bao" / "policies" / "admin.hcl").read_text()
        except (OSError, UnicodeError):
            raise SetupError("The bundled admin policy could not be read")
        bao = Bao(setup_token())
        existing = bao.read("sys/namespaces/" + namespace, "", "Namespace read")
        if existing is None:
            result = bao.run(["namespace", "create", "-format=json", namespace], "", phase="Namespace creation", path="sys/namespaces/" + namespace)
            if result.returncode != 0:
                raise bao.failure(result, "Namespace creation", "", "sys/namespaces/" + namespace)
            existing = json_object(result.stdout, "OpenBao returned an invalid namespace response")
        ensure_namespace(existing, namespace)
        existing_policy = bao.read("sys/policies/acl/admin", namespace, "Admin policy read")
        if existing_policy is None:
            policy_path = str(Path(os.environ["BAO_SETUP_SCRIPT_DIR"]) / "bao" / "policies" / "admin.hcl")
            result = bao.run(["policy", "write", "admin", policy_path], namespace, phase="Admin policy installation", path="sys/policies/acl/admin")
            if result.returncode != 0:
                raise bao.failure(result, "Admin policy installation", namespace, "sys/policies/acl/admin")
            existing_policy = bao.read("sys/policies/acl/admin", namespace, "Admin policy read")
        if existing_policy is None or not policy_matches(existing_policy, expected):
            raise SetupError("The namespace admin policy differs from the bundled policy. Review it before retrying")
        result = bao.run([
            "token", "create", "-policy=admin", "-orphan", "-type=service",
            "-ttl=" + arguments.ttl, "-explicit-max-ttl=" + arguments.ttl,
            "-display-name=bao-mcp", "-format=json",
        ], namespace, phase="Token issuance", path="auth/token/create")
        if result.returncode != 0:
            raise bao.failure(result, "Token issuance", namespace, "auth/token/create")
        auth = json_object(result.stdout, "OpenBao returned an invalid token response").get("auth")
        if not isinstance(auth, dict):
            raise SetupError("OpenBao returned an invalid token response")
        token = auth.get("client_token")
        ttl = auth.get("lease_duration")
        if not isinstance(token, str) or not token or any(char.isspace() or ord(char) < 32 for char in token) or ("token_type" in auth and auth["token_type"] != "service") or auth.get("orphan") is not True or not positive_integer(ttl) or ttl > math.ceil(seconds) or not policies_valid(auth.get("policies")):
            raise SetupError("The issued token did not match the requested service, orphan, TTL, or policies")
        if "token_policies" in auth and not policies_valid(auth["token_policies"]):
            raise SetupError("The issued token did not match the requested policies")
        # The Bao CLI drops token_type from create output; lookup confirms the actual type.
        verified = bao.run(["token", "lookup", "-format=json"], namespace, token=token, phase="Issued-token verification", path="auth/token/lookup-self")
        if verified.returncode != 0:
            raise bao.failure(verified, "Issued-token verification", namespace, "auth/token/lookup-self")
        data = json_object(verified.stdout, "OpenBao returned an invalid issued-token lookup").get("data")
        if not isinstance(data, dict) or data.get("id") != token or data.get("type") != "service" or data.get("orphan") is not True or not policies_valid(data.get("policies")) or not positive_integer(data.get("ttl")) or data["ttl"] > ttl or not positive_integer(data.get("explicit_max_ttl")) or data["explicit_max_ttl"] > math.ceil(seconds) or data.get("namespace_path") != namespace + "/":
            raise SetupError("Issued-token verification did not confirm the namespace, service, orphan, TTL, and policies")
        publish(directory, destination.name, token)
        print("Namespace: " + namespace)
        print("Policy: admin")
        print("Token file: " + str(destination))
        print("Token TTL: " + str(ttl) + " seconds")
    finally:
        os.close(directory)


try:
    main()
except SetupError as error:
    sys.stderr.write("Error: " + str(error) + ".\n")
    sys.exit(1)
except (OSError, ValueError, KeyError, TypeError, KeyboardInterrupt):
    sys.stderr.write("Error: Setup could not finish. Check local permissions and server state; completed namespace and policy changes are retained.\n")
    sys.exit(1)
PY
