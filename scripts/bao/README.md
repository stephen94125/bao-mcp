# OpenBao Administration Scripts

Each script is standalone Bash and calls `bao` directly. Bash and Bao CLI are required; token issuance and engine allocation also require `gum`.

Initialize and unseal Bao, then log in with an authorized administrator identity. `BAO_ADDR` defaults to `https://secret.r5vj.xyz`. Every script asks for a namespace; blank selects root, overriding an inherited namespace setting.

## Policy

The [policies/](policies/) directory contains three HCL policies:

- `admin.hcl`: administrative access to the selected namespace and descendants, not the built-in `root` policy.
- `kv-readwrite.hcl`: KV v2 data reads, writes, soft deletion, and listing across single-segment mount names in that namespace.
- `kv-readonly.hcl`: KV v2 data reads and listing across those mount names.

The KV policies also grant token self-inspection, renewal, revocation, and self-capability lookup. They do not grant engine/policy/auth administration or permanent KV version destruction.

## Usage

| Script | Purpose |
| --- | --- |
| `./scripts/bao/apply-policies.sh <policy_name> <relative-hcl-path>` | Upload one policy in the selected namespace |
| `./scripts/bao/create-admin-user.sh` | Enable userpass if necessary and create an administrator with an interactively supplied password |
| `./scripts/bao/issue-token.sh` | Select namespace/policies with gum, enter display name and TTL, and print the issued token |
| `./scripts/bao/create-namespace.sh` | Create one child beneath the selected parent namespace |
| `./scripts/bao/initial-namespace-default-engine.sh` | Select unused mount names with gum and enable their engines |

Create the namespace if needed, apply policies, then create an account or issue a token. Existing accounts/namespaces are not overwritten; `apply-policies.sh` updates an existing policy with the same name.

HCL paths are relative to `apply-policies.sh`, for example:

```bash
./scripts/bao/apply-policies.sh admin policies/admin.hcl
```

Enable engines:

```bash
./scripts/bao/initial-namespace-default-engine.sh
```

Enter the namespace or leave blank for root. The script uses `bao secrets list` to mark occupied candidate paths and exclude them regardless of engine type. Mounts in different namespaces are independent.

| Mount | Engine type |
| --- | --- |
| `secret/` | `kv-v2` |
| `pki/` | `pki` |
| `ssh/` | `ssh` |
| `transit/` | `transit` |
| `database/` | `database` |
| `kubernetes/` | `kubernetes` |
| `totp/` | `totp` |
| `rabbitmq/` | `rabbitmq` |

In gum, use Up/Down to move, Space to toggle selection, and Enter to submit. Cancellation, empty selection, or all paths occupied causes no writes; a failed query stops execution. Log in with rights to administer the selected namespace first.

This enables mounts only, without CA keys, roles, or backend connections. Configure those separately. LDAP requires an external plugin and is not offered in this list.

`issue-token.sh` uses the CLI's current login state without requiring or changing `BAO_TOKEN`. Its interaction is:

1. Enter the namespace; blank selects root.
2. Select one or more existing policies with Up/Down, Space, and Enter. This checks policy existence, not correspondence with local HCL files.
3. Enter a token display name.
4. Enter TTL, default `365` (blank also means `365d`). Bare numbers gain a `d` suffix. Supported units include `30s`, `30m`, `12h`, `7d`, or `1h30m`; not years `y` or months `M`. Duration must be positive and at most `1825d`; invalid input is rejected.

The script requests an orphan token while preserving Bao's normal `default` policy behavior. The requested TTL remains subject to server limits. Native results/warnings show the actual issued TTL. See [Token TTL](https://openbao.org/docs/concepts/tokens/#token-time-to-live-periodic-tokens-and-explicit-max-ttls).

Administrator passwords are entered at a hidden prompt and passed to `bao write` on standard input, without writing a file. The account belongs to the selected namespace and uses its `admin` policy; token settings use server defaults.

Tokens print directly to the terminal; no credential directory or file is created.

After creating the account, log in through userpass:

```bash
unset BAO_TOKEN
bao login -namespace="" -method=userpass username="your-username"
```

Enter the password when prompted. For another namespace, select the namespace where the account was created.

[Bao recommends](https://openbao.org/docs/concepts/tokens/#root-tokens) using the initial root token only for initial setup or emergency maintenance. After verifying a reusable administrative login, revoke the initial root token rather than retaining it for daily operations.

If administrative credentials are lost, follow [operator generate-root](https://openbao.org/docs/commands/operator/generate-root/) with enough unseal shares to recover a root token, repair access, and revoke the recovery token afterward.

To revoke the CLI's current token, target its owning namespace. Before revoking the initial root token, ensure it is the token currently in use:

```bash
bao token revoke -self
```

Log in again after revocation. Add `-namespace="your-namespace"` when selecting another namespace.
