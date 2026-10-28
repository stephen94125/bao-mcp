# Install this policy inside the agent namespace:
# bao policy write -namespace=agent admin scripts/bao/policies/admin.hcl
# Paths are relative to agent/. This grants agent/* and its descendants.
path "*" {
  capabilities = [
    "create",
    "read",
    "update",
    "patch",
    "delete",
    "list",
    "scan",
    "sudo"
  ]
}
