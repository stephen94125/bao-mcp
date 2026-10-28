# OpenBao Architecture and Access Control: Engines, Policies, Tokens, and KV

[简体中文](architecture-permissions.zh-CN.md)

OpenBao has two related structures:

1. **Secret storage**: which engine and path hold a secret.
2. **Policy and token authorization**: which identities may read, write, or list those resources.

The usual sequence is to configure storage, create a policy, and issue a token carrying that policy. Each token does not require a separate user account.

## 1. Storage hierarchy

For ERP, GitHub, and Cloudflare credentials, a layout might be:

```text
OpenBao Server / Cluster
├── secret/                     ← KV v2 Secret Engine（Mount）
│   ├── erp/                    ← Logical path
│   │   └── database            ← One secret
│   │       ├── username = erp_user
│   │       └── password = ********
│   ├── github/
│   │   └── deploy-token
│   └── cloudflare/
│       └── api-token
└── pki/                        ← Another secrets engine
```

### What each layer means

| Level | Name | Purpose |
| --- | --- | --- |
| 1 | OpenBao server / cluster | The secret-management service |
| 2 | Namespace (optional) | Independent administrative and authorization scope |
| 3 | Secrets engine / mount | A distinct storage or functional unit |
| 4 | Path | A user-defined logical path |
| 5 | Secret | A data item at that path |
| 6 | Field | A value such as `username` or `password` within a secret |

**OpenBao does not define a special "vault" layer for every folder.**

An application-defined vault named `production` could map to:

- `secret/production/`: an ordinary path inside a shared KV engine.
- `production/`: a separate engine mount with its own configuration and storage scope.
- A namespace: a higher administrative scope separating policies, tokens, auth methods, and engines.

For project secrets, **one KV v2 mount with project-specific paths, policies, and tokens** is often sufficient. A mount per project is not mandatory.

## 2. Create KV storage and read or write secrets

These examples assume OpenBao is initialized and unsealed and the CLI has administrative access.

List engines:

```bash
bao secrets list
```

If `secret/` does not exist, enable KV v2:

```bash
bao secrets enable -path=secret kv-v2
```

Write a secret:

```bash
bao kv put -mount=secret erp/database \
  username=erp_user \
  password=test-only
```

Read the complete secret:

```bash
bao kv get -mount=secret erp/database
```

Print one field from the retrieved secret; this output selection does not provide field-level access control:

```bash
bao kv get -mount=secret \
  -field=password erp/database
```

In this example:

- `secret` is the engine mount name.
- `erp` is a logical path; no directory must be created first.
- `database` identifies one secret.
- `username` and `password` are fields in that secret.

## 3. Create a read-only policy

Suppose a token should **read only ERP secrets**, without modifying them or reading GitHub secrets.

Create `erp-readonly.hcl`:

```hcl
# Read secrets under ERP
path "secret/data/erp/*" {
  capabilities = ["read"]
}

# List keys under ERP
path "secret/metadata/erp" {
  capabilities = ["list"]
}

path "secret/metadata/erp/*" {
  capabilities = ["list"]
}
```

### KV v2 policy paths

KV v2 policies **must match native API paths**:

- `secret/data/erp/*`: read ERP secret data.
- `secret/metadata/erp` and `secret/metadata/erp/*`: list keys at those paths.
- Writing only `secret/erp/*` **does not** cover the KV v2 data API.

`bao kv get` inserts the `data/` API segment for you.

Upload the local HCL policy:

```bash
bao policy write erp-readonly erp-readonly.hcl
```

Inspect policies:

```bash
bao policy list
bao policy read erp-readonly
```

**Creating a policy defines rules; it does not automatically grant them to a new token.**

## 4. Issue and manage tokens

Attach `erp-readonly` to a new token:

```bash
bao token create \
  -policy=erp-readonly \
  -ttl=24h \
  -display-name=erp-reader
```

Example output:

```text
token             hvs.xxxxxxxxxxxxx
token_accessor    xxxxxxxxxxxxx
token_duration    24h
token_renewable   true
token_policies    [default erp-readonly]
```

A caller can pass this token through Bao MCP. Bao evaluates its attached policies for each operation.

A token may carry several policies:

```bash
bao token create \
  -policy=erp-readonly \
  -policy=github-readonly \
  -ttl=24h
```

OpenBao evaluates them together using its path-matching and capability rules.

### Token and policy behavior

1. The built-in `default` policy is usually attached automatically; it does not grant access to all secrets.
2. An ordinary issued token's policy-name list cannot be edited directly. Issue a replacement to change that list.
3. Changing the **contents** of `erp-readonly` affects subsequent requests by existing tokens carrying that policy; replacement tokens are unnecessary.

## 5. Verify the authorization relationship

```text
Token：erp-reader
    │
    └── Policy：erp-readonly
           │
           └── secret/data/erp/*
                   │
                   ├── erp/database          ✔ Read allowed
                   └── github/deploy-token   ✘ Outside this grant
```

The GitHub API path would normally be `secret/data/github/deploy-token`. It does not match the ERP policy, so that policy grants no read capability there.

With a management token authorized to inspect capabilities, check the issued token:

```bash
bao token capabilities \
  <issued-token> secret/data/erp/database
```

Revoke the test token when finished:

```bash
bao token revoke <issued-token>
```

## 6. Is a role required?

**Issuing a test token directly does not require a role first.**

A token role constrains issuance, such as allowed policies and maximum TTL. It is not mandatory for every token creation.

For a basic Bao MCP test, the sequence is:

1. **Engine**: create a mount such as `secret/`.
2. **Policy**: create `erp-readonly` with the desired paths and capabilities.
3. **Token**: issue a token carrying that policy and pass it through Bao MCP.

## 7. Access-control boundary

**A path does not grant access by itself. Policies decide which identities may perform which operations on it.**

One KV v2 mount with separate project paths, policies, and tokens provides basic access isolation without introducing another container hierarchy.
