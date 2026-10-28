# Bao MCP

[简体中文](README.zh-CN.md) · [繁體中文設計說明](docs/design.zh-TW.md)

A standalone Rust MCP server over OpenBao. Each request uses the caller's native
OpenBao token and the configured namespace. There is no service/admin token,
local identity or policy database, Web UI, Mission broker, Hub, or background
engine initialization. OpenBao owns secrets, roles, certificates, ACLs and audit.

## Run

Requires Rust 1.92+ and an existing OpenBao namespace (default: `agent`).

```sh
cd bao-mcp
cargo build --release --locked
BAO_ADDR=https://bao.example.com:8200 ./target/release/bao-mcp serve
```

Connect a Streamable HTTP MCP client to `http://127.0.0.1:8000/mcp` with
`Authorization: Bearer <native OpenBao token>`. The binary accepts only `serve`
(or no command), `--help`, and `-h`. `/healthz` is local liveness, not a Bao
readiness or authorization check. The application does not read `.env` files.

Only `BAO_ADDR` is required. Native tokens are issued outside this server using
Bao CLI/API or a Bao auth method. A JWT must first be exchanged through the
configured Bao auth method; the legacy application's tokens are not accepted.
Tokens need `read` on `auth/token/lookup-self` for discovery authentication.
Bao's standard `default` policy normally grants that and discovery endpoints.
Tokens with finite use counts spend uses on metadata requests too; use normal
TTL-limited service tokens for MCP rather than single-use wrapping tokens.
To unwrap, use a normal caller token and pass the wrapping token in the native
`sys/wrapping/unwrap` JSON body. Neither token is cached or logged.

## Configuration

| Environment variable | Default | Purpose |
| --- | --- | --- |
| `BAO_ADDR` | required | Fixed HTTP(S) OpenBao origin; URL credentials are rejected |
| `BAO_NAMESPACE` | `agent` | Existing, nonempty namespace; nested namespaces are supported |
| `BAO_CACERT` | system trust | Optional PEM CA bundle; TLS verification stays enabled |
| `MCP_LISTEN` | `127.0.0.1:8000` | Listen address |
| `MCP_ALLOWED_HOSTS` | loopback hosts | Additional comma-separated exact host/port patterns accepted by RMCP |
| `MCP_ALLOWED_ORIGINS` | loopback HTTP origins | Additional comma-separated allowed browser origins |
| `CACHE_PATH` | `cache.db` | Disposable, unencrypted SQLite discovery cache; parent directory must exist |
| `CACHE_IDLE_TTL_SECONDS` | `3600` | Remove entries unused for this interval |
| `CACHE_MAX_AGE_SECONDS` | `60` | Non-sliding discovery freshness, at most 3600 seconds |
| `CACHE_MAX_ENTRIES` | `1024` | LRU row limit, at most 10000 |
| `BAO_TIMEOUT_SECONDS` | `30` | Upstream timeout, at most 300 seconds |
| `LOG_LEVEL` | `info` | `info`, `warn`, or `error`; HTTP wire tracing is not enabled |

Place remote deployments behind HTTPS. Host and Origin checks are transport
checks, not a replacement for Bao authentication. The HTTP body limit is 2 MiB;
upstream responses are limited to 8 MiB. Redirects and automatic retries are
disabled. Ambiguous mutation failures are reported as `outcome_unknown`; inspect
Bao before retrying an issuance, rotation or other non-idempotent action.

## Tools

34 tools group native API operations. Mounted tools take `mount`, `operation`,
`path`, optional `data`, `query`, and `wrap_ttl`; specialized KV tools supply the
KV v2 API prefixes. `data` is the complete native JSON body, not an invented
local data model. Use `api_help` to obtain the endpoint's fields from Bao.

| Tools | Native capability |
| --- | --- |
| `vault_create` | Allocate a named bundle of engines, idempotently |
| `kv_read`, `kv_write`, `kv_manage` | KV v1/v2, versions, CAS, merge patch, metadata, delete/undelete/destroy |
| `ssh_ca`, `ssh_roles`, `ssh_issue` | SSH issuers, CA material, roles, signing, key/certificate issuance and OTP |
| `pki_ca`, `pki_roles`, `pki_certificates`, `pki_config` | PKI CA/issuers, roles, issuance/signing, revocation, CRL and configuration |
| `transit_keys`, `transit_crypto` | Key lifecycle/import/export/rotation, encryption/decryption, signing/verification, HMAC/hash/random |
| `database_connections`, `database_roles`, `database_credentials` | Database engine configuration, roles, dynamic/static credentials and rotation |
| `kubernetes`, `ldap`, `rabbitmq` | Corresponding native engine APIs; LDAP requires an installed compatible external plugin in Bao 2.7 |
| `totp_keys`, `totp_code`, `cubbyhole` | TOTP and token-private storage |
| `identity`, `auth`, `token`, `policy` | Native identity, auth method, token and policy management |
| `leases`, `mounts`, `namespaces`, `audit`, `system` | Native system administration within the configured namespace and caller ACL |
| `bao_request`, `api_help` | Native JSON API escape hatch and native endpoint/OpenAPI help |
| `cache_refresh` | Refresh only the authenticated caller's discovery cache |

`bao_request` covers additional native JSON endpoints and installed plugins
without adding a tool per REST operation. It does not expose arbitrary upstream
URLs/headers, raw binary uploads, or the ACME client's JWS protocol. ACME engine
configuration is available; an ACME client still performs the ACME protocol.
Installing an engine/plugin or accessing an external database/cloud service is
an operator responsibility, not implied by its MCP tool being present.

### Examples

```json
{"name":"kv_write","arguments":{"mount":"team","path":"service","data":{"data":{"password":"example"},"options":{"cas":0}}}}
{"name":"kv_read","arguments":{"mount":"team","path":"service"}}
{"name":"ssh_issue","arguments":{"mount":"ssh-ca-team","operation":"write","path":"sign/deploy","data":{"public_key":"ssh-ed25519 ...","valid_principals":"deploy","ttl":"5m"}}}
{"name":"transit_crypto","arguments":{"mount":"transit","operation":"write","path":"sign/build","data":{"input":"aGVsbG8="}}}
{"name":"bao_request","arguments":{"operation":"write","path":"sys/wrapping/unwrap","data":{"token":"<wrapping token>"}}}
{"name":"cache_refresh","arguments":{}}
```

### Vault allocation

`vault_create` takes `vault_id`. By default it ensures four mounts:

- `<vault_id>`: KV v2.
- `ssh-ca-<vault_id>`: SSH.
- `_sys_pki_root` and `_sys_pki_int`: shared PKI mounts in the namespace, following the legacy naming scheme.

It does **not** create the namespace, create `_vault_meta`, generate CA keys,
change existing roles, or overwrite existing engine configuration. Configure
and issue certificates explicitly with the native SSH/PKI tools. An existing
mount with the wrong engine type/version is a conflict. A partially allocated
bundle returns `complete:false`, per-mount results, and MCP `isError:true`;
created mounts remain available and a later invocation resumes missing mounts.
Do not infer PKI isolation per vault: the two PKI mounts are namespace-wide.

## Discovery and cache semantics

A cold request validates the token with `lookup-self`, reads visible mounts and
resultant ACL metadata, and confirms bounded concrete candidate paths with
`sys/capabilities-self`. Narrow exact-path read grants are distinguished from
write grants. Cached requests do not repeat that discovery.

**Tool discovery is not a complete authorization oracle.** OpenBao 2.7.2's
internal resultant-ACL endpoint omits `+` segment wildcard rules, and capabilities
queries cannot enumerate unknown paths. Missing metadata rights or bounded
candidate limits can omit tools. `bao_request` and `api_help` remain usable for
authenticated callers. Grouped tools may be visible for one permitted path or
operation while another argument is denied. Every actual operation reaches Bao
with the current caller token; the cached tool list never grants or blocks it.
The internal UI metadata API is version-sensitive. Refresh returns
`complete:false`, concrete capabilities, mounts and explanatory warnings.

The cache key combines the upstream/namespace scope and full SHA-256 token hash.
Only derived discovery metadata, timestamps and hashes are stored; no raw
lookup response, token, secret value, request payload or private key is stored.
The file is plaintext SQLite with mode 0600. It is safe to remove while the
server is stopped; it is rebuilt on demand. A corrupt cache reports a clear
error rather than silently discarding an operator's file.

Freshness is bounded by both token TTL and `CACHE_MAX_AGE_SECONDS`; using an
entry updates `last_used` without extending its fixed freshness. Expired/idle
entries are pruned at startup and cache access. A row limit, 32 MiB database
limit and 512 KiB snapshot limit bound storage. There is no resource-state DB.

`cache_refresh` revalidates the caller before fetching and replacing that
caller's entry. Unknown/invalid tokens cannot fill the cache. Cold/refresh
attempts are limited to once per token per five seconds, 32 starts per second
per process and eight concurrent fetches; HTTP concurrency is limited to 64.
Invalidation after a native rejection permits fresh revalidation under the same
process-wide budget, so one forbidden path does not lock out other allowed paths.
An immediate refresh after first discovery may return 429. A random token does
not bypass these process-wide bounds. Limits are local to each replica.

After ACL changes, refresh or wait for fixed freshness. A revoked token can
briefly retain a cached **tool list**, but protected execution is rejected by
Bao immediately; 401/403 execution failures invalidate the cached entry.
The cache cannot observe task cancellation or other legacy Mission semantics.

## Logging and testing

Text logs go to stderr. Business call logs contain a SHA-256 actor ID, tool
name, hash of the resource identifier, outcome and duration. They never include
Bearer tokens, raw paths, arguments, response values, private keys or arbitrary
upstream error bodies. Bao audit must be configured separately for actual native
request history and identity; these logs are not a replacement for Bao audit.

Run the Rust contract suite once:

```sh
cargo test --locked --all-targets
```

On this macOS workstation, if the Nix linker cannot find `libiconv`, set
`CC=/usr/bin/clang`, `CXX=/usr/bin/clang++`,
`SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk`, and
`CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER=/usr/bin/clang` for the build command.

See [live test instructions](tests/README.md) for the opt-in existing-Bao test.
It retains its isolated test namespace. Rerun only failed test scopes after a
fix, rather than repeating the complete audit. See [upstream pin](UPSTREAM.md).

Validation evidence: [2026-10-28 report](docs/validation-2026-10-28.zh-TW.md).
