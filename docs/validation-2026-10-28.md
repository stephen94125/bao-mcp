# Bao MCP Implementation and Validation Record: 2026-10-28

[简体中文](validation-2026-10-28.zh-CN.md)

This records Bao MCP functional validation completed on 2026-10-28.
Reference source was pinned to OpenBao v2.7.2; live tests used the existing OpenBao v2.7.1 service.
The remote service was not upgraded. Testing v2.7.1 does not establish v2.7.2 runtime validation.

## Delivered behavior

- 34 grouped tools and a generic native JSON API route.
- Request-scoped native token forwarding and a fixed namespace, defaulting to `agent`.
- Bao owns secrets, roles, certificates, policies, tokens, and native operation audit.
- Unencrypted `cache.db` containing only scope, token hashes, allowlisted discovery metadata, and timestamps.
- Fixed freshness, idle eviction, capacity limits, and per-token/process discovery throttling.
- Caller-only refresh; actual execution always uses Bao's live authorization.
- Text logs using actor/resource hashes without secret values or arbitrary upstream error bodies.
- Existing mount settings preserved; partial allocation reports per-mount results without rollback deletion.

## Rust tests

**38 distinct tests passed:**

| Scope | Count |
| --- | ---: |
| Configuration, cache, discovery, and credential units | 9 |
| HTTP transport contracts | 10 |
| Discovery/cache integration | 5 |
| Tool routing, native bodies, and mount allocation | 12 |
| MCP HTTP boundaries | 2 |

The planned test pass was followed by focused fixes/reruns for failures, skipping previously passed scopes.
No final whole-suite replay, broad security audit, or additional reviewer pass was performed.

Fixes covered macOS Nix linker/SDK compatibility, SQLite integer/path handling, chunked-response fixtures,
namespace built-in engine types, and the native 400 response for a missing mount.
A mount 400 is not assumed to prove absence: a valid native mount catalog must confirm it before creation.
A denied or malformed catalog stops allocation.

## Existing Bao live tests

**12 functional checks passed**, plus isolated-namespace and gateway-startup prerequisites:

| Check | Result |
| --- | --- |
| Anonymous/invalid-token rejection | Passed |
| Native capability-based tool discovery | Passed |
| KV/SSH/shared-PKI allocation and preservation on repeat | Passed |
| Exact-path KV read-only discovery | Passed |
| Concurrent caller isolation and forbidden-path rejection | Passed |
| KV historical versions, CAS, deletion/restoration | Passed |
| SSH CA, role, and public-key signing | Passed |
| PKI root/intermediate/leaf issuance | Passed |
| Transit encryption/decryption, signing/verification, denial | Passed |
| Native policy/token creation and inspection | Passed |
| Immediate policy effects, cache refresh, token revocation | Passed |
| Known credentials/KV values absent from cache/log | Passed at the KV/token stage |

At the initial run, `.env` selected `agent`, but that namespace was absent and the supplied token belonged to root.
The harness created an isolated namespace from root without changing `.env` or creating a production `agent` namespace.
Prepare `agent` before using that default; see the current [setup flow](../README.md#2-set-up-the-agent-namespace-and-its-administrator-token).

Retained test namespace:

```text
mcp-live-20261028t143046z-6d14a9af
```

Main retained mounts:

```text
live/
ssh-ca-live/
_sys_pki_root/
_sys_pki_int/
live-transit/
```

Policies, test tokens, KV versions, SSH/PKI material and issuance results, and Transit keys were retained.
Version deletion/restoration and revocation of a dedicated test token were tested operations; there was no remote teardown.
The harness terminated only its own local MCP process.

Sanitized results: `test-output/mcp-live-20261028t143046z-6d14a9af/results.json`.
The same directory's `state.json`, `private-artifacts.json`, and key files contain private test material and are excluded from Git.
Do not paste their contents into docs or logs. The harness supports retained state with `--only` for focused reruns.

## Limits of the evidence

- External database, cloud, LDAP, RabbitMQ, and Kubernetes fixtures were unavailable.
  Their routing has contract coverage, not end-to-end service validation.
- LDAP requires a compatible external plugin in Bao 2.7; the gateway does not install it.
- ACME configuration APIs are reachable, but the ACME client JWS flow was neither implemented nor tested.
- Discovery is incomplete: the internal resultant ACL metadata omits `+` rules.
  Responses declare `complete:false`; generic requests remain subject to Bao authorization.
