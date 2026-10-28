# OpenBao reference source

The sibling `../openbao/` checkout is the official OpenBao source at release
`v2.7.2`, commit `9e0a0d3c3504a0a00b45e61355afac5e35c78379`.
It is a local reference checkout, not vendored application code or a runtime
requirement. The parent repository ignores it.

Reproduce from the parent repository root:

```sh
git clone --depth 1 --branch v2.7.2 https://github.com/openbao/openbao.git openbao
```

Release: https://github.com/openbao/openbao/releases/tag/v2.7.2

This release was the latest stable release checked on 2026-10-28. The new
standalone Rust application lives in `bao-mcp/`; the legacy application in the
parent directory remains separate.
