# Bao MCP Configuration

[简体中文](configuration.zh-CN.md)

For the first connection, follow the [README](../README.md). This reference covers
runtime settings and operation after setup.

## Environment variables

Set variables in the process environment. To use [.env.example](../.env.example),
load the selected settings through your shell or service manager before starting
Bao MCP.

| Variable | Default | Meaning |
| --- | --- | --- |
| `BAO_ADDR` | required | OpenBao HTTP(S) origin, such as `https://bao.example.com:8200` |
| `BAO_NAMESPACE` | `agent` | Existing namespace; nested namespaces are supported |
| `BAO_CACERT` | system trust | Optional PEM CA bundle for the Bao connection |
| `BAO_SKIP_VERIFY` | `false` | `true` or `1` bypasses Bao HTTPS certificate and hostname verification; `false` or `0` keeps verification enabled |
| `MCP_LISTEN` | `127.0.0.1:8000` | MCP listener address |
| `MCP_ALLOWED_HOSTS` | loopback hosts | Additional comma-separated HTTP Host patterns |
| `MCP_ALLOWED_ORIGINS` | loopback HTTP origins | Additional comma-separated browser origins |
| `CACHE_PATH` | `cache.db` | SQLite tool-discovery cache; parent directory must exist |
| `CACHE_IDLE_TTL_SECONDS` | `3600` | Idle interval after which an entry can be removed |
| `CACHE_MAX_AGE_SECONDS` | `60` | Fixed discovery freshness, at most 3600 seconds |
| `CACHE_MAX_ENTRIES` | `1024` | LRU row limit, at most 10000 |
| `BAO_TIMEOUT_SECONDS` | `30` | Bao request timeout, at most 300 seconds |
| `LOG_LEVEL` | `info` | `info`, `warn`, or `error` |

The binary accepts `serve` (also the default command), `--help`, and `-h`.
`/healthz` reports local process liveness. A successful authenticated MCP request
checks the Bao connection and caller permissions.

## Remote clients and HTTPS

For a server behind an HTTPS reverse proxy:

```sh
export BAO_ADDR=https://bao.example.com:8200
export BAO_NAMESPACE=agent
export MCP_LISTEN=0.0.0.0:8000
export MCP_ALLOWED_HOSTS=bao-mcp.example.com
export MCP_ALLOWED_ORIGINS=https://agent-ui.example.com
./target/release/bao-mcp serve
```

Configure the proxy and firewall for your deployment. The public MCP URL in this
example is `https://bao-mcp.example.com/mcp`.

`MCP_ALLOWED_HOSTS` matches the HTTP Host value, so use a hostname, optionally
including a port pattern such as `bao-mcp.example.com:*`. An Origin includes its
scheme, for example `https://agent-ui.example.com`. Browser clients may require
an origin entry; each caller still supplies its own bearer token.

Use `BAO_CACERT` for a private Bao CA. Certificate and hostname verification are
enabled by default. For a test server, `BAO_SKIP_VERIFY=true` bypasses both checks;
enable it only when you accept that loss of peer verification. The option also
accepts `1`/`0`; other values stop startup.
The HTTP request limit is 8 MiB and the Bao response limit is 8 MiB. Upstream
redirects and automatic retries are disabled.

## Token scope and discovery

Tokens belong to Bao namespaces. The namespace in `BAO_NAMESPACE` must be one the
caller may access. In a policy stored inside `agent`, `path "*"` grants matching
paths beneath `agent/`. A root token keeps its root privileges even when used
with that namespace setting.

The gateway validates cold requests with `auth/token/lookup-self`, reads mount
and resultant-ACL metadata, and checks concrete paths through
`sys/capabilities-self`. The supplied [policy templates](../scripts/bao/policies/)
include discovery and token-self-management permissions. The built-in `default`
policy normally also supplies these metadata permissions.

Use TTL-limited service tokens for clients. Metadata calls consume uses on
finite-use tokens. For response wrapping, authenticate with a normal caller token
and pass the wrapping token to `sys/wrapping/unwrap` in its native JSON body.

## Cache behavior

Discovery is cached by the full token hash and the upstream/namespace scope.
`cache.db` stores derived capabilities, mounts, tool names, and timestamps. Secret
values and raw credentials stay out of the cache. SQLite is unencrypted; its file
permissions are `0600`.

Using an entry updates `last_used`, while freshness stays fixed. Token TTL also
bounds expiry. Startup and cache access prune expired or idle records. Limits
include the configured row count, a 32 MiB database, and 512 KiB per snapshot.
Stop the server before deleting a disposable or damaged cache; the next request
rebuilds it.

Call `cache_refresh` after policy changes to refresh only the current caller's
entry. Discovery starts are limited to once per token per five seconds, 32 per
second per process, and eight concurrent fetches. HTTP concurrency is limited to
64. Native authorization failures invalidate the caller's entry and permit
revalidation under the process-wide budget.

A cached tool list can briefly reflect an earlier policy. Actual operations always
reach Bao with the current token and use its current permissions.

### Discovery coverage

OpenBao 2.7.2's internal resultant-ACL metadata omits `+` segment wildcard rules.
Capability queries answer only for supplied paths. Some permitted tools can
therefore be absent from discovery, and one visible grouped tool may support only
a subset of its possible paths or operations. `bao_request` and `api_help` provide
access to other authorized native JSON paths. Refresh reports `complete:false`
and warnings describing this boundary.

## Failures and logging

When a mutation's outcome is ambiguous, inspect Bao before retrying. For example,
a connection failure after a certificate was issued may return `outcome_unknown`.
`vault_create` reports partial mount allocation so the caller can inspect results
and resume missing mounts.

Text logs go to stderr and record the actor's token hash, tool name, resource hash,
result, and duration. Enable Bao audit separately for native request paths,
identities, and operation history.

## Building one executable

The manifest explicitly declares `bao-mcp` as the executable target:

```sh
cargo build --release --locked --bin bao-mcp
```

Deploy `target/release/bao-mcp`. Cargo's intermediate Rust libraries are linked
into this executable. SQLite uses its bundled source and TLS uses Rustls;
separately installed OpenSSL and SQLite shared libraries are unnecessary.

### Static Linux MUSL builds

Use the build-only Docker recipe to export one Linux executable:

```sh
docker buildx build --platform linux/amd64 --target artifact \
  --output type=local,dest=dist/x86_64-unknown-linux-musl \
  -f Dockerfile.build .
```

For ARM64, use `--platform linux/arm64` and
`dest=dist/aarch64-unknown-linux-musl`. Docker needs a working Linux builder;
cross-architecture builds may require emulation. The allowlisted build context
contains only the manifest, lockfile, Cargo configuration, and Rust sources.

The MUSL target configuration explicitly enables `crt-static`. The recipe checks
that the executable has neither an ELF interpreter nor shared-library `NEEDED`
entries, then runs `--help` before exporting it. On a Linux host with the matching
Rust target and MUSL C compiler, the equivalent build is:

```sh
cargo build --release --locked --bin bao-mcp --target x86_64-unknown-linux-musl
```

The resulting file is `target/x86_64-unknown-linux-musl/release/bao-mcp`.
Static linking does not embed the operating system's trusted CA store; retain a
system CA bundle or configure `BAO_CACERT` for verified HTTPS connections.

## Building on macOS with Nix

macOS executables use Apple's built-in libraries and frameworks. Apple does not
support fully static executables; see its [Library Primer](https://developer.apple.com/forums/thread/715385).
Only the executable needs to be distributed; the stock OS supplies those libraries.

If Nix reports `cc`, `libiconv`, or linker errors, use the system C toolchain and SDK:

```sh
CC=/usr/bin/clang CXX=/usr/bin/clang++ \
SDKROOT="$(xcrun --sdk macosx --show-sdk-path)" \
CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER=/usr/bin/clang \
cargo build --release --locked --bin bao-mcp
```

The linker variable above is for Apple Silicon. Use the matching target linker
variable on Intel Macs. General testing instructions are in [tests/README.md](../tests/README.md).
