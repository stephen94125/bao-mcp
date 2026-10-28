#!/usr/bin/env bash
set -euo pipefail
set +x
umask 077

script_dir=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
command -v bao >/dev/null
read -r -p 'Namespace (leave blank for root): ' namespace
case "$namespace" in
  /*|*/|*//*|*..*|.|*/./*|./*|*/.|*[!A-Za-z0-9_./-]*) printf 'Invalid namespace path.\n' >&2; exit 1 ;;
esac
export BAO_NAMESPACE="$namespace"
export BAO_ADDR="${BAO_ADDR:-https://secret.r5vj.xyz}"

read -r -p 'Administrator username: ' username
[[ "$username" =~ ^[A-Za-z0-9][A-Za-z0-9_.-]*$ ]] || { printf 'Invalid username.\n' >&2; exit 1; }
bao read -field=policy sys/policies/acl/admin | cmp -s "$script_dir/policies/admin.hcl" - || {
  printf 'Run apply-policies.sh admin policies/admin.hcl in this namespace first.\n' >&2
  exit 1
}

auth_type=$(bao auth list -format=table | awk '$1 == "userpass/" { print $2 }')
case "$auth_type" in
  '') bao auth enable userpass ;;
  userpass) ;;
  *) printf 'userpass/ is already used by another auth method.\n' >&2; exit 1 ;;
esac
if account=$(bao read "auth/userpass/users/$username" 2>&1); then
  printf 'The account already exists; refusing to overwrite.\n' >&2
  exit 1
else
  case "$account" in
    *'No value found at'*|*'Code: 404'*) ;;
    *) printf 'Unable to determine account status. Check your login and permissions.\n' >&2; exit 1 ;;
  esac
fi

read -r -s -p 'Administrator password: ' password
printf '\n' >&2
printf '%s' "$password" | bao write -namespace="$namespace" "auth/userpass/users/$username" \
  password=- token_policies=admin
unset password
printf 'Created account %s in namespace %s.\n' "$username" "${namespace:-root}"
