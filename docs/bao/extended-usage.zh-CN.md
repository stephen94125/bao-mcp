# OpenBao 延伸使用：Agent、systemd Credentials、K8s Secret 注入、SSH／PKI 与高可用性

[English](extended-usage.md)

> 原始整理日期：2026-10-28。本篇涵盖 VM／systemd 取得凭证、OpenBao Agent、Kubernetes 整合及其他 Engine 的延伸用法。设定为范例，实际部署时仍须依 Bao、Helm Chart、Kubernetes 版本核对。

## 一、定位：不只是密码保险箱

OpenBao 可以逐步成为基础设施的「秘密管理与身分授权中心」。不需要一次把所有 Kubernetes Secret、Linux 服务密码、SSH、TLS 凭证搬进去，可先从少数服务开始，依风险与可用性需求决定扩展顺序。

| 层次 | 目标 | OpenBao 能做的事 |
| --- | --- | --- |
| 1. 秘密集中管理 | 不要到处散落明文密码 | KV v2 + Policy |
| 2. 秘密不持久落地 | VM 磁碟、Git、Container Image 不存服务秘密 | Agent + tmpfs / Credentials |
| 3. 减少长期秘密 | 让原本长期有效的密码变成短期可撤销凭证 | Database、SSH、PKI、短效 Token |

第一层减少管理分散；第二层避免在持久储存中留下明文；第三层透过短期授权、凭证及轮换，缩小单一秘密外泄后的有效时间。三层可独立推进，不必绑成一次性的大型改造。

## 二、OpenBao Agent 与 systemd 的分工

**OpenBao Agent 是 OpenBao 提供的本机常驻程式，不是 systemd Agent。** 它可用 `bao` 的 agent 模式执行：

```bash
bao agent -config=/etc/openbao/agent.hcl
```

它可以在 Linux、VM、容器等环境运作，替不认识 OpenBao API 的应用程式处理自动认证、秘密取得及设定档产生。

### Agent 能力一览

| 功能 | 实际用途 |
| ------------------ | ------------------------- |
| Auto-Auth          | 自动登入 OpenBao，取得及续期 Token  |
| Template           | 把秘密渲染为设定档、密码档或凭证档         |
| API Proxy          | 让应用程式透过本机代理使用 OpenBao API |
| Cache              | 快取部分 Token、租约秘密和 API 回应   |
| Process Supervisor | 启动子程序并注入环境变数              |
| 自动更新               | 秘密或凭证更新时重新渲染档案            |

上述 Process Supervisor 在原研究时仍标示为 Public Beta。Agent Cache 也不是保证所有秘密都可离线恢复的通用储存机制。

例如原本：

```text
/etc/myapp/config.yaml
└── password: 明文密码
```

可以改为：

```text
OpenBao KV → OpenBao Agent → /run/myapp/config.yaml → myapp.service
```

`/run` 通常为 tmpfs，可让秘密只存在执行期记忆体档案系统。服务既可以直接读取 Agent 产生的设定，也可以搭配 systemd Credentials。

## 三、systemd：启动时透过 OpenBao 动态取得 Credentials

### 3.1 原生能力与边界

`systemd-creds` 既有的加密方式主要为 `host`、`tpm2`、`host+tpm2`，**并没有把 OpenBao 当作 `LoadCredentialEncrypted=` 的原生加密后端**。

但是 `systemd.exec` 的 `LoadCredential=` 可以从档案或 Unix Socket 取得 Credential。这可以让 Linux VM 即使没有 TPM，也在服务启动时拿到服务密码，而不必把服务密码明文长期存到虚拟磁碟。

注意：`LoadCredential=` 是**服务启动时载入**，不是程式每次打开密码档都重新向 OpenBao 查询。OpenBao 端轮换后，应用程式通常要配合重启／重载或重新取得秘密。

### 3.2 方案 A：Unix Socket Credential Broker

架构：

```text
OpenBao（KV／Transit）
      ↓ 验证身分、取得或解密秘密
Credential Broker
      ↓ Unix Socket
systemd LoadCredential=
      ↓
$CREDENTIALS_DIRECTORY/db-password
      ↓
应用程式
```

示意 `myapp.service`：

```ini
[Unit]
Description=My Application
Requires=bao-credential.socket
After=bao-credential.socket

[Service]
LoadCredential=db-password:/run/bao-credential.sock
ExecStart=/usr/local/bin/myapp
```

`/run/bao-credential.sock` 必须由额外的 Broker 提供，**OpenBao Agent 并不会因为启动就自动提供这个特定 Socket**。完整使用还需要自行实作 Broker 的读取请求、向 Bao 认证／读密钥及 Socket 回复流程。

执行过程：

1. systemd 准备启动 `myapp.service`。
2. `LoadCredential=` 连线到 Broker 的 Unix Socket。
3. Broker 依 Policy 向 OpenBao 请求或解密秘密。
4. Broker 把明文透过 Socket 交给 systemd。
5. systemd 放入服务专属 Credentials Directory。
6. 服务读取 `$CREDENTIALS_DIRECTORY/db-password`。

优点是没有额外的「中间明文设定档」；代价是维护一支自制 Broker，还要解决 Broker 自身的身分验证。

### 3.3 方案 B：OpenBao Agent + tmpfs + LoadCredential

这是较简单、优先考虑的作法：

```text
OpenBao KV → Agent Auto-Auth / Template → /run/myapp/db-password → systemd LoadCredential=
```

Agent Template 示意：

```hcl
template {
  contents = "{{ with secret \"secret/data/myapp\" }}{{ .Data.data.password }}{{ end }}"
  destination = "/run/myapp/db-password"
  perms = "0400"
  backup = false
}
```

systemd 服务：

```ini
[Service]
LoadCredential=db-password:/run/myapp/db-password
ExecStart=/usr/local/bin/myapp
```

设定上需保证 Agent 已经认证成功、`/run/myapp/db-password` 已产生，服务才开始载入；并设定目录权限，避免其他服务误读。`backup = false` 是避免 Agent 建立额外备份档案，不是整台 VM 自动免除 swap、休眠或快照外泄风险。

若应用程式本来就会读取 `/run/myapp/config.yaml`，也可以直接由 Agent 产生该档，不必硬套 systemd Credential。

### 3.4 方案 C：Git 密文 + OpenBao Transit

也可以把密文存进 Git，让 Broker 在服务启动时呼叫 OpenBao Transit `decrypt` API：

```text
Git 的密文 → Broker → OpenBao Transit decrypt → Unix Socket → systemd LoadCredential=
```

这是透过额外代理进行的解密流程，**不代表 systemd 的 `LoadCredentialEncrypted=` 直接支援 OpenBao 密文格式**。它可以用于想把密文版设定纳入 Git，但集中控制解密权限的场合。

### 3.5 VM 本地没有 TPM 时，如何证明身分？

服务密码不落地，跟「可以用来申请服务密码的长期身分凭据」不落地，是两个问题。

| 认证方式 | VM 磁碟需要存秘密？ | 自动开机 |
| --- | --- | --- |
| OpenBao AppRole + 本地 SecretID | 通常需要 | 可以 |
| OpenBao JWT + 云端工作负载身分 | 不必存长期秘密 | 可以 |
| Kubernetes ServiceAccount JWT | 不必存长期秘密于 VM 映像 | 可以 |
| Hypervisor 代为向 Bao 认证、注入凭证 | VM 不需要 | 可以 |
| 人工在开机时提供认证 | 不需要 | 不适合无人值守自动开机 |

在自管 VM 环境，可以考虑让 Hypervisor 或其他可信身分供应者负责向 OpenBao 认证，再把短期凭证或必要秘密注入 VM。这样攻击者只偷走「关机后的 VM 虚拟磁碟」，不能单凭磁碟读出秘密或重新向 OpenBao 请领。

这个模型不能抵御已控制执行中 VM 的攻击者；也要检查 swap、休眠映像、VM RAM 快照、备份是否可能让秘密意外持久化。

## 四、Kubernetes Secret 自动注入：三种主流做法

OpenBao Server 可以独立部署在 K8s 之外。Kubernetes 中只安装需要的整合元件，再让少数 Pod 试行即可；**不需要整个 Cluster 同步搬迁**。

### 4.1 Agent Injector：Pod 自动加 Agent

透过 Kubernetes Admission Webhook，为有对应 annotation 的新 Pod 注入 Agent init container／sidecar，秘密呈现在容器间共享的记忆体 Volume。

适合：不想改应用程式码、能让程式读档、希望秘密不再存入原生 Kubernetes Secret 的服务。

OpenBao 官方 Helm Chart 支援只启用 Injector、停用 Server 的 External OpenBao 模式。以下为原研究的示意指令，实际 Chart values 须依版本核对：

```bash
helm repo add openbao https://openbao.github.io/openbao-helm
helm repo update

helm upgrade --install openbao openbao/openbao \
  -n openbao --create-namespace \
  --set server.enabled=false \
  --set injector.enabled=true \
  --set csi.enabled=false \
  --set injector.externalBaoAddr=https://bao.example.com
```

对欲注入的 ERP Pod Template 设定，例如：

```yaml
template:
  metadata:
    annotations:
      vault.hashicorp.com/agent-inject: "true"
      vault.hashicorp.com/role: "erp"
      vault.hashicorp.com/agent-inject-secret-db: "secret/data/erp"
```

这些 annotation 的前缀在原研究中仍沿用 `vault.hashicorp.com`。须先在 OpenBao 设置 Kubernetes Auth Method、Role 与 ACL Policy，将 `erp` 绑定至特定 ServiceAccount 和允许的秘密路径。新的 Pod 才能在共享 Volume，例如 `/vault/secrets/db`，读取注入的资料。

其他未加 annotation 的 Deployment 可维持原状。

### 4.2 Secrets Store CSI Driver + OpenBao Provider

由节点端 CSI Provider 取得秘密，透过 `SecretProviderClass` 定义挂载到 Pod 的 Volume，不必每个 Pod 都部署 Agent Sidecar。

需要三个元素：

1. 通用的 Secrets Store CSI Driver。
2. OpenBao CSI Provider。
3. 工作负载的 `SecretProviderClass` 与 CSI Volume 宣告。

原研究中的 Provider Helm 安装示意：

```bash
helm upgrade --install openbao openbao/openbao \
  -n openbao --create-namespace \
  --set server.enabled=false \
  --set injector.enabled=false \
  --set csi.enabled=true \
  --set global.externalBaoAddr=https://bao.example.com
```

**注意：这不会顺便安装通用 Secrets Store CSI Driver；需要另外部署。** 适合偏好 Kubernetes Volume 模式，而且不想在 etcd 留下对应 Kubernetes Secret 的环境。是否及如何更新挂载内容，取决于 CSI Driver／Provider 的轮换设定。

### 4.3 External Secrets Operator（ESO）

ESO 在 K8s 内执行同步控制器，从 OpenBao 相容的 Vault API 把秘密复制为原生 Kubernetes Secret。应用程式原有的 Helm Chart、`secretKeyRef` 不必修改。

既有程式：

```yaml
env:
  - name: DB_PASSWORD
    valueFrom:
      secretKeyRef:
        name: erp-db
        key: password
```

只需另外建立 `ExternalSecret`，使 Operator 读取 `secret/data/erp`，同步到 `erp-db` Secret。

ESO 的 Vault Provider 支援 KV v1/v2 与 Kubernetes Auth；但它是第三方整合，使用 OpenBao 时应确认版本与功能相容性。

优点是渐进迁移成本低，而且 Bao 短暂故障时，已同步的 Kubernetes Secret 通常还在，新 Pod 仍能启动。代价是**秘密依旧存在 Kubernetes Secret／etcd**，要维持 Kubernetes RBAC、etcd 静态加密及备份保护。

### 4.4 K8s 方案快速比较

| 主要需求                           | 优先考虑                |
| ------------------------------ | ------------------- |
| 不修改既有 Helm Chart，保留 K8s Secret | ESO                 |
| 秘密只放在 Pod 记忆体挂载档案              | Injector            |
| 偏好 Kubernetes 原生 Volume 机制     | CSI                 |
| 应用程式需要直接呼叫 Bao API             | App SDK／Agent Proxy |

**逐步替换方式：** 同一 Cluster 中可保留部分原生 Kubernetes Secret，另选一个 Namespace 或 Deployment 试行 ESO、Injector 或 CSI。只要路径、ServiceAccount、Policy 和工作负载设定对应即可，无需一次改造全部 Helm Chart。

## 五、其他常见的 OpenBao 延伸场景

### 5.1 Linux／VM／systemd 服务

- `KV v2` 集中管理 PostgreSQL、Redis、ERP API Key、内部 Service Token。
- Agent 在启动时写入 `/run`，systemd 使用 `LoadCredential=`，或服务直接读取 Agent 产生的设定档。
- 主要收益是脱离静态明文设定与虚拟磁碟，并能依服务配置最小权限 Policy。

### 5.2 Database Secrets Engine：动态资料库帐号

- 让不同 ERP 或内部服务取得各自的 PostgreSQL／MySQL／MariaDB 资料库帐密。
- 可以产生有 TTL 的动态帐号，到期撤销，也能针对部分固定帐号安排密码轮换。
- 例如 24 小时帐号，降低长期共用 `erp_user` 的风险。
- 额外成本是资料库凭证生命周期更依赖 Bao 的可用性，需要规划续期与重新连线行为。

### 5.3 SSH Secrets Engine：SSH CA

- OpenBao 保存 SSH CA 私钥；使用者或自动化工具提供自己的 SSH 公钥请求签章。
- ACL Policy 决定可存取哪些签发 Role；SSH Role 限制 Principal、TTL 与凭证选项。
- 目标 SSH Server 只需事先信任 CA 公钥；**已签发凭证的登入验证不需要即时连回 OpenBao**。
- 适合减少 `authorized_keys` 的手动散布，并让权限过期可控。

### 5.4 PKI Secrets Engine + ACME：内部 TLS／mTLS

- OpenBao 可作为内部 CA，为 ERP、资料库、内部服务签发短期 TLS／mTLS 凭证。
- 透过 ACME Server 与相容 ACME Client 进行申请、更新。
- **OpenBao 在此提供 ACME Server，不是替 Traefik 向 Let's Encrypt 申请公开凭证的 Client。**
- 对外公开 HTTPS 可继续由 Traefik + Let's Encrypt 处理；之后再按需要逐步将内部 PKI 导入 Bao。

### 5.5 CI/CD、部署 Job、Codex 自动化

- 可利用工作负载的 JWT／OIDC 身分向 OpenBao Auth 登入。
- 依环境 Policy 给开发与正式部署不同的秘密与最小权限。
- 若执行平台能提供可验证的短期身分，可减少 CI/CD 平台持有长期固定 Token 的需要。

### 5.6 Docker Compose、Cron、备份

- OpenBao Agent、CLI 或 systemd 在 Job 启动前提供 S3 Token、Restic 密码、备份资料库帐密等。
- 传输形式可选 tmpfs 档案、标准输入、档案描述符（FD）或受控子程序环境。
- 不必让每个备份脚本自行重做 OpenBao API Client。

### 5.7 Transit：加解密、HMAC、签章

- OpenBao Transit 执行加密、解密、HMAC、数位签章及验证，并支援金钥轮换。
- 应用程式可以依 Policy 只有「加密」、「解密」或「签章」等特定权限。
- 适合敏感栏位、档案密文管理及中央代签。
- 若应用每次操作都要呼叫 Transit，OpenBao 会进入执行期的关键路径，需要先评估延迟及故障影响。

此外，Cloudflare API Token、GitHub PAT 及其他第三方 API Key 很适合放进 KV v2，但**集中保管不代表 OpenBao 能替所有第三方 Token 自动轮换**；能否轮换取决于对方 API 与整合实作。

## 六、K8s 的单点故障问题：依赖何时发生，比用了什么元件更重要

Kubernetes 是分散式系统，如果所有 Pod 在启动或每次请求都必须从单一 OpenBao 取得秘密，容易重新产生单点故障。并非所有 OpenBao 整合都会让既有服务立刻中断。

### OpenBao 停机时，各种整合的典型影响

| 使用方式                     | Bao 停机时的影响                  |
| ------------------------ | --------------------------- |
| 已启动的服务，使用既有静态秘密          | 通常继续运作                      |
| Agent Injector           | 现有 Pod 多半能继续；新 Pod 可能拿不到密码  |
| CSI Provider             | 新 Pod 需要挂载秘密时可能无法启动         |
| ESO                      | 已同步的 Secret 保留，新 Pod 通常仍可启动 |
| Database Dynamic Secrets | 现有帐密可用至到期；到期续约会受影响          |
| Transit 即时加解密            | 需要即时呼叫 Bao 的操作会失败           |
| SSH CA                   | 已签发凭证仍可验证；暂时无法签发新凭证         |
| PKI / ACME               | 已签发凭证仍有效；新的签发、续期受影响         |

有两个技术细节：

1. Agent Cache 有助于部分秘密与租约续期，但**不等于通用离线保险箱**；普通 KV v2 Secret 不应预设能从 Persistent Cache 恢复。
2. Injector Admission Webhook 的 `failurePolicy` 会影响注入失败时 Pod 建立的结果。原研究中的 Helm Chart 预设 `Ignore`，因此 Webhook 坏掉时可能放行「未注入秘密」的 Pod；仍须确保应用程式在缺少必要秘密时不会错误启动。

OpenBao 可用 Integrated Storage／Raft 建立高可用部署，但还要规划节点分散、储存、解封与故障域，不能只把 replicas 数字改成 3。

若目前是单节点 Bao，先让它扮演**集中储存与签发中心**，而不是所有核心应用每次操作都需要同步呼叫的服务。这能在安全收益与可用性间保留余裕。

### 目前哪些类型比较适合？

- **现在可逐步整合：** 静态 KV、SSH CA、启动时读取的秘密、一般管理与备份脚本。
- **之后扩展：** 大量动态资料库凭证、全面内部 PKI／ACME、频繁自动轮换服务帐密。
- **先确认高可用性：** 每个业务操作都要即时依赖的 Transit、核心服务高频动态授权。

## 七、建议的渐进式导入顺序

1. **先集中静态秘密到 KV v2。** 从 DevOps API Key、Cloudflare Token、非核心服务资料库密码开始；先建立可控路径与 ACL Policy。
2. **让一个非核心 Linux／systemd 服务使用 Agent + tmpfs。** 验证启动顺序、关机后不留明文，以及服务重启时如何重新取得秘密。
3. **K8s 只选一个 Namespace／Deployment 试行。** 想保留现有 Helm 设定先试 ESO；想避免 etcd 保存秘密，试 Injector／CSI。观察 Pod 重启、Bao 断线及秘密轮换时的行为。
4. **逐步扩展 SSH CA、CI/CD、内部 PKI。** 对外 Traefik／Let's Encrypt 可以先保留，内部凭证再分批改成 Bao 签发。
5. **最后才评估大量动态帐密、即时 Transit 与 OpenBao HA。** 先确认真的需要它们，再处理较高的维运成本与故障依赖。

## 八、总结

**最值得保留的架构原则：** OpenBao 先负责「集中管理、权限控制及短期凭证签发」，把需要它在线的时间缩到最低。

- systemd／VM：优先用 `Agent → /run tmpfs → LoadCredential=`；若真的要求秘密只流经 Socket，再开发 Credential Broker。
- K8s：选择 ESO、Injector 或 CSI，不必整批替换；关键是要决定是否容许秘密复制到 Kubernetes Secret，以及 Bao 停机时新 Pod 是否仍应可启动。
- SSH、PKI：已签发的凭证通常可在一定期间独立验证，是安全与可用性比较容易兼顾的用途。
- Transit、动态帐密：功能强，但要留意 OpenBao 在执行期成为依赖点。
- 真正的安全进步不只是「把密码藏进 Bao」，而是减少明文落地、缩短凭证生命周期，以及依身分／Policy 限缩每个服务拿到的权限。
