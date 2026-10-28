# OpenBao Engine Resources and API Suffixes

[简体中文](engine-storage-api-paths.zh-CN.md)

This reference explains **what each engine stores or generates, which API paths follow a mount, what those paths do, and their ACL capabilities**.

For namespace hierarchy, root/child scopes, wildcard rules, and broad-policy implications, see [Path notation](path-notation.md).

> The tables list main route families, not every endpoint in every release. Consult your deployed version's `bao path-help` and official API documentation for exact routes.

## 1. Engines store different kinds of state

A mount is not necessarily a folder of secret values. Engines may store KV data, manage CAs, issue external-system credentials, or provide cryptographic operations.

| Engine | Resources managed | Operation model |
| --- | --- | --- |
| **KV v1** | Arbitrary secret paths and fields | Store, retrieve, or overwrite values |
| **KV v2** | Secret data, versions, and metadata | Versioned reads/writes, soft deletion, restoration, destruction |
| **PKI** | CA/issuer, CA key, roles, certificates, revocation data | Issue, retrieve, and revoke X.509 certificates |
| **SSH** | CA/issuer, roles, OTP configuration | Issue SSH certificates or OTPs |
| **Transit** | Cryptographic keys managed by Bao | Encryption, signatures, HMAC, and data-key operations rather than ordinary plaintext-key reads |
| **Database** | Connection configuration, dynamic/static roles | Issue dynamic accounts or rotate managed passwords |
| **Kubernetes** | Cluster configuration and roles | Generate ServiceAccount credentials |
| **TOTP** | TOTP key configuration | Generate or validate one-time passwords |
| **RabbitMQ** | Connection configuration and roles | Issue user credentials |
| **LDAP** | Connection settings, roles, static roles, libraries | Manage directory passwords and leased credentials |

**Cubbyhole and Identity** may appear in `bao secrets list`, but are built-in system functions. Cubbyhole is token-private storage; Identity manages entities and groups. They are not ordinary user-mounted application engines.

## 2. Mounts and API suffixes

Conceptually:

```text
https://bao.example.com/v1/<mount>/<api-suffix>
```

For PKI mounted at `internal-ca/` rather than `pki/`:

```text
internal-ca/issue/web
internal-ca/sign/web
internal-ca/roles/web
internal-ca/cert/123456
```

All suffixes below follow the **mount path**. Replace `<name>`, `<role>`, `<key>`, and `<issuer>` with actual resource names.

**`data/` and `metadata/` are KV v2 conventions, not a universal engine layout.**

## 3. Main route families

| Engine | Access or credential-generation suffixes | Configuration or management suffixes |
| --- | --- | --- |
| **KV v1** | `<arbitrary-path>` | No fixed KV v2 data/metadata layering |
| **KV v2** | `data/*`, `subkeys/*` | `metadata`, `metadata/*`, `delete/*`, `undelete/*`, `destroy/*`, `config` |
| **PKI** | `issue/<role>`, `sign/<role>`, `sign-verbatim/*`, `cert/<serial>`, `certs`, `ca`, `ca_chain`, `crl`, `ocsp` | `roles/*`, `issuers`, `issuer/*`, `keys`, `key/*`, `root/*`, `intermediate/*`, `config/*`, `revoke`, `tidy*`, `acme/*` |
| **SSH** | `sign/<role>`, `issue/<role>`, `creds/<role>`, `verify`, `public_key` | `roles`, `roles/*`, `lookup`, `issuers`, `issuer/*`, `config/ca`, `config/issuers`, `config/zeroaddress`, `tidy/*` |
| **Transit** | `encrypt/*`, `decrypt/*`, `rewrap/*`, `sign/*`, `verify/*`, `hmac/*`, `hash/*`, `random/*`, `datakey/*` | `keys`, `keys/*`, `export/*`, `byok-export/*`, `wrapping_key`, `config/*` |
| **Database** | `creds/<role>`, `static-creds/<role>` | `config/*`, `roles`, `roles/*`, `static-roles`, `static-roles/*`, `rotate-root/*`, `rotate-role/*`, `reset/*` |
| **Kubernetes** | `creds/<role>` | `config`, `roles`, `roles/*` |
| **TOTP** | `code/<name>` | `keys`, `keys/<name>` |
| **RabbitMQ** | `creds/<role>` | `config/connection`, `config/lease`, `roles`, `roles/*` |
| **LDAP** | `creds/*`, `static-cred/*`, `library/<name>/check-out` | `config`, `rotate-root`, `role/*`, `static-role/*`, `rotate-role/*`, `library/*` |
| **Cubbyhole (built in)** | `<arbitrary-path>` | Private storage for the current token |
| **Identity (built in)** | `entity/*`, `entity-alias/*`, `group/*`, `group-alias/*`, `lookup/*` | `oidc/*`, `mfa/*`, and identity management |

**Version/plugin differences:** LDAP requires a compatible external plugin in Bao 2.7; older catalogs included LDAP/OpenLDAP. Check the installed version and `bao plugin list secret`.

## 4. KV storage and paths

### 4.1 KV v1

KV v1 stores secrets at **arbitrary resource paths** after its mount. For example:

```text
legacy-kv/erp/database
legacy-kv/github/token
legacy-kv/data/password
legacy-kv/roles/admin
```

Here, `data/` or `roles/` may simply be user-chosen secret names. **KV v1 does not have KV v2's fixed data/metadata routing.**

### 4.2 KV v2

KV v2 separates data, metadata, version deletion, restoration, and destruction into endpoints.

| Suffix | Purpose | Common capabilities |
| --- | --- | --- |
| `data/<key>` | Read, write, patch, or soft-delete | `create/read/update/patch/delete` |
| `metadata` | List root keys | `list` |
| `metadata/<key>` | List child paths or manage metadata/versions | `list/read/update/delete`, by operation |
| `subkeys/<key>` | Inspect field structure without returning values | `read` |
| `delete/<key>` | Soft-delete selected versions | `update` |
| `undelete/<key>` | Restore soft-deleted versions | `update` |
| `destroy/<key>` | Permanently destroy selected versions | `update` |
| `config` | Engine-wide KV configuration | `read/update` |

The logical key `erp/database` has this data API path:

```text
secret/data/erp/database
```

Listing uses `secret/metadata/...`, not `secret/data/...`.

**`metadata/*` is not listing-only.** `delete` can remove a key's metadata and every version; `list` alone cannot.

## 5. PKI: CAs, issuers, roles, and certificates

PKI manages **issuers, CA keys, issuance roles, certificates, and revocation state**, not arbitrary KV records.

| Suffix | Purpose |
| --- | --- |
| `issue/<role>` | Generate a private key and certificate |
| `sign/<role>` | Sign an existing CSR |
| `sign-verbatim/<role>` | Special CSR signing without ordinary role field restrictions |
| `issuer/<issuer>/issue/<role>` | Issue through a selected issuer |
| `issuer/<issuer>/sign/<role>` | Sign through a selected issuer |
| `cert/<serial>`, `certs` | Retrieve certificates or list serials |
| `ca`, `ca_chain`, `crl`, `ocsp` | Retrieve CA and revocation information |
| `roles`, `roles/<role>` | List/manage issuance roles |
| `issuers`, `issuer/<issuer>` | Manage issuers |
| `keys`, `key/<key>` | Manage CA keys |
| `root/*`, `intermediate/*` | Create/manage root and intermediate CAs |
| `config/*` | Configure CRLs, URLs, issuers, and keys |
| `revoke`, `revoke-with-key` | Revoke certificates |
| `tidy`, `tidy-status` | Cleanup and status |
| `acme/*` | ACME protocol endpoints |

Some families have PEM, DER, raw, or issuer-specific variants.

**`issue/` and `sign/` consume CA issuance services; `root/`, `issuer/`, `keys/`, and `roles/` administer the CA and issuance rules.**

## 6. SSH: CAs, roles, and OTP

| Suffix | Purpose |
| --- | --- |
| `sign/<role>` | Sign a public key and obtain an SSH certificate |
| `issue/<role>` | Generate a keypair and certificate |
| `creds/<role>` | Generate an OTP |
| `verify` | Verify an OTP |
| `roles`, `roles/<role>` | Manage roles |
| `lookup` | Find roles by IP |
| `config/ca` | Configure/inspect a CA |
| `config/issuers` | Configure the default issuer |
| `issuers`, `issuer/<ref>` | Manage issuers |
| `issuer/<ref>/public_key`, `public_key` | Retrieve CA public keys |
| `config/zeroaddress` | Manage OTP roles without a specific-IP restriction |

The common application grant is `ssh/sign/<role>`, not role administration through `ssh/roles/*`.

## 7. Transit: keys and cryptographic operations

Transit provides **cryptographic operations** while Bao normally retains the keys. It is not a KV store whose ordinary purpose is to expose key plaintext.

| Suffix | Purpose |
| --- | --- |
| `encrypt/<key>` | Encrypt |
| `decrypt/<key>` | Decrypt |
| `rewrap/<key>` | Re-encrypt old ciphertext with the current key version |
| `sign/<key>` | Sign |
| `verify/<key>` | Verify signatures |
| `hmac/<key>` | Generate HMAC |
| `hash/*`, `random/*` | Hashes and random bytes |
| `datakey/plaintext/<key>` | Generate a data key, returning plaintext and ciphertext |
| `datakey/wrapped/<key>` | Generate a data key, returning ciphertext only |
| `keys`, `keys/<key>` | Manage keys |
| `keys/<key>/rotate` | Rotate a key |
| `keys/<key>/config` | Update key configuration |
| `export/*`, `byok-export/*` | Export eligible key material |

Because encryption and decryption use separate paths, a token can encrypt without decrypting. Key administration/export permissions are separate from ordinary cryptographic use.

## 8. Other engines

| Engine | Important suffixes | Purpose |
| --- | --- | --- |
| **Database** | `config/*`, `roles/*`, `static-roles/*` | Connections and dynamic/static roles |
| | `creds/<role>` | Issue database credentials on read |
| | `static-creds/<role>` | Retrieve managed static credentials |
| | `rotate-root/*`, `rotate-role/*`, `reset/*` | Rotate or reset configuration/credentials |
| **Kubernetes** | `config`, `roles/*`, `creds/<role>` | Configure a cluster/roles and issue ServiceAccount credentials |
| **TOTP** | `keys`, `keys/<name>`, `code/<name>` | Configure keys, generate/validate codes |
| **RabbitMQ** | `config/connection`, `config/lease`, `roles/*`, `creds/<role>` | Configure connections/roles and issue credentials |
| **LDAP** | `config`, `role/*`, `static-role/*` | Connections and dynamic/static roles |
| | `creds/*`, `static-cred/*` | Retrieve directory credentials |
| | `rotate-root`, `rotate-role/*`, `library/<name>/check-out` | Rotation and credential-library checkout |

### Built-in Cubbyhole and Identity

- **Cubbyhole**: private storage per token, addressed by key names rather than a shared application KV mount.
- **Identity**: entities, aliases, groups, lookup, OIDC, MFA, and related administration.

Their appearance in engine listings does not make them interchangeable with user-enabled PKI, SSH, or KV mounts.

## 9. Shared suffix names do not imply shared semantics

| Suffix | Engines using it |
| --- | --- |
| `roles/*` | PKI, SSH, Database, Kubernetes, RabbitMQ |
| `creds/*` | Database, SSH, Kubernetes, RabbitMQ, LDAP |
| `config/*` | PKI, SSH, Database, Transit, and others |
| `keys/*` | Transit, TOTP, PKI |
| `sign/*` | PKI, SSH, Transit |

**The same suffix can require different operations and capabilities.**

For example:

```bash
# Database: reading dynamically issues credentials
bao read database/creds/my-role

# Kubernetes: writing requests a ServiceAccount token
bao write kubernetes/creds/my-role \
  kubernetes_namespace=test
```

The first uses `read`; the second typically needs a write capability such as `update`.

Without knowing the mount type, this rule:

```hcl
path "+/creds/*" {
  capabilities = ["read"]
}
```

does not authorize every engine's issuance route. Some `creds/` APIs use GET, others use writes.

A KV v1 secret can also be named:

```text
legacy-kv/data/password
legacy-kv/roles/admin
```

Those strings do not turn it into KV v2, PKI, or Database. **Interpret suffixes using the mounted engine's actual type.**

## 10. Engine-specific application ACLs

The examples below cover common operations. See [Path notation](path-notation.md) for generic wildcards and namespace administration.

### PKI: issue only through role `web`

```hcl
path "pki/issue/web" {
  capabilities = ["create", "update"]
}
```

### SSH: sign only through role `linux-user`

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

### Database: obtain credentials only for `erp-reader`

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

**Application operations need not be `read`.** Certificate issuance and Transit encryption consume services through write APIs.

### Combined application policy

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

This is a **cross-engine application example**, not every feature of every engine. Add grants for actual mounts, methods, and capabilities as requirements expand.

## 11. Inspect routes for your installed version

Bao provides native path help:

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

It lists route patterns, often in a regular-expression-like notation.

Inspect a specific operation's parameters and behavior:

```bash
bao path-help pki/issue/web
bao path-help ssh/sign/linux-user
bao path-help transit/encrypt/app-key
```

Other useful commands:

```bash
# Installed secrets plugins
bao plugin list secret

# Actual mounts in the current namespace
bao secrets list -detailed
```

Release and external-plugin routes can differ. Use the server's help responses and official API references as the final reference.

## 12. Practical distinctions

1. **KV centers on stored secrets**; PKI/SSH on issuance; Transit on keys and cryptographic operations; other engines may issue credentials in external systems.
2. **Engines define API suffixes**. KV v2 data/metadata layering is not universal.
3. **Suffixes such as `roles/`, `creds/`, `sign/`, and `keys/` can mean different things** and require different methods/capabilities.
4. **Application and administrative APIs can often be granted separately**: issue SSH certificates without editing roles, or encrypt without decrypting/exporting keys.
5. These are main route families. For release-specific details, consult `bao path-help` and native API documentation.

References: [Secrets engines](https://openbao.org/docs/secrets/) · [CLI commands](https://openbao.org/docs/commands/) · [ACL policies](https://openbao.org/docs/concepts/policies/).
