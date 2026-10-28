# Bao 的 Path 表示法详细说明

[English](path-notation.md)

OpenBao 的 `path` 是整套 Secret Engine、Namespace 与 ACL Policy 的共通定位方式。路径表面上都是用 `/` 分隔的字串，但每一段的**语义不是相同的**：前面可能是 Namespace，中间是 Secret Engine 的 Mount，后面是 Engine 定义的 API 端点与资源路径。

本文整理 Namespace 的隐含前缀、Root 与子 Namespace 的 ACL 范围、`*` 与 `+` 的匹配规则、跨 Namespace 存取及常见误解。以下以 OpenBao 的 ACL Policy（HCL）为准。

## 一、Path 是如何拆成 Namespace、Mount 与 API 路径的？

以两笔 KV v2 Secret 为例：

```text
secret/data/erp/database
│      │    └───────── Engine 内的资源路径：erp/database
│      └────────────── KV v2 的 data API（固定后缀）
└───────────────────── Root Namespace 中名叫 secret/ 的 Mount

agent/secret/data/erp/database
│     │      │    └── Engine 内的资源路径：erp/database
│     │      └─────── KV v2 的 data API
│     └────────────── 在 agent 里名叫 secret/ 的 Mount
└──────────────────── Namespace：agent/
```

| 层级 | 示例 | 作用 |
| --- | --- | --- |
| Namespace | Root（没有额外的 `root/` 路径前缀）、`agent/`、`k8s/` | 确定管理及身分隔离空间 |
| Secret Engine Mount | `secret/`、`credentials/`、`pki/` | 将 API 路径交给哪一个 Engine |
| Engine API 子路径 | KV v2 的 `data/`、`metadata/` | 决定操作种类，例如读取资料或列举 Keys |
| Secret／资源路径 | `erp/database` | 真正要操作的 Engine 内资源 |

例如 `secret` 只是自订的 Mount 名称，不是某个固定保留字。换成 `credentials`，实际 API 路径也会换成 `credentials/data/...`。

不同 Engine 的 API 结构并不相同，例如 PKI、SSH、Transit 的路径后缀不能直接套用 KV v2 的 `data/`、`metadata/`。

**重要：Namespace 不是普通资料夹。** 它是一个具备自己 Secret Engines、Auth Methods、Policies、Tokens、Identity 等资源的管理空间。只有从字串外观来看，它才像另一段 `/` 路径。

### API 请求的 Namespace 可以由路径或 Header 指定

以下三种请求可指向同一笔位于 `agent` 的 Secret（假设对应 Namespace、Mount 和资源存在）：

```text
GET /v1/agent/secret/data/erp/database

GET /v1/secret/data/erp/database
X-Vault-Namespace: agent

CLI：BAO_NAMESPACE=agent bao kv get -mount=secret erp/database
```

`BAO_NAMESPACE` 和 `X-Vault-Namespace` 是指定请求的**目标 Namespace**，并不会额外增加 Token 权限。请求路由到正确的 Namespace 之后，仍要经过 ACL 检查。

## 二、Policy 的 Path 会以建立位置的 Namespace 为起点

Policy 的 `path` **以该 Policy 所属 Namespace 为相对起点**。这是 OpenBao 内部解析与 ACL 比对的结果，不代表 HCL 原文真的被改写。

例如完全相同的 HCL：

```hcl
path "secret/data/*" {
  capabilities = ["read"]
}
```

放在不同 Namespace 时，有效路径不同：

| Policy 建立位置 | ACL 比对时相当于 |
| --- | --- |
| Root | `secret/data/*` |
| `agent/` | `agent/secret/data/*` |
| `agent/dev/` | `agent/dev/secret/data/*` |

因此，在 `agent` 里设定：

```hcl
path "*" {
  capabilities = ["create", "read", "update", "patch", "delete", "list", "scan", "sudo"]
}
```

相当于从 `agent/` 这个起点，授权该 Namespace 及其子 Namespace 的符合路径；不会因此取得 Root 或同层 `k8s/` 的权限。

**不要在 `agent` Namespace 的 Policy 里，为了表示自己所在的 NS，再手动多写 `agent/*`。** 那会变成相对于 `agent/` 的另一段 `agent/`，并非原本想要的全域授权。

OpenBao 原始码在解析 ACL 规则时，会以 `Namespace.Path + Policy.Path` 组合内部完整路径；比对请求时，也使用 Namespace 路径和请求路径组合。

## 三、ACL 的 `*` 与 `+` 是不同的万用字元

OpenBao ACL 不是 Regex，也不是一般 Shell Glob。尤其 `*` **只能出现在 Policy Path 的最后一个字元**。

| 写法 | 意义 | 例子 |
| --- | --- | --- |
| `path "*"` | 从 Policy 所在 Namespace 起，匹配任意长度的后续路径 | 在 Root 建立可涵盖 Root 与所有子 NS |
| `path "secret/*"` | 匹配 `secret/` 前缀后的所有深度 | `secret/data/a/b` |
| `path "secret/data/*"` | 固定 `secret/data/`，后面任意深度 | `secret/data/erp/db`、`secret/data/erp/prod/db` |
| `path "secret/+/password"` | `+` 只匹配中间**刚好一个**完整路径段 | `secret/erp/password`，但不匹配 `secret/erp/prod/password` |
| `path "+/data/*"` | 任意一个顶层路径段，接固定的 `data/`，后面任意深度 | `secret/data/a`、`credentials/data/a` |
| `path "+/+/fffff/*"` | 前面刚好两个任意路径段，接 `fffff/`，后面任意深度 | `a/b/fffff/x/y` |
| `path "*/fffff/*"` | **无效写法**：`*` 出现在中间 | 不能用来表示前后任意深度 |

### 3.1 `+`：匹配一整层，不能跨过 `/`

```hcl
path "+/data/*" {
  capabilities = ["read"]
}
```

拆开是：

```text
+             data/           *
任意一层      固定的 API 字串   任意剩余后缀
```

可以匹配：

```text
secret/data/erp/database
credentials/data/github/token
projects/data/a/b/c
```

但不匹配：

```text
team/secret/data/erp/database
```

因为 `+` 不会吞掉 `team/secret` 两层。

要匹配两层可写 `+/+/data/*`，但这是**恰好两层**，不是任意多层。

### 3.2 `*`：最后的前缀匹配，后面可以有多个 `/`

```hcl
path "secret/data/*" {
  capabilities = ["read"]
}
```

这里固定了 `secret/data/`，最后的 `*` 可以涵盖 `erp/database`、`erp/prod/database` 等任意深度。

`*` 不是限定一层。它是**从目前前缀向后匹配任意长度的字串**，所以也要注意前缀界线：

```text
secret/data/*    → 固定 secret/data/ 这段前缀
secret/data*     → 也可能匹配 secret/database... 之类有相同字首的路径
```

为了限定层级，应把想固定的 `/` 一起写出来。

### 3.3 `*/fffff/*` 不是「前后所有层」的合法语法

如果目的是「无论前面有几层，只要出现 `fffff/`，其下任意层都匹配」，**OpenBao ACL 没有能直接用 `*/fffff/*` 表达的语法**。

可以用 `+/fffff/*` 表示前面一层、`+/+/fffff/*` 表示前面两层；若深度不固定，必须重新设计 Mount／路径分布，或明确列出需要的深度。直接用 `path "*"` 虽然能涵盖所有路径，却会失去只限定 `fffff/` 的效果。

## 四、Root Namespace 的 `path "*"` 会涵盖所有子 Namespace

Root 的万用前缀授权不限于直接位于 Root 的资源。

**正确规则：在 Root Namespace 建立的 `path "*"` 是前缀万用授权，能匹配 Root 以及 `agent/`、`k8s/` 等子 NS 的 API 路径。**

例如在 Root 建立：

```hcl
path "*" {
  capabilities = [
    "create", "read", "update", "patch",
    "delete", "list", "scan", "sudo"
  ]
}
```

这份一般 ACL Policy 具有极广泛的跨 NS 权限，能对有相应 API 的 Root 和子 NS 资源执行对应操作，包含管理 Namespace 及其 Engine、Policy 等；但它依然不等于 OpenBao 特殊的内建 `root` Policy，部分内部管理限制与 Token 签发规则仍存在。

「Root 的 `path "*"` 只管 Root，必须另外加上子 Namespace 规则」是错误理解。

以下三段：

```hcl
path "*" {
  capabilities = ["create", "read", "update", "patch", "delete", "list", "scan", "sudo"]
}
path "agent/*" {
  capabilities = ["create", "read", "update", "patch", "delete", "list", "scan", "sudo"]
}
path "k8s/*" {
  capabilities = ["create", "read", "update", "patch", "delete", "list", "scan", "sudo"]
}
```

若已有上面的全权 `path "*"`，**为了让它涵盖子 NS 而追加后两段没有必要**。但如果原本并没有全域 `path "*"`，则可以在 Root Policy 中使用 `agent/*` 等路径，选择性授权特定子 NS。

同理，`agent/` 里的 `path "*"` 本身可以涵盖 `agent/dev/` 等下层路径，不需要再额外附加 `dev/*` 才有子 NS 的 ACL 权限。

## 五、Root 管理员、子 NS 管理员、Root KV 读写的区别

| Policy 建立位置 | HCL | Root NS | `agent/` | `k8s/` |
| --- | --- | --- | --- | --- |
| Root | `path "*"` + 全部 Capabilities | 广泛管理 | 广泛管理 | 广泛管理 |
| `agent/` | `path "*"` + 全部 Capabilities | 无授权 | 广泛管理 | 无授权 |
| Root | `path "*"` + `read` | 读取型 API | 读取型 API | 读取型 API |
| Root | `path "secret/data/*"` + `read` | 仅 Root 的对应 KV 资料 | 无授权 | 无授权 |
| Root | `path "agent/secret/data/*"` + `read` | 无授权 | 仅 agent 指定 KV 资料 | 无授权 |
| `agent/` | `path "secret/data/*"` + `read` | 无授权 | 仅 agent 指定 KV 资料 | 无授权 |

### 5.1 在 `agent` 中建立 Admin

```bash
export BAO_NAMESPACE=agent
bao policy write namespace-admin namespace-admin.hcl
```

```hcl
# namespace-admin.hcl
path "*" {
  capabilities = [
    "create", "read", "update", "patch",
    "delete", "list", "scan", "sudo"
  ]
}
```

这份 Policy 只会以 `agent/` 为起点。拥有它的 `agent` 管理员，可以在有权限的情况下建立 `agent/dev`、`agent/prod` 等子 NS，但不能改用 `BAO_NAMESPACE` 就越权管理 Root 或兄弟 NS `k8s`。

### 5.2 Root 的 KV Policy 不会自动跨 NS

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

这份建立于 Root 的 Policy 只授权 Root 下名叫 `secret/` 的 KV v2 Mount，不会自动授权 `agent/secret/` 或 `k8s/secret/`。

若要从 Root 的 Policy 额外允许 `agent` KV：

```hcl
path "agent/secret/data/*" {
  capabilities = ["create", "read", "update", "patch", "delete"]
}
path "agent/secret/metadata" {
  capabilities = ["list"]
}
path "agent/secret/metadata/*" {
  capabilities = ["list"]
}
```

这不等于取得 Agent 内管理 Engine／Policy 的权限，也不代表能读取 K8s 的 KV。

### 5.3 Root 的 `path "*"` 加 `read` 会读到什么？

```hcl
path "*" {
  capabilities = ["read"]
}
```

在 Root 建立后，ACL 层面会允许 Root 及所有子 NS 中符合 `read` 能力的 API 路径。

但必须分清：

1. **`read` 不包含 `list` 或 `scan`。** 知道正确资源路径时可能读得到，不代表能列出全部 Keys。
2. **`read` 并不是「只能读秘密值」。** 它也涵盖可能开放的 `sys/`、`auth/` 等读取 API，而某些动态 Secret Engine 的读取操作可能产生新凭证。
3. **`read` 不包含 `sudo`。** 若某个 API 还要求 `sudo` 或受特殊 Namespace 限制，只有 `read` 并不够。
4. 「有 ACL read 权限」不保证所有 Engine 的所有资料都能用 GET 读取；仍取决于各 Engine 的 API 与额外条件。

因此 **Root `path "*"` + `read` 是跨 Namespace 的广泛读取授权，不是适合随意发给机器的「所有 Secret 唯读」角色**。

## 六、`+` 和 `*` 只比对 API 路径，不会辨认 Engine 类型

例如：

```hcl
# 匹配任意一个顶层路径段后的 data API
path "+/data/*" {
  capabilities = ["create", "read", "update", "patch", "delete"]
}
path "+/metadata" {
  capabilities = ["list"]
}
path "+/metadata/*" {
  capabilities = ["list"]
}
```

它可以涵盖不同名称的顶层 KV v2 Mount，例如 `secret/`、`credentials/`、`projects/`，但这是因为**路径恰好匹配**，并不是 OpenBao 自动判断其型别为 KV v2。

如果其他 Engine、甚至不同路由情境，恰好具有相同 API 路径形式，也要注意 ACL 可能匹配；若 Mount 位于两层（如 `team/credentials/`），单一 `+` 亦不涵盖。

**Namespace Admin 与跨 Mount 的 Secret R/W 是不同设计**：不能以 `path "*"` 然后去掉 `sudo` 来假装只是 Secret 管理，因为那仍可能授权大量不需要 `sudo` 的 `sys/`、`auth/` 管理 API。

OpenBao 没有一个自动表示「所有类型 Secret Engine 的业务操作，但禁止管理 API」的通用 ACL 关键字；要依各 Engine（KV、PKI、SSH、Transit 等）的实际 API 路径设计能力。

## 七、Policy 路径与权限验证

`BAO_NAMESPACE` 只改变操作的请求目标，不能改变 Token 本身的权限范围。

先检查目前 Token：

```bash
bao token lookup
```

再于 Root Namespace 用待验证的 Token 查看不同路径的 Capabilities：

```bash
unset BAO_NAMESPACE
bao token capabilities secret/data/test
bao token capabilities agent/secret/data/test
bao token capabilities k8s/secret/data/test
```

或在指定 Namespace 内查本地路径：

```bash
export BAO_NAMESPACE=agent
bao token capabilities secret/data/test
```

这类能力检查不需要 Secret 真实存在，验的是 ACL 规则。具体实际操作是否成功仍需搭配 Engine 是否挂载、资源是否存在、其他验证条件等因素。

若确实已建立 KV 且有测试资料，亦可分别测试：

```bash
# Root Namespace
unset BAO_NAMESPACE
bao kv get -mount=secret erp/database

# agent Namespace
bao kv get -ns=agent -mount=secret erp/database

# k8s Namespace
bao kv get -ns=k8s -mount=secret erp/database
```

请注意：以上 `-ns` 只是切换请求 Namespace；它不能替 Token 增加权限。

## 八、常见误解与重要区分

1. **Root 的 `path "*"` 可以涵盖子 Namespace**，不必只为延伸范围而补写每个子 NS。
2. **子 NS 的 `path "*"` 可以涵盖其下层**；例如 `agent/` 可涵盖 `agent/dev/`，不必额外写 `dev/*`。
3. **`path "secret/data/*"` 与 `path "*"` 完全不同。** 前者限制固定的 Mount/API 前缀；后者可以匹配所有下层路径。
4. **`+` 匹配恰好一层，`*` 匹配尾端任意后缀深度。** 但 `*` 只能放在最后，`*/fffff/*` 不是合法 ACL 语法。
5. **不论 Root 或子 NS，完整 ACL 权限仍不等于内建 Root Token。** `sudo`、Namespace 特殊限制、Token 签发规则都应分开理解。

一句话：**Namespace 决定 ACL 路径的起点；Mount 决定 Engine；Engine 的 API 决定后续固定路径；Policy 用 `+`、尾端 `*` 与 Capabilities 对解析后的路径授权。**

---

## 官方来源与实作

1. [OpenBao：Policies（ACL Path、前缀 Glob、`+`、Capability 与优先顺序）](https://openbao.org/docs/concepts/policies/)
2. [OpenBao：Namespaces（阶层、X-Vault-Namespace、子 Namespace 权限边界）](https://openbao.org/docs/concepts/namespaces/)
3. [OpenBao 原始码：Policy Parse，组合 Namespace.Path 与 Policy Path](https://github.com/openbao/openbao/blob/main/internal/vault/policy/policy.go#L581-L605)
4. [OpenBao 原始码：ACL 比对 Namespace 与请求路径](https://github.com/openbao/openbao/blob/main/internal/vault/policy/acl.go)

*核对日期：2026-10-28。以上是官方文件与原始码层面的规则；不是对使用者测试 OpenBao Server 的实际授权测试结果。*
