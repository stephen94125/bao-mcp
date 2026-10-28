# Bao MCP 配置说明

[English](configuration.md)

首次连接请按 [README](../README.zh-CN.md) 操作。本文提供设置后的运行参数与行为参考。

## 环境变量

通过 shell 或服务管理器把变量传入进程。使用 [.env.example](../.env.example) 时，
先把需要的设置加载到环境中，再启动 Bao MCP。

| 变量 | 默认值 | 含义 |
| --- | --- | --- |
| `BAO_ADDR` | 必填 | Bao HTTP(S) 地址，例如 `https://bao.example.com:8200` |
| `BAO_NAMESPACE` | `agent` | 已存在的 namespace，支持嵌套 |
| `BAO_CACERT` | 系统信任 | 可选的 Bao PEM CA bundle |
| `BAO_SKIP_VERIFY` | `false` | 设为 `true` 或 `1`，跳过 Bao HTTPS 证书及主机名验证；`false` 或 `0` 启用验证 |
| `MCP_LISTEN` | `127.0.0.1:8000` | MCP 监听地址 |
| `MCP_BODY_TIMEOUT_SECONDS` | `30` | 接收完整 MCP POST 请求体的期限，最多 300 秒 |
| `MCP_ALLOWED_HOSTS` | 回环主机 | 额外允许的 HTTP Host 模式，逗号分隔 |
| `MCP_ALLOWED_ORIGINS` | 回环 HTTP origin | 额外允许的浏览器 Origin，逗号分隔 |
| `CACHE_PATH` | `cache.db` | 工具发现的 SQLite 缓存，父目录需要存在 |
| `CACHE_IDLE_TTL_SECONDS` | `3600` | 记录可以被清除前的闲置时长 |
| `CACHE_MAX_AGE_SECONDS` | `60` | 固定的发现有效期，最高 3600 秒 |
| `CACHE_MAX_ENTRIES` | `1024` | LRU 记录上限，最高 10000 |
| `BAO_TIMEOUT_SECONDS` | `30` | 上游请求超时，最高 300 秒 |
| `LOG_LEVEL` | `info` | `info`、`warn` 或 `error` |

程序支持 `serve`（也是默认命令）、`--help` 和 `-h`。`/healthz` 表示本地进程存活；
成功的已认证 MCP 请求还会检查 Bao 连接与调用者权限。

## 远程客户端与 HTTPS

通过 HTTPS 反向代理提供服务时，例如：

```sh
export BAO_ADDR=https://bao.example.com:8200
export BAO_NAMESPACE=agent
export MCP_LISTEN=0.0.0.0:8000
export MCP_ALLOWED_HOSTS=bao-mcp.example.com
export MCP_ALLOWED_ORIGINS=https://agent-ui.example.com
./target/release/bao-mcp serve
```

按部署环境配置反向代理和防火墙。上例对外的 MCP URL 是
`https://bao-mcp.example.com/mcp`。

`MCP_ALLOWED_HOSTS` 对应 HTTP Host，填主机名，或含端口的模式
`bao-mcp.example.com:*`。Origin 则包含协议，例如 `https://agent-ui.example.com`。
浏览器客户端可能需要配置对应 Origin；每个调用者仍需提供自己的 bearer token。

私有 Bao CA 通过 `BAO_CACERT` 配置，默认验证 TLS 证书与主机名。HTTP 请求体上限为 8 MiB，
Bao 响应上限为 8 MiB。上游请求不跟随重定向，也不自动重试。
未完成的 POST 请求体在 `MCP_BODY_TIMEOUT_SECONDS` 后返回 HTTP 408 并释放请求名额。
此期限仅用于接收请求体，不会取消已经发出的 Bao 写入；请求体过大时返回 HTTP 413。

`BAO_SKIP_VERIFY=true` 可连接未受信任或主机名不匹配的测试服务器。它会关闭
证书与主机名验证，只应在明确接受此风险的环境中启用。默认值为 `false`，
也接受 `1`／`0`；其他值会导致启动失败。

## Token 范围与工具发现

Token 属于 Bao namespace。`BAO_NAMESPACE` 必须指向调用者有权访问的空间。
建立在 `agent` 内的 `path "*"` 会授权 `agent/` 下匹配的路径；Root Token 切换到
这个 namespace 后仍保有原来的 Root 权限。

冷请求通过 `auth/token/lookup-self` 验证身份，再读取 mount、resultant ACL 信息，
并通过 `sys/capabilities-self` 查询具体路径。
[随附 Policy 模板](../scripts/bao/policies/)包含发现与 Token 自我管理权限；
内建 `default` Policy 通常也提供相关 metadata 权限。

客户端宜使用有 TTL 的 service token。有限使用次数的 Token 也会被 metadata 请求消耗。
拆封 response wrapping 时，以正常调用者 Token 认证，把 wrapping token 放在
`sys/wrapping/unwrap` 的原生 JSON body 中。

## 缓存行为

发现结果按完整 Token hash 与上游/namespace scope 分开缓存。`cache.db` 保存派生的
capabilities、mount、工具名称和时间；秘密值与原始凭证不会进入缓存。SQLite 未加密，
文件权限为 `0600`。

使用时更新 `last_used`，但固定有效期不延长；Token TTL 也会限制记录到期时间。
启动或缓存访问时清除过期、闲置记录，并限制记录数量、32 MiB 数据库和 512 KiB 单笔
snapshot。需要删除可丢弃或损坏的缓存时先停止服务，下次请求会重新建立。

Policy 改变后调用 `cache_refresh`，只刷新自己的记录。发现请求限制为每个 Token
每五秒一次、每个进程每秒最多开始 32 次、最多八次并行；HTTP 并发上限为 64。
原生权限拒绝会使对应缓存失效，并允许在进程总限额内重新验证。

缓存的工具清单可能短暂反映旧 Policy；实际操作始终携带当前 Token 交给 Bao，按当前
权限执行。

### 发现范围

Bao 2.7.2 内部的 resultant ACL 信息省略 `+` 单层通配规则，capabilities 查询也只回答
给定的路径。因此部分有权限的工具可能未显示；分组工具可见，也不代表它的所有参数都
被授权。可用 `bao_request`、`api_help` 访问其他已授权的原生 JSON 路径。
刷新结果以 `complete:false` 和 warnings 说明这一范围。

## 失败处理与日志

写入结果不明确时，先查 Bao 再决定是否重试。例如证书可能已签发，但连接随后失败，
会返回 `outcome_unknown`。`vault_create` 会返回逐个 mount 的部分结果，方便检查并
补建缺少的 mount。

文字日志写入 stderr，记录 Token hash、工具、资源 hash、结果和耗时。需要原生请求
路径、身份与操作历史时，另外启用 Bao audit。

## 构建单个可执行文件

Manifest 已显式声明 `bao-mcp` 可执行目标：

```sh
cargo build --release --locked --bin bao-mcp
```

部署时使用 `target/release/bao-mcp`。Cargo 产生的 Rust 中间库会链接进此文件。
SQLite 使用 bundled 源码，TLS 使用 Rustls，无需另装 OpenSSL 或 SQLite 动态库。

### Linux MUSL 全静态构建

通过只用于构建的 Docker 配方导出单个 Linux 可执行文件：

```sh
docker buildx build --platform linux/amd64 --target artifact \
  --output type=local,dest=dist/x86_64-unknown-linux-musl \
  -f Dockerfile.build .
```

ARM64 改用 `--platform linux/arm64` 和 `dest=dist/aarch64-unknown-linux-musl`。
Docker 需要可用的 Linux builder，跨 CPU 架构时可能需要模拟器。构建上下文仅包含
manifest、lockfile、Cargo 配置及 Rust 源码。

MUSL 目标配置显式启用 `crt-static`。配方会检查 ELF 不含 interpreter 或共享库
`NEEDED` 项目，执行 `--help` 后才导出文件。在已安装对应 Rust target 和 MUSL C
编译器的 Linux 主机上，也可直接执行：

```sh
cargo build --release --locked --bin bao-mcp --target x86_64-unknown-linux-musl
```

产物位于 `target/x86_64-unknown-linux-musl/release/bao-mcp`。静态链接不会嵌入
系统信任的 CA store；需要验证 HTTPS 时，应保留系统 CA bundle 或设置 `BAO_CACERT`。

## macOS / Nix 构建

macOS 使用 Apple 内建的系统库与 framework。Apple 不支持完全静态的可执行文件，
详见 [Library Primer](https://developer.apple.com/forums/thread/715385)。部署时只需复制
可执行文件，这些库由操作系统提供。

如果 Nix 报告 `cc`、`libiconv` 或链接错误，可使用系统 C 工具链与 SDK：

```sh
CC=/usr/bin/clang CXX=/usr/bin/clang++ \
SDKROOT="$(xcrun --sdk macosx --show-sdk-path)" \
CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER=/usr/bin/clang \
cargo build --release --locked --bin bao-mcp
```

上面的 linker 变量用于 Apple Silicon；Intel Mac 应使用对应目标的变量。
测试方式见 [tests/README.md](../tests/README.md)。
