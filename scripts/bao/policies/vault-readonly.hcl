# Install inside the agent namespace. This example targets vault_id "demo".
# Replace demo and ssh-ca-demo with your vault's KV and SSH mount names.
path "demo/*" {
  capabilities = ["read", "list", "scan"]
}
path "ssh-ca-demo/*" {
  capabilities = ["read", "list", "scan"]
}

# Token self-management and Bao MCP capability discovery.
path "auth/token/lookup-self" { capabilities = ["read"] }
path "auth/token/renew-self" { capabilities = ["update"] }
path "auth/token/revoke-self" { capabilities = ["update"] }
path "sys/capabilities-self" { capabilities = ["update"] }
path "sys/internal/ui/resultant-acl" { capabilities = ["read"] }
path "sys/internal/ui/mounts" { capabilities = ["read"] }

# Shared PKI issuance requires a write capability and is not part of read-only access.
