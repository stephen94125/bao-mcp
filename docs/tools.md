# Bao MCP Tools

[简体中文](tools.zh-CN.md)

Tools group related native Bao APIs. Mounted tools normally take `mount`,
`operation`, and a mount-relative `path`, plus optional `data`, `query`, and
`wrap_ttl`. `data` preserves the native JSON body. Use `api_help` for an endpoint's
fields and operations; see the [engine API reference](bao/engine-storage-api-paths.md)
for native routes.

## Tool groups

| Tools | Capability |
| --- | --- |
| `vault_create` | Allocate KV, SSH, and shared PKI mounts |
| `kv_read`, `kv_write`, `kv_manage` | KV v1/v2 data, CAS, versions, metadata, deletion/restoration/destruction |
| `ssh_ca`, `ssh_roles`, `ssh_issue` | SSH CA/issuers, role configuration, signing, issuance, OTP |
| `pki_ca`, `pki_roles`, `pki_certificates`, `pki_config` | PKI CA/keys/issuers, roles, certificates, revocation, configuration |
| `transit_keys`, `transit_crypto` | Key management, encryption, signing, verification, HMAC/hash/random |
| `database_connections`, `database_roles`, `database_credentials` | Connections, roles, dynamic/static credentials and rotation |
| `kubernetes`, `ldap`, `rabbitmq` | Corresponding engine APIs; LDAP needs a compatible external plugin in Bao 2.7 |
| `totp_keys`, `totp_code`, `cubbyhole` | TOTP and token-private storage |
| `identity`, `auth`, `token`, `policy` | Native identity, authentication, token, and policy management |
| `leases`, `mounts`, `namespaces`, `audit`, `system` | Native administration in the configured namespace |
| `bao_request`, `api_help` | Generic JSON API access and native API help |
| `cache_refresh` | Refresh the current caller's discovery |

These are 34 tools. Engine/plugin installation and external service configuration
are prerequisites for the corresponding calls. The JSON interface covers native
JSON routes; ACME clients perform the ACME JWS protocol and raw binary uploads
require an appropriate native client.

## Vault allocation

```json
{"name":"vault_create","arguments":{"vault_id":"demo"}}
```

The allocation is:

| Mount | Type | Scope |
| --- | --- | --- |
| `demo` | KV v2 | This vault |
| `ssh-ca-demo` | SSH | This vault |
| `_sys_pki_root` | PKI | Shared within the namespace |
| `_sys_pki_int` | PKI | Shared within the namespace |

Compatible existing mounts preserve their settings. A type/version conflict stops
allocation. Partial results use `complete:false`, per-mount details, and MCP
`isError:true`; completed mounts remain and a later call can allocate missing ones.

After adding mounts, call `cache_refresh` and list tools again. If refresh returns
`rate_limited`, wait five seconds before retrying.

## KV

`kv_write` uses the KV v2 envelope, including optional CAS:

```json
{"name":"kv_write","arguments":{"mount":"demo","path":"service","data":{"data":{"password":"example"},"options":{"cas":0}}}}
```

Read a historical version:

```json
{"name":"kv_read","arguments":{"mount":"demo","path":"service","version":1}}
```

`kv_manage` exposes native metadata and version-operation paths. Grant deletion
or destruction only when the caller needs it.

## SSH CA

For a newly allocated SSH mount, call these in order.

Generate its CA key once:

```json
{"name":"ssh_ca","arguments":{"mount":"ssh-ca-demo","operation":"write","path":"config/ca","data":{"generate_signing_key":true}}}
```

Create an issuance role:

```json
{"name":"ssh_roles","arguments":{"mount":"ssh-ca-demo","operation":"write","path":"roles/deploy","data":{"key_type":"ca","allow_user_certificates":true,"allowed_users":"deploy","default_user":"deploy","ttl":"5m","max_ttl":"1h"}}}
```

Read the CA public key and configure the target SSH servers to trust it:

```json
{"name":"ssh_ca","arguments":{"mount":"ssh-ca-demo","operation":"read","path":"public_key"}}
```

Submit a real SSH public key for signing:

```json
{"name":"ssh_issue","arguments":{"mount":"ssh-ca-demo","operation":"write","path":"sign/deploy","data":{"public_key":"<your OpenSSH public key>","valid_principals":"deploy","ttl":"5m"}}}
```

Bao enforces the role's principals, TTL, and options. The target host enforces the
resulting SSH certificate. See [Bao SSH documentation](https://openbao.org/docs/secrets/ssh/) for server trust setup.

## PKI

The two PKI mounts are shared. Configure their CAs once per namespace, then use
separate roles for issuance scopes. The following short lifetimes suit a local
trial; choose CA lifetimes for your deployment.

1. `pki_ca`: mount `_sys_pki_root`, operation `write`, path `root/generate/internal`, data `{"common_name":"Demo Root CA","ttl":"72h","key_type":"ec","key_bits":256}`.
2. `pki_ca`: mount `_sys_pki_int`, operation `write`, path `intermediate/generate/internal`, data `{"common_name":"Demo Intermediate CA","key_type":"ec","key_bits":256}`. Keep the returned CSR.
3. `pki_ca`: mount `_sys_pki_root`, operation `write`, path `root/sign-intermediate`, data `{"csr":"<returned CSR>","format":"pem_bundle","ttl":"48h"}`.
4. `pki_ca`: mount `_sys_pki_int`, operation `write`, path `intermediate/set-signed`, data `{"certificate":"<returned certificate bundle>"}`.

Create a role for the demo workload:

```json
{"name":"pki_roles","arguments":{"mount":"_sys_pki_int","operation":"write","path":"roles/demo","data":{"allowed_domains":["demo.internal"],"allow_subdomains":true,"max_ttl":"1h","key_type":"ec","key_bits":256}}}
```

Issue a leaf certificate:

```json
{"name":"pki_certificates","arguments":{"mount":"_sys_pki_int","operation":"write","path":"issue/demo","data":{"common_name":"app.demo.internal","ttl":"15m"}}}
```

Configure the client/server trust store with the CA chain. Certificate issuance
can return private keys; deliver those through the application's secret-handling
mechanism. For an existing CSR, use `sign/<role>` instead of `issue/<role>`.

## Transit

Enable a mount if needed:

```json
{"name":"mounts","arguments":{"operation":"write","path":"mounts/transit","data":{"type":"transit"}}}
```

Create an encryption key, then encrypt Base64-encoded `hello`:

```json
{"name":"transit_keys","arguments":{"mount":"transit","operation":"write","path":"keys/app","data":{"type":"aes256-gcm96"}}}
```

```json
{"name":"transit_crypto","arguments":{"mount":"transit","operation":"write","path":"encrypt/app","data":{"plaintext":"aGVsbG8="}}}
```

Use `decrypt/app` with the returned `ciphertext` to decrypt. For signatures, create
an `ed25519` key through `transit_keys`, call `sign/<key>` with Base64 `input`, then
`verify/<key>` with that input and the returned signature.
