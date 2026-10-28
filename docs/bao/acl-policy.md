# OpenBao ACL Policies: Administrators, KV Access, and Path Restrictions

[简体中文](acl-policy.zh-CN.md)

OpenBao ACL policies control operations by administrative scope, namespace, engine, and API path. This guide presents five common designs with HCL, setup commands, usage examples, capability checks, and limitations.

`path "*"` can define a **broadly privileged administrator**. Both `scan` and `sudo` are supported capabilities, but such a policy is not the special built-in `root` policy.

## 1. Five policy designs

| Policy | Scope | Capabilities |
| --- | --- | --- |
| `super-admin` | Root namespace with broad access | Manage engines, policies, tokens, and namespaces |
| `namespace-admin` | The `agent` namespace | Manage resources within that scope |
| `kv-readwrite` | One KV v2 mount | Read, create, update, and delete secrets |
| `kv-readonly` | One KV v2 mount | Read secrets and list keys |
| `kv-scoped-readonly` | Selected paths | Read only the granted paths |

## 2. Broad administrator policy

File: `super-admin.hcl`

```hcl
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
```

Create the policy:

```bash
unset BAO_NAMESPACE

bao policy write super-admin super-admin.hcl
```

This policy includes all standard operation capabilities:

- `scan`: recursively enumerate resources where supported by the engine.
- `sudo`: access administrative endpoints requiring additional privilege.
- `*`: match paths including `sys/` and `auth/`, not only application secrets.

**This is still not the built-in `root` policy.** Root is special and can bypass ordinary ACL evaluation. `super-admin` remains subject to ACLs, namespace boundaries, and other controls.

OpenBao's ACL documentation also demonstrates a broad administrative policy using `path "*"` and these eight capabilities.

## 3. Administrator for the `agent` namespace

Use a namespace for independent administration instead of maintaining extensive exclusions in root-level policies.

File: `namespace-admin.hcl`

```hcl
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
```

**The HCL is identical to the broad administrator policy.** Its scope depends on where the policy is created and which namespace owns the token.

First, create the namespace using a root administrative identity:

```bash
unset BAO_NAMESPACE

bao namespace create agent
```

Select `agent`:

```bash
export BAO_NAMESPACE=agent

bao policy write namespace-admin namespace-admin.hcl

bao policy read namespace-admin
```

Using the root management token, issue a token in `agent`:

```bash
bao token create \
  -orphan \
  -policy=namespace-admin \
  -ttl=24h
```

The new token is scoped to `agent/` and its permitted descendants, not all of OpenBao.

| Operation | Agent namespace administrator |
| --- | --- |
| Manage `agent/secret/` | Allowed |
| Manage `agent/pki/` | Allowed |
| Manage policies in `agent/` | Allowed |
| Manage auth methods in `agent/` | Allowed |
| Manage sibling namespace `k8s/` | Not allowed |
| Manage root-level global settings | Not allowed |

A child-namespace administrator cannot cross its namespace boundary. Some global `sys/` APIs explicitly prohibit child-namespace access even with `sudo`.

**Setting `BAO_NAMESPACE=agent` does not restrict an existing root token.** Isolation requires an administrative identity and token belonging to `agent`.

The following KV policies assume a KV v2 engine already exists in `agent`:

```bash
export BAO_NAMESPACE=agent

bao secrets enable -path=secret kv-v2
```

## 4. KV read/write policy

File: `kv-readwrite.hcl`

```hcl
# KV v2 data operations and ordinary deletion
path "secret/data/*" {
  capabilities = [
    "create",
    "read",
    "update",
    "patch",
    "delete"
  ]
}

# List the KV root
path "secret/metadata" {
  capabilities = ["list"]
}

# List child paths
path "secret/metadata/*" {
  capabilities = ["list"]
}
```

This policy permits KV reads, writes, and soft deletion under `secret/`. It does not authorize policy changes, engine creation, or PKI operations.

It also does not authorize permanent version destruction or metadata deletion. Those require additional capabilities on `secret/destroy/*` or `secret/metadata/*`.

Create it:

```bash
bao policy write kv-readwrite kv-readwrite.hcl
```

Example operations:

```bash
bao kv put -mount=secret erp/database password=test
bao kv get -mount=secret erp/database
bao kv list -mount=secret erp/
bao kv delete -mount=secret erp/database
```

## 5. KV read-only policy

File: `kv-readonly.hcl`

```hcl
# Read all secret data in this mount
path "secret/data/*" {
  capabilities = ["read"]
}

# List the root
path "secret/metadata" {
  capabilities = ["list"]
}

# List child paths
path "secret/metadata/*" {
  capabilities = ["list"]
}
```

Create it:

```bash
bao policy write kv-readonly kv-readonly.hcl
```

Result:

| Operation | Permission |
| --- | --- |
| `bao kv get` | Allowed |
| `bao kv list` | Allowed |
| `bao kv put` | Not allowed |
| `bao kv patch` | Not allowed |
| `bao kv delete` | Not allowed |

The policy grants `list`, not `scan`; it does not authorize recursive scanning.

## 6. Path-scoped read-only policies

Workloads can be limited to one secret, a subtree, several separate paths, or a subtree with explicit exceptions.

### 6.1 Read one secret

For example, grant access only to ERP database credentials:

```hcl
path "secret/data/erp/database" {
  capabilities = ["read"]
}
```

This grants the one secret, not `erp/redis` or `github/token`.

### 6.2 Read a subtree

Grant access to secrets beneath ERP:

```hcl
path "secret/data/erp/*" {
  capabilities = ["read"]
}

path "secret/metadata/erp" {
  capabilities = ["list"]
}

path "secret/metadata/erp/*" {
  capabilities = ["list"]
}
```

The token can read secrets at any depth below ERP and list its keys, without accessing other projects.

### 6.3 Read several selected paths

For example, permit two API tokens:

```hcl
path "secret/data/github/token" {
  capabilities = ["read"]
}

path "secret/data/cloudflare/api-token" {
  capabilities = ["read"]
}
```

One policy can authorize separate paths; separate engines are unnecessary.

### 6.4 Permit a subtree with exceptions

Grant ERP reads except beneath `private/`:

```hcl
path "secret/data/erp/*" {
  capabilities = ["read"]
}

path "secret/data/erp/private/*" {
  capabilities = ["deny"]
}
```

`deny` is a supported capability and takes precedence over allowed capabilities for the applicable rule.

This handles exceptions. The example grants data reads, not key listing.

**Listing does not automatically hide names that the token cannot read.** A separately granted list on `erp/` may reveal `private/`. Prefer explicit allowlists for strict path restrictions; do not assume data-read ACLs filter list responses.

## 7. Create policies and tokens

Assume the current token is root and the KV policies belong to `agent`:

```bash
export BAO_NAMESPACE=agent

# Create policies
bao policy write kv-readwrite kv-readwrite.hcl
bao policy write kv-readonly kv-readonly.hcl
bao policy write kv-scoped-readonly kv-scoped-readonly.hcl

# Inspect them
bao policy list
```

Issue three kinds of token:

```bash
# Read/write
bao token create \
  -orphan \
  -policy=kv-readwrite \
  -ttl=24h

# Read-only for the mount
bao token create \
  -orphan \
  -policy=kv-readonly \
  -ttl=24h

# Read-only for selected paths
bao token create \
  -orphan \
  -policy=kv-scoped-readonly \
  -ttl=24h
```

### Token issuance restrictions

A root token, or a token with `sudo` on the token-creation endpoint, can issue
namespace-local policies it does not itself carry. The broad administrator
policies above include that capability on `auth/token/create`.

Without that privilege, ordinary child-token creation restricts the requested
policies to a subset of the parent's policies. An orphan request also needs the
appropriate elevated permission. Token roles and auth methods let administrators
define narrower issuance rules for delegated callers. These permissions do not
turn an ordinary token into the special built-in `root` token.

See the [Token API](https://openbao.org/docs/api/auth/token/) and the ready-to-use
[agent policy templates](../../scripts/bao/README.md#policies-for-the-agent-namespace).

## 8. Verify effective permissions

After obtaining the read-only token, set:

```bash
export BAO_TOKEN="the-issued-readonly-token"
export BAO_NAMESPACE=agent
```

Check capabilities:

```bash
# Expect read
bao token capabilities secret/data/erp/database

# Expect deny if outside the granted paths
bao token capabilities secret/data/github/token

# Expect list if this metadata path is granted
bao token capabilities secret/metadata/erp
```

`bao token capabilities` inspects effective capabilities on an API path without actually writing or deleting data.

The CLI can also show the policy required by an operation:

```bash
bao kv put -output-policy \
  -mount=secret erp/database \
  password=test
```

This prints required policy rules without executing the write. It is useful when adding PKI, SSH, or Transit access instead of guessing API permissions.

## 9. Policies and namespace layout

```text
Root Namespace
│
├── super-admin
│   └── OpenBao-wide administrative identity
│
├── agent/ Namespace
│   ├── namespace-admin
│   └── secret/  (KV v2)
│       ├── kv-readwrite
│       ├── kv-readonly
│       └── kv-scoped-readonly
│
└── k8s/ Namespace
    ├── namespace-admin
    ├── secret/  (KV v2)
    └── pki/     (PKI)
```

Policies belong to their namespaces; they are not child objects stored beneath engines. The diagram illustrates which resources each policy controls.

### Three practical rules

1. **Namespaces define administrative scope; policies grant operations on API paths.**
2. **KV v2 policies must use native segments such as `data/` and `metadata/`.**
3. **`path "*"` with `sudo` grants broad administration and privileged token issuance within the namespace, while the built-in `root` policy remains special.**

These examples follow the documented OpenBao ACL syntax. Their original verification covered documentation; **these exact policy examples were not applied or exercised against a live server**.
