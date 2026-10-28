path "+/data/*" {
  capabilities = ["read"]
}
path "+/metadata" {
  capabilities = ["list"]
}
path "+/metadata/*" {
  capabilities = ["list"]
}
path "auth/token/lookup-self" {
  capabilities = ["read"]
}
path "auth/token/renew-self" {
  capabilities = ["update"]
}
path "auth/token/revoke-self" {
  capabilities = ["update"]
}
path "sys/capabilities-self" {
  capabilities = ["update"]
}
