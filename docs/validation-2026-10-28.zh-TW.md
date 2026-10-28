# Bao MCP 實作與驗證紀錄：2026-10-28

新專案位於 `bao-mcp/`。既有 MCP Vault 原始碼、設定與既有 Bao 資源未被替換。
參考原始碼固定於 OpenBao v2.7.2；實機使用專案既有的 OpenBao v2.7.1 測試服務。
沒有升級遠端 Bao，也沒有把 v2.7.1 實測描述成 v2.7.2 執行期驗證。

## 已交付

- 34 個依功能分組的 MCP 工具，以及原生 JSON API 通用入口。
- 原生 Token 逐請求透傳，固定 namespace，預設 `agent`。
- 無服務管理員 Token、本地身分／Policy、Web、業務 metadata 或 audit 資料庫。
- 未加密 `cache.db`：只存 scope、Token hash、經篩選的能力／mount 資訊與時間。
- 固定 freshness、閒置清除、容量上限、單 Token 與進程級查詢限流。
- 僅目前 Token 的快取刷新；實際操作始終由 Bao 即時授權。
- 文字日誌使用 Token／資源 hash，避免記錄秘密內容與上游錯誤正文。
- 已有 engine 不覆寫；Vault 配置部分失敗回傳逐項結果，不回滾刪除。

## Rust 測試結果

共 **38 個不同測試通過**：

| 範圍 | 數量 |
| --- | ---: |
| 設定、快取、發現、憑證單元測試 | 9 |
| HTTP transport 契約 | 10 |
| 能力快取／發現整合 | 5 |
| 工具路由、原生參數、engine 配置 | 12 |
| MCP HTTP 邊界 | 2 |

先完成原定的一輪測試；編譯或測試失敗時，只修正相關範圍，跳過已通過項目。
最後沒有再執行整套測試、全面安全稽核或另一輪 reviewer 驗證。

修正包括 macOS Nix linker／SDK 相容性、SQLite 整數與路徑處理、測試的 chunked
body，以及實機發現的 namespace 內建 engine 類型與不存在 mount 的 400 回應。
對 mount 的 400 不直接當成不存在：必須再由原生 mount 清單證明缺少才建立；
清單拒絕或格式不符時停止。

## 既有 Bao 實測結果

**12 個功能檢查通過**，另有 namespace 與服務啟動兩個前置步驟通過：

| 功能檢查 | 結果 |
| --- | --- |
| 匿名／錯誤 Token 拒絕 | 通過 |
| 原生能力對應工具清單 | 通過 |
| Vault 的 KV／SSH／共享 PKI 配置、再次執行不覆寫 | 通過 |
| 精確 KV 路徑的唯讀發現 | 通過 |
| 並行呼叫者 Token 隔離、越權拒絕 | 通過 |
| KV 歷史版本、CAS、版本刪除／恢復 | 通過 |
| SSH CA、角色與公鑰簽發 | 通過 |
| PKI root／intermediate／leaf 簽發 | 通過 |
| Transit 加解密、簽名驗證與權限拒絕 | 通過 |
| 原生 Policy／Token 建立與讀取 | 通過 |
| Policy 改變即時影響操作、快取刷新、Token 撤銷 | 通過 |
| 已知測試憑證與 KV 值未出現在 cache／log | 通過；於 KV／Token 階段檢查 |

首次測試時，原 `.env` 指定 `agent`，但遠端沒有該 namespace；Token 實際為 root。
確認後，測試從 root 建立獨立 namespace，沒有修改 `.env` 或建立正式的 `agent`。
新程式仍不負責啟動時建立 namespace；正式使用預設值前須由操作員建立 `agent`。

測試 namespace：

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

Policy、測試 Token、KV 版本、SSH／PKI CA 與簽發結果、Transit 金鑰皆保留。
版本刪除後恢復，以及撤銷專用測試 Token，是被驗證的操作；沒有執行遠端 teardown。
本機測試程式已結束其自行啟動的 MCP process。

脫敏結果位於 `../test-output/mcp-live-20261028t143046z-6d14a9af/results.json`。
同目錄的 `state.json`、`private-artifacts.json` 與金鑰檔含私人測試材料，已排除 Git，
不得將其內容貼入文件或日誌。測試腳本支援以既有狀態搭配 `--only` 只跑失敗項目。

## 保留的限制

- 實機沒有配置外部資料庫、雲端、LDAP、RabbitMQ 或 Kubernetes 測試依賴。
  相關 adapter 路由有契約測試，不能宣稱已完成這些外部服務的端到端驗證。
- LDAP 在 Bao 2.7 需要相容外部插件；工具不負責安裝。
- ACME 配置 API 可操作；未實作或驗證 ACME 客戶端 JWS 流程。
- Tool discovery 不是完整的權限列舉。Bao 內部 resultant ACL 會遺漏 `+` 規則，
  結果明示 `complete:false`；通用工具仍交由 Bao 授權。
- 沒有舊版 Mission／Hub／SSH operator workflow、持久化補償或交付確認保證。
