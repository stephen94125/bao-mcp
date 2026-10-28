# OpenBao Auth Method 与管理员登入：Userpass、AppRole、Kubernetes Auth 与 Token 生命周期

[English](auth-methods.md)

OpenBao 的日常授权可拆成三个互相配合的概念：

- **Auth Method**：如何证明使用者或程式的身分。
- **Policy**：通过验证后，允许对哪些 API 路径执行哪些操作。
- **Token**：OpenBao 在验证成功后签发的存取凭证，后续请求会依附加的 Policy 检查权限。

Auth Method 并不是另一种加密机制，也不是 Secret Engine；它主要负责**认证身分、签发 Token**。Secret Engine 则负责保存或产生 Secret。

## 一、Auth Method、Policy、Token 如何配合？

```text
使用者 / 服务 / K8s Pod
         │
         ▼
Auth Method
  ├── Userpass：帐号 + 密码
  ├── AppRole：RoleID + SecretID
  └── Kubernetes：ServiceAccount JWT
         │
         ▼
验证身分成功
         │
         ▼
OpenBao 签发 Token（附加 Policy）
         │
         ▼
后续 API 请求带上 Token
         │
         ▼
Policy 判断操作权限
         │
         ▼
Secret Engine：读取或修改 Secret
```

也就是 **Auth Method 决定如何登入，Policy 决定有权做什么，Token 是登入后实际使用的凭证**。

## 二、三种常见 Auth Method

| Auth Method | 怎么登入 | 适合谁 |
| --- | --- | --- |
| Userpass | Username + Password | 管理员本人，从 Mac 管理 OpenBao |
| AppRole | RoleID + SecretID | Agent、VM、systemd 服务 |
| Kubernetes | Kubernetes ServiceAccount JWT | K8s Pod 里的程式 |

### 1. Userpass：一般帐号密码登入

Userpass 适合由人员操作的管理入口。先由具备足够权限的管理 Token 启用 Auth Method，建立管理帐号并绑定 Policy；之后管理员就能用帐密反复登入取得新的 Token。

### 2. AppRole：程式自行登入

AppRole 通常使用：

```text
RoleID   = 识别程式所使用的 Role
SecretID = 证明程式有权使用该 Role 的秘密
```

登入范例（前提是已建立 AppRole 与发放有效的 RoleID、SecretID）：

```bash
bao write auth/approle/login \
  role_id="..." \
  secret_id="..."
```

OpenBao 验证成功后，会回传附有指定 Policy 的 Token，程式再用 Token 读写 Secret。Token 到期后，若 RoleID、SecretID 仍有效，且符合使用次数、TTL 等限制，可重新登入。

这里的 **AppRole Role 是登入条件与 Policy 绑定设定，不是另一套 Policy**。

参考：[OpenBao AppRole Auth Method](https://openbao.org/docs/auth/approle/)。

### 3. Kubernetes Auth：让 Pod 使用自身身分登入

对部署在 K8s 的工作负载，OpenBao 可以使用 Pod 的 ServiceAccount JWT 作为登入凭证。

```text
K8s Pod（erp-backend）
         │
         ▼
ServiceAccount JWT
         │
         ▼
OpenBao Kubernetes Auth
         │
         ▼
透过 Kubernetes TokenReview 验证
         │
         ▼
依已设定 Role 绑定 ServiceAccount 与 Policy
         │
         ▼
签发 ERP 专用 OpenBao Token
```

这样不需要在每个 Pod 预先放置长期固定的 OpenBao Token，也不必另外配发 AppRole SecretID。

参考：[OpenBao Kubernetes Auth Method](https://openbao.org/docs/auth/kubernetes/)。

## 三、Userpass 管理员的完整建立流程

只签发一把短效 Admin Token，并不足以维持长期管理入口。管理员 Token 到期后仍需要一种能重新登入、取得新 Token 的方式。

**Userpass 的目的就是建立可重复登入的管理身分**，不需要每次都使用 Root Token 手动签发。

### 前置条件

- OpenBao 已初始化且完成 Unseal。
- CLI 已设定 `BAO_ADDR`。
- 目前持有能管理 Auth Method 和 Userpass 使用者的 Token，例如初始设定阶段的 Root Token。
- 已建立 `super-admin` Policy；本节假设该 Policy 已存在。

### 第一步：启用 Userpass

```bash
bao auth enable userpass
```

预设挂载在 `auth/userpass/`。

### 第二步：建立管理员帐号

```bash
# 以下只示范测试用密码，正式密码不要直接写进 Shell History
bao write auth/userpass/users/mac-admin \
  password='测试用强密码' \
  policies=super-admin
```

这会建立 `mac-admin` 帐号，并将 `super-admin` Policy 绑定到该登入身分。

### 第三步：以 Userpass 登入

```bash
# 指定要连线的 OpenBao
export BAO_ADDR="https://bao.example.com"

# 若原先设过 BAO_TOKEN，先清除，以免后续 CLI 仍优先使用旧 Token
unset BAO_TOKEN

bao login -method=userpass username=mac-admin
```

依提示输入密码，OpenBao 验证成功后签发新 Token。CLI 的预设 Token Helper 会储存登入取得的 Token，后续执行管理指令即可使用。

### 第四步：验证管理身分

```bash
bao token lookup
bao policy list
bao secrets list
bao auth list
```

确认目前 Token 附有预期的 Policy，且管理操作成功。

### 第五步：后续登入

Userpass 取得的 Token 仍然会受到 TTL、最大 TTL 等限制；到期不代表管理帐号消失。

```bash
bao login -method=userpass username=mac-admin
```

只要 Auth Method、管理帐号仍有效且身分验证成功，就能取得新的 Token。这解决了「唯一的 Admin Token 过期，之后怎么管理」的持续登入问题。

正式环境撤销初始 Root Token 前，应先验证此管理登入机制可独立使用，并确认既有管理操作都能完成。

参考：[OpenBao Userpass Auth Method](https://openbao.org/docs/auth/userpass/)。

## 四、为什么不只用 `bao token create`？

直接建立 Token 当然可以：

```bash
bao token create \
  -policy=erp-readonly \
  -ttl=24h
```

它是管理者直接签发的 Token；跟透过 Auth Method 登入取得 Token 不同。

| 方法 | Token 过期后怎么办？ |
| --- | --- |
| 手动 `bao token create` | 需要有权限的管理员重新签发 |
| Userpass | 用帐号密码重新登入 |
| AppRole | 用有效的 RoleID、SecretID 重新登入 |
| Kubernetes Auth | 用有效的 ServiceAccount JWT 重新登入 |

**Auth Method 的核心价值就是让人或程式能重复证明身分，持续取得新的短效 Token**，而不必长期使用固定 Token，或每次请 Root Token 签发。

## 五、如何在现有架构中分配？

| 对象 | 建议 Auth Method |
| --- | --- |
| Mac 上的人类管理者 | Userpass |
| VM / systemd 上的 Agent 或服务 | AppRole |
| K8s 里的工作负载 | Kubernetes Auth |
| 初始管理、紧急操作 | Root Token，仅必要时使用 |

Kubernetes 工作负载可直接使用 Kubernetes Auth。对 Bao MCP 而言，是呼叫者依自身环境取得原生 Bao Token 再传给 gateway；gateway 不代替呼叫者执行 AppRole 或 Kubernetes 登入。

## 六、Auth Method 的查看与启用指令

先查看已启用的 Auth Methods：

```bash
bao auth list
```

全新 OpenBao 通常预设有：

```text
Path       Type
----       ----
token/     token
```

Token 本身也是预设启用的 Auth Method。其他类型需要自行启用：

```bash
# 人类帐密登入
bao auth enable userpass

# VM / systemd 等服务身分
bao auth enable approle

# K8s Pod 身分
bao auth enable kubernetes
```

**不需要一开始全部启用；实际有用途时再启用即可。** Auth Methods 位于 `auth/` 之下，可以用 `bao auth list` 管理；Secret Engines 则透过 `bao secrets list` 管理，两者不会混在一起。

参考：[OpenBao Auth CLI](https://openbao.org/docs/commands/auth/)。

## 七、总结

- **Secret Engine**：管理秘密。
- **Policy**：定义 API 存取权限。
- **Auth Method**：验证登入身分。
- **Token**：验证后用于实际操作 OpenBao 的凭证。
- **Unseal Key**：只处理 OpenBao 服务本身的解封，不等于人员或程式的登入 Token。

手动建立 Token 适合测试与直接签发；若要让管理员或服务持续运作，应按身分选择 Userpass、AppRole 或 Kubernetes Auth，让短效 Token 到期后仍能重新登入取得。
