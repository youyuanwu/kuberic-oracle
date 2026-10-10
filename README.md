# Kuberic Oracle

Kuberic Oracle is the Oracle database adapter workspace for
[Kuberic](https://github.com/youyuanwu/kuberic). It follows the deployment and
safety boundary established by
[kuberic-mssql](https://github.com/youyuanwu/kuberic-mssql): production code
observes an already provisioned database, while test-only infrastructure may
own isolated database containers.

The current repository implements the server-free contracts plus an opt-in
test-only live proof:

- digest-pinned image-reference syntax validation;
- explicit standalone and Data Guard deployment profiles;
- Oracle database, instance, initial lineage-evidence, role, open-mode, and SCN
  types;
- validated credential/trust-file references, multitenant row scope, bounded
  observer deadlines, and independent polling cadence;
- driver-independent parsing of bracketed database, instance, and product rows,
  including nullable `CURRENT_SCN` and exact supported logging values;
- fail-closed observation admission for identity, freshness, edition, version,
  and the currently modeled role, logging, and open-mode subset; and
- a pure-Rust, process-isolated Oracle Free live test with exact image,
  least-privilege, row-count, failure, and cleanup checks.

Production code still does **not** connect to Oracle, read credentials, launch
containers, publish Kuberic progress, configure Data Guard, or perform role
changes. The live driver and plaintext connection exist only in the tests
crate; plaintext is not a supported production mode.

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

The current Linux x86-64 PoC pins
`container-registry.oracle.com/database/free@sha256:c4d9b15a1b2240ce90730c23ce298fcdd34f6793792c1b3849181a080009c322`
(tag `23.26.3.0` at verification time).

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
| `standalone` | Oracle AI Database Free | 1 | None | Contract + test-only live PoC |
| `standalone` | Enterprise Developer/Enterprise | 1 | None | Contract implemented |
| `data_guard_physical` | Enterprise Developer/Enterprise | 3 | Oracle Data Guard | Contract only |

The three-member Data Guard count is this repository's supported laboratory
invariant, not an Oracle Data Guard product minimum.

Enterprise Developer is suitable only where the applicable Oracle license
permits development, testing, prototyping, and demonstration. Production use
requires the appropriate Oracle commercial licenses. This repository does not
accept license terms on behalf of an operator.

## Workspace layout

- `crates/kuberic-oracle` contains production-safe configuration, raw-row
  parsing, snapshot contracts, and observation admission.
- `crates/kuberic-oracle-tests` contains cross-crate tests, the pure-Rust PoC
  worker, and the explicitly authorized live container fixture.
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

Run the opt-in live proof only after accepting the applicable Oracle terms:

```bash
KUBERIC_ORACLE_ACCEPT_FREE_TERMS=YES \
KUBERIC_ORACLE_DOCKER_HOST=unix:///path/to/docker.sock \
just live
```

The live proof requires Linux x86-64, Docker, `strace`, sufficient image
storage, the pinned Linux image digest, and one local `FREEPDB1` observer. It
uses Oracle's pure-Rust `oracledb` beta without Instant Client. The fixture
publishes the listener on host loopback only, verifies worker connection
destinations, runs positive/authentication/authorization cases serially, and
removes only identity-matched owned resources.

This plaintext PoC does not establish the production transport design. A future
production observer must connect to an already provisioned database with a
separately reviewed TLS design and will never own Oracle terms or containers.

## Non-goals

The initial profile does not provide:

- Oracle Data Guard on Free edition;
- automatic failover, switchover, promotion, or broker mutation;
- RAC, sharding, GoldenGate, or application-level logical replication;
- Kubernetes manifests or an Oracle operator;
- a production deployment guide; or
- legal advice about Oracle licensing.

This is experimental software.
