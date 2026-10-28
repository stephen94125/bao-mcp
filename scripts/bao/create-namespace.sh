#!/usr/bin/env bash
set -euo pipefail
set +x

command -v bao >/dev/null
read -r -p 'Parent namespace (leave blank for root): ' namespace
case "$namespace" in
  /*|*/|*//*|*..*|.|*/./*|./*|*/.|*[!A-Za-z0-9_./-]*) printf 'Invalid namespace path.\n' >&2; exit 1 ;;
esac
export BAO_NAMESPACE="$namespace"
export BAO_ADDR="${BAO_ADDR:-https://secret.r5vj.xyz}"

read -r -p 'New namespace name (single level): ' name
[[ "$name" =~ ^[A-Za-z0-9][A-Za-z0-9_.-]*$ ]] || { printf 'Invalid name; / is not allowed.\n' >&2; exit 1; }
case "$name" in root|sys|audit|auth|cubbyhole|identity) printf 'This name is reserved.\n' >&2; exit 1 ;; esac
if existing=$(bao read "sys/namespaces/$name" 2>&1); then
  printf 'The namespace already exists; refusing to overwrite.\n' >&2
  exit 1
else
  case "$existing" in
    *'No value found at'*|*'Code: 404'*) ;;
    *) printf 'Unable to determine namespace status. Check your login and permissions.\n' >&2; exit 1 ;;
  esac
fi
bao namespace create "$name"
