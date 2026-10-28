# OpenBao Secret Engine 与 Namespace 架构：内建类型、挂载指令与设计原则

[English](secret-engines-namespaces.md)

OpenBao 使用 **Namespace** 作为管理空间，并在 Namespace 内按需要启用各种 **Secret Engine**。全新的 Root Namespace 只有系统内建的功能，不会自动建立 KV、PKI、SSH 或 Transit 等业务用 Secret Engine。

同一个 Namespace 可启用多种不同 Engine，也可将相同类型的 Engine 挂载多次。Root Namespace 是预设的最上层命名空间，**不是档案系统中的 `/root/` 目录**。

## 一、OpenBao 的 Secret Engine 有哪些？

OpenBao 官方文件涵盖的主要 Secret Engine 如下；实际可用类型取决于版本与已安装外挂。

| Engine | 功能 | 使用情境 |
| --- | --- | --- |
| **KV v1 / v2** | 保存一般 Key-Value 秘密，v2 支援版本纪录 | API Token、密码、环境变数 |
| **PKI** | 建立 CA、签发与撤销 X.509 凭证 | HTTPS、mTLS、内部 CA |
| **SSH** | 使用 SSH CA 签发 SSH 凭证，也支援 OTP 模式 | SSH 短期登入凭证 |
| **Transit** | 加密、解密、签章、验章，金钥留在 Bao | Encryption as a Service |
| **Database** | 动态产生资料库登入凭证，管理有效期限 | PostgreSQL、MySQL 等 |
| **Kubernetes** | 动态管理 Kubernetes ServiceAccount Token 等凭证 | 需要存取 K8s API 的服务 |
| **TOTP** | 产生与验证一次性密码 | 2FA、OTP |
| **LDAP / OpenLDAP** | 管理 LDAP 帐号密码及相关动态凭证；Bao 2.7 需另装相容外挂 | LDAP 目录服务 |
| **RabbitMQ** | 动态产生 RabbitMQ 使用者凭证 | 讯息伫列服务 |

对一般 Secret 管理与基础设施自动化，最值得先认识的四种是：

- **KV**：存放既有密码与 API Token。
- **PKI**：签发与管理 TLS 凭证。
- **SSH**：签发 SSH 登入凭证。
- **Transit**：提供加密、解密、签章等服务，而不把底层金钥直接交给应用程式。

**Kubernetes Secret Engine 与 Kubernetes Auth Method 不同。** 前者处理 K8s 相关凭证的发放，后者用于让 Kubernetes 里的工作负载向 OpenBao 验证身分。

## 二、Root Namespace 预设有哪些东西？

一个刚初始化的 Root Namespace 有系统预设挂载，例如：

```text
OpenBao Cluster
└── Root Namespace（预设）
    ├── sys/          ← 系统控制、Policy、管理 API
    ├── identity/     ← 身分资讯管理
    └── cubbyhole/    ← 每个 Token 私有的 Secret 空间
```

这些不是一般专案拿来存放密码的 KV Engine。想要建立 KV、PKI、SSH 或 Transit，需要额外启用。

启用后的 Root Namespace 可以长这样：

```text
OpenBao Cluster
└── Root Namespace（预设）
    ├── sys/          ← 内建
    ├── identity/     ← 内建
    ├── cubbyhole/    ← 内建
    ├── secret/       ← 自行启用的 KV v2
    ├── pki/          ← 自行启用的 PKI
    ├── ssh/          ← 自行启用的 SSH
    └── transit/      ← 自行启用的 Transit
```

上面四个自行启用的 Engine 都是**选用**，不是每个 Namespace 的固定配置。

## 三、如何建立 Secret Engine？

以下假设 CLI 已连线到 OpenBao，且目前操作的是 Root Namespace，所用 Token 具有相应管理权限。

```bash
# KV v2：保存一般 Secret
bao secrets enable -path=secret kv-v2

# PKI：签发 TLS 凭证
bao secrets enable -path=pki pki

# SSH：签发 SSH 凭证
bao secrets enable -path=ssh ssh

# Transit：加密、解密、签章
bao secrets enable -path=transit transit
```

查看目前已挂载的引擎：

```bash
bao secrets list
```

### Enable 不等于已完成所有设定

`bao secrets enable` 只是建立挂载点，后续仍可能需要配置其功能：

- PKI：配置 CA 与签发用 Role。
- SSH：配置凭证签发方式。
- Transit：建立所需的加密或签章金钥。

### 同一种类型可以挂载多次

例如建立两个 KV v2 Engine：

```bash
bao secrets enable -path=agent-secrets kv-v2
bao secrets enable -path=k8s-secrets kv-v2
```

这时候有两个独立的 Engine 实例：

- `agent-secrets/`
- `k8s-secrets/`

两者拥有各自的 Engine 设定与储存空间。**Mount Path 是挂载名称，不代表两者必须使用不同的 OpenBao Server。**

## 四、Namespace、Engine、Path 该怎么区分？

- **Namespace**：定义管理空间，可以分别管理 Policy、Token、Auth Method 与 Secret Engine。
- **Secret Engine / Mount**：定义一组储存或秘密操作功能，例如 KV、PKI、SSH CA。
- **Path**：Engine 内部的资源名称或路径；对 KV 而言，可用来区分不同专案的 Secret。

如果 Agent 与 Kubernetes 基础设施需要独立的管理体系，可规划如下：

```text
Root Namespace
├── agent/                  ← Namespace
│   └── secret/             ← KV Engine
│       ├── github/token
│       └── cloudflare/api
└── k8s/                    ← Namespace
    ├── secret/             ← KV Engine
    ├── pki/                ← PKI Engine
    └── ssh/                ← SSH Engine
```

但如果所有东西都是由同一个管理者维护，没有分开 Policy、Token 与 Auth Method 管理范围的需求，**直接在 Root Namespace 挂载不同 Engine，再利用 Policy 控制存取即可**，不一定需要再拆 Namespace。

### 什么时候拆 Namespace？什么时候只拆 Engine？

| 需求 | 建议方式 |
| --- | --- |
| 同一个团队管理不同用途的秘密 | 先用不同 KV 路径或不同 Mount |
| 需要不同的凭证功能，例如 KV、PKI、SSH | 启用不同 Secret Engine |
| 需要各自管理 Policy、Token、Auth Method 与 Engine | 考虑独立 Namespace |

## 五、删除 Engine 的注意事项

`bao secrets disable` **不是暂时停用或隐藏 Engine 的开关**。停用 Mount 会删除该 Engine 储存的资料，并撤销它管理的秘密。正式环境不要把 `disable` 当作无风险的切换操作。

## 六、结论

OpenBao 的层级可概括为：

```text
OpenBao Cluster
  └── Namespace（Root 预设存在，可按需要另外建立）
       └── Secret Engine / Mount（依需要自行启用）
            └── Path / Secret / Field（以 KV 为例）
```

**Namespace 决定管理范围；Engine 决定提供何种 Secret 功能；Path 则决定 Engine 内要操作哪个资源。**

对只有单一管理者的环境，可先维持 Root Namespace，启用所需 Engine，再以 Policy 区分 Agent、Kubernetes 与其他应用的权限。真正需要独立管理边界时，再另外使用 Namespace。

## 参考资料

- [OpenBao 官方文件：Secrets Engines](https://openbao.org/docs/secrets/)
