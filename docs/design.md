# Bao MCP: Data Ownership, Authorization, and Cache Boundaries

[简体中文](design.zh-CN.md)

Bao MCP is a native Bao tool interface built with Rust, RMCP, and Axum. Its reference source is
OpenBao v2.7.2. See the [README](../README.md) for setup and environment variables.

## Bao owns resource state

Secret contents, versions, timestamps, SSH/PKI roles and certificates, Transit keys, policies, and tokens
belong to Bao. The gateway has no duplicate resource metadata, `created_by` ownership rule,
`_vault_meta` record, or persistent business-operation journal.

After a direct Bao CLI/GUI change, the next MCP read uses Bao's response. There is no synchronization
job or local success record overriding current resource state. When a multi-step allocation partially succeeds,
the response reports each result and retains completed mounts; a later call can allocate missing mounts without rollback deletion.

## Each request uses its own token

```text
Agent's native Bao token
        ↓ Authorization: Bearer
Bao MCP: validate arguments and map tools to native API paths
        ↓ X-Vault-Token + fixed X-Vault-Namespace
OpenBao: authenticate, enforce native policies/roles, execute, and audit
```

The process has no shared administrator token. The configured namespace defaults to `agent` and must already exist.
Startup creates no Bao resources. Tool arguments cannot override headers or change that configured namespace.
Native permissions may still allow administration of child namespaces beneath it.

Vault allocation uses the existing naming scheme: KV at the vault name, SSH at `ssh-ca-<name>`, and shared PKI at
`_sys_pki_root` and `_sys_pki_int`. Allocation enables mounts without generating CAs or overwriting existing roles.
Configure CA material, SSH principals/TTL/options, and PKI role restrictions explicitly through native tools.

The gateway does not include legacy local tokens, Mission JWT, operator profiles, Hub, or a Web UI.
JWT workloads can exchange identity through Bao's native JWT auth method. Acquiring a token remains
the caller's responsibility rather than a second gateway identity system.

## Capability cache is not an authorization source

`cache.db` is disposable, unencrypted SQLite keyed by upstream/namespace scope and token hash.
It stores capabilities, tool names, mount types, and fetch/expiry/last-use times, not raw tokens,
secret values, or private keys. File permissions are 0600.

Cold discovery validates the token and queries candidate permissions. A fixed freshness interval bounds cache reuse.
`CACHE_IDLE_TTL_SECONDS` controls idle eviction; `CACHE_MAX_AGE_SECONDS` limits freshness even for entries
used continuously. Pruning occurs at startup and cache access. `cache_refresh` revalidates the caller before replacing
only that token's entry; unknown or invalid tokens cannot populate the cache.

Cached policy metadata may briefly be stale, affecting tool discovery only. It neither grants execution nor blocks
a native operation merely because its tool is absent from the list. Bao decides authorization on every execution.

Cold queries and refreshes have per-token intervals, process-wide rate/concurrency limits, row limits, and size limits.
Refreshing within five seconds of initial discovery may be throttled.

## Discovery cannot enumerate every possible permission

The capabilities API answers questions about supplied concrete paths; it cannot enumerate all authorized paths.
The gateway derives candidates from resultant ACL and mount metadata, then checks their capabilities.
OpenBao v2.7.2's internal ACL metadata omits `+` wildcard rules, so discovery cannot claim completeness.

Refresh results expose `complete:false` and warnings. Authenticated callers can use `bao_request` and
`api_help` for undiscovered native JSON paths, still subject to Bao's permissions.
The gateway does not filter individual KV fields; use separate secret paths for different access scopes.

## Failures and logs

Writes that may already have executed are not automatically retried. Ambiguous network or backend failures return
`outcome_unknown`, leaving callers to inspect Bao before retrying. This version has no legacy persistent compensation,
mission replay-prevention, or delivery-confirmation guarantees. Native wrapping tokens retain Bao's one-time behavior.

Text logs record token hashes, tool names, resource-path hashes, outcomes, and durations. Raw tokens, secret values,
full arguments, and arbitrary upstream error bodies are excluded. Enable Bao audit for native paths, identities,
and operation history; gateway logs add the agent's tool-call activity.

## What API coverage means

The 34 tools group native JSON APIs by engine and purpose while preserving request bodies. Transit covers encryption,
decryption, signing, and verification. `bao_request` reaches other native JSON routes and compatible installed plugins
without one tool per endpoint. API reachability does not establish external database, cloud, LDAP, RabbitMQ,
or Kubernetes configuration or end-to-end validation.

LDAP requires a compatible external plugin in Bao 2.7. ACME configuration is available; its JWS protocol remains
the ACME client's responsibility. Raw binary uploads and arbitrary custom headers are outside this JSON interface.
