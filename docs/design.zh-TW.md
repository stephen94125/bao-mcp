# Bao MCP：資料來源、權限與快取的邊界

Bao MCP 是 Rust／RMCP／Axum 寫成的原生 Bao 工具介面。上游參考版本為
OpenBao v2.7.2；完整操作方式與環境變數見 [README](../README.md)。

## 資料只由 Bao 管理

Secret 的內容、版本與時間、SSH／PKI 角色與憑證、Transit 金鑰，以及 Policy、Token
都在 Bao。程式不建立第二份 metadata，不依 `created_by` 判斷擁有者，不產生
`_vault_meta`，不儲存業務操作 journal。

因此人工用 Bao CLI／GUI 改資料，MCP 下次讀取就使用 Bao 回傳結果。沒有本地同步
排程，也沒有本地成功紀錄能替代實際狀態。如果一次多步驟建立只完成部分 engine，
回傳逐項結果並保留已完成部分；下次再補缺少的項目。不替使用者回滾刪除資源。

## 每一個請求使用自己的 Token

```text
Agent 的原生 Bao Token
        ↓ Authorization: Bearer
Bao MCP：解析工具、檢查參數、轉換原生 API 路徑
        ↓ X-Vault-Token + 固定 X-Vault-Namespace
OpenBao：身分驗證、原生 Policy／role 限制、執行與 audit
```

程序沒有共用管理員 Token。namespace 預設 `agent`，必須事先存在；啟動時不連線
建立任何 Bao 資源。呼叫者不能透過工具參數改 Header 或切換配置的 namespace。
若有管理子 namespace 的原生權限，仍能在這個 namespace 之下操作。

新增 Vault 時沿用原命名：KV 為 Vault 名稱、SSH 為 `ssh-ca-<名稱>`、PKI 共用
`_sys_pki_root`／`_sys_pki_int`。這一步只配置 engine，不擅自產生 CA 或覆寫既有角色。
需要的 CA、SSH 使用者／TTL／選項、PKI 角色限制都用原生工具設定在 Bao。

沒有舊版的本地 Token、Mission JWT、SSH operator profile、Hub 或 Web UI。
JWT 工作負載可以使用 Bao 自己的 JWT auth 交換 Token；工作負載如何取得 Token
不是這個 MCP 的第二套身分系統。

## 能力快取不參與實際授權

`cache.db` 是未加密、可刪除的 SQLite，主鍵是上游／namespace scope 與 Token hash。
保存能力查詢結果、工具名稱、mount 類型、建立／到期／最後使用時間；不保存 Token、
Secret 或私鑰。檔案權限仍為 0600。

首次查詢會驗證 Token 並取得候選權限；之後在固定 freshness 內命中快取。
`CACHE_IDLE_TTL_SECONDS` 限制閒置多久清除，`CACHE_MAX_AGE_SECONDS` 限制即使一直
使用也必須重新查詢的時間。清除在啟動或存取時執行。`cache_refresh` 只更新目前
Token 的資料，而且先重新驗證 Token；未知或錯誤的 Token 不能建立快取。

快取可能短暫反映舊 Policy，這只影響工具清單。實際讀寫沒有使用快取放行，也不會
因清單缺少工具就阻擋有效的原生操作。每次都交由 Bao 即時裁決。

為防止大量隨機 Token 或反覆刷新耗盡資源，冷查詢與刷新有單 Token 間隔、進程級
速率、併發、記錄數與資料大小上限。首次發現後五秒內刷新可能被限流。

## 工具清單不能保證精確列舉所有權限

Bao capabilities API 只能回答「這些具體路徑能做什麼」，不能列舉所有有權限路徑。
程式使用 Bao resultant ACL 與 mount 資訊建立候選，再確認具體路徑能力。
OpenBao v2.7.2 的該內部 ACL 介面沒有回傳 `+` 路徑規則，因此不能宣稱完整發現。

刷新結果會明示 `complete:false` 與限制。已驗證 Token 可使用 `bao_request`／
`api_help` 操作沒有被發現的原生 JSON API。這些工具仍然沒有超越 Bao 的權限。
一般 KV 欄位也不會被 MCP 自動過濾；不同權限資料宜分不同原生 Secret 路徑。

## 故障與日誌

禁止自動重試可能已執行的寫操作。網路或後端錯誤使結果無法確認時，回傳
`outcome_unknown`，由呼叫者查 Bao 決定下一步。這個版本沒有舊版的持久化補償、
任務防重播或交付狀態保證；原生 wrapping token 的一次性語義仍由 Bao 負責。

文字日誌記錄 Token hash、工具、資源路徑 hash、結果與耗時。秘密值、原始 Token、
完整參數和上游錯誤正文不寫入 log。詳細的實際路徑、身分和原生操作歷史，使用
已啟用的 Bao audit；應用文字日誌只補充 Agent 呼叫工具的過程。

## 功能覆蓋的定義

34 個工具按 engine 與用途分組，原生 JSON body 保持原樣。Transit 包含加解密與
簽名驗證。其餘 engine 或外掛可透過 `bao_request`，不需要照每一個 REST endpoint
建立一個 MCP tool。這表示 API 可通達，不代表外部資料庫、雲端、LDAP、RabbitMQ
或 Kubernetes 已完成連線設定與端到端驗證。

LDAP 在 Bao 2.7 需要相容外部外掛。ACME 配置可操作，但 ACME 的 JWS 協定流程仍
交由 ACME 客戶端處理。原始二進位上傳與任意自訂 Header 不在這個 JSON 工具介面內。
