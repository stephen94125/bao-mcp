# OpenBao Reference Source

The optional `openbao/` checkout in this repository is pinned to OpenBao
`v2.7.2`, commit `9e0a0d3c3504a0a00b45e61355afac5e35c78379`. It is a local
source reference for API behavior. Building and running Bao MCP uses the Rust
project at this repository's root.

To fetch the same reference source from this repository's root:

```sh
git clone --depth 1 --branch v2.7.2 https://github.com/openbao/openbao.git openbao
```

The checkout is excluded by `.gitignore`. See the
[release notes](https://github.com/openbao/openbao/releases/tag/v2.7.2).
This version was selected on 2026-10-28; the historical
[validation record](docs/validation-2026-10-28.md) identifies the server version
used for runtime checks.
