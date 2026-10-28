# OpenBao Integrations: Agent, systemd Credentials, Kubernetes, SSH, PKI, and Availability

[简体中文](extended-usage.zh-CN.md)

> Research recorded on 2026-10-28. Examples cover VM/systemd credential delivery, OpenBao Agent, Kubernetes integrations, and other engines. Check configuration fields against the deployed Bao, Helm chart, and Kubernetes versions.

## 1. Beyond storing passwords

Bao can provide infrastructure secret management and identity-based authorization. Adopt it incrementally: migrate selected services before moving every Kubernetes Secret, Linux password, SSH key, or TLS certificate.

| Layer | Goal | Mechanism |
| --- | --- | --- |
| 1. Central management | Reduce scattered plaintext credentials | KV v2 and policies |
| 2. Avoid persistent plaintext | Keep service secrets out of VM disks, Git, and images | Agent with tmpfs/credentials |
| 3. Reduce long-lived secrets | Replace fixed passwords with expiring, revocable credentials | Database, SSH, PKI, short-lived tokens |

These layers address management, persistence, and exposure duration respectively. They can progress independently rather than requiring one large migration.

## 2. OpenBao Agent and systemd

**OpenBao Agent is Bao's local daemon, not a systemd component.** Run it using:

```bash
bao agent -config=/etc/openbao/agent.hcl
```

It can authenticate, retrieve secrets, and render configuration for applications on Linux, VMs, or containers that do not implement the Bao API.

### Agent capabilities

| Feature | Purpose |
| ------------------ | ------------------------- |
| Auto-auth | Authenticate and renew tokens |
| Templates | Render configuration, password, or certificate files |
| API proxy | Provide local access to the Bao API |
| Cache | Cache supported tokens, leased secrets, and responses |
| Process supervisor | Start child processes with injected environment variables |
| Updates | Re-render files after secret/certificate changes |

Process supervisor was labeled public beta in the original research. Agent cache is not a universal offline store for every secret.

For example, replace:

```text
/etc/myapp/config.yaml
└── password: plaintext-password
```

with:

```text
OpenBao KV → OpenBao Agent → /run/myapp/config.yaml → myapp.service
```

`/run` is normally tmpfs, allowing runtime-only files. Services can read Agent-rendered files directly or use systemd credentials.

## 3. Obtain systemd credentials at service startup

### 3.1 Native capabilities and boundaries

`systemd-creds` supports mechanisms such as `host`, `tpm2`, and `host+tpm2`. **OpenBao is not a native encryption backend for `LoadCredentialEncrypted=`.**

`LoadCredential=` in `systemd.exec` can obtain data from a file or Unix socket. A VM without a TPM can therefore obtain a service password at startup without persistently storing that password in plaintext.

`LoadCredential=` loads credentials **at service startup**, not on each file open. Secret rotation may require application reload/restart or another retrieval step.

### 3.2 Option A: Unix-socket credential broker

Flow:

```text
OpenBao（KV／Transit）
      ↓ Authenticate; retrieve or decrypt the secret
Credential Broker
      ↓ Unix Socket
systemd LoadCredential=
      ↓
$CREDENTIALS_DIRECTORY/db-password
      ↓
Application
```

Illustrative `myapp.service`:

```ini
[Unit]
Description=My Application
Requires=bao-credential.socket
After=bao-credential.socket

[Service]
LoadCredential=db-password:/run/bao-credential.sock
ExecStart=/usr/local/bin/myapp
```

An additional broker must implement `/run/bao-credential.sock`. **Starting Agent does not automatically provide this socket.** Implement the socket protocol, Bao authentication/retrieval, and response handling.

Sequence:

1. systemd prepares to start `myapp.service`.
2. `LoadCredential=` connects to the broker socket.
3. The broker retrieves or decrypts the secret under Bao policies.
4. The broker sends plaintext to systemd through the socket.
5. systemd places it in the service's credentials directory.
6. The service reads `$CREDENTIALS_DIRECTORY/db-password`.

This avoids an intermediate plaintext configuration file, but requires maintaining and authenticating a custom broker.

### 3.3 Option B: Agent + tmpfs + LoadCredential

This is the simpler starting point:

```text
OpenBao KV → Agent Auto-Auth / Template → /run/myapp/db-password → systemd LoadCredential=
```

Illustrative Agent template:

```hcl
template {
  contents = "{{ with secret \"secret/data/myapp\" }}{{ .Data.data.password }}{{ end }}"
  destination = "/run/myapp/db-password"
  perms = "0400"
  backup = false
}
```

systemd service:

```ini
[Service]
LoadCredential=db-password:/run/myapp/db-password
ExecStart=/usr/local/bin/myapp
```

Ensure Agent has authenticated and generated `/run/myapp/db-password` before the service loads credentials. Set directory permissions to prevent cross-service access. `backup = false` prevents extra template backup files; it does not eliminate swap, hibernation, or VM-snapshot exposure.

An application already reading `/run/myapp/config.yaml` can use Agent rendering directly without systemd credential delivery.

### 3.4 Option C: ciphertext in Git and Transit

A broker can decrypt Git-stored ciphertext through Transit at startup:

```text
Git ciphertext → Broker → Transit decrypt → Unix socket → systemd LoadCredential=
```

This requires an intermediary; **`LoadCredentialEncrypted=` does not natively understand Bao ciphertext**. It suits versioned encrypted configuration with centrally controlled decryption rights.

### 3.5 Identity without a local TPM

Keeping service passwords off disk and keeping the credentials used to request those passwords off disk are separate problems.

| Authentication | Secret on VM disk? | Unattended startup |
| --- | --- | --- |
| AppRole with a local SecretID | Usually yes | Yes |
| JWT with cloud workload identity | No long-lived secret required | Yes |
| Kubernetes ServiceAccount JWT | No long-lived secret in the VM image | Yes |
| Hypervisor authenticates and injects credentials | Not required in the VM | Yes |
| Human supplies credentials at boot | Not required | Unsuitable for unattended startup |

In a self-managed VM environment, a hypervisor or trusted identity provider can authenticate and inject short-lived credentials. Stealing a powered-off VM disk then need not reveal the service secret or a credential capable of obtaining it.

This does not protect against control of a running VM. Check swap, hibernation, RAM snapshots, and backups for unintended persistence.

## 4. Three Kubernetes secret-delivery approaches

Bao can run outside Kubernetes. Install only the integration components you need, and pilot selected Pods; **a whole-cluster migration is unnecessary**.

### 4.1 Agent Injector

An admission webhook adds Agent init containers/sidecars to newly created Pods with matching annotations. Containers share secrets through an in-memory volume.

This suits applications that can read files, need minimal code changes, and should avoid native Kubernetes Secret storage.

The official Helm chart supports injector-only operation with an external Bao server. The original research used this illustrative command; check values against the chart version:

```bash
helm repo add openbao https://openbao.github.io/openbao-helm
helm repo update

helm upgrade --install openbao openbao/openbao \
  -n openbao --create-namespace \
  --set server.enabled=false \
  --set injector.enabled=true \
  --set csi.enabled=false \
  --set injector.externalBaoAddr=https://bao.example.com
```

Example ERP Pod-template annotations:

```yaml
template:
  metadata:
    annotations:
      vault.hashicorp.com/agent-inject: "true"
      vault.hashicorp.com/role: "erp"
      vault.hashicorp.com/agent-inject-secret-db: "secret/data/erp"
```

The recorded annotations use `vault.hashicorp.com`. Configure Bao Kubernetes auth, roles, and ACL policies binding `erp` to the intended ServiceAccount and secret paths. Newly created Pods can then read injected files such as `/vault/secrets/db`.

Deployments without these annotations can remain unchanged.

### 4.2 Secrets Store CSI Driver + OpenBao Provider

A node-side CSI provider retrieves secrets and mounts them through a `SecretProviderClass`, without an Agent sidecar per Pod.

Required components:

1. The generic Secrets Store CSI Driver.
2. OpenBao CSI Provider.
3. The workload's `SecretProviderClass` and CSI volume declaration.

Illustrative provider installation from the original research:

```bash
helm upgrade --install openbao openbao/openbao \
  -n openbao --create-namespace \
  --set server.enabled=false \
  --set injector.enabled=false \
  --set csi.enabled=true \
  --set global.externalBaoAddr=https://bao.example.com
```

**This does not install the generic CSI Driver; deploy it separately.** CSI suits volume-oriented delivery without necessarily persisting a matching Kubernetes Secret. Refresh behavior depends on driver/provider rotation settings.

### 4.3 External Secrets Operator (ESO)

ESO runs a controller that copies secrets from Bao's Vault-compatible API into native Kubernetes Secrets. Existing Helm charts and `secretKeyRef` references can stay unchanged.

Existing application:

```yaml
env:
  - name: DB_PASSWORD
    valueFrom:
      secretKeyRef:
        name: erp-db
        key: password
```

Add an `ExternalSecret` that reads `secret/data/erp` and synchronizes it to `erp-db`.

ESO's Vault provider supports KV v1/v2 and Kubernetes auth. It is a third-party integration; verify version and feature compatibility with Bao.

Migration is gradual, and previously synchronized Secrets normally survive a brief Bao outage. **The secret still exists in Kubernetes Secrets/etcd**, so retain Kubernetes RBAC, encryption at rest, and backup protection.

### 4.4 Compare the approaches

| Main requirement | Starting point |
| ------------------------------ | ------------------- |
| Preserve existing Helm charts and Kubernetes Secrets | ESO |
| Deliver secrets only through in-memory Pod files | Injector |
| Prefer Kubernetes volume integration | CSI |
| Application calls the Bao API directly | SDK or Agent proxy |

**Pilot incrementally:** retain some native Secrets and trial one namespace or deployment with ESO, Injector, or CSI. Align paths, ServiceAccounts, policies, and workload settings rather than changing every chart at once.

## 5. Other infrastructure integrations

### 5.1 Linux, VM, and systemd services

- Use KV v2 for PostgreSQL, Redis, ERP API keys, and internal service tokens.
- Agent renders to `/run`; services use `LoadCredential=` or read the files directly.
- This reduces static plaintext configuration and permits per-service least-privilege policies.

### 5.2 Dynamic database credentials

- Give services their own PostgreSQL, MySQL, or MariaDB credentials.
- Issue TTL-bound accounts with revocation, or rotate managed static accounts where supported.
- For example, 24-hour accounts reduce reliance on one long-lived shared `erp_user`.
- Plan renewal and reconnection because credential lifecycle management becomes more dependent on Bao availability.

### 5.3 SSH Secrets Engine: SSH CA

- Bao holds the SSH CA private key; callers submit public keys for signing.
- ACL policies control access to signing roles; SSH roles constrain principals, TTL, and certificate options.
- Target servers trust the CA public key. **Validating an issued certificate does not require an online Bao call.**
- This reduces manual `authorized_keys` distribution and makes access expiry manageable.

### 5.4 PKI and ACME for internal TLS/mTLS

- Bao can issue short-lived internal certificates for applications and databases.
- Compatible ACME clients request and renew certificates through its ACME server.
- **Bao acts as the ACME server here, not the client obtaining public certificates from Let's Encrypt for Traefik.**
- Keep Traefik/Let's Encrypt for public HTTPS while adopting internal Bao PKI gradually.

### 5.5 CI/CD, deployment jobs, and Codex automation

- Authenticate workload JWT/OIDC identities through a Bao auth method.
- Separate development and production secrets through least-privilege policies.
- A platform with verifiable short-lived identity reduces the need to store fixed CI/CD tokens.

### 5.6 Compose, cron, and backups

- Agent, CLI, or systemd can supply S3 credentials, Restic passwords, and database credentials before a job starts.
- Delivery may use tmpfs files, standard input, file descriptors, or controlled child environments.
- Each backup script need not implement its own Bao client.

### 5.7 Transit cryptography

- Transit encrypts, decrypts, generates HMACs, signs, verifies, and supports key rotation.
- Policies can separate encryption, decryption, and signing rights.
- Typical uses include sensitive fields, encrypted files, and centralized signing.
- If every application operation calls Transit, Bao is on the runtime critical path; assess latency and failure behavior.

KV v2 suits Cloudflare tokens, GitHub PATs, and other API keys, but **central storage does not automatically rotate every third-party token**. Rotation depends on the external API and integration.

## 6. Availability: when the dependency is needed

Requiring every Pod startup or operation to reach one Bao instance can create a single point of failure. Not every integration immediately interrupts already-running applications.

### Typical outage effects

| Integration | Effect of a Bao outage |
| ------------------------ | --------------------------- |
| Running application with existing static credentials | Usually continues |
| Agent Injector | Existing Pods often continue; new Pods may lack secrets |
| CSI provider | New Pods may fail to start while mounting secrets |
| ESO | Synchronized Secrets remain; new Pods can usually start |
| Dynamic database credentials | Existing credentials work until expiry; renewal is affected |
| Online Transit operations | Calls requiring Bao fail |
| SSH CA | Issued certificates remain verifiable; new issuance stops |
| PKI / ACME | Issued certificates remain valid; issuance/renewal is affected |

Two details matter:

1. Agent cache supports certain leased secrets, but **is not a universal offline vault**. Do not assume ordinary KV v2 values can be recovered from persistent cache.
2. Injector webhook `failurePolicy` affects admission on failure. The chart in the original research used `Ignore`, which may admit Pods without injected secrets. Applications must fail appropriately when required secrets are missing.

Integrated Storage/Raft supports HA, but placement, storage, unsealing, and failure domains require design; changing replicas to three is insufficient.

For a single-node deployment, initially use Bao as a **central storage and issuance service** rather than a synchronous dependency for every core operation.

### Suitable adoption stages

- **Start gradually:** static KV, SSH CA, startup-time secrets, administration, and backups.
- **Expand later:** large-scale dynamic database credentials, internal PKI/ACME, and frequent automatic rotation.
- **Plan HA first:** high-frequency Transit and dynamic authorization on the critical application path.

## 7. Suggested adoption sequence

1. **Centralize selected static secrets in KV v2.** Begin with infrastructure API keys and noncritical credentials, using explicit paths and policies.
2. **Pilot Agent + tmpfs for one noncritical Linux service.** Check startup order, persistence boundaries, and secret retrieval after restart.
3. **Pilot one Kubernetes namespace/deployment.** Try ESO to preserve charts, or Injector/CSI to avoid etcd secret copies. Observe restart, outage, and rotation behavior.
4. **Expand SSH CA, CI/CD, and internal PKI gradually.** Public Traefik/Let's Encrypt certificates can remain unchanged.
5. **Evaluate extensive dynamic credentials, online Transit, and HA when needed.** Account for their operational and availability costs.

## 8. Architectural guidance

Use Bao first for **central management, authorization, and short-lived issuance**, while limiting how often applications require it online.

- systemd/VMs: start with `Agent → /run tmpfs → LoadCredential=`. Build a socket broker only if socket-only delivery is required.
- Kubernetes: choose ESO, Injector, or CSI according to whether secrets may persist in Kubernetes and whether new Pods must start during Bao outages.
- SSH/PKI: issued certificates can normally be validated independently for their validity period.
- Transit/dynamic credentials: useful capabilities, with runtime availability dependencies.
- Security improvements come from less plaintext persistence, shorter credential lifetimes, and narrower per-service permissions, as well as central storage.
