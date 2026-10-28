# OpenBao Path Notation

[简体中文](path-notation.zh-CN.md)

Paths locate resources across namespaces, mounts, and ACL policies. Although they all resemble slash-separated strings, their segments have **different meanings**: a namespace prefix, an engine mount, and engine-specific API or resource segments.

This guide covers implicit namespace prefixes, root and child ACL scopes, `*` and `+`, cross-namespace access, and common misconceptions. All examples use OpenBao ACL policies in HCL.

## 1. Namespace, mount, and engine API paths

Consider two KV v2 secret paths:

```text
secret/data/erp/database
│      │    └───────── Engine resource: erp/database
│      └────────────── KV v2 data API segment
└───────────────────── Mount named secret/ in the root namespace

agent/secret/data/erp/database
│     │      │    └── Engine resource: erp/database
│     │      └─────── KV v2 data API
│     └────────────── Mount named secret/ inside agent
└──────────────────── Namespace：agent/
```

| Layer | Examples | Purpose |
| --- | --- | --- |
| Namespace | Root (no extra `root/` prefix), `agent/`, `k8s/` | Administrative and identity scope |
| Engine mount | `secret/`, `credentials/`, `pki/` | Route requests to an engine |
| Engine API segment | KV v2 `data/`, `metadata/` | Select operations such as data reads or key listing |
| Resource path | `erp/database` | Identify the engine resource |

`secret` is a chosen mount name, not a reserved keyword. Mounting at `credentials` changes the API prefix to `credentials/data/...`.

PKI, SSH, and Transit use their own suffixes; KV v2's `data/` and `metadata/` convention does not apply to every engine.

**A namespace is an administrative scope, not an ordinary folder.** It owns engines, auth methods, policies, tokens, and identity resources; only its textual representation resembles another path segment.

### Select the request namespace by path or header

These requests can target the same secret in `agent`, assuming the namespace, mount, and resource exist:

```text
GET /v1/agent/secret/data/erp/database

GET /v1/secret/data/erp/database
X-Vault-Namespace: agent

CLI：BAO_NAMESPACE=agent bao kv get -mount=secret erp/database
```

`BAO_NAMESPACE` and `X-Vault-Namespace` select the **target namespace**. They grant no additional rights; ACL evaluation still follows routing.

## 2. Policy paths are relative to their namespace

A policy's paths are **relative to its owning namespace**. This describes internal parsing and matching, not a literal rewrite of the HCL source.

For the same HCL:

```hcl
path "secret/data/*" {
  capabilities = ["read"]
}
```

The effective prefix depends on its namespace:

| Policy namespace | Effective ACL match |
| --- | --- |
| Root | `secret/data/*` |
| `agent/` | `agent/secret/data/*` |
| `agent/dev/` | `agent/dev/secret/data/*` |

Thus, in `agent`:

```hcl
path "*" {
  capabilities = ["create", "read", "update", "patch", "delete", "list", "scan", "sudo"]
}
```

The rule grants matching paths beneath `agent/`, including descendants. It does not grant root or sibling `k8s/` access.

**Do not add `agent/*` inside an `agent` policy merely to name its own namespace.** That means another `agent/` segment relative to `agent/`.

Internally, Bao combines `Namespace.Path + Policy.Path` while parsing ACL rules, and combines the request namespace and path during matching.

## 3. `*` and `+` have different meanings

ACL paths are neither regular expressions nor general shell globs. In particular, `*` **belongs only at the end of a policy path**.

| Pattern | Meaning | Example |
| --- | --- | --- |
| `path "*"` | Any remaining suffix from the policy's namespace | At root, can cover root and child namespaces |
| `path "secret/*"` | Any depth after `secret/` | `secret/data/a/b` |
| `path "secret/data/*"` | Fixed prefix with an arbitrary-depth suffix | `secret/data/erp/db`, `secret/data/erp/prod/db` |
| `path "secret/+/password"` | Exactly one complete middle segment | Matches `secret/erp/password`, not `secret/erp/prod/password` |
| `path "+/data/*"` | One top-level segment, then `data/`, then any suffix | `secret/data/a`, `credentials/data/a` |
| `path "+/+/fffff/*"` | Two segments before `fffff/`, then any suffix | `a/b/fffff/x/y` |
| `path "*/fffff/*"` | **Invalid**: a middle `*` | Does not express arbitrary-depth matching |

### 3.1 `+` matches one complete segment

```hcl
path "+/data/*" {
  capabilities = ["read"]
}
```

Its components are:

```text
+             data/           *
One segment    Fixed API segment   Any remaining suffix
```

It matches:

```text
secret/data/erp/database
credentials/data/github/token
projects/data/a/b/c
```

It does not match:

```text
team/secret/data/erp/database
```

`+` cannot consume the two segments `team/secret`.

`+/+/data/*` matches exactly two leading segments, not arbitrary depth.

### 3.2 A trailing `*` matches any remaining suffix

```hcl
path "secret/data/*" {
  capabilities = ["read"]
}
```

After the fixed prefix `secret/data/`, the trailing `*` covers paths such as `erp/database` or `erp/prod/database` at any depth.

`*` is not limited to one segment. It matches **any string after the prefix**, so include the intended boundary:

```text
secret/data/*    → Keeps the fixed prefix secret/data/
secret/data*     → May also match paths starting with secret/database...
```

Include fixed `/` separators to preserve the intended boundary.

### 3.3 `*/fffff/*` is not a valid arbitrary-depth expression

OpenBao has no direct ACL syntax meaning "find `fffff/` after any number of preceding segments, then match everything below it" using `*/fffff/*`.

Use `+/fffff/*` for one preceding segment or `+/+/fffff/*` for two. If depth varies, redesign the mount/path layout or enumerate the needed depths. `path "*"` covers all paths but loses the restriction to `fffff/`.

## 4. Root `path "*"` can cover child namespaces

A root-level wildcard prefix is not limited to resources directly in root.

**A policy created in root with `path "*"` can match API paths in root and child namespaces such as `agent/` and `k8s/`.**

For example, create this in root:

```hcl
path "*" {
  capabilities = [
    "create", "read", "update", "patch",
    "delete", "list", "scan", "sudo"
  ]
}
```

This ordinary ACL policy grants broad cross-namespace access to corresponding APIs, including namespace, engine, and policy administration. It remains distinct from the built-in `root` policy and retains internal administrative and token-issuance restrictions.

It is incorrect to assume root `path "*"` requires extra `agent/*` or `k8s/*` rules solely to reach those namespaces.

Consider these three rules:

```hcl
path "*" {
  capabilities = ["create", "read", "update", "patch", "delete", "list", "scan", "sudo"]
}
path "agent/*" {
  capabilities = ["create", "read", "update", "patch", "delete", "list", "scan", "sudo"]
}
path "k8s/*" {
  capabilities = ["create", "read", "update", "patch", "delete", "list", "scan", "sudo"]
}
```

With the broad wildcard rule already present, adding the other two merely for descendant coverage is redundant. Without the broad rule, root policies can use `agent/*` to grant access selectively.

Similarly, `path "*"` within `agent/` can cover `agent/dev/`; an extra `dev/*` is not required solely for descendant coverage.

## 5. Administrative access versus KV access

| Policy namespace | HCL | Root | `agent/` | `k8s/` |
| --- | --- | --- | --- | --- |
| Root | `path "*"` + all capabilities | Broad administration | Broad administration | Broad administration |
| `agent/` | `path "*"` + all capabilities | No grant | Broad administration | No grant |
| Root | `path "*"` + `read` | Read APIs | Read APIs | Read APIs |
| Root | `path "secret/data/*"` + `read` | Matching root KV data | No grant | No grant |
| Root | `path "agent/secret/data/*"` + `read` | No grant | Matching agent KV data | No grant |
| `agent/` | `path "secret/data/*"` + `read` | No grant | Matching agent KV data | No grant |

### 5.1 Create an administrator in `agent`

```bash
export BAO_NAMESPACE=agent
bao policy write namespace-admin namespace-admin.hcl
```

```hcl
# namespace-admin.hcl
path "*" {
  capabilities = [
    "create", "read", "update", "patch",
    "delete", "list", "scan", "sudo"
  ]
}
```

The policy starts at `agent/`. It may allow creating descendants such as `agent/dev` or `agent/prod`; changing `BAO_NAMESPACE` cannot grant root or sibling access.

### 5.2 Root KV policies do not automatically cross namespaces

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

Created in root, this grants the root `secret/` KV mount, not `agent/secret/` or `k8s/secret/`.

To grant agent KV access from a root policy:

```hcl
path "agent/secret/data/*" {
  capabilities = ["create", "read", "update", "patch", "delete"]
}
path "agent/secret/metadata" {
  capabilities = ["list"]
}
path "agent/secret/metadata/*" {
  capabilities = ["list"]
}
```

That grants neither engine/policy administration in agent nor KV access in k8s.

### 5.3 What does root `path "*"` with `read` allow?

```hcl
path "*" {
  capabilities = ["read"]
}
```

At the ACL level, it permits matching read APIs in root and descendants.

Distinguish these properties:

1. **`read` does not imply `list` or `scan`.** Knowing a resource path may permit a read without key enumeration.
2. **`read` is not limited to retrieving existing secret values.** It can cover `sys/` and `auth/` reads; some dynamic-engine reads create credentials.
3. **`read` does not imply `sudo`.** APIs requiring it or imposing namespace restrictions remain protected.
4. A read capability does not guarantee that every resource has a readable GET endpoint; engine behavior and other conditions still apply.

Therefore, root `path "*"` with `read` is broad cross-namespace API access, not a narrowly scoped "all secrets read-only" workload role.

## 6. Wildcards match paths, not engine types

For example:

```hcl
# Match a data API beneath any one top-level segment
path "+/data/*" {
  capabilities = ["create", "read", "update", "patch", "delete"]
}
path "+/metadata" {
  capabilities = ["list"]
}
path "+/metadata/*" {
  capabilities = ["list"]
}
```

The rule can cover top-level KV mounts named `secret/`, `credentials/`, or `projects/` because their paths match, not because Bao identifies them as KV v2.

Other engines or routing arrangements with the same path shape may also match. A nested mount such as `team/credentials/` is not covered by one `+`.

**Namespace administration and cross-mount secret access are different designs.** Removing `sudo` from `path "*"` still permits many administrative `sys/` and `auth/` operations.

There is no universal ACL keyword for "business operations on every engine but no administrative APIs". Use each engine's actual paths and capabilities.

## 7. Inspect policy paths and capabilities

`BAO_NAMESPACE` changes the request target, not the token's authorization scope.

Inspect the current token:

```bash
bao token lookup
```

From root, inspect different paths using the token being tested:

```bash
unset BAO_NAMESPACE
bao token capabilities secret/data/test
bao token capabilities agent/secret/data/test
bao token capabilities k8s/secret/data/test
```

Or select a namespace and query local paths:

```bash
export BAO_NAMESPACE=agent
bao token capabilities secret/data/test
```

Capability inspection evaluates ACL rules without requiring the secret to exist. Actual execution also depends on mounts, resources, and other validation conditions.

If KV mounts and test data exist, test the corresponding operations:

```bash
# Root Namespace
unset BAO_NAMESPACE
bao kv get -mount=secret erp/database

# agent Namespace
bao kv get -ns=agent -mount=secret erp/database

# k8s Namespace
bao kv get -ns=k8s -mount=secret erp/database
```

`-ns` only changes the target namespace; it does not add token permissions.

## 8. Common misconceptions

1. **Root `path "*"` can cover child namespaces.** Additional child prefixes are not required just to extend its wildcard scope.
2. **Child `path "*"` can cover descendants.** In `agent`, it can match `agent/dev` without another `dev/*` rule.
3. **`path "secret/data/*"` and `path "*"` differ.** One fixes an API/mount prefix; the other matches all remaining paths.
4. **`+` matches one segment; a trailing `*` matches an arbitrary-depth suffix.** A middle `*` such as `*/fffff/*` is invalid.
5. **Full ordinary ACL capabilities still do not equal a root token.** Treat `sudo`, namespace-specific restrictions, and issuance rules separately.

**The namespace sets the ACL starting point, the mount selects the engine, engine APIs define subsequent segments, and policy wildcards and capabilities authorize the resolved path.**

---

## Official references and implementation

1. [Policies: paths, globs, `+`, capabilities, and precedence](https://openbao.org/docs/concepts/policies/)
2. [Namespaces: hierarchy, headers, and scope](https://openbao.org/docs/concepts/namespaces/)
3. [Policy parsing: combining namespace and policy paths](https://github.com/openbao/openbao/blob/main/internal/vault/policy/policy.go#L581-L605)
4. [ACL request matching](https://github.com/openbao/openbao/blob/main/internal/vault/policy/acl.go)

*Checked on 2026-10-28 against documentation and source. This is not a report of live authorization tests against the user's Bao server.*
