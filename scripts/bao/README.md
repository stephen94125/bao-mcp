# OpenBao Administration Scripts

For a first Bao MCP setup, use
[`scripts/setup-bao-environment.sh`](../setup-bao-environment.sh). It prepares the
`agent` namespace, installs its admin policy, and saves an admin token to a private
file. See the [getting-started guide](../../README.md) for the complete flow.

The scripts in this directory provide individual administration steps. They call
the Bao CLI directly and require Bash and `bao`; `issue-token.sh` and
`initial-namespace-default-engine.sh` also require [`gum`](https://github.com/charmbracelet/gum).
Set `BAO_ADDR` to your own initialized, unsealed Bao server before running them.
For HTTPS with a private CA, also set `BAO_CACERT`.

These individual scripts use the CLI's current credentials (`BAO_TOKEN` or
`bao login`). Each asks for a namespace: enter `agent` for the examples below;
blank selects root, overriding an inherited namespace. The top-level setup script
specifically uses `BAO_TOKEN` or asks for a setup token with hidden input.

## Policies for the agent namespace

Install the templates in [`policies/`](policies/) **inside `agent`**:

| File | Scope |
| --- | --- |
| [`admin.hcl`](policies/admin.hcl) | Manage `agent` and its descendants, including engines, policies, and tokens |
| [`vault-readwrite.hcl`](policies/vault-readwrite.hcl) | Full read/write access to vault `demo`: KV at `demo/` and SSH at `ssh-ca-demo/` |
| [`vault-readonly.hcl`](policies/vault-readonly.hcl) | Read/list access to the same vault's KV and SSH paths |

Paths in an agent-local policy are relative to `agent/`: `path "*"` grants that
namespace's administrative scope. The effective API scope is `agent/*`.
Adding `agent/` again inside these policies would target a different path.

For another vault, copy the appropriate template and replace `demo` and
`ssh-ca-demo` with its mount names. Read/write includes permanent KV version
deletion and management of that vault's SSH CA and roles. Read-only permits
inspection of existing resources; SSH certificate signing is a write operation.
Both vault templates include token self-management and MCP discovery permissions.

Apply the templates while authenticated as the namespace administrator:

```bash
export BAO_NAMESPACE=agent
bao policy write -namespace=agent admin scripts/bao/policies/admin.hcl
bao policy write -namespace=agent vault-demo-readwrite scripts/bao/policies/vault-readwrite.hcl
bao policy write -namespace=agent vault-demo-readonly scripts/bao/policies/vault-readonly.hcl
```

`bao policy write` updates a policy with the same name. Review the chosen template
and namespace before applying. Create the vault through MCP's `vault_create` tool
using `vault_id: "demo"`, then issue a token with the desired policy:

```bash
bao token create -namespace=agent -orphan -policy=vault-demo-readwrite -ttl=8h -explicit-max-ttl=8h
bao token create -namespace=agent -orphan -policy=vault-demo-readonly -ttl=8h -explicit-max-ttl=8h
```

These commands display the new credentials in the terminal. Put each credential
in its client's secret configuration. The `admin` policy has `sudo` on token
creation, allowing it to issue namespace-local policies it does not itself carry.
An orphan token has its own lifetime, independent of the issuing token; server
limits can shorten the requested TTL.

### PKI permissions

`_sys_pki_root` and `_sys_pki_int` are shared by vaults in the namespace.
Grant access to the required PKI role separately after an administrator configures
that role's domain, TTL, and key restrictions. For example, append these rules to
a read/write policy to issue or sign through the `demo` role:

```hcl
path "_sys_pki_int/issue/demo" { capabilities = ["update"] }
path "_sys_pki_int/sign/demo" { capabilities = ["update"] }
```

This grants certificate issuance through that role. Shared CA administration is
reserved for administrators. Certificate issuance requires a write capability;
add it only to clients that should issue certificates. See
[SSH and PKI setup examples](../../docs/tools.md).

## Individual scripts

| Script | Purpose |
| --- | --- |
| `apply-policies.sh <policy_name> <relative-hcl-path>` | Upload one policy in the selected namespace |
| `create-admin-user.sh` | Enable userpass if needed and create an administrator account |
| `issue-token.sh` | Select policies and issue a token with a chosen display name and TTL |
| `create-namespace.sh` | Create one child under the selected parent namespace |
| `initial-namespace-default-engine.sh` | Select unused mount paths and enable engines |

For the policy helper, HCL paths are relative to `scripts/bao/`:

```bash
./scripts/bao/apply-policies.sh vault-demo-readonly policies/vault-readonly.hcl
```

Enter `agent` at its namespace prompt. Existing accounts and namespaces are
preserved; the policy helper updates a policy with the same name.

### Administrative account

After installing `admin` in `agent`, run:

```bash
./scripts/bao/create-admin-user.sh
```

Select `agent`, enter a username and a hidden password. The password is passed to
Bao on standard input. The account uses the namespace's `admin` policy and server
token defaults. Log in to that account with:

```bash
unset BAO_TOKEN
bao login -namespace=agent -method=userpass username=your-username
```

Enter its password when prompted. A reusable login lets you obtain new tokens
when existing ones expire. Bao recommends using the initial root token for
[setup or emergency maintenance](https://openbao.org/docs/concepts/tokens/#root-tokens).
Once the administrative login works, revoke the initial root token from its owning
root namespace. Ensure you select that token before using `bao token revoke -self`.

If administrative access is lost, use
[`operator generate-root`](https://openbao.org/docs/commands/operator/generate-root/)
with the required quorum, restore access, then revoke the recovery token.

### Interactive token issuance

`issue-token.sh` asks for namespace, existing policies, display name, and TTL.
In gum, move with Up/Down, toggle with Space, and confirm with Enter. The policy
selection checks policies on Bao rather than matching local HCL filenames.

The default TTL is `365d`; a bare number means days. Examples include `30m`, `12h`,
`7d`, and `1h30m`. Input must be positive and at most `1825d`. The script requests
an orphan token, keeps Bao's normal `default` policy behavior, and prints the
result without creating a token file. Check the returned actual TTL and warnings:
[server TTL limits still apply](https://openbao.org/docs/concepts/tokens/#token-time-to-live-periodic-tokens-and-explicit-max-ttls).

### Interactive engine allocation

`initial-namespace-default-engine.sh` offers unused paths for KV v2 (`secret/`),
PKI (`pki/`), SSH (`ssh/`), Transit, database, Kubernetes, TOTP, and RabbitMQ.
Occupied paths are excluded regardless of engine type. Cancellation or empty
selection makes no changes; a failed mount query stops the script.

Use MCP's `vault_create` for Bao MCP's grouped vault naming (`demo/`,
`ssh-ca-demo/`, and shared PKI). The interactive script is for separately chosen
mounts. After enabling an engine, configure its CA, roles, or backend connection
as appropriate. LDAP requires a compatible external plugin.
