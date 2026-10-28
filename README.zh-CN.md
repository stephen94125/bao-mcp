# Bao MCP

[English](README.md) · [设计说明](docs/design.zh-CN.md)

独立的 Rust MCP 服务，将每次请求的原生 OpenBao Token 传递给指定 namespace。
不使用服务管理员 Token，不建立本地身份、Policy、业务数据库、Web UI、Mission
或 Hub，也不会在启动时创建 namespace 或 engine。秘密、角色、证书、ACL 和实际
操作审计都以 Bao 为准。

## 启动

需要 Rust 1.92+，以及已经存在的 Bao namespace，默认是 `agent`。

```sh
cd bao-mcp
cargo build --release --locked
BAO_ADDR=https://bao.example.com:8200 ./target/release/bao-mcp serve
```

MCP 客户端使用 Streamable HTTP，地址 `http://127.0.0.1:8000/mcp`，携带
`Authorization: Bearer <原生 OpenBao Token>`。程序仅支持 `serve`、无参数启动、
`--help` 和 `-h`。`/healthz` 只表示本地服务存活，不表示 Bao 可用或调用者有权限。
程序不会自动读取 `.env`。

只有 `BAO_ADDR` 必填。Token 通过 Bao CLI/API 或认证方法在外部取得；JWT 应先通过
Bao 对应的认证方法交换成 Token。旧 MCP Vault 的 Token 不能用于本服务。
Token 必须能够 `read auth/token/lookup-self`，才能完成发现阶段的身份验证。
Bao 的标准 `default` Policy 通常同时授予该权限与相关发现接口权限。
有限使用次数的 Token 会被 metadata 请求消耗次数，MCP 应使用正常的短 TTL 服务
Token，不要直接用一次性 wrapping token 作为 MCP 身份。拆封时，以正常 Token
调用 `sys/wrapping/unwrap`，将 wrapping token 放进原生 JSON body；两者均不缓存或记录。

## 环境变量

| 变量 | 默认值 | 用途 |
| --- | --- | --- |
| `BAO_ADDR` | 必填 | 固定 HTTP(S) Bao 地址；拒绝 URL 内嵌凭证 |
| `BAO_NAMESPACE` | `agent` | 已存在、非空的 namespace，支持嵌套 |
| `BAO_CACERT` | 系统信任 | 可选 PEM CA bundle，始终验证 TLS |
| `MCP_LISTEN` | `127.0.0.1:8000` | 监听地址 |
| `MCP_ALLOWED_HOSTS` | 本机回环主机 | 额外允许的 Host/端口模式，逗号分隔，交由 RMCP 检查 |
| `MCP_ALLOWED_ORIGINS` | 本机回环 HTTP origin | 额外允许的浏览器 Origin，逗号分隔 |
| `CACHE_PATH` | `cache.db` | 可删除、未加密的 SQLite 能力缓存，父目录须存在 |
| `CACHE_IDLE_TTL_SECONDS` | `3600` | 超过多久未使用便清除记录 |
| `CACHE_MAX_AGE_SECONDS` | `60` | 不随使用延长的固定有效期，上限 3600 秒 |
| `CACHE_MAX_ENTRIES` | `1024` | LRU 记录上限，最高 10000 |
| `BAO_TIMEOUT_SECONDS` | `30` | 上游超时，上限 300 秒 |
| `LOG_LEVEL` | `info` | `info`、`warn` 或 `error`，不启用 HTTP 明文追踪 |

远程部署应通过 HTTPS。Host/Origin 检查不能代替 Bao 认证。HTTP 请求体上限 2 MiB，
上游响应上限 8 MiB。不跟随重定向，不自动重试。写操作的结果不确定时返回
`outcome_unknown`；签发、轮换等非幂等操作应先检查 Bao，再决定是否重试。

## 工具

34 个工具按原生功能分组。Engine 工具通常接收 `mount`、`operation`、`path`，
以及可选的 `data`、`query`、`wrap_ttl`。KV 便捷工具自动加入 KV v2 的 API 路径。
`data` 就是完整的原生 JSON 请求体，不使用另一套本地数据模型。
可用 `api_help` 从 Bao 查询端点字段与操作方式。

| 工具 | 能力 |
| --- | --- |
| `vault_create` | 幂等地分配一组具名 engine |
| `kv_read`、`kv_write`、`kv_manage` | KV v1/v2、版本、CAS、merge patch、metadata、删除/恢复/销毁 |
| `ssh_ca`、`ssh_roles`、`ssh_issue` | SSH issuer、CA、角色、签名、密钥/证书签发和 OTP |
| `pki_ca`、`pki_roles`、`pki_certificates`、`pki_config` | PKI CA/issuer、角色、签发/签名、撤销、CRL 和配置 |
| `transit_keys`、`transit_crypto` | 密钥导入/导出/轮换、加解密、签名/验证、HMAC/hash/random |
| `database_connections`、`database_roles`、`database_credentials` | 数据库连接、角色、动态/静态凭证与轮换 |
| `kubernetes`、`ldap`、`rabbitmq` | 相应 engine 的原生 API；Bao 2.7 的 LDAP 需要另装兼容外部插件 |
| `totp_keys`、`totp_code`、`cubbyhole` | TOTP 与 Token 私有存储 |
| `identity`、`auth`、`token`、`policy` | 原生身份、认证方法、Token 和 Policy 管理 |
| `leases`、`mounts`、`namespaces`、`audit`、`system` | 在配置 namespace 与调用者 ACL 内进行原生系统管理 |
| `bao_request`、`api_help` | 原生 JSON API 通用入口和原生端点/OpenAPI 帮助 |
| `cache_refresh` | 只刷新当前已认证 Token 的发现缓存 |

`bao_request` 支持其他原生 JSON API 与已安装插件，不必为每个 REST 端点新增工具。
它不接受任意上游 URL/Header、原始二进制上传或 ACME 客户端 JWS 协议。
可以管理 ACME engine 配置，实际 ACME 流程仍由 ACME 客户端执行。
工具存在不代表服务器已安装对应插件，也不代表已配置外部数据库、云端或其他服务。

### 调用示例

```json
{"name":"kv_write","arguments":{"mount":"team","path":"service","data":{"data":{"password":"example"},"options":{"cas":0}}}}
{"name":"kv_read","arguments":{"mount":"team","path":"service"}}
{"name":"ssh_issue","arguments":{"mount":"ssh-ca-team","operation":"write","path":"sign/deploy","data":{"public_key":"ssh-ed25519 ...","valid_principals":"deploy","ttl":"5m"}}}
{"name":"transit_crypto","arguments":{"mount":"transit","operation":"write","path":"sign/build","data":{"input":"aGVsbG8="}}}
{"name":"bao_request","arguments":{"operation":"write","path":"sys/wrapping/unwrap","data":{"token":"<wrapping token>"}}}
{"name":"cache_refresh","arguments":{}}
```

### Vault 分配

`vault_create` 接收 `vault_id`，默认确保以下四个 mount 存在：

- `<vault_id>`：KV v2。
- `ssh-ca-<vault_id>`：SSH。
- `_sys_pki_root`、`_sys_pki_int`：沿用旧命名，由整个 namespace 共享的 PKI。

不会创建 namespace、`_vault_meta` 或 CA 私钥，不修改既有角色和 engine 配置。
CA、角色和签发通过原生 SSH/PKI 工具显式操作。已有 mount 的 engine 类型或 KV 版本
不符时返回冲突。部分完成时返回 `complete:false`、每个 mount 的结果及 MCP
`isError:true`；已经创建的 mount 会保留，再次调用只补缺少的 mount。
两个 PKI mount 不提供按 vault 的独立 PKI 隔离。

## 发现与缓存

首次请求通过 `lookup-self` 验证 Token，取得可见 mount 与 resultant ACL，再用
`sys/capabilities-self` 批量确认有数量上限的具体候选路径。精确路径的读取与写入
权限分别映射到工具。缓存命中时不重复执行这些查询。

**发现结果不能保证列出全部可用权限。** Bao 2.7.2 的 internal resultant ACL 接口
省略 `+` 单层通配规则，capabilities 查询也不能枚举未知路径。缺少 metadata
权限、候选数量限制等都可能使部分工具未显示。已认证调用者仍可使用
`bao_request` 和 `api_help`。一个分组工具可见，只代表发现某个可用路径或操作，
不代表所有参数都被授权。每次实际调用仍直接交给 Bao，缓存清单不授予也不阻止执行。
internal UI metadata API 随 Bao 版本变化；刷新会返回 `complete:false`、具体
capabilities、mount 和限制说明。

缓存主键包含上游地址/namespace 的 scope 与 Token 完整 SHA-256 hash。只保存经过
筛选的发现资料、时间与 hash，不保存完整 lookup 响应、明文 Token、秘密值、请求体或
私钥。数据库未加密，文件权限为 0600。停止服务后可删除，之后按需重建；损坏时明确
报错，不静默删除操作员的文件。

缓存有效期同时受 Token TTL 和 `CACHE_MAX_AGE_SECONDS` 限制。使用时更新
`last_used`，但不延长固定有效期。过期、闲置记录在启动及缓存存取时清除；同时限制
记录数量、32 MiB 数据库及 512 KiB 单笔 snapshot。没有业务状态数据库。

`cache_refresh` 先重新验证调用者，再更新自己的记录。错误 Token 不会进入数据库。
冷查询和刷新共享限制：每个 Token 每五秒一次、每个进程每秒最多开始 32 次查询、
最多八次并发查询；HTTP 最多 64 个并发请求。首次发现后立即刷新可能收到 429。
随机 Token 也受到进程级限制。多副本各自计算限额。原生拒绝导致缓存失效后，允许
在同一进程级预算内重新验证，避免一个禁止路径使其他允许路径也暂时无法使用。

修改 Policy 后可刷新或等待固定有效期。已撤销的 Token 可能短暂保留旧的工具清单，
但受保护的操作会立即由 Bao 拒绝，401/403 执行错误会清除对应缓存。
此缓存不实现旧 Mission 的任务取消等额外语义。

## 日志与测试

文字日志写入 stderr，业务调用记录 Token 的 SHA-256 ID、工具名称、资源标识 hash、
结果与耗时。不记录 Bearer、原始路径、参数、秘密值、私钥或任意上游错误正文。
实际原生操作与身份历史由另行配置的 Bao audit 负责；应用日志不替代 Bao audit。

执行一轮 Rust 契约测试：

```sh
cargo test --locked --all-targets
```

此 macOS 工作站如果 Nix linker 找不到 `libiconv`，为构建命令设置
`CC=/usr/bin/clang`、`CXX=/usr/bin/clang++`、
`SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk` 和
`CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER=/usr/bin/clang`。

[在线测试说明](tests/README.md) 描述如何使用现有 Bao，并保留隔离测试 namespace。
修复后只重跑失败范围，不重复执行整套稽核。[上游版本记录](UPSTREAM.md)。

验证证据：[2026-10-28 报告](docs/validation-2026-10-28.zh-CN.md)。
