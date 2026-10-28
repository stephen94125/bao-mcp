# OpenBao 基础架构与权限管理：Secret Engine、Policy、Token 与 KV 实作

[English](architecture-permissions.md)

OpenBao 的使用可以拆成两套结构：

1. **Secret 储存结构**：决定秘密储存在哪个引擎与路径。
2. **Policy 与 Token 权限结构**：决定哪些身分能对哪些秘密执行读取、写入或列举等操作。

基本操作顺序是先配置储存位置，建立 Policy，再签发附有该 Policy 的 Token。每个 Token 不必对应一个独立的帐号。

## 一、OpenBao 的储存阶层

假设需要储存 ERP、GitHub 与 Cloudflare 的密钥，结构可以如下：

```text
OpenBao Server / Cluster
├── secret/                     ← KV v2 Secret Engine（Mount）
│   ├── erp/                    ← 逻辑路径
│   │   └── database            ← 一笔 Secret
│   │       ├── username = erp_user
│   │       └── password = ********
│   ├── github/
│   │   └── deploy-token
│   └── cloudflare/
│       └── api-token
└── pki/                        ← 另一个 Secret Engine
```

### 各层的本质

| 层级 | 名称 | 实际作用 |
| --- | --- | --- |
| 1 | OpenBao Server / Cluster | 整个秘密管理服务 |
| 2 | Namespace（选用） | 独立的管理与权限空间 |
| 3 | Secret Engine / Mount | 真正有特殊意义的储存或功能单位 |
| 4 | Path | 自行命名的逻辑路径 |
| 5 | Secret | 路径下储存的资料项目 |
| 6 | Field | Secret 中的 `username`、`password` 等栏位 |

**「Vault」并不是每个资料夹都有的 OpenBao 特殊层级。**

应用程式自行定义、名为 `production` 的 Vault，可以映射成：

- `secret/production/`：同一个 KV Engine 内的普通路径。
- `production/`：独立启用的 Secret Engine（Mount），具备各自的 Engine 设定与储存边界。
- Namespace：更高层的管理范围，可以隔离 Policy、Token、Auth Method、Secret Engine 等资源。

若用途只是依专案保存 Secret，一开始采用**单一 KV v2 Engine，以路径、Policy 与 Token 区分专案**即可，没有必要每个专案都建立独立 Mount。

## 二、建立 KV 储存空间并读写资料

以下假设 OpenBao 已完成初始化与 Unseal，且 CLI 已具备管理权限。

先查看 Secret Engines：

```bash
bao secrets list
```

如果还没有 `secret/` Mount，建立一个 KV v2：

```bash
bao secrets enable -path=secret kv-v2
```

写入一笔 Secret：

```bash
bao kv put -mount=secret erp/database \
  username=erp_user \
  password=test-only
```

读取整笔资料：

```bash
bao kv get -mount=secret erp/database
```

只输出取回资料中的指定栏位；这是 CLI 输出选项，不是栏位级存取权限：

```bash
bao kv get -mount=secret \
  -field=password erp/database
```

这个例子中：

- `secret` 是 Secret Engine 的 Mount 名称。
- `erp` 是普通的逻辑路径，无须预先建立资料夹。
- `database` 是一笔 Secret。
- `username`、`password` 是 Secret 的资料栏位。

## 三、建立唯读 Policy

假设需要一个 Token，**只能读取 ERP 范围内的 Secret**，不能修改内容，也不能读取 GitHub 的 Secret。

建立 `erp-readonly.hcl`：

```hcl
# 允许读取 ERP 底下的 Secret
path "secret/data/erp/*" {
  capabilities = ["read"]
}

# 允许列出 ERP 底下有哪些 Key
path "secret/metadata/erp" {
  capabilities = ["list"]
}

path "secret/metadata/erp/*" {
  capabilities = ["list"]
}
```

### KV v2 Policy 路径的差别

KV v2 的 Policy **必须配合实际 API 路径**，例如：

- `secret/data/erp/*`：读取 ERP 范围内的 Secret 资料。
- `secret/metadata/erp`、`secret/metadata/erp/*`：列出指定路径下的 Key。
- **不是**直接写 `secret/erp/*` 就能涵盖 KV v2 的资料 API。

使用 `bao kv get` 时，CLI 会自行处理 `data/` 的 API 路径。

把本机 HCL 写入 OpenBao：

```bash
bao policy write erp-readonly erp-readonly.hcl
```

检查 Policy：

```bash
bao policy list
bao policy read erp-readonly
```

**建立 Policy 只是定义规则，还没有任何新 Token 自动取得该权限。**

## 四、建立并管理 Token

将刚才的 `erp-readonly` Policy 绑定到新 Token：

```bash
bao token create \
  -policy=erp-readonly \
  -ttl=24h \
  -display-name=erp-reader
```

回传示例：

```text
token             hvs.xxxxxxxxxxxxx
token_accessor    xxxxxxxxxxxxx
token_duration    24h
token_renewable   true
token_policies    [default erp-readonly]
```

呼叫者可将此 Token 透过 Bao MCP 传递给 Bao；实际权限由 Bao 依 Token 的 Policy 判断。

同一个 Token 也可以附加多份 Policy：

```bash
bao token create \
  -policy=erp-readonly \
  -policy=github-readonly \
  -ttl=24h
```

多份 Policy 的权限会依 OpenBao 路径匹配与能力规则一并评估。

### Token 与 Policy 的重要特性

1. 内建 `default` Policy 通常自动附加，但并不表示拥有读取所有 Secret 的权限。
2. 一般 Token 签发后，不能直接修改其绑定的 Policy 名单；若要更换 Policy 名单，通常需要重新签发 Token。
3. 若修改了 `erp-readonly` 这份 Policy 的**内容**，持有此 Policy 的既有 Token，在后续请求中便会套用更新后的规则，无须重新签发。

## 五、整体权限关系与验证

```text
Token：erp-reader
    │
    └── Policy：erp-readonly
           │
           └── secret/data/erp/*
                   │
                   ├── erp/database          ✔ 可读取
                   └── github/deploy-token   ✘ 不在授权范围内
```

更精确地说，GitHub 的资料通常位于 `secret/data/github/deploy-token`，不符合上述 ERP Policy，因此没有这份 Policy 授予的读取权限。

使用有权执行检查的管理 Token，验证新 Token 对特定 API 路径的能力：

```bash
bao token capabilities \
  <新Token> secret/data/erp/database
```

测试完成后撤销：

```bash
bao token revoke <新Token>
```

## 六、需不需要建立 Role？

**直接建立测试 Token 时，不必先建立 Role。**

OpenBao 的 Token Role 用于规范 Token 签发行为，例如限制可附加的 Policy、最长 TTL 等，不是每次建立 Token 的必备步骤。

对单纯串接 Bao MCP 的测试案例，基本流程是：

1. **Secret Engine**：建立储存 Mount，例如 `secret/`。
2. **Policy**：建立 `erp-readonly`，限制路径与操作能力。
3. **Token**：签发附加 `erp-readonly` 的 Token，由呼叫者透过 Bao MCP 使用。

## 七、核心结论

**OpenBao 的路径本身不会自动形成存取权限边界；真正决定谁能对该路径执行何种操作的是 Policy。**

对一般 KV Secret 储存需求，一个 KV v2 Mount，加上不同专案路径、不同 Policy 和不同 Token，就能达到基本的权限隔离。没有必要再发明一套额外的 Vault 容器阶层。
