# Bao MCP：资料来源、权限与快取的边界

[English](design.md)

Bao MCP 是 Rust／RMCP／Axum 写成的原生 Bao 工具介面。上游参考版本为
OpenBao v2.7.2；完整操作方式与环境变数见 [README](../README.zh-CN.md)。

## 资料只由 Bao 管理

Secret 的内容、版本与时间、SSH／PKI 角色与凭证、Transit 金钥，以及 Policy、Token
都在 Bao。程式不建立第二份 metadata，不依 `created_by` 判断拥有者，不产生
`_vault_meta`，不储存业务操作 journal。

因此人工用 Bao CLI／GUI 改资料，MCP 下次读取就使用 Bao 回传结果。没有本地同步
排程，也没有本地成功纪录能替代实际状态。如果一次多步骤建立只完成部分 engine，
回传逐项结果并保留已完成部分；下次再补缺少的项目。不替使用者回滚删除资源。

## 每一个请求使用自己的 Token

```text
Agent 的原生 Bao Token
        ↓ Authorization: Bearer
Bao MCP：解析工具、检查参数、转换原生 API 路径
        ↓ X-Vault-Token + 固定 X-Vault-Namespace
OpenBao：身分验证、原生 Policy／role 限制、执行与 audit
```

程序没有共用管理员 Token。namespace 预设 `agent`，必须事先存在；启动时不连线
建立任何 Bao 资源。呼叫者不能透过工具参数改 Header 或切换配置的 namespace。
若有管理子 namespace 的原生权限，仍能在这个 namespace 之下操作。

新增 Vault 时沿用原命名：KV 为 Vault 名称、SSH 为 `ssh-ca-<名称>`、PKI 共用
`_sys_pki_root`／`_sys_pki_int`。这一步只配置 engine，不擅自产生 CA 或覆写既有角色。
需要的 CA、SSH 使用者／TTL／选项、PKI 角色限制都用原生工具设定在 Bao。

没有旧版的本地 Token、Mission JWT、SSH operator profile、Hub 或 Web UI。
JWT 工作负载可以使用 Bao 自己的 JWT auth 交换 Token；工作负载如何取得 Token
不是这个 MCP 的第二套身分系统。

## 能力快取不参与实际授权

`cache.db` 是未加密、可删除的 SQLite，主键是上游／namespace scope 与 Token hash。
保存能力查询结果、工具名称、mount 类型、建立／到期／最后使用时间；不保存 Token、
Secret 或私钥。档案权限仍为 0600。

首次查询会验证 Token 并取得候选权限；之后在固定 freshness 内命中快取。
`CACHE_IDLE_TTL_SECONDS` 限制闲置多久清除，`CACHE_MAX_AGE_SECONDS` 限制即使一直
使用也必须重新查询的时间。清除在启动或存取时执行。`cache_refresh` 只更新目前
Token 的资料，而且先重新验证 Token；未知或错误的 Token 不能建立快取。

快取可能短暂反映旧 Policy，这只影响工具清单。实际读写没有使用快取放行，也不会
因清单缺少工具就阻挡有效的原生操作。每次都交由 Bao 即时裁决。

为防止大量随机 Token 或反复刷新耗尽资源，冷查询与刷新有单 Token 间隔、进程级
速率、并发、记录数与资料大小上限。首次发现后五秒内刷新可能被限流。

## 工具清单不能保证精确列举所有权限

Bao capabilities API 只能回答「这些具体路径能做什么」，不能列举所有有权限路径。
程式使用 Bao resultant ACL 与 mount 资讯建立候选，再确认具体路径能力。
OpenBao v2.7.2 的该内部 ACL 介面没有回传 `+` 路径规则，因此不能宣称完整发现。

刷新结果会明示 `complete:false` 与限制。已验证 Token 可使用 `bao_request`／
`api_help` 操作没有被发现的原生 JSON API。这些工具仍然没有超越 Bao 的权限。
一般 KV 栏位也不会被 MCP 自动过滤；不同权限资料宜分不同原生 Secret 路径。

## 故障与日志

禁止自动重试可能已执行的写操作。网路或后端错误使结果无法确认时，回传
`outcome_unknown`，由呼叫者查 Bao 决定下一步。这个版本没有旧版的持久化补偿、
任务防重播或交付状态保证；原生 wrapping token 的一次性语义仍由 Bao 负责。

文字日志记录 Token hash、工具、资源路径 hash、结果与耗时。秘密值、原始 Token、
完整参数和上游错误正文不写入 log。详细的实际路径、身分和原生操作历史，使用
已启用的 Bao audit；应用文字日志只补充 Agent 呼叫工具的过程。

## 功能覆盖的定义

34 个工具按 engine 与用途分组，原生 JSON body 保持原样。Transit 包含加解密与
签名验证。其余 engine 或外挂可透过 `bao_request`，不需要照每一个 REST endpoint
建立一个 MCP tool。这表示 API 可通达，不代表外部资料库、云端、LDAP、RabbitMQ
或 Kubernetes 已完成连线设定与端到端验证。

LDAP 在 Bao 2.7 需要相容外部外挂。ACME 配置可操作，但 ACME 的 JWS 协定流程仍
交由 ACME 客户端处理。原始二进位上传与任意自订 Header 不在这个 JSON 工具介面内。
