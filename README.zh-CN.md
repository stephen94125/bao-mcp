> [!WARNING]
> 你最好还是多少搞清楚这个系统以及 OpenBao 的运作方式以及安全配置，不然其实你去用 `.env` 就好，没必要搞那么复杂。

# Bao MCP

[English](README.md)

Bao MCP 将 OpenBao 的能力转换成 MCP 工具，供 AI Agent 调用。Agent 可以读写秘密、
申请 SSH 和 TLS 证书，以及使用 Transit 加解密和签名。每次工具调用都携带 Agent
的原生 OpenBao Token，由 Bao 在选定的 namespace 内执行 Policy 检查。

```text
Agent → MCP → Bao MCP → OpenBao / agent namespace
```

## 开始之前

- 已安装 OpenBao 2.7.x；安装方式见 [OpenBao 官方文档](https://openbao.org/docs/install/)。
- 设置脚本需要 Bash 和 Python 3。
- 自行构建需要 Rust 1.92+ 与 C 编译器。
- MCP 客户端需要支持 Streamable HTTP 和 bearer-token 请求头。

下面按顺序启动本地 Bao、准备 `agent` namespace、构建 MCP，再存取第一笔秘密。
如果 Bao 已在运行，使用它的地址，跳过启动新服务器的步骤。

先克隆或下载本仓库，在仓库根目录打开终端。使用实际仓库 URL，例如：

```sh
git clone <repository-url> bao-mcp
cd bao-mcp
```

## 1. 启动或连接 OpenBao

本地体验时，在第一个终端启动 Bao，并保持它运行：

```sh
bao server -dev -dev-no-store-token -dev-listen-address=127.0.0.1:8200
```

[开发模式](https://openbao.org/docs/concepts/dev-server/) 会自动初始化并解封，
在终端显示用于设置的初始 Root Token。数据保存在内存中，重启即丢失，适合本地试用。
持久部署请按 Bao 的 [服务器配置文档](https://openbao.org/docs/configuration/) 设置。

在第二个终端执行：

```sh
export BAO_ADDR=http://127.0.0.1:8200
bao status
```

已有服务器则把 `BAO_ADDR` 换成它的 HTTPS 地址；使用私有 CA 时，再设置
`BAO_CACERT=/path/to/ca.pem`。确认 `Initialized` 为 `true`、`Sealed` 为 `false` 后继续。
全新的持久服务器应先完成[初始化与解封](docs/bao/cli-basics.zh-CN.md#3-查状态与首次初始化)，
并安全保存 Unseal Key Shares 和初始 Root Token。

## 2. 设置 `agent` namespace 和管理员 Token

在本仓库根目录执行：

```sh
./scripts/setup-bao-environment.sh --token-file ./agent-admin.token
```

脚本优先使用环境变量 `BAO_TOKEN`；未设置时，会在终端隐藏输入并询问 Root Token。
本地试用请输入第一步 Bao 显示的 Root Token；已有服务器则使用它的 Root Token 完成设置。
如果 `BAO_TOKEN` 当前是其他身份的凭证，先执行 `unset BAO_TOKEN`，即可改用隐藏输入。

脚本会：

1. 按需创建 `agent`，已有 namespace 会保留。
2. 从 [admin.hcl](scripts/bao/policies/admin.hcl) 安装 namespace 内的 `admin` Policy；相同 Policy 直接沿用。
3. 签发带有 `admin` 和 `default` Policy 的 orphan service token，请求有效期为 24 小时。
4. 以 `0600` 权限写入 `agent-admin.token`，显示实际签发的有效期。

目标 Token 文件已经存在，或已有 `admin` Policy 内容不同时，脚本会停止，方便你选择
新文件或检查 Policy。也可以自行指定参数：

```sh
./scripts/setup-bao-environment.sh \
  --namespace agent --ttl 8h --token-file ./agent-admin-8h.token
```

**Policy 建立在 `agent` namespace 内。** 其中的 `path "*"` 覆盖 `agent/` 及其下层，
相当于对 `/agent/*` 授权。如果在这里写 `path "agent/*"`，就会额外多一层 `agent/`。
新 Token 本身属于 `agent`；只改变 Root Token 的 namespace 设置，并不能把 Root Token
变成受限的 namespace 管理员。

初始 Root Token 用于设置；MCP 客户端使用生成的 namespace Token。请保管好 Token 文件。
Orphan 表示该 Token 的生命周期独立于设置时使用的 Token。服务器 TTL 上限可能缩短请求，
显式最大 TTL 也会限制它的总生命周期。

## 3. 构建并启动 Bao MCP

在源码仓库根目录执行：

```sh
cargo build --release --locked --bin bao-mcp
BAO_NAMESPACE=agent ./target/release/bao-mcp serve
```

Release 产物是单个可执行文件 `target/release/bao-mcp`，SQLite 和 TLS 实现会链接
到该文件。Linux 全静态构建见 [MUSL 构建说明](docs/configuration.zh-CN.md#linux-musl-全静态构建)。
macOS 使用 Apple 内建系统库；若 Nix 环境报告 `cc` 或链接错误，请用
[完整 macOS 编译命令](docs/configuration.zh-CN.md#macos--nix-构建)。

服务会使用第一步设置的 `BAO_ADDR`，默认 MCP 地址为 `http://127.0.0.1:8000/mcp`。
保持服务运行，再让 Agent 连接。

Bao 地址、namespace、监听地址和缓存都通过环境变量配置。远程 HTTPS 部署设置及
构建问题处理见[配置说明](docs/configuration.zh-CN.md)。

## 4. 配置 MCP 客户端

新增一个 HTTP MCP 服务，填写：

| 设置 | 值 |
| --- | --- |
| 传输方式 | Streamable HTTP |
| URL | `http://127.0.0.1:8000/mcp` |
| Authorization 请求头 | `Bearer <agent-admin.token 的内容>` |

通过客户端的 bearer-token 或秘密配置机制提供实际 Token；表格里的尖括号是占位说明。
连接后让客户端列出工具，MCP 会按该 Token 的 Bao 权限发现可用工具。

## 5. 创建 Vault 并存取秘密

让 Agent 依次调用下面的 MCP 工具。这里的 **Vault** 是一组具名的 Bao engine mount。

建立名为 `demo` 的 Vault：

```json
{"name":"vault_create","arguments":{"vault_id":"demo"}}
```

它会准备 `demo/` 的 KV v2、`ssh-ca-demo/` 的 SSH，以及 namespace 共用的
`_sys_pki_root/`、`_sys_pki_int/` 两个 PKI mount。已有且类型相符的 mount 保留原配置。

首次建立 mount 后，刷新工具发现，再让客户端重新列出工具：

```json
{"name":"cache_refresh","arguments":{}}
```

如果刷新返回 `rate_limited`，等待五秒后重试。Engine 尚未建立时，工具清单可能
还没有对应的 engine 工具。

写入示例值：

```json
{"name":"kv_write","arguments":{"mount":"demo","path":"hello","data":{"data":{"message":"Hello from Bao MCP"}}}}
```

读取：

```json
{"name":"kv_read","arguments":{"mount":"demo","path":"hello"}}
```

KV 在 mount 建好后就能使用。SSH／TLS 签发需要先通过 MCP 的 CA、角色工具设置签发规则，
再申请证书；[工具示例](docs/tools.zh-CN.md)提供完整顺序，也包含 Transit。

## 给日常 Agent 较小的权限

设置时生成的 Token 可以管理整个 `agent`，包括 Policy 和 Token。只处理某一个 Vault
的 Agent，可以使用 [Policy 模板](scripts/bao/policies/)：

| 模板 | 在 `agent` 内的权限 |
| --- | --- |
| `admin.hcl` | 管理 namespace 及其下层 |
| `vault-readwrite.hcl` | 完整操作 `demo` 的 KV、SSH mount |
| `vault-readonly.hcl` | 读取、列举、扫描这两个 mount |

套用前，将 `demo` 改成你的 Vault 名称。完整读写包含永久销毁 KV 版本及修改 SSH CA。
PKI mount 由 namespace 共用，应另外按具体的签发角色授权，见[脚本说明](scripts/bao/README.md)。

`scripts/bao/` 提供交互式工具，可套用 Policy、建立可重复登录的管理员账号、签发 Token，
或开启更多 engine。具体输入与命令见[脚本 README](scripts/bao/README.md)。
Policy 改变后，可用 `cache_refresh` 刷新发现的工具清单；实际操作权限由 Bao 即时检查。

## 延伸阅读

- [工具与证书示例](docs/tools.zh-CN.md)
- [配置、缓存、日志及构建问题](docs/configuration.zh-CN.md)
- [请求与授权设计](docs/design.zh-CN.md)
- [Bao CLI 基础](docs/bao/cli-basics.zh-CN.md)、[认证方式](docs/bao/auth-methods.zh-CN.md)、[ACL Policy](docs/bao/acl-policy.zh-CN.md)
- [测试说明](tests/README.md)及[既有验证记录](docs/validation-2026-10-28.zh-CN.md)
- [OpenBao 参考版本](UPSTREAM.md)
