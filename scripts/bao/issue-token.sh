#!/usr/bin/env bash
set +x
set -euo pipefail

command -v bao >/dev/null || { printf 'bao is required.\n' >&2; exit 1; }
command -v gum >/dev/null || { printf 'gum is required.\n' >&2; exit 1; }
export BAO_ADDR="${BAO_ADDR:-https://secret.r5vj.xyz}"

gum style --border rounded --border-foreground 212 --padding '0 2' \
  --foreground 212 'OpenBao · Issue Token'
namespace=$(gum input --header 'Namespace' --placeholder 'Leave blank for root')
export BAO_NAMESPACE="$namespace"

policies=$(bao policy list -namespace="$namespace" -format=table)
[[ -n "$policies" ]] || { printf 'This namespace has no policies to select.\n' >&2; exit 1; }
selected=$(printf '%s\n' "$policies" | gum choose --no-limit \
  --header 'Policies · ↑↓ move, Space toggle, Enter submit')
[[ -n "$selected" ]] || { printf 'Select at least one policy.\n' >&2; exit 1; }

args=(-namespace="$namespace" -orphan)
while IFS= read -r policy; do
  bao policy read -namespace="$namespace" "$policy" >/dev/null
  args+=(-policy="$policy")
done <<< "$selected"

display_name=$(gum input --header 'Token name' --placeholder 'Enter a token name')
ttl=$(gum input --header 'TTL · Bare numbers are days; supports 30s, 30m, 12h, 7d, 1h30m; y and M are unsupported; max 1825d' \
  --value '365' --placeholder 'Leave blank for 365d')
ttl=${ttl:-365}
if [[ "$ttl" =~ ^[0-9]+$ ]]; then
  ttl="${ttl}d"
fi

if ! printf '%s\n' "$ttl" | LC_ALL=C awk '
  BEGIN { seconds["s"]=1; seconds["m"]=60; seconds["h"]=3600; seconds["d"]=86400 }
  {
    rest=$0
    while (match(rest, /^([0-9]+(\.[0-9]*)?|\.[0-9]+)[smhd]/)) {
      part=substr(rest, 1, RLENGTH)
      total+=(part+0)*seconds[substr(part, length(part), 1)]
      rest=substr(rest, RLENGTH+1)
    }
    if (rest != "" || total <= 0 || total > 1825*86400) exit 1
  }
'; then
  printf 'Invalid TTL: enter a positive number of days or a duration using s, m, h, d (combinations allowed); maximum 1825d; y and M are unsupported.\n' >&2
  exit 1
fi

bao token create "${args[@]}" -display-name="$display_name" -ttl="$ttl"
