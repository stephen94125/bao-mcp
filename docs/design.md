# Bao MCP Architecture

[简体中文](design.zh-CN.md)

Bao MCP exposes OpenBao's HTTP APIs as tools for AI agents. Rust, RMCP, and Axum
provide the MCP transport; OpenBao stores resources and enforces access. Start with
the [setup guide](../README.md), [configuration](configuration.md), and
[tool examples](tools.md).

## Request flow

```text
Agent's OpenBao token
        ↓ Authorization: Bearer
Bao MCP: validate arguments and map tools to API paths
        ↓ X-Vault-Token + configured X-Vault-Namespace
OpenBao: authenticate, authorize, execute, and audit
```

Every call forwards the caller's token to the configured namespace, which defaults
to `agent`. Prepare that namespace with the setup script before connecting clients.
Tool arguments select resources within this scope. Bao policies determine access
to those resources and any child namespaces.

## Resource state belongs to Bao

Secrets, versions, timestamps, SSH and PKI roles, certificates, Transit keys,
policies, and tokens are read from and written to Bao. Changes through the Bao CLI,
UI, or another API client are reflected in subsequent MCP reads.

`vault_create` groups related mounts: KV at the vault name, SSH at
`ssh-ca-<name>`, and namespace-wide PKI at `_sys_pki_root` and `_sys_pki_int`.
CA generation, role restrictions, and external backend connections are explicit
configuration steps. Allocation preserves existing mounts and reports each step.
After a partial result, inspect it and repeat the call to add missing mounts.

## Tool discovery and caching

At discovery, Bao MCP validates the token and derives candidate API paths from
Bao's resultant ACL and mount metadata. It checks native capabilities on those
paths to select relevant tools. The cache stores these results by upstream,
namespace, and token hash. See [cache settings](configuration.md#cache-behavior).

`cache.db` contains capabilities, tool names, mount types, and timing metadata.
It is disposable SQLite with mode `0600`. Fixed freshness and idle limits bound
reuse; capacity, concurrency, and rate limits bound discovery work. A caller's
`cache_refresh` validates that token and replaces its own entry.

Discovery guides tool selection. OpenBao authorizes every actual operation using
the forwarded token, including calls made while cached discovery data is stale.

## Discovery limits

Capabilities answer questions about supplied paths. OpenBao v2.7.2's internal
resultant ACL metadata omits `+` wildcard rules, so discovery reports
`complete:false`. Authenticated callers can use `bao_request` and `api_help` for
native JSON routes outside the discovered groups, subject to the same Bao ACLs.
Use separate KV secret paths when their values need different access policies.

## Failures and observability

A write with an uncertain outcome returns `outcome_unknown`. Inspect Bao before
retrying because the operation may already have completed. Multi-step allocation
reports completed and failed steps so the caller can resume deliberately.

Text logs record token hashes, tool names, resource-path hashes, outcomes, and
durations. Tokens, secret values, full arguments, and arbitrary upstream error
bodies are excluded. Bao audit supplies native request paths, authenticated
identities, and operation history; MCP logs add tool-call context.

## API coverage

The 34 tools group JSON APIs by engine and purpose and preserve native request
bodies. Transit includes encryption, decryption, signing, and verification.
`bao_request` extends access to other native JSON APIs and installed plugins.
Database, cloud, directory, broker, and Kubernetes engines require their own
backend configuration and credentials.

LDAP needs a compatible external plugin in Bao 2.7. An ACME client handles the
ACME JWS protocol after PKI configuration. This JSON interface supports structured
API requests; use the appropriate native client for binary transfers.
