#!/usr/bin/env bash
set +x
set -euo pipefail
umask 077
if [[ $# -ne 2 ]]; then
    printf 'Usage: %s <policy_name> <hcl_relative_to_script>\n' "$0" >&2
    exit 1
fi
policy_name=$1
hcl_path=$2
case "$hcl_path" in
    /*) printf 'HCL path must be relative to this script.\n' >&2; exit 1 ;;
esac
command -v bao >/dev/null || { printf 'bao is required.\n' >&2; exit 1; }
read -r -p 'Namespace/project (blank = root): ' namespace
case "$namespace" in
    /*|*/|.|./*|*/./*|*/.|*..*|*//*|*[!A-Za-z0-9_./-]*) printf 'Invalid namespace.\n' >&2; exit 1 ;;
esac
export BAO_NAMESPACE="$namespace"
export BAO_ADDR="${BAO_ADDR:-https://secret.r5vj.xyz}"
script_dir=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
bao policy write "$policy_name" "$script_dir/$hcl_path"
