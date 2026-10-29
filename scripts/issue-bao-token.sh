#!/usr/bin/env bash
set +x
set -euo pipefail
umask 077
unset BAO_WRAP_TTL VAULT_WRAP_TTL

usage() {
  printf '%s\n' 'Usage: issue-bao-token.sh [--namespace agent] [--policy admin] [--ttl 1095d] [--token-file PATH]' \
    'Repeat --policy to attach multiple policies. Without --token-file, the token is printed.'
}
fail() { printf 'Error: %s\n' "$1" >&2; exit 2; }
namespace=${BAO_NAMESPACE:-agent}
ttl=1095d
token_file=
policies=()
while (($#)); do
  case "$1" in
    --namespace|--ttl|--policy|--token-file)
      (($# >= 2)) || fail "Missing value for $1"
      case "$1" in
        --namespace) namespace=$2 ;;
        --ttl) ttl=$2 ;;
        --policy) [[ -n $2 ]] || fail 'Policy must not be empty'; policies+=("-policy=$2") ;;
        --token-file) [[ -n $2 ]] || fail 'Token file path must not be empty'; token_file=$2 ;;
      esac
      shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; fail 'Unknown option' ;;
  esac
done
command -v bao >/dev/null || fail 'bao is required'
command -v python3 >/dev/null || fail 'python3 is required'
[[ -n ${BAO_ADDR:-} ]] || fail 'Set BAO_ADDR to your OpenBao server'
if [[ -n $token_file ]]; then
  [[ ! -e $token_file && ! -L $token_file ]] || fail 'Token file already exists; choose a new path'
  [[ -d $(dirname -- "$token_file") && -w $(dirname -- "$token_file") ]] || fail 'Token file directory must exist and be writable'
fi
((${#policies[@]})) || policies=(-policy=admin)
bao token create -namespace="$namespace" -orphan -type=service \
  "${policies[@]}" -ttl="$ttl" -explicit-max-ttl="$ttl" -format=json |
python3 -c '
import json, os, sys
try:
    auth = json.load(sys.stdin)["auth"]
    token, ttl = auth["client_token"], auth["lease_duration"]
    if not isinstance(token, str) or not token or type(ttl) is not int or ttl <= 0:
        raise ValueError()
except (ValueError, KeyError, TypeError):
    sys.exit("Error: Bao did not return a valid token and TTL.")
path, namespace = sys.argv[1:]
if path:
    try:
        fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(fd, "w") as output:
            output.write(token + "\n")
    except OSError:
        sys.exit("Error: Token issued but could not be saved; choose a new writable --token-file path.")
print("WARNING: This credential defaults to a three-year lifetime. Keep it secure. The Bao server has a default maximum TTL, so the TTL returned by the server below is authoritative. To extend it, change the server configuration setting `max_lease_ttl`.")
print("Namespace: " + (namespace or "root"))
print("Token file: " + path if path else "Token: " + token)
print("\033[31mToken TTL: " + str(ttl) + " seconds\033[0m")
' "$token_file" "$namespace"
