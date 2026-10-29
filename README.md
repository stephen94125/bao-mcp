> [!WARNING]
> You'd better have at least some understanding of how this system and OpenBao work, including their security configuration. Otherwise, you might as well just use `.env`; there's no need to make things so complicated.

# Bao MCP

[简体中文](README.zh-CN.md)

Bao MCP turns OpenBao into MCP tools for AI agents. Agents can read and write
secrets, request SSH and TLS certificates, and use Transit encryption and signing.
Each tool call uses the agent's OpenBao token; OpenBao applies its policies in the
selected namespace.

```text
Agent → MCP → Bao MCP → OpenBao / agent namespace
```

## Before you start

- OpenBao 2.7.x installed. For installation, see the [OpenBao documentation](https://openbao.org/docs/install/).
- Bash and Python 3 for the setup script.
- Rust 1.92+ and a C compiler to build Bao MCP.
- An MCP client supporting Streamable HTTP and a bearer-token header.

The steps below start a local Bao server, prepare the `agent` namespace, build
Bao MCP, and store a first secret. If Bao is already running, use its address and
skip starting another server.

Clone or download this repository and open a terminal at its root. With your
repository URL, for example:

```sh
git clone <repository-url> bao-mcp
cd bao-mcp
```

## 1. Start or connect to OpenBao

For a local trial, start Bao in one terminal and leave it running:

```sh
bao server -dev -dev-no-store-token -dev-listen-address=127.0.0.1:8200
```

[Development mode](https://openbao.org/docs/concepts/dev-server/) initializes and
unseals Bao automatically. It prints an initial root token for setup. This mode
stores data in memory and loses it on restart; use it for local experimentation.
For a persistent deployment, follow Bao's [server configuration](https://openbao.org/docs/configuration/).

In a second terminal:

```sh
export BAO_ADDR=http://127.0.0.1:8200
bao status
```

For an existing server, set `BAO_ADDR` to its HTTPS address instead. Set
`BAO_CACERT=/path/to/ca.pem` if it uses a private CA. Continue when `Initialized`
is `true` and `Sealed` is `false`. For a new persistent server, follow the
[initialization and unseal guide](docs/bao/cli-basics.md#3-status-and-first-initialization)
first; keep its unseal shares and initial root token secure.

## 2. Set up the `agent` namespace and its administrator token

From this repository's root:

```sh
./scripts/setup-bao-environment.sh
```

The script uses `BAO_TOKEN` when set. Otherwise it asks for a setup token with
hidden terminal input. For the local server, the root token printed in step 1
works. For an existing server, use a root-namespace administrator token authorized
to read/create the namespace, read/install its `admin` policy, and issue orphan
tokens (`sudo` on token creation). A suitably authorized `super-admin` token works;
Bao evaluates the actual API permissions. See the [required permissions](scripts/README.md#setup-permissions).
Run `unset BAO_TOKEN` if you prefer to enter the setup token at the hidden prompt.

The default requested lifetime is three years. Keep the credential secure. The
server-returned TTL is authoritative; extending the server limit requires changing
its `max_lease_ttl` setting.

The script:

1. Creates `agent` if needed and preserves an existing namespace.
2. Installs the namespace's `admin` policy from [admin.hcl](scripts/bao/policies/admin.hcl), or reuses an identical policy.
3. Issues an orphan service token carrying `admin` and `default`, with a requested lifetime of three years (`1095d`).
4. Prints the token and the actual server-issued TTL in red. Add `--token-file ./agent-admin.token` to save it with mode `0600` instead of printing it.

When `--token-file` is used, an existing token file stops setup. A different existing
`admin` policy also stops setup so you
can choose a new output file or inspect the policy. Other options:

```sh
./scripts/setup-bao-environment.sh \
  --namespace agent --ttl 8h --token-file ./agent-admin-8h.token
```

**The policy is stored inside `agent`.** Its `path "*"` covers `agent/` and its
descendants, effectively `/agent/*`. Writing `path "agent/*"` inside this policy
would add another `agent/` segment. The issued token belongs to `agent`; merely
changing a root token's namespace setting does not restrict that root token.

Use the generated namespace token for the MCP client, and keep token files private.
Its orphan status makes its lifetime independent of the token used for setup. Server TTL limits can shorten
the request, and the explicit maximum limits its total lifetime.

## 3. Build and start Bao MCP

From a source checkout, run:

```sh
cargo build --release --locked --bin bao-mcp
BAO_NAMESPACE=agent ./target/release/bao-mcp serve
```

The release artifact is the single executable `target/release/bao-mcp`. SQLite
and the TLS implementation are linked into it. For a fully static Linux build,
see [Linux MUSL builds](docs/configuration.md#static-linux-musl-builds).
macOS uses Apple's built-in system libraries; the
[macOS compiler command](docs/configuration.md#building-on-macos-with-nix) also
handles Nix environments that report `cc` or linker errors.

The service uses `BAO_ADDR` from step 1 and listens at
`http://127.0.0.1:8000/mcp`. Leave it running while your agent connects.

The server's address, namespace, listener, and cache can be configured through
environment variables. See [configuration and build troubleshooting](docs/configuration.md),
including the settings for remote HTTPS deployments.

## 4. Connect your MCP client

Add an HTTP MCP server with these settings:

| Setting | Value |
| --- | --- |
| Transport | Streamable HTTP |
| URL | `http://127.0.0.1:8000/mcp` |
| Authorization header | `Bearer <issued OpenBao token>` |

Use your client's bearer-token or secret-setting mechanism to supply the token;
the angle-bracket text above is a placeholder. Ask the client to list tools.
Available tools are discovered from that token's Bao permissions.

## 5. Create a vault and store a secret

Ask your agent to call these MCP tools in order. Here, a **vault** is a named
bundle of Bao engine mounts.

Create a vault named `demo`:

```json
{"name":"vault_create","arguments":{"vault_id":"demo"}}
```

This prepares KV v2 at `demo/`, SSH at `ssh-ca-demo/`, and the namespace's shared
PKI mounts `_sys_pki_root/` and `_sys_pki_int/`. Existing compatible mounts retain
their settings.

After the first allocation, refresh discovery so the new engine tools appear,
then ask your client to list tools again:

```json
{"name":"cache_refresh","arguments":{}}
```

If refresh returns `rate_limited`, wait five seconds and retry it. Before mounts
exist, their engine-specific tools may be absent from the tool list.

Write a sample value:

```json
{"name":"kv_write","arguments":{"mount":"demo","path":"hello","data":{"data":{"message":"Hello from Bao MCP"}}}}
```

Read it:

```json
{"name":"kv_read","arguments":{"mount":"demo","path":"hello"}}
```

KV is ready after allocation. For SSH or TLS issuance, use the MCP CA and role
tools to configure the CA and issuance rules, then request a certificate.
[Tool examples](docs/tools.md) show the sequence and cover Transit as well.

## Give everyday agents narrower access

The setup token administers all of `agent`, including policies and tokens.
For an agent working with one vault, use the [policy templates](scripts/bao/policies/):

| Template | Scope inside `agent` |
| --- | --- |
| `admin.hcl` | Namespace administration and descendants |
| `vault-readwrite.hcl` | Full access to the `demo` KV and SSH mounts |
| `vault-readonly.hcl` | Read/list/scan access to those mounts |

Replace `demo` with your vault name before applying a template. Full read/write
includes permanent KV version deletion and SSH CA management. The PKI mounts are
shared; grant selected PKI issuance roles separately, as shown in the
[scripts guide](scripts/bao/README.md).

Use the interactive helpers in `scripts/bao/` to apply a policy, create a
reusable administrator login, issue a token, or enable additional engines.
[Their README](scripts/bao/README.md) explains the inputs and commands.
When policies change, `cache_refresh` updates the client's discovered tool list;
Bao evaluates actual operations immediately.

## Further reading

- [Tools and certificate examples](docs/tools.md)
- [Configuration, cache, logs, and build troubleshooting](docs/configuration.md)
- [Request and authorization design](docs/design.md)
- [Bao CLI basics](docs/bao/cli-basics.md), [authentication](docs/bao/auth-methods.md), and [ACL policies](docs/bao/acl-policy.md)
- [Tests](tests/README.md) and [recorded validation](docs/validation-2026-10-28.md)
- [OpenBao reference version](UPSTREAM.md)
