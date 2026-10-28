# Bao MCP 实作与验证纪录：2026-10-28

[English](validation-2026-10-28.md)

本篇记录最初在旧专案旁的 `bao-mcp/` 建立重写版本时的验证；当时没有替换既有程式、设定或 Bao 资源。
参考原始码固定于 OpenBao v2.7.2；实机使用专案既有的 OpenBao v2.7.1 测试服务。
没有升级远端 Bao，也没有把 v2.7.1 实测描述成 v2.7.2 执行期验证。

## 已交付

- 34 个依功能分组的 MCP 工具，以及原生 JSON API 通用入口。
- 原生 Token 逐请求透传，固定 namespace，预设 `agent`。
- 无服务管理员 Token、本地身分／Policy、Web、业务 metadata 或 audit 资料库。
- 未加密 `cache.db`：只存 scope、Token hash、经筛选的能力／mount 资讯与时间。
- 固定 freshness、闲置清除、容量上限、单 Token 与进程级查询限流。
- 仅目前 Token 的快取刷新；实际操作始终由 Bao 即时授权。
- 文字日志使用 Token／资源 hash，避免记录秘密内容与上游错误正文。
- 已有 engine 不覆写；Vault 配置部分失败回传逐项结果，不回滚删除。

## Rust 测试结果

共 **38 个不同测试通过**：

| 范围 | 数量 |
| --- | ---: |
| 设定、快取、发现、凭证单元测试 | 9 |
| HTTP transport 契约 | 10 |
| 能力快取／发现整合 | 5 |
| 工具路由、原生参数、engine 配置 | 12 |
| MCP HTTP 边界 | 2 |

先完成原定的一轮测试；编译或测试失败时，只修正相关范围，跳过已通过项目。
最后没有再执行整套测试、全面安全稽核或另一轮 reviewer 验证。

修正包括 macOS Nix linker／SDK 相容性、SQLite 整数与路径处理、测试的 chunked
body，以及实机发现的 namespace 内建 engine 类型与不存在 mount 的 400 回应。
对 mount 的 400 不直接当成不存在：必须再由原生 mount 清单证明缺少才建立；
清单拒绝或格式不符时停止。

## 既有 Bao 实测结果

**12 个功能检查通过**，另有 namespace 与服务启动两个前置步骤通过：

| 功能检查 | 结果 |
| --- | --- |
| 匿名／错误 Token 拒绝 | 通过 |
| 原生能力对应工具清单 | 通过 |
| Vault 的 KV／SSH／共享 PKI 配置、再次执行不覆写 | 通过 |
| 精确 KV 路径的唯读发现 | 通过 |
| 并行呼叫者 Token 隔离、越权拒绝 | 通过 |
| KV 历史版本、CAS、版本删除／恢复 | 通过 |
| SSH CA、角色与公钥签发 | 通过 |
| PKI root／intermediate／leaf 签发 | 通过 |
| Transit 加解密、签名验证与权限拒绝 | 通过 |
| 原生 Policy／Token 建立与读取 | 通过 |
| Policy 改变即时影响操作、快取刷新、Token 撤销 | 通过 |
| 已知测试凭证与 KV 值未出现在 cache／log | 通过；于 KV／Token 阶段检查 |

首次测试时，原 `.env` 指定 `agent`，但远端没有该 namespace；Token 实际为 root。
确认后，测试从 root 建立独立 namespace，没有修改 `.env` 或建立正式的 `agent`。
新程式仍不负责启动时建立 namespace；正式使用预设值前须由操作员建立 `agent`。

测试 namespace：

```text
mcp-live-20261028t143046z-6d14a9af
```

保留的主要 mount：

```text
live/
ssh-ca-live/
_sys_pki_root/
_sys_pki_int/
live-transit/
```

Policy、测试 Token、KV 版本、SSH／PKI CA 与签发结果、Transit 金钥皆保留。
版本删除后恢复，以及撤销专用测试 Token，是被验证的操作；没有执行远端 teardown。
本机测试程式已结束其自行启动的 MCP process。

脱敏结果位于 `../test-output/mcp-live-20261028t143046z-6d14a9af/results.json`。
同目录的 `state.json`、`private-artifacts.json` 与金钥档含私人测试材料，已排除 Git，
不得将其内容贴入文件或日志。测试脚本支援以既有状态搭配 `--only` 只跑失败项目。

## 保留的限制

- 实机没有配置外部资料库、云端、LDAP、RabbitMQ 或 Kubernetes 测试依赖。
  相关 adapter 路由有契约测试，不能宣称已完成这些外部服务的端到端验证。
- LDAP 在 Bao 2.7 需要相容外部插件；工具不负责安装。
- ACME 配置 API 可操作；未实作或验证 ACME 客户端 JWS 流程。
- Tool discovery 不是完整的权限列举。Bao 内部 resultant ACL 会遗漏 `+` 规则，
  结果明示 `complete:false`；通用工具仍交由 Bao 授权。
- 没有旧版 Mission／Hub／SSH operator workflow、持久化补偿或交付确认保证。
