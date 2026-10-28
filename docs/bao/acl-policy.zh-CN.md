# OpenBao ACL Policy 完整实作：Super Admin、Namespace Admin、KV 读写与路径限制

[English](acl-policy.md)

OpenBao 的 ACL Policy 可依管理层级、Namespace、Secret Engine 与 API 路径控制操作权限。以下依 OpenBao 官方 ACL Policy 文件的规则，整理五种常见权限设计，包含 HCL、建立指令、操作范例、权限验证及限制。

`path "*"` 可用来建立**几乎全权的 Super Admin**；其中 `scan`、`sudo` 都是正式支援的 Capability，但这种 Policy 不等于 OpenBao 的特殊内建 `root` Policy。

## 一、五种 Policy 的使用范围

| Policy | 管理范围 | 能做什么 |
| --- | --- | --- |
| `super-admin` | Root Namespace，具广泛管理权限 | 管理 Engine、Policy、Token、Namespaces 等 |
| `namespace-admin` | 单一 `agent` Namespace | 管理该 NS 内的资源 |
| `kv-readwrite` | 一个 KV v2 Engine | 读取、建立、修改、删除 Secret |
| `kv-readonly` | 一个 KV v2 Engine | 读取 Secret、列出 Key |
| `kv-scoped-readonly` | 指定的路径 | 只能读取授权范围内的 Secret |

## 二、Super Admin：几乎全部权限

档案：`super-admin.hcl`

```hcl
path "*" {
  capabilities = [
    "create",
    "read",
    "update",
    "patch",
    "delete",
    "list",
    "scan",
    "sudo"
  ]
}
```

建立 Policy：

```bash
unset BAO_NAMESPACE

bao policy write super-admin super-admin.hcl
```

这份 Policy 包含所有标准操作能力，其中：

- `scan`：递回列出资源，并非每种 Engine 都支援。
- `sudo`：允许执行需要额外高权限的管理 API。
- `*`：万用路径，不限于 `secret/`，也涵盖 `sys/`、`auth/` 等管理 API。

**它仍不完全等于内建 `root` Policy。** 内建 `root` 是特殊的最高权限，能绕过一般 ACL。这份 `super-admin` 则仍经过 ACL 评估，也受 Namespace 边界和其他机制限制。

OpenBao 官方的 ACL 文件也有使用相同 `path "*"` 与八种 Capability 建立高权限管理 Policy 的范例。

## 三、Namespace Admin：只能管理 `agent` Namespace

Namespace 管理员的需求，适合直接利用 OpenBao 的 Namespace 隔离，而非在 Root Namespace 写大量排除规则。

档案：`namespace-admin.hcl`

```hcl
path "*" {
  capabilities = [
    "create",
    "read",
    "update",
    "patch",
    "delete",
    "list",
    "scan",
    "sudo"
  ]
}
```

**这份 HCL 与 Super Admin 完全一样。** 差别是这份 Policy 建立在哪个 Namespace，以及使用它的 Token 属于哪个 Namespace。

首先，用 Root 管理身分建立 Namespace：

```bash
unset BAO_NAMESPACE

bao namespace create agent
```

接著切换到 `agent`：

```bash
export BAO_NAMESPACE=agent

bao policy write namespace-admin namespace-admin.hcl

bao policy read namespace-admin
```

接著使用 Root 管理 Token，在 `agent` Namespace 签发：

```bash
bao token create \
  -orphan \
  -policy=namespace-admin \
  -ttl=24h
```

新 Token 的权限范围限定在 `agent/` 及其被允许的子 Namespace，而不是整个 OpenBao。

| 操作 | agent Namespace Admin |
| --- | --- |
| 管理 `agent/secret/` | 允许 |
| 管理 `agent/pki/` | 允许 |
| 管理 `agent/` 内的 Policy | 允许 |
| 管理 `agent/` 内的 Auth Method | 允许 |
| 管理 `k8s/` | 不允许 |
| 管理 Root Namespace 全域设定 | 不允许 |

OpenBao 子 Namespace 的管理员无法越过自己的 Namespace 边界；部分全域 `sys/` API 也明确禁止子 Namespace 使用，即使 Token 持有 `sudo`。

**注意：单纯设定 `BAO_NAMESPACE=agent` 不会让原本的 Root Token 变成受限 Token。** 真正的隔离必须由隶属 `agent` 的管理身分与 Token 达成。

以下三种 KV Policy 均假设已在 `agent` Namespace 中建立 KV v2 Engine：

```bash
export BAO_NAMESPACE=agent

bao secrets enable -path=secret kv-v2
```

## 四、一般读写 Policy：KV Read/Write

档案：`kv-readwrite.hcl`

```hcl
# KV v2 资料读写及一般删除
path "secret/data/*" {
  capabilities = [
    "create",
    "read",
    "update",
    "patch",
    "delete"
  ]
}

# 列出 KV 根目录
path "secret/metadata" {
  capabilities = ["list"]
}

# 列出各个子目录
path "secret/metadata/*" {
  capabilities = ["list"]
}
```

这份 Policy 允许对 `secret/` 里的一般 KV 资料进行读写与软删除，但不允许改 Policy、建立 Secret Engine 或操作 PKI。

它也没有开放 KV v2 的永久销毁版本或删除全部 Metadata API。这些分别属于 `secret/destroy/*` 和 `secret/metadata/*` 上额外的写入／删除能力。

建立：

```bash
bao policy write kv-readwrite kv-readwrite.hcl
```

对应操作：

```bash
bao kv put -mount=secret erp/database password=test
bao kv get -mount=secret erp/database
bao kv list -mount=secret erp/
bao kv delete -mount=secret erp/database
```

## 五、一般唯读 Policy：KV Read Only

档案：`kv-readonly.hcl`

```hcl
# 可以读取所有 Secret 资料
path "secret/data/*" {
  capabilities = ["read"]
}

# 可以列出根目录
path "secret/metadata" {
  capabilities = ["list"]
}

# 可以列出子目录
path "secret/metadata/*" {
  capabilities = ["list"]
}
```

建立：

```bash
bao policy write kv-readonly kv-readonly.hcl
```

效果：

| 操作 | 权限 |
| --- | --- |
| `bao kv get` | 允许 |
| `bao kv list` | 允许 |
| `bao kv put` | 不允许 |
| `bao kv patch` | 不允许 |
| `bao kv delete` | 不允许 |

这里只开放 `list`，没有开放 `scan`，因此不授权新的递回扫描能力。

## 六、限定路径的唯读 Policy

工作负载的限定路径唯读权限，可分成单一 Secret、指定资料夹、多个指定路径，以及整个资料夹排除特定子路径等四种方式。

### 6.1 只能读取单一 Secret

例如只允许读 ERP 的资料库凭证：

```hcl
path "secret/data/erp/database" {
  capabilities = ["read"]
}
```

效果是只能读取这笔 Secret，不能读 `erp/redis` 或 `github/token`。

### 6.2 只能读取指定资料夹

例如只允许读取 ERP 底下全部 Secret：

```hcl
path "secret/data/erp/*" {
  capabilities = ["read"]
}

path "secret/metadata/erp" {
  capabilities = ["list"]
}

path "secret/metadata/erp/*" {
  capabilities = ["list"]
}
```

能读取 ERP 底下任意深度的 Secret，也可以列出 ERP 内的 Key，但不能读取其他专案的 Secret。

### 6.3 可以读多个指定路径

例如同时允许两个 API Token：

```hcl
path "secret/data/github/token" {
  capabilities = ["read"]
}

path "secret/data/cloudflare/api-token" {
  capabilities = ["read"]
}
```

这是一个 Policy 授权多个互不相连的路径，不需要建立多个 Secret Engine。

### 6.4 允许整个目录，但排除某些秘密

例如允许读 ERP 底下全部 Secret，唯独 `private/` 禁止：

```hcl
path "secret/data/erp/*" {
  capabilities = ["read"]
}

path "secret/data/erp/private/*" {
  capabilities = ["deny"]
}
```

`deny` 是 OpenBao 正式支援的 Capability，而且会优先于允许规则。

这种写法适合处理例外。不过上面只有开放资料读取，没有开放 Key 列举。

**注意：`list` 不会自动过滤没权限读取的 Secret 名称。** 如果另外授权列出 `erp/`，可能仍然看得到 `private/` 这个名称。因此非常严格的路径限制，优先采用明确白名单。

## 七、建立 Policy 与 Token 的完整指令

假设目前使用 Root Token，且所有 KV Policy 都建在 `agent` Namespace：

```bash
export BAO_NAMESPACE=agent

# 建立 Policy
bao policy write kv-readwrite kv-readwrite.hcl
bao policy write kv-readonly kv-readonly.hcl
bao policy write kv-scoped-readonly kv-scoped-readonly.hcl

# 查看
bao policy list
```

接著签发三种 Token：

```bash
# 可读写
bao token create \
  -orphan \
  -policy=kv-readwrite \
  -ttl=24h

# 全部唯读
bao token create \
  -orphan \
  -policy=kv-readonly \
  -ttl=24h

# 指定路径唯读
bao token create \
  -orphan \
  -policy=kv-scoped-readonly \
  -ttl=24h
```

### Token 签发限制

Root Token，或在 Token 建立端点具有 `sudo` 的 Token，可以签发包含其他
namespace 本地 Policy 的 Token。上面的广泛管理员 Policy 已包含对
`auth/token/create` 的这项权限。

没有该权限时，一般子 Token 的 Policy 必须是父 Token Policy 的子集。要求 orphan
也需要对应的高权限。Token Role 和 Auth Method 可供管理员为委派签发设置更窄的
规则。这些权限不会将普通 Token 变成特殊的内建 `root` Token。

参见 [Token API](https://openbao.org/docs/api/auth/token/)，以及可直接套用的
[agent Policy 模板](../../scripts/bao/README.md#policies-for-the-agent-namespace)。

## 八、怎么验证 Policy 真的有效？

假设已取得唯读 Token，设定：

```bash
export BAO_TOKEN="刚才产生的唯读 Token"
export BAO_NAMESPACE=agent
```

直接检查：

```bash
# 预期 read
bao token capabilities secret/data/erp/database

# 预期 deny（若不在授权路径）
bao token capabilities secret/data/github/token

# 预期 list（有授权该 metadata 路径时）
bao token capabilities secret/metadata/erp
```

`bao token capabilities` 可以直接检查目前 Token 对指定 API 路径的有效权限，不必真的执行删除或写入。

OpenBao CLI 也提供查询指令所需 Policy 的功能：

```bash
bao kv put -output-policy \
  -mount=secret erp/database \
  password=test
```

这会印出该操作需要的 Policy 规则，而不是直接执行写入。往后开始使用 PKI、SSH、Transit Engine 时，这比自己猜 API 权限方便得多。

## 九、五种 Policy 与 Namespace 架构的对应

```text
Root Namespace
│
├── super-admin
│   └── OpenBao 全域管理身分
│
├── agent/ Namespace
│   ├── namespace-admin
│   └── secret/  (KV v2)
│       ├── kv-readwrite
│       ├── kv-readonly
│       └── kv-scoped-readonly
│
└── k8s/ Namespace
    ├── namespace-admin
    ├── secret/  (KV v2)
    └── pki/     (PKI)
```

这里的 Policy 是属于各自 Namespace 的权限设定，不是真的挂在 Secret Engine 底下的子物件。图中只是表示它管理哪些资源。

### 最值得记住的三件事

1. **Namespace 决定管理边界，Policy 决定 API 路径的操作权限。**
2. **KV v2 的 Policy 要使用 `data/`、`metadata/` 等真实 API 路径。**
3. **`path "*"` 配合 `sudo` 可授予 namespace 内的广泛管理和特权 Token 签发权限；内建 `root` Policy 仍有特殊地位。**

上述五种 Policy 的 HCL 依 OpenBao 官方 ACL 语法编写，可作为测试环境的配置范例；目前确认范围是官方文件，**尚未在实际 OpenBao Server 上套用或执行权限验证**。
