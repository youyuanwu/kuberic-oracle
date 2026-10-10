# Oracle External-Replication Adapter

> **Status:** Safety contract and server-free observation admission.
> Database connectivity, container fixtures, Kuberic runtime hosting, Data
> Guard mutation, and failover effects are not implemented.
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

The future native-replication profile is separate:

| Area | Future Data Guard contract |
|---|---|
| Edition | Enterprise Developer for permitted non-production work; Enterprise for production |
| Topology | One primary and two physical standbys |
| Replication | Oracle Data Guard physical standby |
| Broker | Explicit later design decision |
| Protection mode | Explicit later design decision |
| Failover | Disabled until fencing and promotion proofs are implemented |

Oracle Free is never represented as Data Guard capable. Running multiple Free
containers creates isolated standalone databases; it does not create a
supported physical standby topology.

## Why containers

Oracle AI Database Free can be installed directly in a supported Oracle Linux
environment. That is not the initial repository profile.

Containers are selected because:

1. the current development and CI hosts are Ubuntu rather than supported native
   Oracle Linux RPM environments;
2. Free edition permits only one installation in a logical environment;
3. each fixture needs an isolated listener, database home, persistent volume,
   process tree, and cleanup identity;
4. a container boundary maps naturally to one Kuberic replica incarnation; and
5. test-only Docker authority can remain outside production adapter code.

Containerization does not weaken Oracle's edition or license restrictions.
Image use and Oracle license acceptance remain explicit deployer actions.

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

- immutable container image references pinned by SHA-256 digest;
- explicit Free, Enterprise Developer, and Enterprise edition profiles;
- explicit standalone and physical Data Guard replication profiles;
- fail-closed rejection of Data Guard with Free edition;
- exact replica-count contracts for the initial profiles;
- Oracle DBID, database unique name, role, open mode, log mode, SCN, resetlogs,
  activation, instance, and artifact types;
- fixed read-only SQL statements for the future observer;
- observation freshness and future-time rejection;
- target DBID and `DB_UNIQUE_NAME` validation;
- edition and major-version validation;
- standalone-primary admission; and
- preliminary physical-primary/standby admission requiring `ARCHIVELOG` and
  `FORCE LOGGING`.

The implementation does not yet claim that admitted Data Guard evidence is
sufficient for promotion, Kuberic progress publication, or write authority.

## Provisioning boundary

### Production

Production code receives:

- a direct database endpoint, never an unverified generic load balancer;
- mounted references to an observation username, password, and trust material;
- expected DBID and `DB_UNIQUE_NAME`;
- expected Oracle major version and edition profile;
- a digest-pinned provisioning artifact identity; and
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

The first fixture will own one Oracle Free container. A later Enterprise
Developer fixture may own three containers only after license handling and Data
Guard design are reviewed.

Before launch, the fixture must persist desired immutable identities for:

- fixture root;
- network name and ID;
- volume name and ID;
- container name and ID;
- image repository, tag, and digest;
- listener port allocation;
- database unique name;
- generated non-secret incarnation ID; and
- credential and trust-file paths.

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

## Observation contract

Every attempt will use fixed, read-only queries. Arbitrary SQL is not accepted
from JSON, environment variables, or command-line arguments.

The initial snapshot reads at least:

- `V$DATABASE` for DBID, database name, unique name, role, open mode, current
  SCN, resetlogs SCN, activation number, log mode, force logging, and CDB state;
- `V$INSTANCE` for instance name, host, version, startup time, instance status,
  and database status; and
- `PRODUCT_COMPONENT_VERSION` for product and version cross-checking.

The observation is bracketed in a future implementation: stable database and
instance identity must be sampled before and after the detailed reads. A SQL
transaction is not assumed to make dynamic performance views globally atomic.

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

## Identity and lineage

The minimum standalone identity is:

- DBID;
- `DB_UNIQUE_NAME`;
- database name;
- product major version;
- instance name;
- instance startup time; and
- configured Kuberic replica incarnation.

DBID alone is not sufficient for recovery lineage. Data Guard work must also
retain and cross-check:

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

## Native progress

`CURRENT_SCN` is retained as an exact Oracle native value. It is not currently
published as Kuberic progress.

Reasons include:

- a primary's current SCN is not a standby apply position;
- transport, receive, archived, applied, and recoverable positions differ;
- SCNs require resetlogs/incarnation lineage;
- apply lag and transport lag are not promotion authority;
- a reachable standby may still be unsafe to promote;
- Data Guard protection mode and required destinations affect durability; and
- Kuberic's scalar progress contract cannot by itself express fencing,
  transport gaps, broker state, or flashback/reinstatement requirements.

See [Kuberic progress](kuberic-progress.md).

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
explicit choice. The repository will not mix them opportunistically.

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

## Staged implementation

### Stage 0: safety contract

Implemented:

- workspace, CI, formatting, Clippy, and tests;
- typed support profiles and pinned artifacts;
- native snapshot types and fixed query inventory;
- freshness, identity, edition, version, role, logging, and open-mode
  admission; and
- design and progress documentation.

### Stage 1: one-member Free observer

- choose and pin an Oracle Rust driver and Instant Client strategy;
- define strict observer configuration and secret-file handling;
- implement TLS-verified connection establishment;
- parse bracketed fixed-query snapshots;
- add sanitized CLI output and watch mode; and
- add server-free parser and failure-classification tests.

### Stage 2: owned Free container fixture

- add explicit Oracle terms acknowledgement for tests;
- implement exact Docker ownership journaling;
- launch one digest-pinned Free container;
- provision the observation principal;
- validate restart incarnation and volume reuse;
- test authentication, authorization, TLS, timeout, signal, and cleanup paths;
  and
- keep live tests ignored by default unless CI explicitly opts in.

### Stage 3: Kuberic observe-only runtime

- host one already provisioned Oracle member through Kuberic's public process
  boundary;
- bind expected DBID, unique name, artifact, replica, Pod, PVC, and process
  session;
- validate requested role against fresh Oracle evidence;
- expose no client service address until routing is separately designed; and
- publish no generic progress until the progress contract is proven.

### Stage 4: Enterprise Data Guard research and fixture

- confirm the exact development license and image-distribution path;
- choose broker-managed or brokerless operation;
- construct a three-member Enterprise Developer laboratory;
- document redo transport, apply, gap, lineage, flashback, and reinstatement
  evidence;
- determine whether Kuberic's fixed progress API can represent the required
  authority safely; and
- stop if the contract cannot express the required safety evidence.

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
- [Oracle Data Guard Concepts and Administration](https://docs.oracle.com/en/database/oracle/oracle-database/26/sbydb/)
- [Oracle Technology Network License Agreement](https://www.oracle.com/downloads/licenses/standard-license.html)
- [Kuberic PostgreSQL design](https://github.com/youyuanwu/kuberic/blob/main/docs/features/postgres/design.md)
- [Kuberic MSSQL design](https://github.com/youyuanwu/kuberic-mssql/blob/main/docs/design.md)
