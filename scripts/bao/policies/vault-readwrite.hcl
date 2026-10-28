# Install inside the agent namespace. This example targets vault_id "demo".
# Replace demo and ssh-ca-demo with your vault's KV and SSH mount names.
# Full access includes permanent KV version deletion and SSH CA management.
path "demo/*" {
  capabilities = ["create", "read", "update", "patch", "delete", "list", "scan", "sudo"]
}
path "ssh-ca-demo/*" {
  capabilities = ["create", "read", "update", "patch", "delete", "list", "scan", "sudo"]
}

# Token self-management and Bao MCP capability discovery.
path "auth/token/lookup-self" { capabilities = ["read"] }
path "auth/token/renew-self" { capabilities = ["update"] }
path "auth/token/revoke-self" { capabilities = ["update"] }
path "sys/capabilities-self" { capabilities = ["update"] }
path "sys/internal/ui/resultant-acl" { capabilities = ["read"] }
path "sys/internal/ui/mounts" { capabilities = ["read"] }

# Shared _sys_pki_root / _sys_pki_int access must be granted separately by role.
