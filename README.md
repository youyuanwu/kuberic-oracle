# Kuberic Oracle

Kuberic Oracle is the Oracle database adapter workspace for
[Kuberic](https://github.com/youyuanwu/kuberic). It follows the deployment and
safety boundary established by
[kuberic-mssql](https://github.com/youyuanwu/kuberic-mssql): production code
observes an already provisioned database, while test-only infrastructure may
own isolated database containers.

The current repository implements the first server-free foundation:

- digest-pinned image-reference syntax validation;
- explicit standalone and Data Guard deployment profiles;
- Oracle database, instance, initial lineage-evidence, role, open-mode, and SCN
  types;
- fail-closed observation admission for identity, freshness, edition, version,
  and the currently modeled role, logging, and open-mode subset; and
- contract tests for Oracle Free and future Enterprise Data Guard profiles.

It does **not** yet connect to Oracle, launch containers, publish Kuberic
progress, configure Data Guard, or perform role changes. No live observer
currently populates the snapshot contracts.

See the [support and safety design](docs/design.md) and
[Kuberic progress analysis](docs/kuberic-progress.md).

## Initial deployment decision

The first live profile will use Oracle AI Database Free in a container:

```text
container-registry.oracle.com/database/free:<version>@sha256:<digest>
```

The tag and digest above are placeholders. Tests and deployment configuration
must use a reviewed version and immutable digest; `latest` alone is rejected.
Stage 0 validates digest-reference syntax only. It does not yet require a tag
or Oracle repository, resolve the registry object, inspect its manifest or
platforms, establish provenance, or determine the terms governing the selected
image. Those checks are mandatory before a live fixture uses the reference.

Containers are required for the initial repository profile because:

- the development host is Ubuntu, outside the documented native RPM platforms
  selected for this repository's initial evaluation;
- Oracle Free permits only one installation per logical environment;
- one container per test member provides filesystem, process, listener, and
  cleanup isolation; and
- the fixture can record exact immutable container identities without giving
  production adapter code Docker authority.

Containers do not add Oracle Data Guard to Free edition. A Free container is
therefore a standalone database member, not a physical standby.

Oracle Free is an evaluation target, not a capacity or patching analogue for a
production Enterprise deployment. Oracle documents limits including two
foreground CPUs, 2 GB combined SGA/PGA memory, and 12 GB of user data, and
states that Free does not receive support requests or patches, including
security patches. See the
[Oracle Database Free FAQ](https://www.oracle.com/database/free/faq/).

## Supported profiles

| Profile | Edition | Replicas | Replication | Current status |
|---|---|---:|---|---|
| `standalone` | Oracle AI Database Free | 1 | None | Contract implemented |
| `standalone` | Enterprise Developer/Enterprise | 1 | None | Contract implemented |
| `data_guard_physical` | Enterprise Developer/Enterprise | 3 | Oracle Data Guard | Contract only |

The three-member Data Guard count is this repository's supported laboratory
invariant, not an Oracle Data Guard product minimum.

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
4. validate identity, role, open mode, SCN, startup-time restart evidence,
   future explicit incarnation/resource bindings, TLS, and CLI behavior;
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
