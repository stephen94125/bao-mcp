# OpenBao 各 Secrets Engine 的储存结构与 API 路径后缀详解

[English](engine-storage-api-paths.md)

本篇著重于 **各 Secrets Engine 保存或产生什么资源、Mount 后面有哪些 API 路径、每条路径能做什么，以及实际需要哪些 ACL Capability**。

Namespace 层级、Root／子 Namespace 权限、万用字元及广泛授权的限制，见 [路径表示法](path-notation.zh-CN.md)。

> 路径总表沿用原始 OpenBao 讨论笔记中的主要 API 族，**不是**宣称列出任一版本的所有 Endpoint。各 Engine 的细部路由以部署版本与 `bao path-help`／官方 API 文件为准。

## 一、不同 Engine 的「储存」本质并不相同

OpenBao 的 Mount 不一定是存放一笔笔 Secret 的资料夹。有些 Engine 储存 Key-Value，有些管理 CA／凭证，有些向外部系统动态签发凭证，也有些提供加解密运算。

| Engine | 主要管理／保存的资源 | 对应操作的本质 |
| --- | --- | --- |
| **KV v1** | 以任意路径命名的秘密资料及栏位 | 直接保存、读取或覆写秘密 |
| **KV v2** | 秘密资料、版本、Metadata | 附版本纪录的 KV 读写、软删、还原、销毁 |
| **PKI** | CA／Issuer、CA Key、Role、凭证及撤销资讯 | 签发、查询及撤销 X.509 凭证 |
| **SSH** | SSH CA／Issuer、Role、OTP 设定 | 签发 SSH 凭证或 OTP |
| **Transit** | 由 Bao 管理的加密／签章 Key | 加解密、签验章、HMAC、Data Key 等 API；不是「读取 Key 明文」的 KV |
| **Database** | DB 连线设定、动态 Role、Static Role | 向外部 DB 签发动态帐密或轮换静态帐号 |
| **Kubernetes** | K8s 连线设定、Role | 依 Role 产生 Kubernetes ServiceAccount 凭证 |
| **TOTP** | TOTP Key 设定 | 生成／验证一次性密码 |
| **RabbitMQ** | 连线设定、Role | 动态产生 RabbitMQ 使用者凭证 |
| **LDAP** | LDAP 连线、Role、Static Role、Library | 管理 LDAP 帐密与租借式凭证等功能 |

**Cubbyhole 和 Identity** 虽然也可能出现在 `bao secrets list`，但属于 OpenBao 内建系统用途：Cubbyhole 是每个 Token 私有的储存空间，Identity 管理身分及群组，不能直接当成一般自行 Mount 的业务 Secrets Engine。

## 二、Mount 与 Engine API 后缀

API URL 的概念式结构：

```text
https://bao.example.com/v1/<Mount>/<API 后缀>
```

例如将 PKI 挂在 `internal-ca/` 而非预设举例的 `pki/`，实际路径便是：

```text
internal-ca/issue/web
internal-ca/sign/web
internal-ca/roles/web
internal-ca/cert/123456
```

下文所有表格的「后缀」均指 **Mount 后面** 的 API 路径；`<name>`、`<role>`、`<key>`、`<issuer>` 是替换成具体资源名称的位置，不是固定字串。

**`data/`、`metadata/` 是 KV v2 的 API 结构，不是所有 Secrets Engine 都遵循的共同格式。**

## 三、各 Engine 主要 API 路径总表

| Engine | 存取／产生秘密的 API 后缀 | 设定／管理的 API 后缀 |
| --- | --- | --- |
| **KV v1** | `<任意路径>` | 不使用 KV v2 的固定 `data/`、`metadata/` 分层 |
| **KV v2** | `data/*`、`subkeys/*` | `metadata`、`metadata/*`、`delete/*`、`undelete/*`、`destroy/*`、`config` |
| **PKI** | `issue/<role>`、`sign/<role>`、`sign-verbatim/*`、`cert/<serial>`、`certs`、`ca`、`ca_chain`、`crl`、`ocsp` | `roles/*`、`issuers`、`issuer/*`、`keys`、`key/*`、`root/*`、`intermediate/*`、`config/*`、`revoke`、`tidy*`、`acme/*` |
| **SSH** | `sign/<role>`、`issue/<role>`、`creds/<role>`、`verify`、`public_key` | `roles`、`roles/*`、`lookup`、`issuers`、`issuer/*`、`config/ca`、`config/issuers`、`config/zeroaddress`、`tidy/*` |
| **Transit** | `encrypt/*`、`decrypt/*`、`rewrap/*`、`sign/*`、`verify/*`、`hmac/*`、`hash/*`、`random/*`、`datakey/*` | `keys`、`keys/*`、`export/*`、`byok-export/*`、`wrapping_key`、`config/*` |
| **Database** | `creds/<role>`、`static-creds/<role>` | `config/*`、`roles`、`roles/*`、`static-roles`、`static-roles/*`、`rotate-root/*`、`rotate-role/*`、`reset/*` |
| **Kubernetes** | `creds/<role>` | `config`、`roles`、`roles/*` |
| **TOTP** | `code/<name>` | `keys`、`keys/<name>` |
| **RabbitMQ** | `creds/<role>` | `config/connection`、`config/lease`、`roles`、`roles/*` |
| **LDAP** | `creds/*`、`static-cred/*`、`library/<name>/check-out` | `config`、`rotate-root`、`role/*`、`static-role/*`、`rotate-role/*`、`library/*` |
| **Cubbyhole（内建）** | `<任意路径>` | Token 专属私密储存区 |
| **Identity（内建）** | `entity/*`、`entity-alias/*`、`group/*`、`group-alias/*`、`lookup/*` | `oidc/*`、`mfa/*` 等身分管理端点 |

**版本与 Plugin 差异：**Bao 2.7 的 LDAP 需另装相容外挂；旧版清单曾包含 LDAP／OpenLDAP。实际以安装版本及 `bao plugin list secret` 为准。

## 四、KV Engine：储存与路径设计

### 4.1 KV v1

KV v1 以 Mount 后的**任意资源路径**作为秘密位置。例如：

```text
legacy-kv/erp/database
legacy-kv/github/token
legacy-kv/data/password
legacy-kv/roles/admin
```

其中 `data/`、`roles/` 即使外观与其他 Engine 的 API 后缀相同，在 KV v1 里仍可能只是使用者自订的秘密路径。**KV v1 没有 KV v2 的固定 `data/`、`metadata/` 路由分层。**

### 4.2 KV v2

KV v2 将秘密值、Metadata、版本删除、还原与销毁分到不同端点。

| API 后缀 | 用途 | 常见 ACL |
| --- | --- | --- |
| `data/<key>` | 读、写、修改、软删 Secret | `create/read/update/patch/delete` |
| `metadata` | 列出 KV 根目录 | `list` |
| `metadata/<key>` | 列出子路径、读写 Metadata、管理版本 | `list/read/update/delete`，依操作而定 |
| `subkeys/<key>` | 查看资料栏位结构，不回传 Secret 值 | `read` |
| `delete/<key>` | 软删指定版本 | `update` |
| `undelete/<key>` | 复原已软删的版本 | `update` |
| `destroy/<key>` | 永久销毁指定版本 | `update` |
| `config` | 整个 Engine 的设定 | `read/update` |

例如一笔逻辑 Secret `erp/database` 的资料 API 路径是：

```text
secret/data/erp/database
```

而列举同一 Engine 内的路径，走的是 `secret/metadata/...`，不是 `secret/data/...`。

**重要：`metadata/*` 不等于只有列举功能。** 如果赋予 `delete`，可以删除整笔 Secret 的 Metadata 与所有版本；与只给 `list` 完全不同。

## 五、PKI Engine：CA、Issuer、Role、Certificate

PKI Engine 的资源核心是 **CA／Issuer、CA Key、签发 Role、凭证、撤销状态**，不是任意路径的 KV 资料。

| API 后缀 | 用途 |
| --- | --- |
| `issue/<role>` | 产生新私钥及凭证 |
| `sign/<role>` | 替现有 CSR 签发凭证 |
| `sign-verbatim/<role>` | 特殊 CSR 签署流程，不走一般 Role 栏位限制 |
| `issuer/<issuer>/issue/<role>` | 指定 Issuer 签发 |
| `issuer/<issuer>/sign/<role>` | 指定 Issuer 签署 CSR |
| `cert/<serial>`、`certs` | 查询凭证、列出序号 |
| `ca`、`ca_chain`、`crl`、`ocsp` | 取得 CA 与撤销资讯 |
| `roles`、`roles/<role>` | 列出及管理签发 Role |
| `issuers`、`issuer/<issuer>` | 管理 CA Issuer |
| `keys`、`key/<key>` | 管理 CA 私钥 |
| `root/*`、`intermediate/*` | 建立或管理 Root／Intermediate CA |
| `config/*` | CRL、URL、Issuer、Key 等设定 |
| `revoke`、`revoke-with-key` | 撤销凭证 |
| `tidy`、`tidy-status` | 清理及查询状态 |
| `acme/*` | ACME 协定端点 |

部分 API 还有 `pem`、`der`、`raw`、指定 Issuer 等格式或路由变体，这里列的是主要族别。

一个实务区分是：**`issue/`、`sign/` 是使用 CA 的签发功能；`root/`、`issuer/`、`keys/`、`roles/` 则偏向 CA 与签发规则管理。**

## 六、SSH Engine：SSH CA、Role 与 OTP

| API 后缀 | 用途 |
| --- | --- |
| `sign/<role>` | 签署 SSH Public Key、取得 SSH Certificate |
| `issue/<role>` | 产生 SSH Key Pair 与 Certificate |
| `creds/<role>` | 产生 SSH OTP |
| `verify` | 验证 SSH OTP |
| `roles`、`roles/<role>` | 管理 SSH Role |
| `lookup` | 依 IP 查询可使用的 Role |
| `config/ca` | SSH CA 设定／查询 |
| `config/issuers` | 预设 Issuer 设定 |
| `issuers`、`issuer/<ref>` | 管理多个 Issuer |
| `issuer/<ref>/public_key`、`public_key` | 取得 SSH CA 公钥 |
| `config/zeroaddress` | 管理不受特定 IP 限制的 OTP Role |

对采用 SSH CA 的服务，最常授权的实际业务 API 是 `ssh/sign/<role>`，而不是管理签发 Role 的 `ssh/roles/*`。

## 七、Transit Engine：管理 Key 与密码学操作

Transit 与 KV 最大的不同，是它提供**加解密、签验章等运算 API**，Key 通常留在 Engine 内由 Bao 管理，不是一般应用程式读取一笔 Secret 明文。

| API 后缀 | 用途 |
| --- | --- |
| `encrypt/<key>` | 加密 |
| `decrypt/<key>` | 解密 |
| `rewrap/<key>` | 重新包装旧版密文 |
| `sign/<key>` | 签章 |
| `verify/<key>` | 验证签章 |
| `hmac/<key>` | 产生 HMAC |
| `hash/*`、`random/*` | 杂凑与随机数 |
| `datakey/plaintext/<key>` | 产生 Data Key，并回传明文 |
| `datakey/wrapped/<key>` | 产生 Data Key，只回传加密版本 |
| `keys`、`keys/<key>` | 管理 Transit Key |
| `keys/<key>/rotate` | 轮换该 Key |
| `keys/<key>/config` | 修改该 Key 设定 |
| `export/*`、`byok-export/*` | 汇出符合条件的金钥材料 |

`encrypt/` 和 `decrypt/` 分开，代表可以让一个 Token 只具备加密能力、不授权解密。不要把 `keys/*`、`export/*` 与一般加解密权限混为一谈。

## 八、其他 Engine 的资源结构

| Engine | 重要 API 后缀 | 语义 |
| --- | --- | --- |
| **Database** | `config/*`、`roles/*`、`static-roles/*` | 设定资料库连线、动态与静态帐号 Role |
| | `creds/<role>` | 读取时动态签发资料库帐密 |
| | `static-creds/<role>` | 取得已管理的静态角色帐密 |
| | `rotate-root/*`、`rotate-role/*`、`reset/*` | 轮换与重设相关设定或凭证 |
| **Kubernetes** | `config`、`roles/*`、`creds/<role>` | 设定目标 K8s、Role，并签发服务帐号相关凭证 |
| **TOTP** | `keys`、`keys/<name>`、`code/<name>` | 设定 TOTP Key、生成／验证 OTP |
| **RabbitMQ** | `config/connection`、`config/lease`、`roles/*`、`creds/<role>` | 连线与 Role 设定、动态凭证 |
| **LDAP** | `config`、`role/*`、`static-role/*` | LDAP 连线、动态及静态 Role |
| | `creds/*`、`static-cred/*` | 取得 LDAP 帐密 |
| | `rotate-root`、`rotate-role/*`、`library/<name>/check-out` | 轮换管理及 Library Checkout |

### 8.1 Cubbyhole 和 Identity 是系统内建用途

- **Cubbyhole**：每个 Token 专属的私密储存，路径由其 Key 名称决定，非一般共享 KV Mount。
- **Identity**：`entity/*`、`entity-alias/*`、`group/*`、`group-alias/*`、`lookup/*`、`oidc/*`、`mfa/*` 等身分资源与管理端点。

它们虽然出现在 `bao secrets list` 等系统资讯里，但不要等同于自己新增的 PKI、SSH、KV 等一般 Engine。

## 九、不同 Engine 的后缀可能撞名

| 相同后缀 | 可能使用的 Engine |
| --- | --- |
| `roles/*` | PKI、SSH、Database、Kubernetes、RabbitMQ |
| `creds/*` | Database、SSH、Kubernetes、RabbitMQ、LDAP |
| `config/*` | PKI、SSH、Database、Transit 等 |
| `keys/*` | Transit、TOTP、PKI |
| `sign/*` | PKI、SSH、Transit |

**相同后缀不代表同一种操作，也不代表使用同一种 ACL Capability。**

例如：

```bash
# Database：读取时动态产生资料库帐密
bao read database/creds/my-role

# Kubernetes：写入请求以建立 ServiceAccount Token
bao write kubernetes/creds/my-role \
  kubernetes_namespace=test
```

前者使用 `read`，后者是写入型 API，通常要 `update` 等对应能力。

因此在不了解 Mount 类型时：

```hcl
path "+/creds/*" {
  capabilities = ["read"]
}
```

并不能确保所有 Engine 都能产生凭证：某些 Engine 的 `creds/` 是 GET，另一些使用写入操作。

此外，KV v1 可将一般 Secret 命名成：

```text
legacy-kv/data/password
legacy-kv/roles/admin
```

这些字串不代表后缀真的属于 KV v2、PKI 或 Database。**相同路径后缀只能依实际挂载的 Engine 类型解释。**

## 十、Engine 专用业务 ACL 范例

以下整理跨 Engine 的核心范例。通用 ACL 万用字元与 Namespace 管理员范例，见 [路径表示法](path-notation.zh-CN.md)。

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

### Database：只允许取得 `erp-reader` 的动态帐密

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

从上面可见，**Engine 的「业务操作」不一定是 ACL `read`**。SSH／TLS 凭证签发与 Transit 加密，虽是使用服务的操作，但实际是写入型 API。

### 跨 Engine 业务 Policy 范例

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

这只是一份**跨 Engine 业务操作的示范**，不代表涵盖表格中的全部 Engine 与完整功能。新增用途时要依实际 Mount、API 方法与所需 Capability 增加规则。

## 十一、查看自己安装版本的路径

OpenBao 官方提供路由查询：

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

可以列出该 Engine 支援的路由模式，通常以类似正规表示式的表示法呈现。

还能直接查看具体操作的参数与用途：

```bash
bao path-help pki/issue/web
bao path-help ssh/sign/linux-user
bao path-help transit/encrypt/app-key
```

其他辅助指令：

```bash
# 列出目前安装的 Secret Plugin
bao plugin list secret

# 列出当前 Namespace 实际挂载的 Engine 及详细资讯
bao secrets list -detailed
```

不同 OpenBao 版本及外部 Plugin 的 Endpoint 可能不完全相同，以上仍需以伺服器实际回传及官方 API 文件为准。

## 十二、结论

1. **KV 以储存 Secret 为中心**；PKI／SSH 以凭证签发为中心；Transit 以金钥与加解密运算为中心；Database 等 Engine 可以从外部系统动态取得或签发凭证。
2. **Mount 后缀由 Engine 决定**。KV v2 的 `data/`、`metadata/` 并不是共通标准。
3. **不同 Engine 可能使用同名后缀**，例如 `roles/`、`creds/`、`sign/`、`keys/`，但实际语义、HTTP 方法与 ACL Capability 不同。
4. **Engine 的资料 API 与管理 API 通常可分开授权**，例如只签发 SSH 凭证、不允许管理 SSH Role；只让 Transit 加密、不让它解密或汇出 Key。
5. 所列为各 Engine 的主要路由族；遇到版本差异或需要全部细分端点时，使用 `bao path-help` 与官方文件核对。

官方参考：[OpenBao Secrets Engines](https://openbao.org/docs/secrets/) · [OpenBao CLI Commands](https://openbao.org/docs/commands/) · [OpenBao ACL Policies](https://openbao.org/docs/concepts/policies/)。
