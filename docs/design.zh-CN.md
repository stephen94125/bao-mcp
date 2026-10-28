# Bao MCP 架构

[English](design.md)

Bao MCP 把 OpenBao 的 HTTP API 转成供 AI Agent 调用的工具。Rust、RMCP 和 Axum
负责 MCP 传输，OpenBao 负责保存资源与权限控制。使用方式见
[入门指南](../README.zh-CN.md)、[配置说明](configuration.zh-CN.md)及
[工具示例](tools.zh-CN.md)。

## 请求流程

```text
Agent 的 OpenBao Token
        ↓ Authorization: Bearer
Bao MCP：检查参数，将工具操作转换成 API 路径
        ↓ X-Vault-Token + 配置的 X-Vault-Namespace
OpenBao：认证、授权、执行与审计
```

每次调用都把调用者的 Token 传给配置的 namespace，默认是 `agent`。连接客户端前，
先通过设置脚本准备好该 namespace。工具参数选择此范围内的资源，Bao Policy
决定调用者对这些资源及子 namespace 的权限。

## 资源状态由 Bao 管理

Secret、版本、时间戳、SSH 和 PKI 角色、证书、Transit 密钥、Policy 及 Token
都通过 Bao 读写。通过 Bao CLI、UI 或其他 API 客户端作出的修改，会反映在之后的
MCP 读取结果中。

`vault_create` 组合相关 mount：KV 使用 Vault 名称，SSH 使用 `ssh-ca-<名称>`，
PKI 使用 namespace 共用的 `_sys_pki_root` 和 `_sys_pki_int`。产生 CA、设置角色
限制与连接外部后端，都是明确的后续配置步骤。配置流程保留已有 mount，并逐项
回报结果。部分完成时，先检查结果，再重复调用以补齐缺少的 mount。

## 工具发现与缓存

发现工具时，Bao MCP 先验证 Token，再根据 Bao 的 resultant ACL 和 mount 信息
建立候选 API 路径，查询原生 capabilities，选择相关工具。缓存按上游、namespace
及 Token hash 区分结果。详见[缓存配置](configuration.zh-CN.md#缓存行为)。

`cache.db` 保存 capabilities、工具名称、mount 类型及时间信息，是权限为 `0600`、
可重建的 SQLite。固定有效期和闲置期限限制复用时间；容量、并发和速率限制控制
查询成本。调用者使用 `cache_refresh` 时，会重新验证其 Token 并更新自己的记录。

工具发现帮助 Agent 选择工具。每次实际操作仍由 OpenBao 使用传入的 Token 授权，
包括发现缓存尚未更新的情况。

## 工具发现的限制

Capabilities API 回答的是指定路径的权限。OpenBao v2.7.2 的内部 resultant ACL
信息会遗漏 `+` 通配符规则，因此发现结果标明 `complete:false`。已验证的调用者
可以用 `bao_request` 和 `api_help` 操作分组之外的原生 JSON API，同样受 Bao ACL
限制。不同值需要不同权限时，应将它们放在不同的 KV Secret 路径。

## 失败处理与日志

无法确定写入结果时，返回 `outcome_unknown`。该操作可能已经完成，应先查询 Bao
再决定是否重试。多步骤的 mount 配置会分别回报完成与失败项目，供调用者接续处理。

文字日志记录 Token hash、工具名称、资源路径 hash、结果及耗时。Token、秘密值、
完整参数与上游错误正文不写入日志。Bao audit 提供原生请求路径、认证身份与操作
历史；MCP 日志补充工具调用的上下文。

## API 覆盖范围

34 个工具按 engine 与用途组合 JSON API，保留原生请求 body。Transit 包含加密、
解密、签名与验证。`bao_request` 可访问其他原生 JSON API 及已安装的插件。
数据库、云端、目录服务、消息代理和 Kubernetes engine 仍需配置各自的后端与凭证。

Bao 2.7 的 LDAP 需要兼容外部插件。PKI 配置完成后，ACME JWS 协议交由 ACME
客户端处理。此接口用于结构化 JSON 请求；二进制传输应使用对应的原生客户端。
