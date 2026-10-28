# Bao MCP 工具

[English](tools.md)

工具按原生 Bao API 功能分组。Engine 工具通常接收 `mount`、`operation`、mount 内
的 `path`，以及可选的 `data`、`query`、`wrap_ttl`。`data` 保留原生 JSON 请求体。
可以用 `api_help` 查询具体端点的字段与操作；路径参考见[各 engine API 说明](bao/engine-storage-api-paths.zh-CN.md)。

## 工具分组

| 工具 | 能力 |
| --- | --- |
| `vault_create` | 分配 KV、SSH 与共用 PKI mount |
| `kv_read`、`kv_write`、`kv_manage` | KV v1/v2、CAS、版本、metadata、删除/恢复/销毁 |
| `ssh_ca`、`ssh_roles`、`ssh_issue` | SSH CA/issuer、角色、签名、签发与 OTP |
| `pki_ca`、`pki_roles`、`pki_certificates`、`pki_config` | PKI CA/密钥/issuer、角色、证书、撤销与配置 |
| `transit_keys`、`transit_crypto` | 密钥管理、加解密、签名验证、HMAC/hash/random |
| `database_connections`、`database_roles`、`database_credentials` | 数据库连接、角色、动态/静态凭证及轮换 |
| `kubernetes`、`ldap`、`rabbitmq` | 对应 engine API；Bao 2.7 的 LDAP 需要兼容外部插件 |
| `totp_keys`、`totp_code`、`cubbyhole` | TOTP 与 Token 私有存储 |
| `identity`、`auth`、`token`、`policy` | 原生身份、认证、Token、Policy 管理 |
| `leases`、`mounts`、`namespaces`、`audit`、`system` | 配置 namespace 内的原生管理 |
| `bao_request`、`api_help` | 通用 JSON API 与原生 API 帮助 |
| `cache_refresh` | 刷新当前调用者的工具发现 |

总计 34 个工具。使用对应工具前，先安装相应 engine/插件并配置外部服务。
JSON 接口处理原生 JSON 路径；ACME JWS 流程由 ACME 客户端执行，原始二进制上传
使用对应的原生客户端。

## Vault 分配

```json
{"name":"vault_create","arguments":{"vault_id":"demo"}}
```

分配结果：

| Mount | 类型 | 范围 |
| --- | --- | --- |
| `demo` | KV v2 | 此 Vault |
| `ssh-ca-demo` | SSH | 此 Vault |
| `_sys_pki_root` | PKI | namespace 共用 |
| `_sys_pki_int` | PKI | namespace 共用 |

已有且相符的 mount 会保留设置，类型/版本冲突则停止。部分成功时返回
`complete:false`、逐项结果与 MCP `isError:true`；已完成的 mount 保留，后续可以补建缺少的 mount。

新增 mount 后，调用 `cache_refresh` 并重新列出工具；遇到 `rate_limited` 时等待五秒后重试。

## KV

`kv_write` 使用 KV v2 的原生请求体，可以附上 CAS：

```json
{"name":"kv_write","arguments":{"mount":"demo","path":"service","data":{"data":{"password":"example"},"options":{"cas":0}}}}
```

读取历史版本：

```json
{"name":"kv_read","arguments":{"mount":"demo","path":"service","version":1}}
```

`kv_manage` 提供原生 metadata 与版本操作路径，按调用者实际需要授予删除、销毁权限。

## SSH CA

新建 SSH mount 后，按顺序调用。

为该 mount 生成一次 CA 密钥：

```json
{"name":"ssh_ca","arguments":{"mount":"ssh-ca-demo","operation":"write","path":"config/ca","data":{"generate_signing_key":true}}}
```

建立签发角色：

```json
{"name":"ssh_roles","arguments":{"mount":"ssh-ca-demo","operation":"write","path":"roles/deploy","data":{"key_type":"ca","allow_user_certificates":true,"allowed_users":"deploy","default_user":"deploy","ttl":"5m","max_ttl":"1h"}}}
```

读取 CA 公钥，并在目标 SSH 服务器配置对它的信任：

```json
{"name":"ssh_ca","arguments":{"mount":"ssh-ca-demo","operation":"read","path":"public_key"}}
```

提交真实 SSH 公钥来签发证书：

```json
{"name":"ssh_issue","arguments":{"mount":"ssh-ca-demo","operation":"write","path":"sign/deploy","data":{"public_key":"<your OpenSSH public key>","valid_principals":"deploy","ttl":"5m"}}}
```

Bao 执行角色的 principal、TTL、选项限制，目标主机按证书内容执行 SSH 验证。
服务器信任设置见 [Bao SSH 文档](https://openbao.org/docs/secrets/ssh/)。

## PKI

两个 PKI mount 为 namespace 共用。每个 namespace 设置一次 CA，再按签发范围分角色。
下例短有效期用于本地试用，部署时应自行选择 CA 生命周期。

1. `pki_ca`：mount `_sys_pki_root`，operation `write`，path `root/generate/internal`，data `{"common_name":"Demo Root CA","ttl":"72h","key_type":"ec","key_bits":256}`。
2. `pki_ca`：mount `_sys_pki_int`，operation `write`，path `intermediate/generate/internal`，data `{"common_name":"Demo Intermediate CA","key_type":"ec","key_bits":256}`。保留返回的 CSR。
3. `pki_ca`：mount `_sys_pki_root`，operation `write`，path `root/sign-intermediate`，data `{"csr":"<returned CSR>","format":"pem_bundle","ttl":"48h"}`。
4. `pki_ca`：mount `_sys_pki_int`，operation `write`，path `intermediate/set-signed`，data `{"certificate":"<returned certificate bundle>"}`。

为 demo 工作负载建立角色：

```json
{"name":"pki_roles","arguments":{"mount":"_sys_pki_int","operation":"write","path":"roles/demo","data":{"allowed_domains":["demo.internal"],"allow_subdomains":true,"max_ttl":"1h","key_type":"ec","key_bits":256}}}
```

签发 leaf 证书：

```json
{"name":"pki_certificates","arguments":{"mount":"_sys_pki_int","operation":"write","path":"issue/demo","data":{"common_name":"app.demo.internal","ttl":"15m"}}}
```

在客户端/服务器信任库中配置 CA chain。签发结果可能包含私钥，应通过应用的秘密
管理机制交付。已有 CSR 时，用 `sign/<role>`，而不是 `issue/<role>`。

## Transit

按需开启 mount：

```json
{"name":"mounts","arguments":{"operation":"write","path":"mounts/transit","data":{"type":"transit"}}}
```

建立加密密钥，再加密 Base64 编码的 `hello`：

```json
{"name":"transit_keys","arguments":{"mount":"transit","operation":"write","path":"keys/app","data":{"type":"aes256-gcm96"}}}
```

```json
{"name":"transit_crypto","arguments":{"mount":"transit","operation":"write","path":"encrypt/app","data":{"plaintext":"aGVsbG8="}}}
```

把返回的 `ciphertext` 交给 `decrypt/app` 即可解密。签名时，通过 `transit_keys`
建立 `ed25519` 密钥，再用 `sign/<key>` 传入 Base64 `input`，最后把 input 和返回的
signature 交给 `verify/<key>`。
