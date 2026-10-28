# OpenBao Authentication: Userpass, AppRole, Kubernetes, and Token Lifecycles

[简体中文](auth-methods.zh-CN.md)

Three concepts work together:

- **Auth method**: how a user or workload proves its identity.
- **Policy**: which operations that identity may perform on API paths.
- **Token**: the credential Bao issues after authentication and evaluates on later requests.

An auth method authenticates identities and issues tokens; it is not another encryption mechanism or a secrets engine. Engines store or generate secrets.

## 1. How authentication and authorization connect

```text
User / service / Kubernetes Pod
         │
         ▼
Auth Method
  ├── Userpass: username + password
  ├── AppRole：RoleID + SecretID
  └── Kubernetes：ServiceAccount JWT
         │
         ▼
Identity authenticated
         │
         ▼
OpenBao issues a token with policies
         │
         ▼
Subsequent API requests carry that token
         │
         ▼
Policies authorize the operation
         │
         ▼
Secrets engine: read or modify a resource
```

**The auth method determines how to log in, policies determine permitted actions, and the resulting token carries the operating identity.**

## 2. Three common auth methods

| Auth method | Login credentials | Typical caller |
| --- | --- | --- |
| Userpass | Username + password | Human administrator using a CLI |
| AppRole | RoleID + SecretID | Agents and services on VMs or systemd |
| Kubernetes | ServiceAccount JWT | Workloads running in Kubernetes |

### Userpass: interactive account login

An authorized administrator enables userpass, creates an account, and attaches policies. The user can then log in repeatedly to obtain new tokens.

### AppRole: workload login

AppRole typically uses:

```text
RoleID   = Identifies the workload's role
SecretID = Secret proving permission to use that role
```

Example, assuming an AppRole and valid RoleID/SecretID have already been provisioned:

```bash
bao write auth/approle/login \
  role_id="..." \
  secret_id="..."
```

On successful authentication, Bao returns a policy-bearing token. After it expires, the workload can authenticate again if its RoleID/SecretID remain valid and satisfy TTL and usage limits.

An **AppRole role configures login conditions and policy attachment**; it is not a second policy language.

Reference: [AppRole auth method](https://openbao.org/docs/auth/approle/).

### Kubernetes: use the Pod's identity

Kubernetes workloads can authenticate with their ServiceAccount JWT.

```text
K8s Pod（erp-backend）
         │
         ▼
ServiceAccount JWT
         │
         ▼
OpenBao Kubernetes Auth
         │
         ▼
Validate through Kubernetes TokenReview
         │
         ▼
Match the configured role's ServiceAccount and policy bindings
         │
         ▼
Issue an ERP-scoped OpenBao token
```

This avoids preloading every Pod with a long-lived Bao token or a separate AppRole SecretID.

Reference: [Kubernetes auth method](https://openbao.org/docs/auth/kubernetes/).

## 3. Set up a userpass administrator

One short-lived admin token is not a durable login mechanism. When it expires, the administrator still needs a way to authenticate for a replacement.

**Userpass provides a reusable administrative identity**, avoiding repeated manual token issuance with a root token.

### Prerequisites

- Bao is initialized and unsealed.
- The CLI has `BAO_ADDR` configured.
- The current token may manage auth methods and userpass accounts, such as root during initial setup.
- A `super-admin` policy already exists.

### Step 1: Enable userpass

```bash
bao auth enable userpass
```

Its default mount is `auth/userpass/`.

### Step 2: Create an administrator account

```bash
# Illustrative test password only; keep real passwords out of shell history
bao write auth/userpass/users/mac-admin \
  password='test-only-strong-password' \
  policies=super-admin
```

This creates `mac-admin` and attaches `super-admin` to that login identity.

### Step 3: Log in

```bash
# Select the server
export BAO_ADDR="https://bao.example.com"

# Remove a prior BAO_TOKEN so later CLI calls use the newly stored token
unset BAO_TOKEN

bao login -method=userpass username=mac-admin
```

Enter the password when prompted. Bao issues a new token, and the default CLI helper stores it for subsequent commands.

### Step 4: Verify the identity

```bash
bao token lookup
bao policy list
bao secrets list
bao auth list
```

Check that the token carries the expected policy and the required management operations succeed.

### Step 5: Log in again later

Userpass tokens still have TTL and maximum-TTL constraints. Token expiry does not delete the account.

```bash
bao login -method=userpass username=mac-admin
```

As long as the method and account remain valid and authentication succeeds, the user can obtain another token. This provides continued access after the only current admin token expires.

Before revoking the initial root token in production, verify this login mechanism independently and confirm that required management operations work.

Reference: [Userpass auth method](https://openbao.org/docs/auth/userpass/).

## 4. Why not only use `bao token create`?

Direct token issuance is supported:

```bash
bao token create \
  -policy=erp-readonly \
  -ttl=24h
```

That token is issued by an authorized administrator rather than obtained by authenticating through another method.

| Method | After token expiry |
| --- | --- |
| Manual `bao token create` | An authorized administrator issues another token |
| Userpass | Log in with username/password |
| AppRole | Log in with valid RoleID/SecretID |
| Kubernetes auth | Log in with a valid ServiceAccount JWT |

**Auth methods let people and workloads repeatedly prove identity and obtain short-lived tokens**, instead of relying indefinitely on one fixed token or requesting root issuance each time.

## 5. Choose authentication by caller

| Caller | Suggested method |
| --- | --- |
| Human administrator on a Mac | Userpass |
| Agent or service on a VM/systemd | AppRole |
| Kubernetes workload | Kubernetes auth |
| Initial setup or emergency repair | Root token, only when necessary |

Kubernetes workloads can use Kubernetes auth directly. For Bao MCP, the caller acquires its native Bao token through an appropriate method and passes it to the gateway; the gateway does not perform AppRole or Kubernetes login on the caller's behalf.

## 6. Inspect and enable auth methods

List enabled methods:

```bash
bao auth list
```

A new Bao installation normally includes:

```text
Path       Type
----       ----
token/     token
```

Token is itself a built-in auth method. Enable other types explicitly:

```bash
# Human username/password login
bao auth enable userpass

# VM or systemd workload identity
bao auth enable approle

# Kubernetes workload identity
bao auth enable kubernetes
```

**Enable methods when needed, rather than enabling everything upfront.** Auth methods live under `auth/` and are managed with `bao auth list`; engines are managed with `bao secrets list`.

Reference: [Auth CLI](https://openbao.org/docs/commands/auth/).

## 7. Distinguish the components

- **Secrets engine**: manages secrets.
- **Policy**: defines API access rules.
- **Auth method**: validates login identity.
- **Token**: credential used for Bao operations after authentication.
- **Unseal key**: unseals Bao itself; it is not a workload or human login token.

Manual tokens suit tests and direct issuance. For continued access, use Userpass, AppRole, or Kubernetes auth as appropriate so callers can authenticate again after a token expires.
