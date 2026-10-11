# Oracle External-Replication Adapter

> **Status:** Safety contract, server-free observation admission, and
> test-only Oracle Free live proof. Production database connectivity, Kuberic
> runtime hosting, Data Guard mutation, and failover effects are not
> implemented.
>
> **Support level:** Experimental. This is not an Oracle-supported Kubernetes
> high-availability solution or a production deployment guide.

## Decision

Kuberic will treat Oracle as an external database engine. Oracle owns database
files, redo generation, recovery, native role state, and—when licensed and
configured—Data Guard transport and apply. Kuberic may eventually own durable
intent and reconciliation around those native operations.

The deployment and test boundary follows `kuberic-mssql`:

- production adapter code connects to an already provisioned database;
- production code has no Docker, Podman, Kubernetes, root, package-manager, or
  license-acceptance authority;
- test-only fixture code may own explicitly licensed containers;
- fixture ownership is recorded before a resource is exposed to tests; and
- cleanup removes only resources whose immutable identities still match the
  ownership journal.

The first live target is deliberately narrow:

| Area | Initial contract |
|---|---|
| Engine | Oracle AI Database Free, major version pinned by configuration |
| Platform | Linux x86-64 container |
| Artifact | Oracle Container Registry image pinned by tag and SHA-256 digest |
| Topology | One standalone database |
| Database role | `PRIMARY` |
| Open mode | `READ WRITE` |
| Replication | None |
| Default behavior | Observe only |

The tag-plus-digest form is the intended live-fixture policy. Stage 0 currently
validates only digest-pinned image-reference syntax; it does not require a tag
or Oracle repository, resolve the registry object, inspect its manifest or
platform, establish provenance, or determine the terms applicable to the exact
image.

The future native-replication profile is separate:

| Area | Future Data Guard contract |
|---|---|
| Edition | Enterprise-capable artifact under explicitly approved non-production terms; commercially licensed Enterprise for production |
| Topology | One primary and two physical standbys |
| Replication | Oracle Data Guard physical standby |
| Broker | Explicit later design decision |
| Protection mode | Explicit later design decision |
| Failover | Disabled until fencing and promotion proofs are implemented |

The three-member topology is the repository's supported laboratory and
integration invariant, not an Oracle Data Guard product minimum.

`EnterpriseDeveloper` remains a repository profile name, not an
Oracle-published edition or license tier. The exact artifact, terms, platform,
and evidence requirements are governed by the
[Data Guard readiness contract](data-guard-readiness.md).

Oracle Free is never represented as Data Guard capable. Running multiple Free
containers creates isolated standalone databases; it does not create a
supported physical standby topology.

## Why containers

Oracle AI Database Free can be installed directly on documented native RPM
platforms. The current Ubuntu development host is outside the native package
profile selected for this repository, so direct installation is not the initial
repository path.

Containers are selected because:

1. the current development and CI hosts are Ubuntu rather than the documented
   native RPM platforms selected for this repository;
2. Free edition permits only one installation in a logical environment;
3. each fixture needs an isolated listener, database home, persistent volume,
   process tree, and cleanup identity;
4. a container boundary provides fixture isolation and a convenient allocation
   boundary, while durable Kuberic incarnation and immutable resource identity
   are generated, persisted, and cross-checked separately; and
5. test-only Docker authority can remain outside production adapter code.

Containerization does not weaken Oracle's edition or license restrictions.
Image use and Oracle license acceptance remain explicit deployer actions.
Oracle Free also remains subject to its documented CPU, memory, user-data,
support, and patching limits; it is not a production Enterprise capacity or
security-patching analogue.

## Why the SQL Server adapter is the closest model

The PostgreSQL adapter starts and owns native PostgreSQL processes and delegates
WAL streaming, backup, rewind, and promotion directly to PostgreSQL binaries.
Oracle Free does not provide an equivalent per-directory process model, and it
does not include Data Guard.

The SQL Server repository has the more appropriate boundary:

- native database instances are provisioned externally;
- the adapter starts observe-only;
- the database remains the source of native identity and replication evidence;
- licensed containers belong to isolated test infrastructure; and
- mutation is deferred until its complete safety protocol is understood.

Oracle-specific identity, SCN, redo, resetlogs, incarnation, role, transport,
apply, and broker semantics must not be translated from SQL Server AG concepts
by name alone.

## Current implementation

The production crate implements:

- digest-pinned image-reference syntax validation;
- explicit Free, Enterprise Developer, and Enterprise edition profiles;
- explicit standalone and physical Data Guard replication profiles;
- fail-closed rejection of Data Guard with Free edition;
- exact replica-count contracts for the initial profiles;
- Oracle DBID, database unique name, role, open mode, a currently supported
  logging subset, SCN, resetlogs, activation, instance, and current
  whole-database incarnation evidence types;
- nullable `CURRENT_SCN` representation for mounted-database truth;
- explicit enabled/disabled force-logging representation;
- validated credential/trust-file references, multitenant scope, expected row
  counts, bounded deadlines, and independent polling cadence;
- fixed read-only SQL statements and driver-independent raw-row contracts;
- bracketed raw-row parsing with database, current-incarnation, and instance
  identity stability checks;
- fail-closed parsing of supported role, open-mode, logging, Boolean, numeric,
  product-version, and identity values;
- independently observed native edition and native major version;
- retained Oracle AI Database product, version, and release evidence;
- observation freshness and future-time rejection;
- target DBID, `DB_UNIQUE_NAME`, and current-incarnation identity validation;
- edition and major-version validation;
- standalone-primary admission; and
- preliminary physical-primary/standby admission requiring `ARCHIVELOG` and
  `FORCE LOGGING`; and
- a default-authorized test-only live PoC using Oracle's pure-Rust `oracledb`
  `26.0.0-beta.4`, one disposable worker process, and one owned Oracle Free
  container.

The implementation does not yet claim that admitted Data Guard evidence is
sufficient for promotion, Kuberic progress publication, or write authority.
Production code remains server-free; the PoC's plaintext connection and Docker
authority exist only in the tests crate.

## Provisioning boundary

### Production

Production code receives:

- a direct database endpoint, never an unverified generic load balancer;
- mounted references to an observation username, password, and trust material;
- expected DBID and `DB_UNIQUE_NAME`;
- expected current incarnation number, resetlogs ID, and resetlogs change;
- expected Oracle major version and edition profile;
- a digest-pinned provisioning image reference; and
- Kuberic resource, replica, Pod, PVC, and process-session identities when the
  runtime integration is added.

Production code must not:

- pull or launch images;
- mount the container runtime socket;
- accept Oracle terms;
- initialize a database;
- create administrative users;
- enable archive logging or force logging;
- configure Data Guard;
- invoke DGMGRL;
- promote, switchover, reinstate, flash back, or recreate a database; or
- silently downgrade failed observation to a healthy default.

### Test fixture

The default-authorized PoC fixture owns one Oracle Free container. A later Enterprise
Developer fixture may own three containers only after license handling and Data
Guard design are reviewed.

Before each Docker mutation, the fixture persists creation intent; after
creation it persists the observed ownership evidence for:

- fixture root;
- network name and ID;
- volume name and revalidated creation/mount/driver identity;
- container name and ID;
- image repository, tag, and digest;
- listener port allocation;
- credential and trust-file paths.

DBID, database unique name, and current Oracle incarnation are observed after
readiness and kept separate from the fixture-resource deletion authority.

Cleanup must reject:

- missing or unsupported journal versions;
- paths outside the canonical fixture root;
- symlinked ownership files;
- resource names not created by the fixture;
- replaced containers, networks, or volumes;
- resources attached to foreign containers; and
- incomplete identity evidence.

Cleanup success means every owned resource was removed or was already absent
with conclusive identity evidence. Operational and cleanup failures must be
reported together.

The fixture root is a same-UID owner-only directory, not a symlink. Persisted
file paths must remain direct children of that root, and recovery is bound to
the exact normalized local Unix Docker endpoint and daemon ID recorded in the
journal.
Journal publication synchronizes both file content and the containing
directory; a complete owner-matching temporary publication is recovered,
while malformed or conflicting temporary state fails closed.
Timed-out Docker creation remains explicitly ambiguous: automatic cleanup
retains the journal rather than treating short-term absence as proof that the
daemon canceled the mutation.

## Observation contract

Every attempt will use fixed, read-only queries. Arbitrary SQL is not accepted
from JSON, environment variables, or command-line arguments.

The fixed snapshot reads:

- `V$DATABASE` for DBID, database name, unique name, role, open mode, current
  SCN, resetlogs SCN, activation number, log mode, force logging, and CDB state;
- `V$DATABASE_INCARNATION` before and after the detailed rows for exactly one
  `STATUS='CURRENT' AND CON_ID=0` database-incarnation record, including
  incarnation number, resetlogs ID/change/time, prior-incarnation evidence,
  status, and container scope;
- `V$INSTANCE` for instance name, host, version, startup time, instance status,
  and database status; and
- `PRODUCT_COMPONENT_VERSION` for product and version cross-checking.

The production parser consumes raw rows corresponding to the fixed queries and
retains parsed product/version evidence. The test-only PoC executes those
queries against the pinned Oracle Free image and independently reads
`V$INSTANCE.EDITION`. It verifies native version `23.26.3.0.0` separately from
the Oracle AI Database 26ai marketing name. Production still has no live
connection path.

A mounted physical standby may return `NULL` for `CURRENT_SCN` because the
database is not open. The snapshot and admission contracts preserve that
nullability and permit it only for a mounted standby. Primary and open-standby
admission require a current SCN. Zero, last-known, or other fabricated SCN
substitutes are prohibited.

The raw-row parser requires stable database, current-incarnation, and instance
identity before and after the detailed rows and rejects any change. The
test-only worker executes that bracket; a SQL transaction is not assumed to
make dynamic performance views globally atomic.

Failures remain classified rather than converted to absence:

- target database absent or unreachable;
- authentication or authorization denied;
- TLS or hostname verification failed;
- malformed or unsupported value;
- identity changed during observation;
- stale observation;
- future-dated observation;
- unsupported edition or version; and
- unsupported native role or open mode.

The parser models only a subset of Oracle's native logging domains. It fails
closed on `LOG_MODE=MANUAL`, standby-nologging `FORCE_LOGGING` states, and every
other unmodeled native value. Future expansion must retain new values
explicitly; coercing every value other than `NO` to enabled is prohibited.

## Identity and lineage

The minimum standalone identity is:

- DBID;
- `DB_UNIQUE_NAME`;
- database name;
- current incarnation number;
- resetlogs ID and resetlogs change;
- product major version;
- instance name;
- instance startup time; and
- configured Kuberic replica incarnation.

DBID alone is not sufficient for recovery lineage. The current observation
therefore retains the whole-database current incarnation separately from
instance restart evidence. Data Guard work must additionally retain and
cross-check:

- resetlogs change number;
- activation number;
- database role;
- database incarnation;
- resetlogs time;
- standby control-file state;
- primary and standby unique names; and
- exact Kuberic process sessions.

A database restored, duplicated, flashed back, activated, or opened with
resetlogs must not be mistaken for the previous native lineage merely because a
service endpoint or volume name was reused.

Instance startup time is restart evidence, not a durable incarnation identity.
Oracle database incarnation, resetlogs lineage, Kuberic replica incarnation,
Kuberic process session, and immutable container-resource identity are separate
evidence domains and must be persisted and cross-checked independently.

The pinned Oracle Free image reproduces its DBID and current-incarnation tuple
when initialized on a fresh owned volume. Changed container/volume identity is
therefore fixture evidence, not proof of a different Oracle-native database.
The adapter does not manufacture divergence with DBID rewriting or database
creation and does not claim it can reject this clone.

## Native progress

The snapshot contract carries `CURRENT_SCN` as an exact Oracle native value,
and the test-only live worker populates it. Production does not publish it as
Kuberic progress.

Reasons include:

- a primary's current SCN is not a standby apply position;
- transport, receive, archived, applied, and recoverable positions differ;
- SCNs require resetlogs/incarnation lineage;
- apply lag and transport lag are not promotion authority;
- a reachable standby may still be unsafe to promote;
- Data Guard protection mode and required destinations affect durability; and
- Kuberic's scalar progress contract cannot by itself express fencing,
  transport gaps, broker state, or flashback/reinstatement requirements.

See [Kuberic progress](kuberic-progress.md) and the
[Data Guard native evidence matrix](data-guard-readiness.md#native-evidence-matrix).

## Native HA boundary

Free edition stops at standalone observation.

For Enterprise Data Guard, normal role transitions must follow Oracle's native
protocol. Kuberic must not issue an isolated role-changing SQL statement or
DGMGRL command and infer safety from command success.

Before any future promotion, the implementation must prove:

1. exact old-primary identity and Kuberic session;
2. revoked or expired Kuberic write authority;
3. infrastructure fencing or equivalent proof that the old primary cannot
   write or restart;
4. candidate native lineage compatibility;
5. redo transport and apply evidence required by the configured protection
   policy;
6. absence or explicit acceptance of data loss;
7. successful native role transition;
8. post-transition database role, open mode, and incarnation;
9. client routing convergence; and
10. durable publication of the resulting authority and native evidence.

Broker-managed and brokerless Data Guard are separate protocols and require an
explicit choice. The repository will not mix them opportunistically. The
readiness contract keeps that choice blocked until the selected target version,
privilege model, fencing, and exact-completion evidence are safety-reviewed;
see [broker tradeoffs and stop conditions](data-guard-readiness.md#broker-managed-and-brokerless-tradeoffs).

## Credentials and transport security

The future observer will use a dedicated least-privileged principal. It will
not use `SYS`, `SYSTEM`, or a fixture administration credential.

Credentials must be mounted as separate bounded files. Passwords must not
appear in:

- repository files;
- JSON configuration;
- command-line arguments;
- process environments when a file mechanism is available;
- logs;
- error values; or
- ownership journals.

TLS must verify the server certificate chain and configured connection name.
There will be no plaintext mode or certificate-verification bypass in the
supported profile.

The exact dynamic-performance-view grants will be determined experimentally
and documented. The observer must fail closed on missing privilege rather than
retrying with an administrative identity.

The experiment must document whether the observation principal is placed in
the CDB root or a PDB, which container the observer connects to, applicable
`CONTAINER_DATA` configuration, and the expected row scope for each supported
Oracle version. It must prove that the principal sees every required row and no
broader cross-container scope than intended. Broad dictionary roles or exact
direct grants must not be prescribed before authoritative evidence and live
testing establish the minimum.

## Staged implementation

### Stage 0: safety contract

Implemented:

- workspace, CI, formatting, Clippy, and tests;
- typed support profiles and digest-pinned image-reference syntax;
- native snapshot types and fixed query inventory;
- freshness, identity, edition, version, role, and currently modeled logging
  and open-mode admission; and
- design and progress documentation.

### Stage 1: one-member Free observer

Implemented server-free foundations:

- validated credential/trust-file references without reading their contents;
- bounded connect, per-query, whole-snapshot, and graceful-shutdown deadlines;
- independent polling cadence;
- explicit CDB-root/PDB principal placement, connection container,
  `CONTAINER_DATA`, and expected row counts;
- driver-independent fixed-query row contracts and bracketed parsing;
- nullable mounted-database `CURRENT_SCN`;
- fail-closed logging and native-value parsing; and
- server-free configuration, parsing, identity-change, and admission tests.

Implemented test-only PoC:

- exact pure-Rust beta pin with no Instant Client/native Oracle dependency;
- bounded credential reads and process-bounded driver copies;
- versioned parent/worker IPC, stage deadlines, caller cancellation, complete
  process-group termination/reaping, and sanitized failures;
- loopback-only plaintext fixture connection, verified with captured
  `connect(2)` destinations;
- exact row-count enforcement and fixed-query parsing/admission;
- exact current-incarnation bracketing and target-lineage admission;
- independent live edition, native version, and product/release evidence; and
- positive, wrong-password, removed-grant, privilege-audit, restart,
  same-volume recreation, fresh-volume clone-ambiguity, deadline, recovery,
  and cleanup proof.

Interrupted fixtures retain their owner claim and journal. The explicit
`live-cleanup` path reloads that evidence, distinguishes unavailable Docker
from confirmed absence, removes containers and networks by recorded immutable
ID, and retains the journal whenever cleanup remains incomplete.

Remaining production live-observer work:

- select a production-supported Oracle client strategy;
- implement separately reviewed TLS-verified connection establishment;
- read production credentials and trust material;
- add a production observer API and sanitized operational output;
- prove production timeout, cancellation, redirect, privilege, and transport
  semantics; and
- add watch mode only after the one-shot production observer is safe.

### Stage 2: owned Free container fixture

Implemented for the local PoC:

- explicit terms acknowledgement and exact Linux x86-64 digest;
- atomic suite ownership and durable desired/observed Docker journal;
- one network, volume, container, loopback port, and owner-only credential set;
- local `FREEPDB1` observer with exact direct grants and prohibited-privilege
  audit;
- positive, authentication, authorization, process-signal, row-scope, and
  cleanup cases; and
- ownership-safe same-container restart, same-volume container recreation, and
  fresh-volume generation with revalidated container, volume, mount, network,
  image, port, and journal evidence;
- restricted-worker observation after every generation, including the
  fresh-volume clone;
- interruption recovery across cleared and unrecorded replacement identities;
  and
- an ordinary Rust test enabled by repository Cargo configuration and the
  default local/CI validation path.

Still deferred:

- TCPS fixture provisioning and certificate lifecycle;
- production-equivalent transport claims.

### Stage 3: Kuberic observe-only runtime

Stage 3 is blocked on Kuberic Phase 5's future public V1 values/evidence
contract. Oracle DBID/incarnation/resetlogs/startup/SCN evidence is not itself
Kuberic `HistoryContext`, replica incarnation, `ReplicatorCapabilities`, or
progress. Successful future role or lifecycle callbacks cannot be
observational no-ops: public completion is semantic evidence of the exact
operation's postcondition.

The local protocol-11/schema-8 Public V1 preview is reference evidence only:
it is incompatible, disposable, has no migration contract, is rejected by
production evaluation, and has not selected a replacement Replicator. The
[fit/no-fit matrix](data-guard-readiness.md#dormant-kuberic-public-v1-fitno-fit)
does not make it a dependency.

- host one already provisioned Oracle member through Kuberic's public process
  boundary;
- bind expected DBID, unique name, artifact, replica, Pod, PVC, and process
  session;
- validate requested role against fresh Oracle evidence;
- expose no client service address until routing is separately designed; and
- publish no generic progress until the progress contract is proven.

### Stage 4: Enterprise Data Guard research and fixture

- retain the exact development terms and image-distribution path as stop
  conditions until an authorized 23-or-later artifact is verified;
- choose broker-managed or brokerless operation only after the documented
  tradeoffs receive a separate safety review;
- construct a three-member Enterprise Developer laboratory as a repository
  support invariant rather than an Oracle product minimum;
- use the readiness contract's redo transport, apply, gap, lineage, flashback,
  operation-completion, and identity matrices as the minimum evidence set;
- determine the product and licensing implications of every admitted native
  state, including `READ ONLY WITH APPLY`, for the selected CDB/PDB use;
- wait for a stable selected Kuberic public contract rather than mapping SCN
  to a dormant scalar progress preview; and
- enforce the [readiness stop conditions](data-guard-readiness.md#stop-conditions).

### Stage 5: authority-changing operations

Only after a separate safety review:

- durable operation envelopes and replay classification;
- external fencing receipts;
- write-authority revocation;
- switchover;
- failover with explicit data-loss policy;
- reinstate or rebuild;
- replacement; and
- client routing.

## Non-goals

The initial implementation excludes:

- production deployment;
- Oracle RAC;
- Data Guard on Free edition;
- logical standby;
- snapshot standby;
- far sync;
- Active Data Guard read-only licensing decisions;
- GoldenGate;
- sharding;
- autonomous database;
- multiple managed PDBs;
- backup orchestration;
- storage snapshots;
- cross-region disaster recovery;
- application-level logical replication; and
- mixed Oracle major versions.

## References

- [Oracle AI Database Free licensing restrictions](https://docs.oracle.com/en/database/oracle/oracle-database/26/xeinl/licensing-restrictions.html)
- [Oracle AI Database Free installation guide](https://docs.oracle.com/en/database/oracle/oracle-database/26/xeinl/installing-oracle-database-free.html)
- [Oracle Database Free FAQ](https://www.oracle.com/database/free/faq/)
- [Oracle `V$DATABASE` reference](https://docs.oracle.com/en/database/oracle/oracle-database/26/refrn/V-DATABASE.html)
- [Oracle `V$DATABASE_INCARNATION` reference](https://docs.oracle.com/en/database/oracle/oracle-database/26/refrn/V-DATABASE_INCARNATION.html)
- [Oracle dynamic performance views](https://docs.oracle.com/en/database/oracle/oracle-database/26/refrn/about-dynamic-performance-views.html)
- [Oracle Data Guard Concepts and Administration](https://docs.oracle.com/en/database/oracle/oracle-database/26/sbydb/)
- [Oracle Technology Network License Agreement](https://www.oracle.com/downloads/licenses/standard-license.html)
- [Oracle Data Guard readiness contract](data-guard-readiness.md)
- [Kuberic PostgreSQL design](https://github.com/youyuanwu/kuberic/blob/main/docs/features/postgres/design.md)
- [Kuberic MSSQL design](https://github.com/youyuanwu/kuberic-mssql/blob/main/docs/design.md)
- [Kuberic Service Fabric API semantics](https://github.com/youyuanwu/kuberic/blob/main/docs/features/kuberic/service-fabric-api-semantics.md)
- [Kuberic Service Fabric alignment](https://github.com/youyuanwu/kuberic/blob/main/docs/features/kuberic/service-fabric-alignment.md)
- [Kuberic Replicator boundary](https://github.com/youyuanwu/kuberic/blob/main/docs/features/kuberic/replicator-boundary.md)
- [Kuberic stateless default Replicator](https://github.com/youyuanwu/kuberic/blob/main/docs/features/kuberic/stateless-default-replicator.md)
