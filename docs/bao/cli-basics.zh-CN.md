# OpenBao CLI 基本操作笔记

[English](cli-basics.md)

适用于以 macOS／Nix 的 `bao` CLI，远端管理已架设在 Kubernetes 等环境的 OpenBao。以下采 **Shamir 手动 Unseal**，不讨论 Auto Unseal。

## 1. 安装与执行档

OpenBao 的 **CLI 与 Server 是同一支 `bao` 执行档**，不是独立的精简 CLI。安装套件包含两者的功能，但不代表会自动启动本机 Server。

```bash
# 使用 Nix 安装
nix profile install nixpkgs#openbao

# 确认
bao version

# 暂时使用，不永久安装
nix shell nixpkgs#openbao
```

`bao server` 用于启动 Server；`bao status`、`bao operator unseal` 等是呼叫远端 Server API 的 CLI 操作。CLI 不需要 SSH 登入伺服器或进入 Pod，只要能连线到 OpenBao API。

参考：[OpenBao CLI 文件](https://openbao.org/docs/commands/)、[Server 指令](https://openbao.org/docs/commands/server/)。

## 2. 设定连线与身分

```bash
export BAO_ADDR="https://bao.example.com"
export BAO_TOKEN="你的管理 Token"

# 如使用内部 CA，可额外指定
export BAO_CACERT="/path/to/ca.crt"
```

| 变数 | 功能 |
| --- | --- |
| `BAO_ADDR` | OpenBao API 的 URL，决定连到哪一台／哪个丛集 |
| `BAO_TOKEN` | 身分验证 Token，依所附 Policy 决定权限 |
| `BAO_CACERT` | 选用，TLS 凭证的信任 CA 路径 |
| `BAO_TOKEN_PATH` | 选用，指定 `bao login` 储存 Token 的本机路径 |

设定好 `BAO_ADDR` + `BAO_TOKEN`，就**不需要再执行 `bao login`**。

另一种方式是只设定 `BAO_ADDR`，再执行：

```bash
bao login
# 输入现有的 Token，验证后会保存在本机
```

预设 Token Helper 储存在 `~/.vault-token`，后续 CLI 会自动取用。也能使用其他验证方法，例如 `bao login -method=userpass username=admin`（前提是 Server 启用了对应 Auth Method）。当 `BAO_TOKEN` 已设定时，CLI 优先使用环境变数，而不是本机储存的 Token。

**注意：** `bao login` 取代的是手动提供 `BAO_TOKEN`，**不会取代设定远端 URL 的 `BAO_ADDR`**。不要把管理 Token 写进 Git 或公开脚本。初始化时产生的 Initial Root Token 有最高权限，与日常受限 Policy 的 Admin Token 不同。

参考：[CLI 环境变数](https://openbao.org/docs/commands/)、[Token Helpers](https://openbao.org/docs/commands/token-helper/)。

## 3. 查状态与首次初始化

```bash
bao status

# 仅在全新、尚未初始化的储存后端执行一次
bao operator init -key-shares=1 -key-threshold=1
```

`bao status` 会显示 `Initialized`、`Sealed`、`Key Shares`、`Key Threshold` 等资讯；`Sealed: true` 代表还不能正常存取 Secret。

`operator init` 会产生 Unseal Key Shares 与 **Initial Root Token**，必须立即妥善保存。已初始化的丛集不可再次执行 `init`；HA 共用同一储存后端，只需初始化一次。

### Unseal Key 的 1/1、2/3、3/5

`-key-shares` 是产生的分片总数；`-key-threshold` 是一次解封所需的不同分片数量。

| 设定 | 初始化指令 | 解封条件 |
| --- | --- | --- |
| **1 of 1（1/1）** | `bao operator init -key-shares=1 -key-threshold=1` | 只有一把有效 Unseal Key，输入一次即可 |
| **2 of 3（2/3）** | `bao operator init -key-shares=3 -key-threshold=2` | 产生三片，任选其中两片 |
| **3 of 5（3/5）** | `bao operator init -key-shares=5 -key-threshold=3` | 产生五片，任选其中三片 |

OpenBao 的 Shamir Shares **不是 SSH authorized_keys 那种能逐把授权、逐把撤销的独立登入钥匙**。例如 2/3 的任一片不能单独 Unseal，且不能只撤销 B 而保留原本 A、C 的分片组。

如果需求是「任何一份备份都能单独解封」，选 **1/1，再把同一把 Unseal Key 备份到三个不同安全位置**。这是同一把钥匙的三份副本，并非三把可独立撤销的钥匙；一份泄漏时应轮换整组并更新所有备份。

参考：[operator init](https://openbao.org/docs/commands/operator/init/)、[Seal／Unseal](https://openbao.org/docs/concepts/seal/)。

## 4. 手动 Unseal

OpenBao 重启后，Shamir 模式通常回到 Sealed 状态。CLI 可以从 Mac 远端呼叫 API，**不需要 `BAO_TOKEN`、不需要先 `bao login`**：

```bash
bao status
bao operator unseal  # 依提示输入一片，输入内容隐藏
bao status
```

1/1 只需执行一次；2/3 需用**两片不同的 Key**执行两次；3/5 需用三片不同的 Key 执行三次。不要把钥匙直接写在指令参数里，以免进 Shell History。

如果是 K8s，外部 Ingress 在 Pod 尚未 Ready 时可能无法连通。可以从 Mac 对指定 Pod 使用 `kubectl port-forward`（需有 Kubernetes 存取权限），再设定 `BAO_ADDR` 指向本机转发埠。多节点丛集的每个节点都要各自完成 Unseal。

参考：[operator unseal](https://openbao.org/docs/commands/operator/unseal/)。

## 5. `init` 与 Unseal Key 轮换：指令差异与完整流程

**`bao operator init` 和 `bao operator rotate-keys -init` 完全不一样。** 前者是首次初始化整个 OpenBao，后者只是启动一次「轮换 Unseal Key」作业。第二个 `-init` 不会重新初始化 OpenBao、不会清空 Secrets，也不会立刻产生新钥匙。

| 指令 | 实际意义 | 何时使用 |
| --- | --- | --- |
| `bao operator init` | 初始化整个 OpenBao 储存后端，建立初始金钥与 Initial Root Token | **全新丛集，只能成功执行一次** |
| `bao operator rotate-keys -init` | 启动一次 Unseal Key 轮换程序，设定新 Shares 数量和门槛 | **已初始化且已解封**，要换钥匙时 |
| `bao operator rotate-keys` | 提交旧 Unseal Key，达到旧门槛后完成轮换并产生新 Shares | **已启动轮换程序时** |
| `bao operator rotate-keys -status` | 查看轮换进度 | 轮换过程中 |
| `bao operator rotate-keys -cancel` | 取消**尚未完成**的轮换，清掉已提交 Shares 与待处理设定 | 轮换进行中决定不换时 |

### 执行流程：把旧的 1/1 Unseal Key 换成新的一把

```bash
# 1. 启动轮换，不是重新初始化 OpenBao
bao operator rotate-keys -init -key-shares=1 -key-threshold=1

# 2. 可查询轮换状态、nonce
bao operator rotate-keys -status

# 3. 提交原本的 Unseal Key，依 CLI 提示提供轮换 nonce
bao operator rotate-keys

# 4. 如还未达到旧 Threshold，重复上一步，提交不同的旧 Key
```

原本是 **1/1**，提交一份旧 Key 即达到门槛；原本是 **2/3**，要提交不同的两片；原本是 **3/5**，需要三片。预设不启用额外验证阶段时，达到门槛即完成轮换，输出新的 Unseal Shares，**整组旧 Shares 从此失效**。OpenBao 不必停机或重新初始化，已有 Secrets 和 Admin Token 不受此轮换影响。新的 Key 要立即安全备份。

若进行到一半不想换了，使用：

```bash
bao operator rotate-keys -cancel
```

`-cancel` **只取消尚未完成的轮换**；不能把已完成轮换后失效的旧 Key 重新启用。

`-init` **有进行中的轮换时不能重复启动**；完成或取消后，才能开始下一次轮换。注意 `-key-shares` 和 `-key-threshold` 的预设值是 **5/3**，要维持 1/1 时必须明确指定。OpenBao 官方文件采用单破折号 `-init`，可用 `bao operator rotate-keys -help` 查看目前 CLI 支援的参数。

### `bao operator rotate`：轮换的是资料加密金钥，不是 Unseal Key

以下两个指令**不能混用**：

| 指令 | 轮换哪一种金钥 | 旧 Unseal Key 会失效吗？ |
| --- | --- | --- |
| `bao operator rotate-keys`（完成轮换流程） | Unseal Shares／保护底层 Keyring 的 Root Key | **会**，整组旧 Shares 失效 |
| `bao operator rotate` | OpenBao Storage Backend 的加密 Keyring 使用的资料加密金钥 | **不会**，Unseal Key 原封不动 |

```bash
# 仅轮换底层资料加密金钥，不会换 Unseal Key
bao operator rotate
```

`bao operator rotate` 建立新的 Keyring 加密金钥供**后续写入**使用；旧金钥保留在内部 Keyring，以便读取先前加密的资料，不需要把整个资料库重加密。这不是手动解封，也不是更换管理员 Token。

参考：[OpenBao operator init](https://openbao.org/docs/commands/operator/init/)、[OpenBao operator rotate-keys](https://openbao.org/docs/commands/operator/rotate-keys/)、[OpenBao operator rotate](https://openbao.org/docs/commands/operator/rotate/)、[OpenBao Key rotation 原理](https://openbao.org/docs/internals/rotation/)。

## 6. 管理与 KV 常用指令

下列命令通常需要有效的 Token，且实际权限取决于 Token 的 Policy：

```bash
# 查目前使用的 Token 与身分资讯
bao token lookup

# 列出已启用的 Secret Engines
bao secrets list

# 列出 Policies
bao policy list

# 读取 KV Secret（假设 KV 挂在 secret/，key 为 app/config）
bao kv get -mount=secret app/config

# 只输出单一栏位，例如 password；这不是栏位级存取控制
bao kv get -mount=secret -field=password app/config

# 输出 JSON
bao kv get -mount=secret -format=json app/config
```

`kv get` 的 `-mount=secret` 是 Secret Engine 挂载位置，不是固定必须叫 `secret`；`app/config` 才是该挂载点下的 Key 路径。若该 KV Engine 尚未启用或 Token 权限不足，读取会失败。对 KV v2，使用 `-mount` 可以免去自己处理 `data/` API 路径。

参考：[token lookup](https://openbao.org/docs/commands/token/lookup/)、[KV get](https://openbao.org/docs/commands/kv/get/)。

## 7. 最小操作速查

```bash
# 设定远端 Server
export BAO_ADDR="https://bao.example.com"

# 查看目前是否已初始化／解封
bao status

# 全新丛集仅执行一次，择一：1/1、2/3 或 3/5
bao operator init -key-shares=1 -key-threshold=1

# 重启后手动解封（依门槛输入足额分片）
bao operator unseal

# 管理操作二选一：设定 Token，或执行 bao login
export BAO_TOKEN="你的管理 Token"
# bao login

bao token lookup
bao secrets list
bao policy list
bao kv get -mount=secret app/config
```

**核心区分：** `BAO_ADDR` 决定操作哪台 Server；`BAO_TOKEN` 或 `bao login` 提供日常管理身分；**Unseal Key 专门用于解封，不等于 Admin Token**。
