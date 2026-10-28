# OpenBao Engine Permissions: Namespaces, ACL Wildcards, and API Paths

[简体中文](engine-permissions.zh-CN.md)

> This reference distinguishes namespace administration, cross-engine application access, and single-mount KV access. It includes wildcard rules, engine API families, HCL, CLI examples, exceptions, and design guidance.

**Policies match native API paths, not engine types. Namespaces define administrative scope, mounts supply API prefixes, and engines define the remaining routes.**

## 1. Three different permission designs

1. **Namespace administrator**: manages engines, policies, tokens, auth methods, and administrative APIs in that namespace.
2. **Namespace-wide application access**: uses secrets across engines without managing engines or policies.
3. **One KV mount**: reads and writes data within a selected mount.

These are distinct goals. The second has no universal engine-type ACL selector; grant each engine's actual application API paths.

### 1.1 A policy for only the `secret/` KV v2 mount

```hcl
path "secret/data/*" {
  capabilities = ["create", "read", "update", "patch", "delete"]
}
path "secret/metadata" {
  capabilities = ["list"]
}
path "secret/metadata/*" {
  capabilities = ["list"]
}
```

`secret` **is a mount name, not a reserved keyword**.

```text
agent/ Namespace
├── secret/        KV v2
├── credentials/   KV v2
├── projects/      KV v2
├── pki/           PKI
└── ssh/           SSH
```

| Mount | Covered by the `secret/...` policy? |
| --- | --- |
| `secret/` | Yes, within its granted KV v2 data paths |
| `credentials/` | No |
| `projects/` | No |
| `pki/` | No |
| `ssh/` | No |

Adding another KV mount does not extend a policy whose prefix is `secret/...`.

### 1.2 Data operations and listing across top-level KV v2 mounts

`+` matches one complete path segment:

```hcl
# Match a data API beneath any one top-level mount name
path "+/data/*" {
  capabilities = ["create", "read", "update", "patch", "delete"]
}

# List keys beneath any one top-level mount name
path "+/metadata" {
  capabilities = ["list"]
}

path "+/metadata/*" {
  capabilities = ["list"]
}
```

Examples:

- `secret/data/erp/database`: read/write allowed.
- `credentials/data/github/token`: read/write allowed.
- `projects/data/project-a/password`: read/write allowed.

`+` matches one segment such as `secret`, `credentials`, or `projects`.

**Two limitations:**

1. This matches **paths, not engine types**. Another engine with the same `/data/...` shape can also match.
2. It does not cover PKI, SSH, Transit, or nested mounts such as `team/credentials/`. KV restore/destruction routes also need separate grants; this is not a complete policy for every KV v2 operation.

### 1.3 Removing `sudo` from `path "*"` does not limit it to secrets

```hcl
path "*" {
  capabilities = [
    "create",
    "read",
    "update",
    "patch",
    "delete",
    "list",
    "scan"
  ]
}
```

This is **not a narrowly scoped application-secrets policy**. Bao ACLs apply to every API family, including:

- `sys/`: system administration.
- `auth/`: identity, token, and authentication configuration.
- `secret/`, `pki/`, and other engines.

Without `sudo`, endpoints requiring it remain blocked, but many system, auth, or policy operations do not require it. **Removing `sudo` alone does not restrict access to secret data.**

### 1.4 Administrator versus cross-mount KV access

| Operation | Namespace administrator | Top-level KV v2 read/write |
| --- | --- | --- |
| Read/write granted KV secrets | Yes | Yes |
| Enable/disable KV mounts | Yes | No |
| Manage PKI/SSH engines | Yes | No |
| Create/update policies | Yes | No |
| Manage auth methods | Yes | No |
| Manage tokens | Subject to issuance rules | Only separately granted self-management, such as `default` |
| Use `sudo`-protected APIs | Subject to namespace restrictions | No |

Do not treat `path "*"` as a generic read/write rule for all application engines.

## 2. ACL wildcards

- **`+`: one path segment, without crossing `/`.**
- **Trailing `*`: any remaining suffix, including multiple `/` segments.**

### 2.1 Examples

| Policy path | Request path | Match? |
| --- | --- | --- |
| `secret/+/token` | `secret/abc/token` | ✅ |
| `secret/+/token` | `secret/abc/def/token` | ❌ |
| `secret/*` | `secret/abc` | ✅ |
| `secret/*` | `secret/abc/def/ghi` | ✅ |
| `secret/+/+/token` | `secret/abc/def/token` | ✅ |
| `secret/+/+/token` | `secret/abc/token` | ❌ |

### 2.2 Components of `+/data/*`

```hcl
path "+/data/*" {
  capabilities = ["read"]
}
```

- `+`: exactly one mount-name segment.
- `data/`: a fixed literal segment.
- `*`: the remaining suffix at any depth.

```text
secret/data/abc              ✅
secret/data/abc/def/ghi      ✅
github/data/token            ✅
cloudflare/data/api          ✅

secret/abc/data/token        ❌
secret/metadata/abc          ❌
```

### 2.3 Trailing-star restrictions

**`*` belongs at the end of the policy path.**

```hcl
# Valid pattern
path "secret/*" {}

# Valid pattern
path "secret/+/data/*" {}

# Invalid: * in the middle
path "secret/*/data" {}
```

The empty `{}` blocks illustrate patterns only; real authorization rules require `capabilities`.

Unlike a shell glob, `secret/abc*` can match both `secret/abcdef` and `secret/abcdef/xyz`.

Remember: **`+` means one segment; trailing `*` means the rest.** This lets `+/data/*` match data APIs under single-segment mount names.

## 3. Engine API path structure

General API URL:

```text
https://bao.example.com/v1/<mount>/<api-suffix>
```

For PKI mounted at `internal-ca/`:

```text
internal-ca/issue/web
internal-ca/sign/web
internal-ca/roles/web
internal-ca/cert/123456
```

Suffixes below are relative to the mount. Replace `<name>`, `<role>`, and `<key>` with real names; do not paste placeholders literally into a policy.

**`data/` and `metadata/` belong to KV v2, not every engine.** Common families include:

- KV v2: `data/*`, `metadata/*`, `delete/*`, `undelete/*`, `destroy/*`.
- PKI: `roles/*`, `issue/*`, `sign/*`, `cert/*`, `revoke`.
- SSH: `roles/*`, `sign/*`, `creds/*`.
- Transit: `keys/*`, `encrypt/*`, `decrypt/*`, `sign/*`.
- Database: `config/*`, `roles/*`, `creds/*`.

`+/data/*` selects an API-path shape, not "all KV v2 engines" by type.

## 4. Main engine API families

This lists **main route families**, not every endpoint in every release or plugin. PKI and Transit have further advanced routes; use `bao path-help` against your deployment.

| Engine | Access or credential-generation suffixes | Configuration or management suffixes |
| --- | --- | --- |
| **KV v1** | `<arbitrary-path>` | No fixed KV v2 `data/` and `metadata/` separation |
| **KV v2** | `data/*`, `subkeys/*` | `metadata`, `metadata/*`, `delete/*`, `undelete/*`, `destroy/*`, `config` |
| **PKI** | `issue/<role>`, `sign/<role>`, `sign-verbatim/*`, `cert/<serial>`, `certs`, `ca`, `ca_chain`, `crl`, `ocsp` | `roles/*`, `issuers`, `issuer/*`, `keys`, `key/*`, `root/*`, `intermediate/*`, `config/*`, `revoke`, `tidy*`, `acme/*` |
| **SSH** | `sign/<role>`, `issue/<role>`, `creds/<role>`, `verify`, `public_key` | `roles`, `roles/*`, `lookup`, `issuers`, `issuer/*`, `config/ca`, `config/issuers`, `config/zeroaddress`, `tidy/*` |
| **Transit** | `encrypt/*`, `decrypt/*`, `rewrap/*`, `sign/*`, `verify/*`, `hmac/*`, `hash/*`, `random/*`, `datakey/*` | `keys`, `keys/*`, `export/*`, `byok-export/*`, `wrapping_key`, `config/*` |
| **Database** | `creds/<role>`, `static-creds/<role>` | `config/*`, `roles`, `roles/*`, `static-roles`, `static-roles/*`, `rotate-root/*`, `rotate-role/*`, `reset/*` |
| **Kubernetes** | `creds/<role>` | `config`, `roles`, `roles/*` |
| **TOTP** | `code/<name>` | `keys`, `keys/<name>` |
| **RabbitMQ** | `creds/<role>` | `config/connection`, `config/lease`, `roles`, `roles/*` |
| **LDAP** | `creds/*`, `static-cred/*`, `library/<name>/check-out` | `config`, `rotate-root`, `role/*`, `static-role/*`, `rotate-role/*`, `library/*` |
| **Cubbyhole** | `<arbitrary-path>` | Private storage for the current token |
| **Identity** | `entity/*`, `entity-alias/*`, `group/*`, `group-alias/*`, `lookup/*` | `oidc/*`, `mfa/*`, and identity administration |

**Version differences:** LDAP requires an external plugin in Bao 2.7; older inventories included LDAP/OpenLDAP. Check your installed version and plugin catalog. Cubbyhole and Identity are built-in system functions, not ordinary application mounts.

## 5. Engine-specific routes

### 5.1 KV v2

| Suffix | Purpose | Common ACL capabilities |
| --- | --- | --- |
| `data/<key>` | Read, write, patch, or soft-delete a secret | `create/read/update/patch/delete` |
| `metadata` | List the KV root | `list` |
| `metadata/<key>` | List paths or manage metadata and versions | `list/read/update/delete`, depending on operation |
| `subkeys/<key>` | Inspect field structure without secret values | `read` |
| `delete/<key>` | Soft-delete selected versions | `update` |
| `undelete/<key>` | Restore soft-deleted versions | `update` |
| `destroy/<key>` | Permanently destroy selected versions | `update` |
| `config` | Mount-wide KV configuration | `read/update` |

**`metadata/*` is not listing-only.** Granting `delete` can remove metadata and every version; `list` alone cannot.

### 5.2 PKI

PKI separates certificate issuance from CA and role administration:

| Suffix | Purpose |
| --- | --- |
| `issue/<role>` | Generate a private key and certificate |
| `sign/<role>` | Sign an existing CSR |
| `sign-verbatim/<role>` | Special signing flow bypassing ordinary role field restrictions |
| `issuer/<issuer>/issue/<role>` | Issue with a selected issuer |
| `issuer/<issuer>/sign/<role>` | Sign a CSR with a selected issuer |
| `cert/<serial>`, `certs` | Retrieve certificates or list serial numbers |
| `ca`, `ca_chain`, `crl`, `ocsp` | CA and revocation information |
| `roles`, `roles/<role>` | Manage issuance roles |
| `issuers`, `issuer/<issuer>` | Manage CA issuers |
| `keys`, `key/<key>` | Manage CA keys |
| `root/*`, `intermediate/*` | Create or manage root/intermediate CAs |
| `config/*` | CRL, URL, issuer, and key configuration |
| `revoke`, `revoke-with-key` | Revoke certificates |
| `tidy`, `tidy-status` | Start cleanup or inspect its status |
| `acme/*` | ACME protocol endpoints |

Some families include PEM, DER, raw, or issuer-specific variants.

### 5.3 SSH

| Suffix | Purpose |
| --- | --- |
| `sign/<role>` | Sign a public key to obtain an SSH certificate |
| `issue/<role>` | Generate a keypair and certificate |
| `creds/<role>` | Generate an SSH OTP |
| `verify` | Validate an OTP |
| `roles`, `roles/<role>` | Manage roles |
| `lookup` | Find roles by IP |
| `config/ca` | Configure or inspect the CA |
| `config/issuers` | Select the default issuer |
| `issuers`, `issuer/<ref>` | Manage multiple issuers |
| `issuer/<ref>/public_key`, `public_key` | Retrieve a CA public key |
| `config/zeroaddress` | Manage OTP roles without a specific-IP restriction |

For SSH CA use, grant `ssh/sign/<role>` when possible rather than role administration at `ssh/roles/*`.

### 5.4 Transit

Transit performs cryptographic operations rather than merely storing readable secret values:

| Suffix | Purpose |
| --- | --- |
| `encrypt/<key>` | Encrypt |
| `decrypt/<key>` | Decrypt |
| `rewrap/<key>` | Re-encrypt older ciphertext with the current key version |
| `sign/<key>` | Sign |
| `verify/<key>` | Verify signatures |
| `hmac/<key>` | Generate an HMAC |
| `hash/*`, `random/*` | Hashing and random bytes |
| `datakey/plaintext/<key>` | Generate a data key and return plaintext plus ciphertext |
| `datakey/wrapped/<key>` | Generate a data key and return only ciphertext |
| `keys`, `keys/<key>` | Manage keys |
| `keys/<key>/rotate` | Rotate a key |
| `keys/<key>/config` | Change key configuration |
| `export/*`, `byok-export/*` | Export eligible key material |

Encryption and decryption are separate paths, so an application can receive only encryption rights.

## 6. Identical suffixes can mean different operations

| Suffix | Engines using it |
| --- | --- |
| `roles/*` | PKI, SSH, Database, Kubernetes, RabbitMQ |
| `creds/*` | Database, SSH, Kubernetes, RabbitMQ, LDAP |
| `config/*` | PKI, SSH, Database, Transit, and others |
| `keys/*` | Transit, TOTP, PKI |
| `sign/*` | PKI, SSH, Transit |

Even `creds/<role>` can have different semantics:

```bash
# Database: a read dynamically issues database credentials
bao read database/creds/my-role

# Kubernetes: a write requests a ServiceAccount token
bao write kubernetes/creds/my-role \
  kubernetes_namespace=test
```

The first uses `read`; the second is a write API that typically requires `update`.

Consequently:

```hcl
path "+/creds/*" {
  capabilities = ["read"]
}
```

**This does not authorize credential issuance for every engine**, because not all engines use GET for it.

KV v1 paths can be named freely:

```text
legacy-kv/data/password
legacy-kv/roles/admin
```

These may be ordinary KV v1 data, not a KV v2 data API or another engine's roles API. **Wildcards match path strings, not types.**

## 7. Application-operation policy examples

### PKI: issue only with role `web`

```hcl
path "pki/issue/web" {
  capabilities = ["create", "update"]
}
```

### SSH: sign only with role `linux-user`

```hcl
path "ssh/sign/linux-user" {
  capabilities = ["create", "update"]
}
```

### Transit: encrypt only with key `app-key`

```hcl
path "transit/encrypt/app-key" {
  capabilities = ["update"]
}
```

### Database: obtain dynamic credentials only for `erp-reader`

```hcl
path "database/creds/erp-reader" {
  capabilities = ["read"]
}
```

### Kubernetes: obtain a token only for role `erp`

```hcl
path "kubernetes/creds/erp" {
  capabilities = ["update"]
}
```

**Application operations are not necessarily `read`.** TLS/SSH issuance and Transit encryption use write APIs even though callers are consuming a service.

## 8. Inspect your deployment's API routes

The CLI provides:

```bash
bao path-help <mount>
```

For example:

```bash
export BAO_NAMESPACE=agent

bao secrets list

bao path-help secret
bao path-help pki
bao path-help ssh
bao path-help transit
bao path-help database
bao path-help kubernetes
```

This lists engine route patterns, often resembling regular expressions. Inspect one concrete endpoint with:

```bash
bao path-help pki/issue/web
bao path-help ssh/sign/linux-user
bao path-help transit/encrypt/app-key
```

The response describes parameters and behavior for the deployed version, mount, and caller permissions.

Also useful:

```bash
# Installed secrets plugins
bao plugin list secret

# Engines mounted in the current namespace
bao secrets list -detailed
```

## 9. Design cross-engine policies

### 9.1 Data access across top-level KV v2 mounts

With known mount names and types, `+/data/*`, `+/metadata`, and `+/metadata/*` can implement common data/list access. They are **not strict engine-type filters**.

### 9.2 Grant KV, PKI, SSH, Transit, and Database together

Use each engine's actual mount and API path:

```hcl
# KV v2
path "secret/data/*" {
  capabilities = ["create", "read", "update", "patch", "delete"]
}

path "secret/metadata" {
  capabilities = ["list"]
}

path "secret/metadata/*" {
  capabilities = ["list"]
}

# PKI issuance
path "pki/issue/*" {
  capabilities = ["create", "update"]
}

# SSH certificate issuance
path "ssh/sign/*" {
  capabilities = ["create", "update"]
}

# Transit encryption and decryption
path "transit/encrypt/*" {
  capabilities = ["update"]
}

path "transit/decrypt/*" {
  capabilities = ["update"]
}

# Dynamic database credentials
path "database/creds/*" {
  capabilities = ["read"]
}
```

**This is an application-access example, not the complete feature set of every engine.** Add paths for additional operations explicitly.

### 9.3 Permission layers for Bao MCP callers

- `namespace-admin`: `path "*"` and necessary `sudo` for namespace administration, subject to namespace restrictions.
- `kv-all-readwrite`: `+/data/*`, `+/metadata`, and `+/metadata/*` across single-segment KV mounts.
- `kv-readwrite`: one mount such as `secret/`.
- `kv-scoped-readonly`: selected secrets or paths in one mount.
- Cross-engine application roles: concrete PKI/SSH/Transit/Database issuance, signing, encryption, decryption, and credential paths.

OpenBao has no `all-secret-engines-only` or `secret-engine/*` type selector for "all application operations, but no engine/policy administration". Use native API paths.

**Do not use `path "*"` as a general secret-operator policy.** It also matches `sys/` and `auth/` and can approach administrator access.

## 10. Design rules

1. Namespaces set administrative boundaries; ACL paths match APIs, not engine types.
2. `+` matches one segment; trailing `*` matches the remaining suffix at any depth.
3. KV v2 uses `data/` and `metadata/`; other engines define other suffixes.
4. Engines may share names such as `roles/`, `creds/`, `sign/`, or `keys/` with different meanings and capabilities.
5. Removing `sudo` from `path "*"` does not make it secrets-only.
6. Cross-engine policies must follow actual mounts and routes.
7. This reference lists main route families, not all endpoints in every release. Check the deployed version using `bao path-help`.

References: [Secrets engines](https://openbao.org/docs/secrets/) · [ACL policies](https://openbao.org/docs/concepts/policies/) · [CLI commands](https://openbao.org/docs/commands/).
