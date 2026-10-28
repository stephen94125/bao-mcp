# OpenBao CLI Basics

[简体中文](cli-basics.zh-CN.md)

These notes cover remote OpenBao administration from macOS/Nix, including servers running in Kubernetes. They assume **manual Shamir unsealing**; auto-unseal is outside this guide.

## 1. Installation and executable

The CLI and server use **the same `bao` executable**. Installing the package provides both capabilities but does not automatically start a local server.

```bash
# Install with Nix
nix profile install nixpkgs#openbao

# Check the version
bao version

# Use temporarily without installing
nix shell nixpkgs#openbao
```

`bao server` starts a server. Commands such as `bao status` and `bao operator unseal` call the remote API. The CLI needs network access to Bao, not SSH access or a shell inside its Pod.

References: [CLI documentation](https://openbao.org/docs/commands/), [server command](https://openbao.org/docs/commands/server/).

## 2. Connection and identity

```bash
export BAO_ADDR="https://bao.example.com"
export BAO_TOKEN="your-admin-token"

# Optional internal CA
export BAO_CACERT="/path/to/ca.crt"
```

| Variable | Purpose |
| --- | --- |
| `BAO_ADDR` | API URL selecting the server or cluster |
| `BAO_TOKEN` | Authentication token; policies determine its permissions |
| `BAO_CACERT` | Optional trusted CA certificate path |
| `BAO_TOKEN_PATH` | Optional local token-storage path used by `bao login` |

With `BAO_ADDR` and `BAO_TOKEN` set, **`bao login` is unnecessary**.

Alternatively, set only `BAO_ADDR`, then run:

```bash
bao login
# Enter an existing token; the helper stores it after validation
```

The default token helper uses `~/.vault-token`. Subsequent CLI commands read it automatically. Other methods include `bao login -method=userpass username=admin`, if enabled on the server. `BAO_TOKEN` takes precedence over the stored token.

**`bao login` replaces manually supplying `BAO_TOKEN`, not configuring `BAO_ADDR`.** Keep management tokens out of Git and public scripts. The initial root token has special privileges and differs from an ordinary administrative token carrying restricted policies.

References: [CLI environment variables](https://openbao.org/docs/commands/), [token helpers](https://openbao.org/docs/commands/token-helper/).

## 3. Status and first initialization

```bash
bao status

# Run once against a new, uninitialized storage backend
bao operator init -key-shares=1 -key-threshold=1
```

`bao status` reports `Initialized`, `Sealed`, `Key Shares`, and `Key Threshold`. `Sealed: true` means normal secret operations are unavailable.

`operator init` produces unseal key shares and an **initial root token**. Store them securely immediately. Do not initialize an existing cluster again; HA nodes sharing a backend initialize that backend once.

### Share thresholds: 1/1, 2/3, and 3/5

`-key-shares` selects the total number of shares; `-key-threshold` selects how many distinct shares are required to unseal.

| Scheme | Initialization command | Unseal requirement |
| --- | --- | --- |
| **1 of 1 (1/1)** | `bao operator init -key-shares=1 -key-threshold=1` | Submit the one valid share |
| **2 of 3 (2/3)** | `bao operator init -key-shares=3 -key-threshold=2` | Submit any two distinct shares |
| **3 of 5 (3/5)** | `bao operator init -key-shares=5 -key-threshold=3` | Submit any three distinct shares |

Shamir shares **are not independently authorized login keys like SSH `authorized_keys` entries**. One share in a 2/3 scheme cannot unseal by itself; revoking share B while retaining the original A/C share set is not supported.

If every backup must unseal independently, use **1/1 and store copies of the same key in separate secure locations**. These are copies, not independently revocable keys. Exposure of one copy requires rotating the set and updating all backups.

References: [operator init](https://openbao.org/docs/commands/operator/init/), [seal/unseal](https://openbao.org/docs/concepts/seal/).

## 4. Manual unsealing

After restarting in Shamir mode, Bao normally returns to the sealed state. Unseal remotely from the CLI **without `BAO_TOKEN` or `bao login`**:

```bash
bao status
bao operator unseal  # Enter one share at the hidden prompt
bao status
```

Run once for 1/1, twice with **different shares** for 2/3, or three times for 3/5. Avoid putting shares in command arguments or shell history.

A Kubernetes ingress may be unavailable while the Pod is not ready. With Kubernetes access, use `kubectl port-forward` to the specific Pod and point `BAO_ADDR` to the forwarded local port. Unseal each node separately in a multi-node cluster.

Reference: [operator unseal](https://openbao.org/docs/commands/operator/unseal/).

## 5. Initialization versus unseal-key rotation

**`bao operator init` and `bao operator rotate-keys -init` are different operations.** The first initializes Bao; the second starts an unseal-key rotation. Rotation initialization neither reinitializes storage nor deletes secrets, and does not immediately produce new shares.

| Command | Meaning | When to use |
| --- | --- | --- |
| `bao operator init` | Initialize storage, initial keys, and the initial root token | **New cluster; succeeds only once** |
| `bao operator rotate-keys -init` | Start rotating shares with the requested count and threshold | **Initialized and unsealed** cluster |
| `bao operator rotate-keys` | Submit an old share; completing the old threshold produces new shares | Rotation already started |
| `bao operator rotate-keys -status` | Inspect progress | During rotation |
| `bao operator rotate-keys -cancel` | Cancel an **unfinished** rotation and discard pending submissions/settings | Before rotation completes |

### Replace an existing 1/1 key with a new 1/1 key

```bash
# 1. Start rotation, not storage initialization
bao operator rotate-keys -init -key-shares=1 -key-threshold=1

# 2. Inspect status and nonce
bao operator rotate-keys -status

# 3. Submit the old share; supply the rotation nonce when prompted
bao operator rotate-keys

# 4. If needed, repeat with different old shares until the old threshold is met
```

A previous 1/1 scheme needs one old share; 2/3 needs two, and 3/5 needs three. Without an additional verification stage, meeting the old threshold completes rotation and outputs new shares. **All old shares become invalid.** No downtime or storage reinitialization is required; existing secrets and admin tokens are unaffected. Back up the new shares immediately.

To stop an unfinished rotation:

```bash
bao operator rotate-keys -cancel
```

`-cancel` **only cancels an unfinished rotation**. It cannot restore shares invalidated by a completed rotation.

Do not start another `-init` while a rotation is in progress. Start again after completion or cancellation. The default count/threshold is **5/3**; specify both flags to retain 1/1. Official examples use the single-dash `-init`; check `bao operator rotate-keys -help` for your CLI version.

### `bao operator rotate` rotates storage encryption keys

Do not interchange these commands:

| Command | Key rotated | Invalidates old unseal shares? |
| --- | --- | --- |
| `bao operator rotate-keys` (completed) | Unseal shares / root key protecting the keyring | **Yes**, the old share set is invalid |
| `bao operator rotate` | Data-encryption key in the storage keyring | **No**, unseal shares stay unchanged |

```bash
# Rotate the data-encryption key only
bao operator rotate
```

`bao operator rotate` creates a new keyring encryption key for **subsequent writes**. Old keys remain in the keyring to read older ciphertext; the whole database need not be re-encrypted. This neither unseals Bao nor replaces an administrator token.

References: [operator init](https://openbao.org/docs/commands/operator/init/), [rotate-keys](https://openbao.org/docs/commands/operator/rotate-keys/), [rotate](https://openbao.org/docs/commands/operator/rotate/), [key rotation internals](https://openbao.org/docs/internals/rotation/).

## 6. Common management and KV commands

These commands normally require a valid token with the appropriate policies:

```bash
# Inspect the current token and identity
bao token lookup

# List enabled engines
bao secrets list

# List policies
bao policy list

# Read a KV secret at app/config in the secret/ mount
bao kv get -mount=secret app/config

# Print one field; this does not provide field-level access control
bao kv get -mount=secret -field=password app/config

# Print JSON
bao kv get -mount=secret -format=json app/config
```

`-mount=secret` selects a mount; it need not be named `secret`. `app/config` is the key within it. Reads fail if the engine is missing or the token lacks permission. For KV v2, `-mount` handles the `data/` API segment.

References: [token lookup](https://openbao.org/docs/commands/token/lookup/), [KV get](https://openbao.org/docs/commands/kv/get/).

## 7. Quick reference

```bash
# Select the remote server
export BAO_ADDR="https://bao.example.com"

# Check initialization and seal status
bao status

# Run once for a new cluster; choose a share scheme such as 1/1, 2/3, or 3/5
bao operator init -key-shares=1 -key-threshold=1

# Unseal after restart, submitting enough distinct shares
bao operator unseal

# For management, either set a token or use bao login
export BAO_TOKEN="your-admin-token"
# bao login

bao token lookup
bao secrets list
bao policy list
bao kv get -mount=secret app/config
```

**`BAO_ADDR` selects the server; `BAO_TOKEN` or `bao login` supplies the operating identity. An unseal key only unseals Bao and is not an admin token.**
