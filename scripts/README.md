# Setup Scripts

Run [`setup-bao-environment.sh`](setup-bao-environment.sh) from the repository root
to prepare Bao for MCP clients. Requires an initialized, unsealed OpenBao server,
Bash, Python 3, and the Bao CLI.

```sh
export BAO_ADDR=https://bao.example.com:8200
./scripts/setup-bao-environment.sh
```

The script uses `BAO_TOKEN` if set; otherwise it asks for an administrator setup
token with hidden input. It creates `agent` if absent, installs the bundled
[`admin` policy](bao/policies/admin.hcl), and issues an orphan service token with
`admin` and `default`. The requested TTL and explicit maximum default to three years (`1095d`).
Use `--namespace` and `--ttl` to change those values; `--help` lists options.

The token is printed by default. Add `--token-file ./agent-admin.token` to save it
with mode `0600` instead; the destination directory must exist, and an existing
file or symlink is refused. Existing namespace metadata and an identical
admin policy are preserved; a conflicting policy stops setup for review. Actual
The actual TTL returned by Bao is displayed in red. The script prints an English
warning to keep this three-year credential secure and to use the server-returned
TTL; increase the server configuration `max_lease_ttl` if a longer limit is needed.

Use the resulting token in the MCP client's bearer-token setting. For individual
administration steps, see [`bao/README.md`](bao/README.md).

## Issue another token

[`issue-bao-token.sh`](issue-bao-token.sh) issues a token without repeating namespace
setup. It uses `BAO_TOKEN` or the Bao CLI login. The namespace defaults to
`BAO_NAMESPACE` or `agent`, and the policy defaults to `admin`.

```sh
./scripts/issue-bao-token.sh
./scripts/issue-bao-token.sh --policy vault-demo-readonly --ttl 8h
./scripts/issue-bao-token.sh --token-file ./agent-admin.token
```

Use `--namespace` to select another namespace, or repeat `--policy` to attach
multiple policies. The default TTL and explicit maximum are `1095d`. Tokens print
to the terminal unless `--token-file` is provided; files use `0600` and existing
files are refused. The English warning and red TTL use the actual server result.
The namespace and policies must already exist, and Bao controls issuance rights.

## Setup permissions

Use a root-namespace administrator token with rights on the paths below. For the
default `agent` target, these are paths in a policy stored in the root namespace:

| API path | Capabilities |
| --- | --- |
| `sys/namespaces/agent` | `read`; `update` when creating the namespace |
| `agent/sys/policies/acl/admin` | `read`; `update` when installing the policy |
| `agent/auth/token/create` | `update`, `sudo` for cross-namespace orphan issuance |

Replace `agent` when using `--namespace`. Bao checks each real operation; a
suitably authorized `super-admin` policy is sufficient. There is no mandatory
setup-token self-lookup. Existing namespace and matching policy reuse need only
their read permissions. The token must also meet Bao's native issuance conditions,
such as being a service token without a limited use count. A child-namespace
administrator cannot use its local rights to manage a top-level namespace.

Failures identify the operation and namespace, with the HTTP status when
available or a TLS, connectivity, timeout, or CLI error category. Raw CLI output,
server error bodies, and credentials are kept out of diagnostics. Completed
namespace/policy changes are retained on a later failure. Issued-token checks
still confirm its namespace, service type, orphan status, policies, and lifetime.

---

设置脚本用于准备 Bao namespace 和客户端 Token，需要已初始化、已解封的 Bao、
Bash、Python 3 及 Bao CLI。设置 `BAO_ADDR` 后执行上面的命令；有 `BAO_TOKEN`
时直接使用，否则隐藏询问设置用管理员 Token。

默认建立或复用 `agent`，安装 `admin` Policy，签发包含 `admin`、`default` 的
orphan service Token。请求 TTL 和最大总有效期默认为三年（`1095d`），可通过 `--namespace`
和 `--ttl` 调整，实际 TTL 由 Bao 回应并以红字显示。默认输出 Token 到终端；
指定 `--token-file` 才存档，文件权限为 `0600`，禁止覆盖已有文件或符号链接；namespace 资料和相同 Policy 会保留，不同 Policy 会停止设置。
脚本会以英文警告妥善保管三年期凭证，实际以服务器回传的 TTL 为准；若需加长
主机上限，应修改其 `max_lease_ttl`。将 Token 配置到 MCP 客户端的 bearer-token 设置中。

设置所需的 root namespace ACL 路径见上表；`--namespace` 改名时应替换 `agent`。
权限足够的 `super-admin` Token 可以使用，脚本直接执行实际 API 操作，由 Bao 授权。
复用 namespace 和相同 Policy 时只需相应读取权限；跨 namespace 签发 orphan Token
需要 `sudo`，Token 本身仍受 Bao 的签发条件限制。子 namespace 管理员只能管理自己的
权限范围。错误会显示操作、namespace 和可取得的 HTTP 状态或 TLS／连接／超时等类别，
不显示原始响应或凭证；后续步骤失败时保留已完成的 namespace／Policy 修改。

另行签发 Token 可使用 `./scripts/issue-bao-token.sh`，不会重复建立 namespace。
它使用 `BAO_TOKEN` 或 Bao CLI 登录，默认选择 `BAO_NAMESPACE` 或 `agent`，Policy
为 `admin`；`--policy` 可重复指定。默认三年（`1095d`），以英文提示保管凭证及
主机 TTL 上限，并以红字显示实际 TTL。默认输出到终端，使用 `--token-file` 才存档；
namespace／Policy 需事先存在。
