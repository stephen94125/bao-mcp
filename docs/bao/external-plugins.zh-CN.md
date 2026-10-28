# OpenBao External Plugins 索引：官方与第三方扩展

[English](external-plugins.md)

> 原始整理日期：2026-10-28。本篇是使用者提供的外挂调查索引，仅涵盖独立载入的 Auth、Secrets、Database、KMS Server 外挂，不含内建 Engine、MCP 或一般 CLI；不是完整名录，也不是本次重新进行的相容性验证。

## 一、OpenBao 官方维护的 External Plugins

原始索引来源：[OpenBao 官方 openbao-plugins 仓库](https://github.com/openbao/openbao-plugins)，记录日期共 **24 个**（7 Auth + 7 Secrets + 1 Database + 9 KMS），可由 [Releases](https://github.com/openbao/openbao-plugins/releases) 或 OCI Artifact 取得。

### Auth Plugins（7）

| Plugin | 简述 |
|---|---|
| AWS Auth | 用 AWS IAM／EC2 身分登入 Bao。 |
| Azure Auth | 用 Azure 身分或 Managed Identity 登入。 |
| GCP Auth | 用 GCP Service Account／VM 身分登入。 |
| GitHub Auth | 用 GitHub Token、组织／团队身分登入。 |
| Kerberos Auth | 用 Kerberos Ticket 登入。 |
| LDAP Auth | 以 LDAP／AD 帐号、群组登入。 |
| RADIUS Auth | 经 RADIUS Server 验证使用者。 |

### Secrets Plugins（7）

| Plugin | 简述 |
|---|---|
| AWS Secrets | 依 IAM Policy 动态签发 AWS 凭证。 |
| Azure Secrets | 动态建立 Azure Service Principal。 |
| GCP Secrets | 动态建立 GCP Service Account Key／OAuth Token。 |
| GCPKMS Secrets | 透过 Google Cloud KMS 加解密与管理金钥。 |
| Consul Secrets | 动态产生 Consul ACL Token。 |
| Nomad Secrets | 动态产生 Nomad ACL Token。 |
| LDAP Secrets | 管理、建立或轮换 LDAP／AD 帐密。 |

### Database Plugins（1）

| Plugin | 简述 |
|---|---|
| MongoDB Database | 动态建立、管理、撤销 MongoDB 使用者帐密。 |

### KMS／Auto Unseal Plugins（9）

| Plugin | 简述 |
|---|---|
| AWS KMS | AWS KMS Auto Unseal。 |
| Azure Key Vault | Azure Key Vault Auto Unseal。 |
| Google Cloud KMS | Google Cloud KMS Auto Unseal。 |
| AliCloud KMS | 阿里云 KMS Auto Unseal。 |
| OCI KMS | Oracle Cloud KMS Auto Unseal。 |
| OVHcloud KMS | OVHcloud KMS Auto Unseal。 |
| T Cloud Public KMS | T Cloud Public KMS Auto Unseal。 |
| PKCS#11 | 透过 HSM Auto Unseal；支援 External Keys。 |
| Securosys | 透过 Securosys Primus HSM Auto Unseal／External Keys。 |

**官方另外维护，但不在上述仓库 24 个清单内：** [OAuthApp Secrets](https://github.com/openbao/openbao-plugin-secrets-oauthapp)，统一包装 OAuth 2 授权码、Refresh Token、Device Code 和 Client Credentials 流程。

## 二、第三方：以 OpenBao 为主要开发目标

以下有直接以 OpenBao 开发、使用 OpenBao SDK，或专案明示以 OpenBao 为目标的证据；**不表示全部已验证 OpenBao 2.7，更不表示全部可投产**。

### 密钥整合、同步与 API Token

| 专案 | 简述 |
|---|---|
| [GitHub Secrets](https://github.com/rknightion/openbao-plugin-secrets-github) | 透过 GitHub App 动态产生短效 Installation Token。 |
| [Bitwarden Secrets](https://github.com/salir-se/openbao-plugin-secrets-bitwarden) | 将 Bao KV 单向同步到 Bitwarden／Vaultwarden；仍为 pre-1.0，主要测试 Vaultwarden。 |
| [1Password Secrets](https://github.com/NavistAu/openbao-plugin-secrets-onepassword) | 透过 Bao ACL 读取 1Password Secrets；仅记忆体快取，不持久复制到 Bao。 |
| [Secret Sync](https://github.com/adfinis/openbao-plugin-secrets-sync) | 把 Bao 的密钥同步到 AWS Secrets Manager、K8s Secrets、GitLab Variables；**Preview，非正式生产版**。 |
| [Voidstar Secrets](https://github.com/NavistAu/openbao-plugin-secrets-voidstar) | 将其他 Secrets 映射成唯读虚拟路径，维持单一来源。 |
| [Kanidm Secrets](https://github.com/bjd5o/openbao-plugin-secrets-kanidm) | 为 Kanidm Service Account 动态签发及撤销 API Token；测试过 Bao 2.7.1。 |

### 基础设施、网路、凭证与密码学

| 专案 | 简述 |
|---|---|
| [Proxmox Secrets](https://github.com/dryvist/openbao-plugin-secrets-proxmox) | 动态发放 Proxmox VE API Token，租约到期撤销；真实 Proxmox 验收尚未完成。 |
| [Garage S3 Secrets](https://github.com/Tech-S1/openbao-plugin-secrets-garage) | 动态建立 Garage S3 Access Key 与 Bucket 权限。 |
| [Tailscale Secrets](https://github.com/SamPlaysKeys/openbao-plugin-secrets-tailscale) | 动态产生 Tailscale 装置加入用 Auth Key。 |
| [NATS Secrets](https://github.com/BonesOfGiants/openbao-plugin-secrets-nats) | 管理 NATS Operator／Account／User JWT 与连线凭证。 |
| [ACME Secrets](https://github.com/chisaato/openbao-plugin-secrets-acme) | ACME／Let's Encrypt 自动签发 TLS 凭证；支援 Cloudflare DNS-01。 |
| [GPG Secrets](https://github.com/contain-security/openbao-plugin-secrets-gpg) | OpenPGP／GPG 相关金钥与密码学整合。 |
| [secp256k1 Secrets](https://github.com/IndependentImpact/openbao-plugin-secp256k1) | 在 Bao 内保管 secp256k1 金钥并签署杂凑，适用 EVM。 |
| [Ethereum HD Wallet](https://github.com/lattn/openbao-plugin-eth-hdwallet) | 派生 Ethereum 地址及签署区块链交易。 |

### 资料库、认证与特定系统

| 专案 | 简述 |
|---|---|
| [ClickHouse Database](https://github.com/elaunira/openbao-plugin-database-clickhouse) | 动态建立与撤销 ClickHouse 帐号，支援 Cluster。 |
| [Teradata Secrets](https://github.com/JavierLimon/openbao-teradata-secret-plugin) | 使用 ODBC 管理 Teradata 动态帐号；仍在开发。 |
| [Public-Client JWT Auth](https://github.com/kylemd/openbao-public-client-plugin) | 扩充 JWT／OIDC Auth，支援无 Client Secret 的 PKCE 登入。 |
| [Open Horizon Auth](https://github.com/open-horizon/openbao-plugin-auth-openhorizon) | 使用 Open Horizon 身分验证登入 Bao。 |
| [GraphQL Secrets](https://github.com/markchristopherwest/openbao-plugin-secrets-graphql) | 对特定 GraphQL 身分伺服器签发／续期／撤销 JWT。 |
| [Vault Replicator](https://github.com/JavierLimon/openbao-vault-replicator-secret-plugin) | 把 HashiCorp Vault KV v2 单向复制到 OpenBao。 |

### KMS／Auto Unseal

| 专案 | 简述 |
|---|---|
| [SPIRE Auto-Unseal](https://github.com/magnusp/openbao-seal-spire) | 用 SPIFFE／SPIRE 动态身分存取上游 Transit，完成 Auto Unseal／External Keys。 |
| [Attested Unseal](https://github.com/adfinis/openbao-attested-unseal) | TPM 本地解封或远端 Broker 验证解封；**Functional Alpha**。 |
| [Securosys HSM Auto-Unseal](https://github.com/securosys-com/securosys-openbao-autounseal-plugin) | 经 Securosys HSM／CloudsHSM 做 Auto Unseal、External Keys；和官方清单的 Securosys 整合属相近功能来源，部署前核对实际版本。 |

## 三、第三方：Vault 起源，但明确提及／测试 OpenBao

| 专案 | 简述与相容性证据 |
|---|---|
| [GitLab Secrets](https://github.com/ilijamt/vault-plugin-secrets-gitlab) | 动态建立／撤销 GitLab Personal、Project、Group、Deploy 等 Token；README 明列 Vault／OpenBao。 |
| [Harbor Secrets](https://github.com/corelyr-oss/vault-plugin-secrets-harbor) | 动态建立短效 Harbor Robot Account；CI 涵盖 OpenBao 2.x。 |
| [NetBox Secrets](https://github.com/ljb2of3/vault-plugin-secrets-netbox) | 动态建立 NetBox API Token；README 记录 OpenBao 2.5.5 测试。 |
| [Honeycomb Secrets](https://github.com/jharley/vault-plugin-secrets-honeycombio) | 动态建立 Honeycomb API Key；已更新 OpenBao 依赖至 2.7.1，但不代表完成实机相容测试。 |

## 四、第三方：2026 年仍有维护的 Vault Plugins，OpenBao 2.7 相容性未确认

| 专案 | 简述 |
|---|---|
| [Cloudflare Secrets](https://github.com/Believe-SA/vault-cloudflare-secret-engine) | 动态产生、限制及撤销 Cloudflare API Token；支援衍生 R2 S3 Credentials。 |
| [JFrog Artifactory Secrets](https://github.com/jfrog/vault-plugin-secrets-artifactory) | 动态建立 Artifactory Access Token。 |
| [Vault ACME](https://github.com/Boostport/vault-plugin-secrets-acme) | 透过 ACME CA 签发 TLS 凭证。 |
| [Grafana Secrets](https://github.com/Boostport/vault-plugin-secrets-grafana) | 动态建立 Grafana Cloud 或 Service Account Token。 |
| [LiteLLM Secrets](https://github.com/benemon/vault-plugin-secrets-litellm) | 管理 LiteLLM Virtual Key、租约与撤销。 |
| [Keycloak Secrets](https://github.com/Serviceware/vault-plugin-secrets-keycloak) | 读取／管理 Keycloak Client Secret。 |
| [Vercel Secrets](https://github.com/thevilledev/vault-plugin-secrets-vercel) | 动态建立 Vercel 部署用 API Token。 |
| [Temporal Cloud Secrets](https://github.com/temporal-sa/vault-plugin-secrets-temporalcloud) | 动态建立 Temporal Cloud Service Account／API Key。 |
| [Exoscale Secrets](https://github.com/exoscale/vault-plugin-secrets-exoscale) | 动态建立带 IAM Role 的 Exoscale API Key。 |
| [WebAuthn Auth](https://github.com/trublast/vault-plugin-auth-webauthn) | 以 FIDO2／Passkey／安全金钥登入。 |

**特别关注：** Cloudflare Secrets 很贴近把固定 Cloudflare API Token 改成动态、按权限及 TTL 发放的需求，但现有 CI 显示 Vault 版本测试，**未核实 OpenBao 2.7 实际相容性**。

## 五、曾查到但近期不活跃／暂不列为维护中的项目

| 专案 | 简述 | 最近查得的提交 |
|---|---|---|
| [Nebula Secrets](https://github.com/mkrauser/openbao-plugin-secrets-nebula) | 管理 Nebula CA、节点凭证与轮换。 | 2025-08 |
| [Oracle Database](https://github.com/cmmoran/openbao-plugin-database-oracle) | Oracle 动态资料库帐号管理。 | 2025-07 |
| [Nomad Secrets（旧第三方）](https://github.com/JanMa/openbao-plugin-secrets-nomad) | Nomad ACL Token 管理；官方另有 Nomad Secrets。 | 2025-07 |

## 六、查找与使用时的判断

- **官方 24 个**指原索引记录时 `openbao/openbao-plugins` README 的清单，不等于全部外挂；OAuthApp 是官方组织另有独立 Repo 的例子。
- **有 2026 年 GitHub Commit ≠ 积极功能维护**：部分只是 Dependabot／CI 更新；也不代表能安全投入正式环境。
- **支援 OpenBao ≠ 已验证 OpenBao 2.7**：看 README、SDK、CI 是否真的在 Bao Server 注册、挂载、签发、续期、撤销并测试成功。
- 这份清单是**精简功能索引**。安装前仍以各 Repo 最新 README、Release、相容矩阵与安全纪录为准；没有将 Built-in Engine、MCP 工具或 Smallstep 的 CLI 插件列入 Server External Plugin。

主要入口：[OpenBao Plugin 文件](https://openbao.org/docs/plugins/)｜[OpenBao 官方外挂仓库](https://github.com/openbao/openbao-plugins)｜[官方发行档](https://github.com/openbao/openbao-plugins/releases)。
