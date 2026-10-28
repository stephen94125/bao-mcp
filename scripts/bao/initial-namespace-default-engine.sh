#!/usr/bin/env bash
set +x
set -euo pipefail

command -v bao >/dev/null || { printf 'bao is required.\n' >&2; exit 1; }
command -v gum >/dev/null || { printf 'gum is required.\n' >&2; exit 1; }
export BAO_ADDR="${BAO_ADDR:-https://secret.r5vj.xyz}"

gum style --border rounded --border-foreground 212 --padding '0 2' \
  --foreground 212 'OpenBao · Enable Secret Engines'
namespace=$(gum input --header 'Namespace' --placeholder 'Leave blank for root')
export BAO_NAMESPACE="$namespace"

mounts=$(bao secrets list -namespace="$namespace" -format=table)
paths=$(printf '%s\n' "$mounts" | awk '{ print $1 }')
engines=('secret kv-v2' 'pki pki' 'ssh ssh' 'transit transit'
         'database database' 'kubernetes kubernetes' 'totp totp' 'rabbitmq rabbitmq')
available=()
printf 'Namespace: %s\n' "${namespace:-root}"
for engine in "${engines[@]}"; do
  mount=${engine%% *}
  type=${engine#* }
  label="$mount/ → $type"
  if grep -Fxq -- "$mount/" <<< "$paths"; then
    printf '[In use] %s\n' "$label"
  else
    printf '[Available] %s\n' "$label"
    available+=("$label")
  fi
done

if [[ ${#available[@]} -eq 0 ]]; then
  printf 'All candidate mount paths are already in use. No changes made.\n'
  exit 0
fi
selected=$(printf '%s\n' "${available[@]}" | gum choose --no-limit \
  --header 'Secret Engines · ↑↓ move, Space toggle, Enter submit')
if [[ -z "$selected" ]]; then
  printf 'No engines selected. No changes made.\n'
  exit 0
fi

for engine in "${engines[@]}"; do
  mount=${engine%% *}
  type=${engine#* }
  if grep -Fxq -- "$mount/ → $type" <<< "$selected"; then
    bao secrets enable -namespace="$namespace" -path="$mount" "$type"
  fi
done
