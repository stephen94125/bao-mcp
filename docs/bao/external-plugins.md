# OpenBao External Plugins: Official and Community Index

[简体中文](external-plugins.zh-CN.md)

> Inventory recorded on 2026-10-28. This lists independently loaded server plugins for auth, secrets, databases, and KMS, excluding built-in engines, MCP servers, and ordinary CLI tools. It is the supplied research inventory, not an exhaustive registry or a new compatibility test.

## 1. Officially maintained external plugins

Source inventory: [openbao/openbao-plugins](https://github.com/openbao/openbao-plugins), with **24 entries** at the recorded date: 7 auth, 7 secrets, 1 database, and 9 KMS. Distribution includes [releases](https://github.com/openbao/openbao-plugins/releases) and OCI artifacts.

### Auth Plugins (7)

| Plugin | Purpose |
|---|---|
| AWS Auth | Authenticate AWS IAM/EC2 identities. |
| Azure Auth | Authenticate Azure identities or managed identities. |
| GCP Auth | Authenticate GCP service accounts or VM identities. |
| GitHub Auth | Authenticate with GitHub tokens and organization/team identity. |
| Kerberos Auth | Authenticate with Kerberos tickets. |
| LDAP Auth | Authenticate LDAP/AD accounts and groups. |
| RADIUS Auth | Authenticate through a RADIUS server. |

### Secrets Plugins (7)

| Plugin | Purpose |
|---|---|
| AWS Secrets | Issue AWS credentials under IAM policies. |
| Azure Secrets | Create Azure service principals dynamically. |
| GCP Secrets | Create service-account keys or OAuth tokens. |
| GCPKMS Secrets | Use Google Cloud KMS for cryptography and key management. |
| Consul Secrets | Issue Consul ACL tokens. |
| Nomad Secrets | Issue Nomad ACL tokens. |
| LDAP Secrets | Manage, create, or rotate LDAP/AD credentials. |

### Database Plugins (1)

| Plugin | Purpose |
|---|---|
| MongoDB Database | Create, manage, and revoke MongoDB credentials. |

### KMS/Auto Unseal Plugins (9)

| Plugin | Purpose |
|---|---|
| AWS KMS | AWS KMS Auto Unseal. |
| Azure Key Vault | Azure Key Vault Auto Unseal. |
| Google Cloud KMS | Google Cloud KMS Auto Unseal. |
| AliCloud KMS | Auto-unseal through Alibaba Cloud KMS. |
| OCI KMS | Oracle Cloud KMS Auto Unseal. |
| OVHcloud KMS | OVHcloud KMS Auto Unseal. |
| T Cloud Public KMS | T Cloud Public KMS Auto Unseal. |
| PKCS#11 | HSM-based auto-unseal and external keys. |
| Securosys | Securosys Primus HSM auto-unseal/external keys. |

**Officially maintained outside that 24-entry repository inventory:** [OAuthApp Secrets](https://github.com/openbao/openbao-plugin-secrets-oauthapp), covering OAuth 2 authorization code, refresh token, device code, and client credentials flows.

## 2. Community projects targeting OpenBao

These projects target OpenBao explicitly, use its SDK, or show direct development evidence. **That does not establish Bao 2.7 compatibility or production readiness.**

### Secret integration, synchronization, and API tokens

| Project | Purpose |
|---|---|
| [GitHub Secrets](https://github.com/rknightion/openbao-plugin-secrets-github) | Generate short-lived GitHub App installation tokens. |
| [Bitwarden Secrets](https://github.com/salir-se/openbao-plugin-secrets-bitwarden) | One-way synchronization from Bao KV to Bitwarden/Vaultwarden; pre-1.0, primarily tested with Vaultwarden. |
| [1Password Secrets](https://github.com/NavistAu/openbao-plugin-secrets-onepassword) | Read 1Password secrets through Bao ACLs, with in-memory caching rather than persistent copies in Bao. |
| [Secret Sync](https://github.com/adfinis/openbao-plugin-secrets-sync) | Synchronize secrets to AWS Secrets Manager, Kubernetes Secrets, and GitLab variables; **preview, not a production release**. |
| [Voidstar Secrets](https://github.com/NavistAu/openbao-plugin-secrets-voidstar) | Read-only virtual mappings of other secrets, preserving one source of truth. |
| [Kanidm Secrets](https://github.com/bjd5o/openbao-plugin-secrets-kanidm) | Issue/revoke service-account API tokens; the supplied notes record Bao 2.7.1 testing. |

### Infrastructure, networking, certificates, and cryptography

| Project | Purpose |
|---|---|
| [Proxmox Secrets](https://github.com/dryvist/openbao-plugin-secrets-proxmox) | Issue leased Proxmox VE API tokens; acceptance against real Proxmox was not complete in the recorded research. |
| [Garage S3 Secrets](https://github.com/Tech-S1/openbao-plugin-secrets-garage) | Create Garage S3 access keys and bucket permissions. |
| [Tailscale Secrets](https://github.com/SamPlaysKeys/openbao-plugin-secrets-tailscale) | Generate device-enrollment auth keys. |
| [NATS Secrets](https://github.com/BonesOfGiants/openbao-plugin-secrets-nats) | Manage operator/account/user JWTs and connection credentials. |
| [ACME Secrets](https://github.com/chisaato/openbao-plugin-secrets-acme) | Issue TLS certificates through ACME/Let's Encrypt; supports Cloudflare DNS-01. |
| [GPG Secrets](https://github.com/contain-security/openbao-plugin-secrets-gpg) | OpenPGP/GPG key and cryptographic integration. |
| [secp256k1 Secrets](https://github.com/IndependentImpact/openbao-plugin-secp256k1) | Keep secp256k1 keys in Bao and sign hashes for EVM use. |
| [Ethereum HD Wallet](https://github.com/lattn/openbao-plugin-eth-hdwallet) | Derive addresses and sign blockchain transactions. |

### Databases, authentication, and specialized systems

| Project | Purpose |
|---|---|
| [ClickHouse Database](https://github.com/elaunira/openbao-plugin-database-clickhouse) | Create/revoke accounts, including clustered deployments. |
| [Teradata Secrets](https://github.com/JavierLimon/openbao-teradata-secret-plugin) | Manage dynamic accounts over ODBC; still under development. |
| [Public-Client JWT Auth](https://github.com/kylemd/openbao-public-client-plugin) | Extend JWT/OIDC with PKCE login without a client secret. |
| [Open Horizon Auth](https://github.com/open-horizon/openbao-plugin-auth-openhorizon) | Authenticate Open Horizon identities. |
| [GraphQL Secrets](https://github.com/markchristopherwest/openbao-plugin-secrets-graphql) | Issue, renew, and revoke JWTs for a specific GraphQL identity server. |
| [Vault Replicator](https://github.com/JavierLimon/openbao-vault-replicator-secret-plugin) | One-way KV v2 replication from HashiCorp Vault to OpenBao. |

### KMS/Auto Unseal

| Project | Purpose |
|---|---|
| [SPIRE Auto-Unseal](https://github.com/magnusp/openbao-seal-spire) | Use SPIFFE/SPIRE identities with an upstream Transit service for auto-unseal/external keys. |
| [Attested Unseal](https://github.com/adfinis/openbao-attested-unseal) | Local TPM or remote broker attestation for unsealing; **functional alpha**. |
| [Securosys HSM Auto-Unseal](https://github.com/securosys-com/securosys-openbao-autounseal-plugin) | Securosys HSM/CloudsHSM auto-unseal and external keys; overlaps with official Securosys integration, so verify exact versions before deployment. |

## 3. Vault-origin projects explicitly mentioning or testing OpenBao

| Project | Purpose and compatibility evidence in the supplied inventory |
|---|---|
| [GitLab Secrets](https://github.com/ilijamt/vault-plugin-secrets-gitlab) | Issue/revoke personal, project, group, and deploy tokens; its README names Vault and OpenBao. |
| [Harbor Secrets](https://github.com/corelyr-oss/vault-plugin-secrets-harbor) | Issue short-lived robot accounts; CI includes OpenBao 2.x. |
| [NetBox Secrets](https://github.com/ljb2of3/vault-plugin-secrets-netbox) | Issue API tokens; README records testing with Bao 2.5.5. |
| [Honeycomb Secrets](https://github.com/jharley/vault-plugin-secrets-honeycombio) | Issue API keys; a dependency update to Bao 2.7.1 alone does not establish live compatibility. |

## 4. Vault plugins with recorded 2026 activity but unconfirmed Bao 2.7 compatibility

| Project | Purpose |
|---|---|
| [Cloudflare Secrets](https://github.com/Believe-SA/vault-cloudflare-secret-engine) | Issue, restrict, and revoke Cloudflare API tokens, including derived R2 S3 credentials. |
| [JFrog Artifactory Secrets](https://github.com/jfrog/vault-plugin-secrets-artifactory) | Issue Artifactory access tokens. |
| [Vault ACME](https://github.com/Boostport/vault-plugin-secrets-acme) | Issue TLS certificates through an ACME CA. |
| [Grafana Secrets](https://github.com/Boostport/vault-plugin-secrets-grafana) | Issue Grafana Cloud or service-account tokens. |
| [LiteLLM Secrets](https://github.com/benemon/vault-plugin-secrets-litellm) | Manage virtual keys, leases, and revocation. |
| [Keycloak Secrets](https://github.com/Serviceware/vault-plugin-secrets-keycloak) | Read/manage client secrets. |
| [Vercel Secrets](https://github.com/thevilledev/vault-plugin-secrets-vercel) | Issue deployment API tokens. |
| [Temporal Cloud Secrets](https://github.com/temporal-sa/vault-plugin-secrets-temporalcloud) | Create service accounts/API keys. |
| [Exoscale Secrets](https://github.com/exoscale/vault-plugin-secrets-exoscale) | Issue API keys with IAM roles. |
| [WebAuthn Auth](https://github.com/trublast/vault-plugin-auth-webauthn) | Authenticate with FIDO2, passkeys, or hardware security keys. |

Cloudflare Secrets may suit dynamically issuing restricted, TTL-bound tokens instead of distributing a fixed token. The recorded CI evidence covered Vault; **OpenBao 2.7 compatibility was not verified**.

## 5. Previously found projects without recent activity in the recorded inventory

| Project | Purpose | Latest commit found at the recorded date |
|---|---|---|
| [Nebula Secrets](https://github.com/mkrauser/openbao-plugin-secrets-nebula) | CA, node-certificate, and rotation management. | 2025-08 |
| [Oracle Database](https://github.com/cmmoran/openbao-plugin-database-oracle) | Dynamic database accounts. | 2025-07 |
| [Nomad Secrets (older community project)](https://github.com/JanMa/openbao-plugin-secrets-nomad) | ACL token management; an official Nomad plugin also exists. | 2025-07 |

## 6. Interpreting the inventory

- **24 official entries** refers to the recorded `openbao/openbao-plugins` README inventory, not all external plugins. OAuthApp illustrates another official repository.
- **A 2026 commit is not proof of active feature development**: activity may only update CI or dependencies, and does not establish production readiness.
- **OpenBao support is not verified Bao 2.7 compatibility**. Look for real registration, mounting, issuance, renewal, and revocation tests against Bao.
- This is a **compact functional index**. Before installing, consult the project's current README, releases, compatibility matrix, and security history. Built-in engines, MCP tools, and Smallstep CLI plugins are outside this server-plugin index.

Entry points: [Plugin documentation](https://openbao.org/docs/plugins/) · [Official plugin repository](https://github.com/openbao/openbao-plugins) · [Releases](https://github.com/openbao/openbao-plugins/releases).
