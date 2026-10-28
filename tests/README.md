# Live regression / 在线回归测试

The live harness is opt-in and uses only Python's standard library plus optional
`ssh-keygen`. Build the gateway first, then run **one full pass** from the repository
root:

```sh
cargo build --manifest-path bao-mcp/Cargo.toml
python3 bao-mcp/tests/live.py --live
```

It reads the root `.env` **as data**, without shell execution. Existing environment
variables take precedence. Supported inputs are `BAO_ADDR` / `OPENBAO_URL`,
`OPENBAO_TOKEN`, `BAO_NAMESPACE` / `OPENBAO_NAMESPACE` (default `agent`), and optional
`BAO_CACERT` / `OPENBAO_CA_FILE`. The source token must be able to create a child
namespace and issue its limited fixture tokens. Missing namespace permission stops
setup; the harness never falls back to an existing namespace. The gateway runs with
`BAO_ADDR`, the isolated child `BAO_NAMESPACE`, `MCP_LISTEN`, and `CACHE_PATH`; its
environment contains no service token. MCP clients send native tokens as bearer
credentials.

The full pass checks anonymous/invalid credentials, the native tool catalog,
idempotent vault engine allocation, exact-path KV discovery, concurrent caller
isolation, KV versions/CAS/delete/undelete, SSH CA/role/signing, PKI
root/intermediate/leaf issuance, Transit encryption/signing and denial, native
policies/tokens, discovery refresh/revocation, and plaintext credential absence in
gateway cache/log files. Default namespace configuration has offline unit coverage;
the live server deliberately uses the isolated child namespace. Engines requiring
external database/cloud/directory/broker/Kubernetes fixtures are reported as
blocked; their adapter routing has offline coverage.

All remote resources are retained for inspection. This includes the child
namespace, mounts, policies, active tokens, KV versions, SSH key/CA, PKI keys and
certificates, and Transit keys. Native version deletion/undeletion and one token
revocation are the explicit tested operations; no teardown deletes remote data.
The harness only terminates its own local gateway process.

Private output is stored under ignored `bao-mcp/test-output/<run-id>/`, with directory
mode `0700` and file mode `0600`:

- `results.json`: sanitized passed/failed/blocked checks and retained backend paths.
- `state.json`: source/provisioned tokens and fixture references; **contains secrets**.
- `private-artifacts.json`: synthetic values, issued certificates/private keys and
  successful operation outputs; **contains secrets**.
- `cache.db*`, `server.log`, and synthetic SSH key files.

Do not print or commit private state/artifacts. Request/response bodies and exception
details are suppressed in console output, including backend error bodies.

Fix only failed scopes, then reuse the printed state path to rerun a check or family:

```sh
python3 bao-mcp/tests/live.py --live \
  --state bao-mcp/test-output/<run-id>/state.json --only kv
python3 bao-mcp/tests/live.py --live \
  --state bao-mcp/test-output/<run-id>/state.json \
  --only cache.refresh_scope_and_revocation --only cache.no_plaintext_credentials
python3 bao-mcp/tests/live.py --list
```

Retained-state execution requires `--only`; it cannot silently rerun the full suite.
Dependent checks require the original vault/Transit fixture to have passed. A
selected check updates its own result while preserving earlier results. Exit status
is `0` when selected runtime checks pass, `1` for failures, and `2` for blocked
prerequisites; the explicitly reported external-integration gap alone is not a
failure.

---

在线测试必须显式加上 `--live`。先构建程序，再从仓库根目录执行一次完整测试。
脚本仅以数据方式解析原有 `.env`，不会执行其中的命令，也不会修改该文件。输入优先
使用环境变量；地址、命名空间、CA 和源令牌变量见上文。源令牌必须能够创建子命名空间
并签发受限测试令牌；没有权限时停止，不会改用已有命名空间。

测试在唯一的子命名空间中运行，通过实际 MCP HTTP 和每位调用者的 bearer 令牌验证。
服务进程不继承服务令牌。测试覆盖鉴权、工具发现、原生引擎分配与幂等性、调用者隔离、
KV 版本和 CAS、SSH CA、PKI、Transit、原生策略与令牌、缓存刷新和撤销，以及缓存与
日志中没有明文凭证。默认命名空间由离线单元测试覆盖；依赖外部服务的引擎会明确标为
受阻，其路由由离线测试覆盖。

所有远端测试资源均保留供检查，不执行清理。KV 版本删除后恢复，以及撤销一个专用
测试令牌，属于被验证的操作。只有脚本自行启动的本地服务进程会被结束。

输出保存在已忽略的 `bao-mcp/test-output/<run-id>/`，目录权限为 `0700`，文件权限
为 `0600`。`results.json` 仅含脱敏结果和后端路径；`state.json`、
`private-artifacts.json`、SSH 私钥及相关文件可能包含秘密，禁止输出或提交。
控制台不输出请求、响应、后端错误正文或异常详情。

需要修复时，使用上面的 `--state ... --only <检查名或类别>` 命令，仅重跑失败范围。
已有状态必须指定 `--only`，不会重复执行整套测试。新结果仅替换所选检查，保留之前的
结果。退出码 `0` 表示所选运行时检查通过，`1` 表示失败，`2` 表示前置条件受阻；
单独缺少外部集成测试环境不会导致失败。
