# Setup Scripts

Run [`setup-bao-environment.sh`](setup-bao-environment.sh) from the repository root
to prepare Bao for MCP clients. Requires an initialized, unsealed OpenBao server,
Bash, Python 3, and the Bao CLI.

```sh
export BAO_ADDR=https://bao.example.com:8200
./scripts/setup-bao-environment.sh --token-file ./agent-admin.token
```

The script uses `BAO_TOKEN` if set; otherwise it asks for a root setup token with
hidden input. It creates `agent` if absent, installs the bundled
[`admin` policy](bao/policies/admin.hcl), and issues an orphan service token with
`admin` and `default`. The requested TTL and explicit maximum are 24 hours.
Use `--namespace` and `--ttl` to change those values; `--help` lists options.

The token is saved with mode `0600`. The destination directory must exist, and an
existing file or symlink is refused. Existing namespace metadata and an identical
admin policy are preserved; a conflicting policy stops setup for review. Actual
TTL remains subject to Bao limits.

Use the resulting token in the MCP client's bearer-token setting. For individual
administration steps, see [`bao/README.md`](bao/README.md).

---

设置脚本用于准备 Bao namespace 和客户端 Token，需要已初始化、已解封的 Bao、
Bash、Python 3 及 Bao CLI。设置 `BAO_ADDR` 后执行上面的命令；有 `BAO_TOKEN`
时直接使用，否则隐藏询问 Root Token。

默认建立或复用 `agent`，安装 `admin` Policy，签发包含 `admin`、`default` 的
orphan service Token。请求 TTL 和最大总有效期为 24 小时，可通过 `--namespace`
和 `--ttl` 调整，实际 TTL 受 Bao 限制。Token 文件权限为 `0600`，禁止覆盖已有
文件或符号链接；namespace 资料和相同 Policy 会保留，不同 Policy 会停止设置。
将产生的 Token 配置到 MCP 客户端的 bearer-token 设置中。
