# OpenBao 各 Engine 的权限详细说明：Namespace、ACL 万用字元与 API 路径

[English](engine-permissions.md)

> 本篇区分 Namespace 管理员、跨 Engine 业务权限与单一 KV Mount 权限，整理 ACL 万用字元、主要 API 路径、对照表、HCL、CLI、例外与设计建议。

**核心原则：OpenBao 的 Policy `path` 比对实际 API 路径，而不是 Secret Engine 类型。Namespace 是管理边界，Mount 是 API 前缀，Engine 定义后缀端点。**

## 一、Namespace Admin 与跨 Engine 读写的三种角色

1. **Namespace Admin**：管理该 NS 的 Secret Engine、Policy、Token、Auth Method 及其他管理端点。
2. **Namespace 全域 Secret Read/Write**：希望读写该 NS 的所有 Secret，但不能管理 Policy、Engine 等系统资源。
3. **单一 KV Engine Read/Write**：只读写某一个已指定的 KV Engine Mount。

三种概念可以区分，但第二种「所有 Engine 的业务读写」没有统一的 ACL 类型选择器，必须按照不同 Engine 的实际 API 路径授权。

### 1.1 既有 Policy 只对 `secret/` KV v2 生效

```hcl
path "secret/data/*" {
  capabilities = ["create", "read", "update", "patch", "delete"]
}
path "secret/metadata" {
  capabilities = ["list"]
}
path "secret/metadata/*" {
  capabilities = ["list"]
}
```

`secret` **不是保留字，是 Mount 名称**。

```text
agent/ Namespace
├── secret/        KV v2
├── credentials/   KV v2
├── projects/      KV v2
├── pki/           PKI
└── ssh/           SSH
```

| Engine | `secret/...` Policy 可读写？ |
| --- | --- |
| `secret/` | ✅ 可以（在授权的 KV v2 资料 API 范围内） |
| `credentials/` | ❌ 不可以 |
| `projects/` | ❌ 不可以 |
| `pki/` | ❌ 不可以 |
| `ssh/` | ❌ 不可以 |

新增其他 KV Engine 不会自动让原来的 `secret/...` Policy 涵盖它。

### 1.2 所有顶层 KV v2 Mount 的一般资料读写与列表

OpenBao ACL 的 `+` 可匹配单一层的路径：

```hcl
# 匹配任意顶层 KV v2 Mount 的资料 API
path "+/data/*" {
  capabilities = ["create", "read", "update", "patch", "delete"]
}

# 列出任意顶层 KV v2 Mount 的 Key
path "+/metadata" {
  capabilities = ["list"]
}

path "+/metadata/*" {
  capabilities = ["list"]
}
```

匹配范例：

- `secret/data/erp/database`：可读写。
- `credentials/data/github/token`：可读写。
- `projects/data/project-a/password`：可读写。

因为 `+` 会匹配 `secret`、`credentials`、`projects` 等单一层级的 Mount 名称。

**两个限制：**

1. 这是**路径匹配，不是 Engine 类型匹配**。其他 Engine 若刚好有 `/data/...` 路由，也可能被涵盖。
2. 这不涵盖 PKI、SSH、Transit 等使用其他路径模式的 Engine；也不涵盖 `team/credentials/` 这类多层 Mount。另外，KV v2 的版本还原、销毁等端点需要额外授权，这不是全部 KV v2 操作的完整政策。

### 1.3 `path "*"` 不给 `sudo`，仍不是纯 Secret Read/Write

```hcl
path "*" {
  capabilities = [
    "create",
    "read",
    "update",
    "patch",
    "delete",
    "list",
    "scan"
  ]
}
```

这**不是安全的全域 Secret Read/Write Policy**。OpenBao 的 ACL 套在所有 API 上，包含：

- `sys/`：系统管理。
- `auth/`：身分、Token 与验证设定。
- `secret/`、`pki/` 等：不同 Secrets Engine。

没有 `sudo` 就不能呼叫要求额外 `sudo` 的管理 API，但仍可能管理不需要 `sudo` 的系统、Auth、Policy 资源。**只移除 `sudo`，不代表只剩秘密资料权限。**

### 1.4 Namespace Admin 和全域 KV v2 读写对照

| 操作 | Namespace Admin | KV v2 全域读写 |
| --- | --- | --- |
| 读写所有已授权的 KV Secret | 可以 | 可以 |
| 建立／移除 KV Engine | 可以 | 不可以 |
| 管理 PKI、SSH Engine | 可以 | 不可以 |
| 建立／修改 Policy | 可以 | 不可以 |
| 管理 Auth Method | 可以 | 不可以 |
| 管理 Token | 可以，但仍受签发规则限制 | 只有 `default` 等另行授予的 Token 自我管理能力 |
| 操作需要 `sudo` 的管理 API | 可以，仍受 NS 限制 | 不可以 |

不能把 `path "*"` 当成所有 Secrets Engine 的通用读写规则。

## 二、ACL 万用字元 `+` 和 `*`

- **`+`：只匹配一层路径，不跨越 `/`。**
- **`*`：前缀匹配，匹配后面所有字元，可跨任意多层 `/`。**

### 2.1 直接看范例

| Policy Path | 路径 | 匹配？ |
| --- | --- | --- |
| `secret/+/token` | `secret/abc/token` | ✅ |
| `secret/+/token` | `secret/abc/def/token` | ❌ |
| `secret/*` | `secret/abc` | ✅ |
| `secret/*` | `secret/abc/def/ghi` | ✅ |
| `secret/+/+/token` | `secret/abc/def/token` | ✅ |
| `secret/+/+/token` | `secret/abc/token` | ❌ |

### 2.2 拆解 `+/data/*`

```hcl
path "+/data/*" {
  capabilities = ["read"]
}
```

- `+`：任意 Mount，**恰好一层**。
- `data/`：固定字串，**必须相同**。
- `*`：任意后缀，**不限深度**。

```text
secret/data/abc              ✅
secret/data/abc/def/ghi      ✅
github/data/token            ✅
cloudflare/data/api          ✅

secret/abc/data/token        ❌
secret/metadata/abc          ❌
```

### 2.3 `*` 的限制与前缀匹配

**OpenBao 的 `*` 在 Policy Path 中只能放最后。**

```hcl
# ✅ 合法的路径 Pattern
path "secret/*" {}

# ✅ 合法的路径 Pattern
path "secret/+/data/*" {}

# ❌ 不能把 * 放在中间
path "secret/*/data" {}
```

这里空白的 `{}` 只用来展示 Path Pattern，不是有能力设定的正式授权规则；实际 Policy 需指定 `capabilities`。

`*` 与 Shell 万用字元不完全相同：`secret/abc*` 可匹配 `secret/abcdef`，也可能匹配 `secret/abcdef/xyz`。

**记法：`+` = 一层；`*` = 剩下全部。** 这就是 `+/data/*` 可以跨单层 Mount 名称匹配 KV v2 资料 API 的原因。

## 三、Secrets Engine 的 API 路径怎么组成

通用 API URL：

```text
https://bao.example.com/v1/<Mount>/<API 后缀>
```

若 PKI 自订 Mount 是 `internal-ca/`：

```text
internal-ca/issue/web
internal-ca/sign/web
internal-ca/roles/web
internal-ca/cert/123456
```

以下后缀均相对于 Mount。`<name>`、`<role>`、`<key>` 等表示实际名称，不应原样填入 Policy。

**`data/*`、`metadata/*` 不是所有 Engine 的标准，而是 KV v2 的 API 格式。** 主要路径族先记：

- KV v2：`data/*`、`metadata/*`、`delete/*`、`undelete/*`、`destroy/*`。
- PKI：`roles/*`、`issue/*`、`sign/*`、`cert/*`、`revoke` 等。
- SSH：`roles/*`、`sign/*`、`creds/*` 等。
- Transit：`keys/*`、`encrypt/*`、`decrypt/*`、`sign/*` 等。
- Database：`config/*`、`roles/*`、`creds/*` 等。

`+/data/*` 并非「所有 KV v2 Engine」类型选择器，而是匹配任意一层名称后方的 `data/...` 字串。

## 四、各 Engine 的主要 API 后缀总表

这是主要 Secrets Engine 和系统内建用途的**主要路径族**，不代表每种版本、Plugin 的完整端点全集。尤其 PKI、Transit 还有其他进阶路由，可透过 `bao path-help` 查部署版本。

| Engine | 存取／产生秘密的路径后缀 | 设定／管理的路径后缀 |
| --- | --- | --- |
| **KV v1** | `<任意路径>` | 没有 KV v2 固定的 `data/`、`metadata/` 分层 |
| **KV v2** | `data/*`、`subkeys/*` | `metadata`、`metadata/*`、`delete/*`、`undelete/*`、`destroy/*`、`config` |
| **PKI** | `issue/<role>`、`sign/<role>`、`sign-verbatim/*`、`cert/<serial>`、`certs`、`ca`、`ca_chain`、`crl`、`ocsp` | `roles/*`、`issuers`、`issuer/*`、`keys`、`key/*`、`root/*`、`intermediate/*`、`config/*`、`revoke`、`tidy*`、`acme/*` |
| **SSH** | `sign/<role>`、`issue/<role>`、`creds/<role>`、`verify`、`public_key` | `roles`、`roles/*`、`lookup`、`issuers`、`issuer/*`、`config/ca`、`config/issuers`、`config/zeroaddress`、`tidy/*` |
| **Transit** | `encrypt/*`、`decrypt/*`、`rewrap/*`、`sign/*`、`verify/*`、`hmac/*`、`hash/*`、`random/*`、`datakey/*` | `keys`、`keys/*`、`export/*`、`byok-export/*`、`wrapping_key`、`config/*` |
| **Database** | `creds/<role>`、`static-creds/<role>` | `config/*`、`roles`、`roles/*`、`static-roles`、`static-roles/*`、`rotate-root/*`、`rotate-role/*`、`reset/*` |
| **Kubernetes** | `creds/<role>` | `config`、`roles`、`roles/*` |
| **TOTP** | `code/<name>` | `keys`、`keys/<name>` |
| **RabbitMQ** | `creds/<role>` | `config/connection`、`config/lease`、`roles`、`roles/*` |
| **LDAP** | `creds/*`、`static-cred/*`、`library/<name>/check-out` | `config`、`rotate-root`、`role/*`、`static-role/*`、`rotate-role/*`、`library/*` |
| **Cubbyhole** | `<任意路径>` | Token 专属的私密储存区 |
| **Identity** | `entity/*`、`entity-alias/*`、`group/*`、`group-alias/*`、`lookup/*` | `oidc/*`、`mfa/*` 与身分管理端点 |

**LDAP 的版本差异：**Bao 2.7 的 LDAP 需另装相容外挂；旧版清单曾包含 LDAP、OpenLDAP。实际以安装版本与 Plugin 清单为准。Cubbyhole、Identity 是内建用途，不宜当作一般自行 Mount 的业务 Engine。

## 五、最重要的 Engine 路径拆解

### 5.1 KV v2

| 后缀 | 用途 | 常见 ACL |
| --- | --- | --- |
| `data/<key>` | 读、写、修改、软删 Secret | `create/read/update/patch/delete` |
| `metadata` | 列出 KV 根目录 | `list` |
| `metadata/<key>` | 列出子路径、读写 Metadata、管理版本 | 视操作使用 `list/read/update/delete` |
| `subkeys/<key>` | 查看资料栏位结构，不回传 Secret 值 | `read` |
| `delete/<key>` | 软删指定版本 | `update` |
| `undelete/<key>` | 复原软删的版本 | `update` |
| `destroy/<key>` | 永久销毁指定版本 | `update` |
| `config` | Engine 全域设定 | `read/update` |

**`metadata/*` 不只是列举用途。** 若授予 `delete`，可以删除 Metadata 和所有 Secret 版本；单纯 `list` 没有这个效果。

### 5.2 PKI

PKI 区分签发凭证与管理 CA／Role：

| 后缀 | 用途 |
| --- | --- |
| `issue/<role>` | 产生新私钥及凭证 |
| `sign/<role>` | 签署现有 CSR |
| `sign-verbatim/<role>` | 特殊签署方式，不走一般角色栏位限制 |
| `issuer/<issuer>/issue/<role>` | 透过指定 Issuer 签发 |
| `issuer/<issuer>/sign/<role>` | 透过指定 Issuer 签署 CSR |
| `cert/<serial>`、`certs` | 查询凭证与列出序号 |
| `ca`、`ca_chain`、`crl`、`ocsp` | 查询 CA、凭证撤销资讯 |
| `roles`、`roles/<role>` | 管理签发 Role |
| `issuers`、`issuer/<issuer>` | 管理 CA Issuer |
| `keys`、`key/<key>` | 管理 CA 私钥 |
| `root/*`、`intermediate/*` | 建立或管理 Root／Intermediate CA |
| `config/*` | CRL、URL、Issuer、Key 等设定 |
| `revoke`、`revoke-with-key` | 撤销凭证 |
| `tidy`、`tidy-status` | 清理与查询清理状态 |
| `acme/*` | ACME 协定端点 |

这些是主要路径族，部分还包含 `pem`、`der`、`raw`、Issuer-specific 等细分端点。

### 5.3 SSH

| 后缀 | 用途 |
| --- | --- |
| `sign/<role>` | 签署 SSH Public Key、取得 SSH Certificate |
| `issue/<role>` | 产生 SSH Key Pair 与 Certificate |
| `creds/<role>` | 产生 SSH OTP |
| `verify` | 验证 SSH OTP |
| `roles`、`roles/<role>` | 管理 SSH Role |
| `lookup` | 依 IP 查询可用 Role |
| `config/ca` | SSH CA 设定与查询 |
| `config/issuers` | 预设 Issuer 设定 |
| `issuers`、`issuer/<ref>` | 管理多个 Issuer |
| `issuer/<ref>/public_key`、`public_key` | 取得 SSH CA 公钥 |
| `config/zeroaddress` | 管理不受特定 IP 限制的 OTP Role |

如需使用 SSH CA，最常见的业务权限是 `ssh/sign/<role>`，不是管理 Role 的 `ssh/roles/*`。

### 5.4 Transit

Transit 提供密码学操作，而不只是储存并读取 Secret：

| 后缀 | 用途 |
| --- | --- |
| `encrypt/<key>` | 加密 |
| `decrypt/<key>` | 解密 |
| `rewrap/<key>` | 重新包装旧版密文 |
| `sign/<key>` | 签章 |
| `verify/<key>` | 验证签章 |
| `hmac/<key>` | 产生 HMAC |
| `hash/*`、`random/*` | 杂凑与随机数 |
| `datakey/plaintext/<key>` | 产生 Data Key，回传明文 |
| `datakey/wrapped/<key>` | 产生 Data Key，只回传加密版本 |
| `keys`、`keys/<key>` | 管理 Transit Key |
| `keys/<key>/rotate` | 轮换该 Key |
| `keys/<key>/config` | 修改该 Key 设定 |
| `export/*`、`byok-export/*` | 汇出符合条件的金钥材料 |

同一 Transit Engine 可以授权某服务只能呼叫 `encrypt/`，不授权 `decrypt/`，两者可依路径分开控制。

## 六、不同 Engine 的后缀可能撞名

| 相同后缀 | 可能出现的 Engine |
| --- | --- |
| `roles/*` | PKI、SSH、Database、Kubernetes、RabbitMQ |
| `creds/*` | Database、SSH、Kubernetes、RabbitMQ、LDAP |
| `config/*` | PKI、SSH、Database、Transit 等 |
| `keys/*` | Transit、TOTP、PKI |
| `sign/*` | PKI、SSH、Transit |

即使同样是 `creds/<role>`，实际用法也可能不同：

```bash
# Database：读取时动态产生资料库帐密
bao read database/creds/my-role

# Kubernetes：写入请求以建立 ServiceAccount Token
bao write kubernetes/creds/my-role \
  kubernetes_namespace=test
```

前者是 `read` Capability；后者是写入型 API，通常需要 `update` 等能力。

所以：

```hcl
path "+/creds/*" {
  capabilities = ["read"]
}
```

**不代表所有 Engine 都能取到凭证**，因为不是每个 Engine 都以 GET 产生 Credential。

KV v1 的 Secret 路径完全可以自由命名：

```text
legacy-kv/data/password
legacy-kv/roles/admin
```

这些也许只是 KV v1 里普通的资料，而不是 KV v2 的 Data API 或其他 Engine 的 Roles API。**ACL 万用字元判断的是路径字串，不是 Engine 类型。**

## 七、Engine 专用业务 Policy 范例

### PKI：只允许使用 `web` Role 签发凭证

```hcl
path "pki/issue/web" {
  capabilities = ["create", "update"]
}
```

### SSH：只允许 SSH CA 签署 `linux-user` Role

```hcl
path "ssh/sign/linux-user" {
  capabilities = ["create", "update"]
}
```

### Transit：只允许使用 `app-key` 加密

```hcl
path "transit/encrypt/app-key" {
  capabilities = ["update"]
}
```

### Database：只允许取得 `erp-reader` 的动态凭证

```hcl
path "database/creds/erp-reader" {
  capabilities = ["read"]
}
```

### Kubernetes：只允许取得 `erp` Role 的 Token

```hcl
path "kubernetes/creds/erp" {
  capabilities = ["update"]
}
```

**各 Engine 的业务权限不一定是 `read`。** TLS／SSH 凭证签发、Transit 加密等虽然属于「使用服务」，却是写入型 API；ACL 必须按实际端点授权。

## 八、查询目前 OpenBao 版本的完整 API 路径

官方提供：

```bash
bao path-help <mount>
```

例如：

```bash
export BAO_NAMESPACE=agent

bao secrets list

bao path-help secret
bao path-help pki
bao path-help ssh
bao path-help transit
bao path-help database
bao path-help kubernetes
```

可以列出该 Engine 的路由模式，通常是近似正规表示式的具体路由。也可指定某项操作：

```bash
bao path-help pki/issue/web
bao path-help ssh/sign/linux-user
bao path-help transit/encrypt/app-key
```

这能显示端点参数与用途，避免人工一页页猜测。实际资料取决于部署版本、Mount 与存取权限。

另外：

```bash
# 目前安装的 Secrets Plugin
bao plugin list secret

# 当前 Namespace 实际挂载的 Engine
bao secrets list -detailed
```

## 九、通用与跨 Engine Policy 怎么设计

### 9.1 只要全部顶层 KV v2 的一般读写

在已确认 Mount 命名与类型的环境，`+/data/*`、`+/metadata`、`+/metadata/*` 可以作为常见 Data/List ACL，但**不是严格的 KV v2 类型过滤器**。

### 9.2 KV、PKI、SSH、Transit、Database 等一起授权

依 Engine 类型与实际 Mount 产生 Policy：

```hcl
# KV v2
path "secret/data/*" {
  capabilities = ["create", "read", "update", "patch", "delete"]
}

path "secret/metadata" {
  capabilities = ["list"]
}

path "secret/metadata/*" {
  capabilities = ["list"]
}

# PKI 签发
path "pki/issue/*" {
  capabilities = ["create", "update"]
}

# SSH 凭证签发
path "ssh/sign/*" {
  capabilities = ["create", "update"]
}

# Transit 加密与解密
path "transit/encrypt/*" {
  capabilities = ["update"]
}

path "transit/decrypt/*" {
  capabilities = ["update"]
}

# Database 动态凭证
path "database/creds/*" {
  capabilities = ["read"]
}
```

**这只是跨 Engine 的业务操作示范，不是各 Engine 的所有功能全集**。需要额外操作时，仍应按实际 Mount 与端点扩充。

### 9.3 Bao MCP 呼叫者的权限分层

- `namespace-admin`：`path "*"` 以及管理用途需要的 `sudo`，负责 NS 内管理端点，仍受 Namespace 限制。
- `kv-all-readwrite`：对单层 KV v2 Mount 使用 `+/data/*`、`+/metadata`、`+/metadata/*`。
- `kv-readwrite`：针对单一 Mount，例如 `secret/`。
- `kv-scoped-readonly`：限定某 Mount 的指定 Secret 或路径。
- 跨 Engine 业务角色：按 PKI／SSH／Transit／Database 等类型的实际 `issue/`、`sign/`、`encrypt/`、`decrypt/`、`creds/` API 授权。

如果需求是「NS 底下所有 Engine 的业务操作都能用，但不准管理 Engine 与 Policy」，OpenBao 没有原生的 `all-secret-engines-only` 或 `secret-engine/*` ACL 类型关键字，必须依 Engine API 路径授权。

**不要拿 `path "*"` 当作全域 Secret Operator。** 因为它连 `sys/`、`auth/` 都匹配，最后可能得到的是接近管理员的能力。

## 十、结论

1. Namespace 负责管理边界；Policy 的 `path` 比对 API 路径，而不是 Engine 类型。
2. `+` 匹配一层；`*` 在尾端作前缀匹配，涵盖剩余多层。
3. `data/`、`metadata/` 是 KV v2 的路径族，其他 Engine 各有不同的后缀。
4. 不同 Engine 的 `roles/`、`creds/`、`sign/`、`keys/` 等可能重名，但操作与 Capability 不同。
5. `path "*"` 没有 `sudo` 也不保证只剩秘密资料权限。
6. 跨 Engine 管理需要按实际挂载与路由制定 Policy。
7. 本篇整理主要路径族，不代表每个版本的全部端点；实际以部署版本与 `bao path-help` 为准。

参考官方文件：[OpenBao Secrets Engines](https://openbao.org/docs/secrets/) · [OpenBao ACL Policies](https://openbao.org/docs/concepts/policies/) · [OpenBao Commands](https://openbao.org/docs/commands/)。
