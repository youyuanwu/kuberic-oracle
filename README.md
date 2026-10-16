# Kuberic Oracle

Kuberic Oracle is the Oracle database adapter workspace for
[Kuberic](https://github.com/youyuanwu/kuberic). It follows the deployment and
safety boundary established by
[kuberic-mssql](https://github.com/youyuanwu/kuberic-mssql): production code
observes an already provisioned database, while test-only infrastructure may
own isolated database containers.

The current repository implements the server-free contracts plus a default
test-only live proof:

- digest-pinned image-reference syntax validation;
- explicit standalone and Data Guard deployment profiles;
- separate Oracle database identity, current-incarnation/resetlogs lineage,
  instance-restart, role, open-mode, and exact SCN types;
- validated credential/trust-file references, multitenant row scope, bounded
  observer deadlines, and independent polling cadence;
- driver-independent parsing of bracketed identity, current-incarnation,
  database, instance, and product rows, including nullable `CURRENT_SCN` and
  exact supported logging values;
- fail-closed observation admission for identity, freshness, edition, version,
  and the currently modeled role, logging, and open-mode subset; and
- a pure-Rust, process-isolated Oracle Free live test with exact image,
  least-privilege, row-count, restart, same-volume recreation, fresh-volume
  clone-ambiguity, failure, recovery, and cleanup checks.

Production code still does **not** connect to Oracle, read credentials, launch
containers, publish Kuberic progress, configure Data Guard, or perform role
changes. The live driver and plaintext connection exist only in the tests
crate; plaintext is not a supported production mode.

See the [support and safety design](docs/design.md) and
[Kuberic progress analysis](docs/kuberic-progress.md). The
[Data Guard readiness contract](docs/data-guard-readiness.md) records the
Enterprise lab evidence matrix, selected Public V1 fit analysis, and explicit
implementation stop conditions. The
[observation-only hosting contract](docs/observation-only-hosting-contract.md)
separates facts from the selected upstream baseline from the generic hosting
proposal that Kuberic does not yet provide.

The selected upstream baseline is Kuberic commit
`28b181bb1846d057cce5913513a46ed72de4c977`: Public V1 protocol `11` and local
agent schema `12` are the sole selected production contract, with the legacy
and preview selectors removed
(`28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/protocol/public_operations.rs:12-29`;
`28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/README.md:52-58`).
Kuberic remains experimental and source-only, without published production
images or an activation procedure
(`28b181bb1846d057cce5913513a46ed72de4c977:README.md:3-5`;
`28b181bb1846d057cce5913513a46ed72de4c977:docs/features/kuberic/stateless-default-replicator.md:160-170`).

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
therefore a standalone, observe-only, non-electable database member, not a
physical standby.

Oracle Free is an evaluation target, not a capacity or patching analogue for a
production Enterprise deployment. Oracle documents limits including two
foreground CPUs, 2 GB combined SGA/PGA memory, and 12 GB of user data, and
states that Free does not receive support requests or patches, including
security patches. See the
[Oracle Database Free FAQ](https://www.oracle.com/database/free/faq/).

## Supported profiles

| Profile | Repository edition profile / artifact | Replicas | Replication | Current status |
|---|---|---:|---|---|
| `standalone` | Oracle AI Database Free | 1 | None | Observe-only contract + test-only live proof |
| `standalone` | `EnterpriseDeveloper` / `Enterprise` with an approved Enterprise-capable artifact | 1 | None | Contract implemented |
| `data_guard_physical` | `EnterpriseDeveloper` / `Enterprise` with an approved Enterprise-capable artifact | 3 | Oracle Data Guard | Contract only |

The three-member Data Guard count is this repository's supported laboratory
invariant, not an Oracle Data Guard product minimum.

`EnterpriseDeveloper` is a repository profile name, not an Oracle-published
edition or license tier. A non-production Enterprise-capable lab is permitted
only when the applicable terms for the exact selected artifact allow its
development, testing, prototyping, or demonstration use. Production use
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
- `docs/observation-only-hosting-contract.md` pins the selected Public V1
  baseline and proposes a generic Open/Observe/Close/Abort hosting boundary
  with explicit replication-capability absence.

Dependency versions are declared in the root `Cargo.toml`.

## Development

Run all checks, including the Oracle Free live proof:

```bash
just check
```

Run only the tracked documentation contract:

```bash
just docs-check
```

The equivalent commands are:

```bash
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-features
```

Repository Cargo configuration sets
`KUBERIC_ORACLE_ACCEPT_FREE_TERMS=YES`, so default Cargo tests require Linux
x86-64, Docker, `strace`, and sufficient image storage. Run
`just check-server-free` when only the non-container contracts should execute.

Run only the live proof:

```bash
KUBERIC_ORACLE_DOCKER_HOST=unix:///path/to/docker.sock \
just live
```

If a test process is interrupted after the ownership journal is written, use
the same environment and run `just live-cleanup`. Cleanup loads the retained
journal, revalidates immutable identities, and refuses foreign replacements.

GitHub Actions and local `cargo test` execute the same default Rust test set,
including the live proof. Setting
`KUBERIC_ORACLE_ACCEPT_FREE_TERMS=NO` disables only the live body.

The live proof requires Linux x86-64, Docker, `strace`, sufficient image
storage, the pinned Linux image digest, and one local `FREEPDB1` observer. It
uses Oracle's pure-Rust `oracledb` beta without Instant Client. The fixture
accepts only a normalized absolute local Unix Docker socket, binds recovery to
that endpoint, publishes the listener on host loopback only, verifies worker connection
destinations, runs positive/authentication/authorization cases serially,
brackets the current whole-database incarnation, proves instance restart and
same-volume container recreation separately, and removes only
identity-matched owned resources.

The pinned image seeds a fresh owned volume with the same DBID and current
incarnation evidence. The live proof therefore records changed fixture
container/volume identity as a clone ambiguity and confirms the new generation
through the restricted one-shot observer; it does not claim that Oracle-native
observation can reject that clone as a different database.

This plaintext PoC does not establish the production transport design. A future
production observer must connect to an already provisioned database with a
separately reviewed TLS design and will never own Oracle terms or containers.

## Non-goals

The initial profile does not provide:

- Oracle Data Guard on Free edition;
- automatic failover, switchover, promotion, or broker mutation;
- election, role/configuration mutation, catch-up, build/copy, data-loss, or
  client-service routing behavior;
- a fake Replicator, fabricated progress, or success-shaped placeholders for
  absent capabilities;
- RAC, sharding, GoldenGate, or application-level logical replication;
- Kubernetes manifests or an Oracle operator;
- a production deployment guide; or
- legal advice about Oracle licensing.

This is experimental software.
