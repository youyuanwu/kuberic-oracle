# Kuberic Oracle

Kuberic Oracle is the Oracle database adapter workspace for
[Kuberic](https://github.com/youyuanwu/kuberic). It follows the deployment and
safety boundary established by
[kuberic-mssql](https://github.com/youyuanwu/kuberic-mssql): production code
observes an already provisioned database, while test-only infrastructure may
own isolated database containers.

The current repository implements the first server-free foundation:

- digest-pinned Oracle container artifact validation;
- explicit standalone and Data Guard deployment profiles;
- Oracle database, instance, lineage, role, open-mode, and SCN types;
- fail-closed observation admission for identity, freshness, edition, version,
  role, logging, and open-mode evidence; and
- contract tests for Oracle Free and future Enterprise Data Guard profiles.

It does **not** yet connect to Oracle, launch containers, publish Kuberic
progress, configure Data Guard, or perform role changes.

See the [support and safety design](docs/design.md) and
[Kuberic progress analysis](docs/kuberic-progress.md).

## Initial deployment decision

The first live profile will use Oracle AI Database Free in a container:

```text
container-registry.oracle.com/database/free:<version>@sha256:<digest>
```

The tag and digest above are placeholders. Tests and deployment configuration
must use a reviewed version and immutable digest; `latest` alone is rejected.

Containers are required for the initial repository profile because:

- the development host is Ubuntu while Oracle's native Free packages target
  Oracle Linux RPM environments;
- Oracle Free permits only one installation per logical environment;
- one container per test member provides filesystem, process, listener, and
  cleanup isolation; and
- the fixture can record exact immutable container identities without giving
  production adapter code Docker authority.

Containers do not add Oracle Data Guard to Free edition. A Free container is
therefore a standalone database member, not a physical standby.

## Supported profiles

| Profile | Edition | Replicas | Replication | Current status |
|---|---|---:|---|---|
| `standalone` | Oracle AI Database Free | 1 | None | Contract implemented |
| `standalone` | Enterprise Developer/Enterprise | 1 | None | Contract implemented |
| `data_guard_physical` | Enterprise Developer/Enterprise | 3 | Oracle Data Guard | Contract only |

Enterprise Developer is suitable only where the applicable Oracle license
permits development, testing, prototyping, and demonstration. Production use
requires the appropriate Oracle commercial licenses. This repository does not
accept license terms on behalf of an operator.

## Workspace layout

- `crates/kuberic-oracle` contains production-safe contracts and observation
  admission.
- `crates/kuberic-oracle-tests` contains cross-crate contract tests and is the
  future home of explicitly licensed live container fixtures.
- `docs/design.md` defines deployment ownership, support boundaries, staged
  implementation, and safety rules.
- `docs/kuberic-progress.md` explains why Oracle SCNs are retained as native
  evidence but are not yet exposed as Kuberic promotion progress.

Dependency versions are declared in the root `Cargo.toml`.

## Development

Run all server-free checks:

```bash
just check
```

The equivalent commands are:

```bash
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-features
```

No Oracle installation or container runtime is needed for these checks.

## Planned live fixture

The first live fixture will own exactly one digest-pinned Oracle Free container.
It will:

1. create an isolated network, volume, container, and credential files;
2. wait for the listener and database to become ready;
3. create a least-privileged observation principal;
4. validate identity, role, open mode, SCN, restart incarnation, TLS, and CLI
   behavior;
5. journal immutable resource identities before exposing them to tests; and
6. remove only resources whose current identities match the journal.

Production code will connect to an already provisioned database and will never
implicitly pull an image, accept Oracle terms, start a container, or use a
container socket.

## Non-goals

The initial profile does not provide:

- Oracle Data Guard on Free edition;
- automatic failover, switchover, promotion, or broker mutation;
- RAC, sharding, GoldenGate, or application-level logical replication;
- Kubernetes manifests or an Oracle operator;
- a production deployment guide; or
- legal advice about Oracle licensing.

This is experimental software.
