# OpenBao Secrets Engines and Namespaces: Types, Mounts, and Design

[简体中文](secret-engines-namespaces.zh-CN.md)

OpenBao uses **namespaces** as administrative scopes. Within each namespace, enable the **secrets engines** needed by your applications. A freshly initialized root namespace contains system components; it does not automatically create application engines such as KV, PKI, SSH, or Transit.

A namespace can contain several engine types and multiple mounts of the same type. The root namespace is the top-level scope, **not a filesystem directory named `/root/`**.

## 1. Secrets engine types

The main engine types documented by OpenBao include the following. Availability depends on the release and installed plugins.

| Engine | Capability | Typical use |
| --- | --- | --- |
| **KV v1 / v2** | Store key-value secrets; v2 adds version history | API tokens, passwords, environment variables |
| **PKI** | Create CAs and issue or revoke X.509 certificates | HTTPS, mTLS, internal CAs |
| **SSH** | Issue SSH certificates with a CA; also supports OTP | Short-lived SSH access |
| **Transit** | Encrypt, decrypt, sign, and verify while keeping keys in Bao | Encryption as a service |
| **Database** | Generate database credentials and manage their leases | PostgreSQL, MySQL, and other databases |
| **Kubernetes** | Generate Kubernetes credentials, including ServiceAccount tokens | Services accessing the Kubernetes API |
| **TOTP** | Generate and validate time-based one-time passwords | 2FA and OTP |
| **LDAP / OpenLDAP** | Manage directory passwords and dynamic credentials; requires a compatible external plugin in Bao 2.7 | LDAP directories |
| **RabbitMQ** | Generate RabbitMQ user credentials | Message brokers |

For general secret management and infrastructure automation, start with these four:

- **KV**: store existing passwords and API tokens.
- **PKI**: issue and manage TLS certificates.
- **SSH**: issue SSH login certificates.
- **Transit**: provide cryptographic operations without giving applications the underlying keys.

**The Kubernetes secrets engine and Kubernetes auth method serve different purposes.** The engine issues Kubernetes credentials; the auth method authenticates Kubernetes workloads to OpenBao.

## 2. Default root namespace components

A newly initialized root namespace includes system mounts such as:

```text
OpenBao Cluster
└── Root namespace (default)
    ├── sys/          ← System control, policies, administrative APIs
    ├── identity/     ← Identity management
    └── cubbyhole/    ← Private secret storage for each token
```

These are not application KV mounts. Enable KV, PKI, SSH, or Transit separately when needed.

After enabling engines, the root namespace might look like this:

```text
OpenBao Cluster
└── Root namespace (default)
    ├── sys/          ← Built in
    ├── identity/     ← Built in
    ├── cubbyhole/    ← Built in
    ├── secret/       ← Explicitly enabled KV v2
    ├── pki/          ← Explicitly enabled PKI
    ├── ssh/          ← Explicitly enabled SSH
    └── transit/      ← Explicitly enabled Transit
```

The four application engines above are **optional**, not a fixed namespace configuration.

## 3. Enable a secrets engine

These examples assume the CLI is connected to OpenBao, targets the root namespace, and uses a token with the required administrative permissions.

```bash
# KV v2: store application secrets
bao secrets enable -path=secret kv-v2

# PKI: issue TLS certificates
bao secrets enable -path=pki pki

# SSH: issue SSH certificates
bao secrets enable -path=ssh ssh

# Transit: encryption, decryption, and signing
bao secrets enable -path=transit transit
```

List enabled engines:

```bash
bao secrets list
```

### Enabling a mount does not finish its configuration

`bao secrets enable` creates the mount. Its functionality may still need configuration:

- PKI: configure a CA and issuance roles.
- SSH: configure the certificate issuance mechanism.
- Transit: create encryption or signing keys.

### Mount the same engine type more than once

For example, enable two KV v2 mounts:

```bash
bao secrets enable -path=agent-secrets kv-v2
bao secrets enable -path=k8s-secrets kv-v2
```

This creates two independent engine instances:

- `agent-secrets/`
- `k8s-secrets/`

Each has its own configuration and storage scope. **The mount path names an instance; separate mounts do not require separate OpenBao servers.**

## 4. Distinguish namespace, mount, and path

- **Namespace**: an administrative scope with its own policies, tokens, auth methods, and engines.
- **Secrets engine / mount**: a storage or secret-generation capability, such as KV, PKI, or SSH CA.
- **Path**: an engine's resource name or API path; KV paths can separate project secrets.

If agents and Kubernetes infrastructure need independent administration, one layout is:

```text
Root Namespace
├── agent/                  ← Namespace
│   └── secret/             ← KV Engine
│       ├── github/token
│       └── cloudflare/api
└── k8s/                    ← Namespace
    ├── secret/             ← KV Engine
    ├── pki/                ← PKI Engine
    └── ssh/                ← SSH Engine
```

If one administrator manages everything and does not need separate policy, token, or auth-method administration, **mount engines in the root namespace and control access with policies**. Additional namespaces are optional.

### When to split namespaces or mounts

| Requirement | Suggested approach |
| --- | --- |
| One team manages secrets for different purposes | Separate KV paths or mounts first |
| Different capabilities, such as KV, PKI, and SSH | Enable different engine types |
| Independent policy, token, auth-method, and engine administration | Consider separate namespaces |

## 5. Disabling an engine

`bao secrets disable` **does not merely pause or hide an engine**. Disabling a mount deletes its stored data and revokes the secrets it manages. Do not treat it as a harmless production toggle.

## 6. Relationship between the layers

The hierarchy is:

```text
OpenBao Cluster
  └── Namespace (root exists by default; create others as needed)
       └── Secrets engine / mount (explicitly enabled)
            └── Path / secret / field (KV example)
```

**Namespaces define administrative scope, engines provide secret capabilities, and paths identify resources within an engine.**

For a single-administrator environment, begin with the root namespace, the engines you need, and policies separating agent, Kubernetes, and application access. Add namespaces when independent administration becomes necessary.

## References

- [OpenBao secrets engines documentation](https://openbao.org/docs/secrets/)
