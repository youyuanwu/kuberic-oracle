# Oracle Data Guard Readiness Contract

> **Status:** Research and design contract only. No Data Guard fixture,
> production Oracle connection, Data Guard runtime adapter, role mutation, or
> authority-changing operation is implemented. The separate Oracle Free
> singleton custom Replicator and runtime-host library remain observation-only
> and mutation-free.
>
> **Oracle Free boundary:** Oracle AI Database Free remains one standalone,
> observe-only, non-electable, mutation-free database. It is not a Data Guard
> member and does not implement replication-runtime callbacks.
>
> **Licensing:** This document records technical research, not legal advice.
> Image access and use require approval by the organization-designated
> licensing and registry authority for the exact Oracle terms and artifact.

## Claim labels

The proposed [replication and HA join design](replication-ha-design.md)
develops the custom Replicator, immutable standby build, role-specific LSN
certificates, native quorum policy, and fenced transition state machines.
Its decisions remain proposals subject to this contract's stop conditions;
it neither implements these paths nor supplies stop-condition closure evidence.

Every conclusion in this document uses one of these labels:

- **Verified fact** — supported by repository code at the cited lines or by an
  authoritative Oracle source for the cited version.
- **Repository design choice** — a support boundary selected by this
  repository; it is not presented as an Oracle product requirement.
- **Unknown** — insufficiently sourced for implementation or a stronger
  support claim. Each unknown has a stop condition.

In the matrices below, Oracle behavior and native-source descriptions are
verified facts only for the cited version. Binding requirements, allowed and
forbidden inferences, topology rules, and completion rules are repository
design choices unless a cell explicitly marks an item unknown.

The Kuberic analysis uses the independently reviewable selected upstream
baseline `youyuanwu/kuberic` commit
`28b181bb1846d057cce5913513a46ed72de4c977`. Citations use immutable
`<commit>:<path>:<line-range>` references. Public V1 protocol `11` and local
agent schema `12` are the selected production contract
(`28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/protocol/public_operations.rs:12-29`;
`28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/README.md:52-58`).
The [observation-only hosting contract](observation-only-hosting-contract.md#selected-upstream-facts)
preserves the complete evidence table and defines the implemented Oracle Free
custom singleton boundary.

## Current repository boundary

**Verified fact:** production code currently exposes configuration, raw-row
parsing, observation snapshots, and admission only. The support contract
defines one standalone replica and three Data Guard replicas
([`config.rs:7-8`](../crates/kuberic-oracle/src/config.rs#L7-L8)), rejects Data
Guard for the Free profile
([`config.rs:56-58`](../crates/kuberic-oracle/src/config.rs#L56-L58)), and
requires the standalone database to be `PRIMARY`, `READ WRITE`, with a current
SCN ([`observation.rs:71-82`](../crates/kuberic-oracle/src/observation.rs#L71-L82)).

**Verified fact:** Oracle identity, lineage, restart, and progress are already
separate types. `DatabaseIncarnationIdentity` retains incarnation number,
resetlogs ID, and resetlogs change
([`types.rs:230-234`](../crates/kuberic-oracle/src/types.rs#L230-L234));
`DatabaseSnapshot` retains DBID, `DB_UNIQUE_NAME`, role, open mode, SCN,
resetlogs change, activation, and logging state
([`types.rs:250-262`](../crates/kuberic-oracle/src/types.rs#L250-L262)); and
`InstanceSnapshot.startup_time` is separate restart evidence
([`types.rs:266-274`](../crates/kuberic-oracle/src/types.rs#L266-L274)).

**Verified fact:** observation is bracketed by before/after database,
incarnation, and instance identity. A change rejects the sample rather than
merging evidence from different native states
([`parser.rs:77-128`](../crates/kuberic-oracle/src/parser.rs#L77-L128)).
Admission then independently binds DBID, `DB_UNIQUE_NAME`, and the complete
current-incarnation tuple to the configured target
([`observation.rs:9-13`](../crates/kuberic-oracle/src/observation.rs#L9-L13),
[`observation.rs:41-49`](../crates/kuberic-oracle/src/observation.rs#L41-L49)).
The server-free tests separately reject a target DBID/name mismatch and every
field mismatch in the current-incarnation tuple
([`target_identity_must_match`](../crates/kuberic-oracle-tests/tests/contract.rs#L305-L326),
[`target_incarnation_must_match_even_when_dbid_matches`](../crates/kuberic-oracle-tests/tests/contract.rs#L329-L357)).

**Verified fact:** the test-only Oracle Free proof shows that a fresh
image-derived volume can reproduce the same DBID/current-incarnation tuple.
Changed container and volume identity therefore remains external evidence; it
cannot be replaced by an Oracle-native clone-rejection claim
([`live_observer.rs:140-177`](../crates/kuberic-oracle-tests/tests/live_observer.rs#L140-L177)).

## Enterprise lab feasibility

### Terminology

**Verified fact:** `EnterpriseDeveloper` is a repository enum name
([`config.rs:13-17`](../crates/kuberic-oracle/src/config.rs#L13-L17)). Oracle's
published agreement is titled
[Oracle Technology Network License Agreement](https://www.oracle.com/downloads/licenses/standard-license.html);
it does not define an Oracle edition or license tier named “Enterprise
Developer.”

**Repository design choice:** this repository may retain
`EnterpriseDeveloper` as shorthand for a non-production Enterprise-capable
research profile. The shorthand does not select a license or artifact.

### Licensing and distribution paths

| Path | Sourced fact | Unknown / required decision |
|---|---|---|
| OTN developer use | Oracle's agreement permits internal development, testing, prototyping, and demonstration, and excludes data-processing, business, commercial, and production use. It also prohibits third-party distribution and does not include technical support. [Source](https://www.oracle.com/downloads/licenses/standard-license.html) | The organization-designated licensing authority must confirm that the exact selected binary or image is offered under this agreement and that the intended lab use complies. |
| Direct Oracle binary download | Oracle presents a separate software-license acceptance gate for direct downloads. [Source](https://www.oracle.com/downloads/software-license-agreement.html) | The selected database version, platform artifact, retention rules, and applicable terms must be recorded before automation. |
| Oracle Container Registry | Oracle states that the registry contains licensed and open-source software and that licensed images require login plus acceptance of Oracle Standard Terms and Restrictions. [Source](https://docs.oracle.com/en/operating-systems/oracle-linux/docker/docker-UsingDockerRegistries.html) | The session-protected `database/enterprise` page, exact 23+/26 tag, Linux x86-64 manifest, pull entitlement, redistribution restriction, and digest must be verified by an authorized operator. |
| Oracle `docker-images` repository | Oracle publishes build files, while the Database image instructions require separately obtained binaries and acceptance of the license attached to those binaries. [Source](https://github.com/oracle/docker-images/tree/main/OracleDatabase/SingleInstance) | This path does not provide a redistributable pre-built Enterprise image. The exact source binary and resulting image handling remain operator responsibilities. |
| Oracle AI Database Free | Oracle documents the public Free image and its resource/support limits. [FAQ](https://www.oracle.com/database/free/faq/) | Free is not an Enterprise/Data Guard substitute and remains outside this lab. |

**Unknown:** a reproducible, authorized Enterprise-capable image path for the
repository's Oracle major-version floor is not yet proven. The code rejects a
configured native major version below 23
([`config.rs:38-42`](../crates/kuberic-oracle/src/config.rs#L38-L42)).
Availability and native evidence must therefore be verified for the selected
23-or-later artifact, even when a concept is also documented in Oracle 19c.

### Source verification ledger

The following claims were re-checked against the linked Oracle pages on
2026-10-11. The ledger records the semantic checked; it does not establish
artifact entitlement, least-privilege grants, or lab behavior.

| Claim | Version/source | Semantic checked | Status |
|---|---|---|---|
| Developer-use terms | [OTN agreement](https://www.oracle.com/downloads/licenses/standard-license.html) | Internal development/testing/prototyping/demonstration scope; no production or redistribution; no included support | Verified for the linked agreement |
| Licensed registry workflow | [Oracle Linux registry documentation](https://docs.oracle.com/en/operating-systems/oracle-linux/docker/docker-UsingDockerRegistries.html) | Licensed images require registry login and acceptance of Oracle Standard Terms and Restrictions | Verified generically; exact `database/enterprise` repository remains unknown |
| Broker readiness | [Oracle AI Database 26 `VALIDATE DATABASE`](https://docs.oracle.com/en/database/oracle/oracle-database/26/dgbkr/validate-database.html) | Switchover/failover readiness, strict checks, flashback, apply, transport, and gap output | Verified for 26 |
| Role/readiness values | [Oracle AI Database 26 `V$DATABASE`](https://docs.oracle.com/en/database/oracle/oracle-database/26/refrn/V-DATABASE.html) | Documented switchover states and database role/protection fields used by this contract | Verified for 26 |
| Incarnation | [Oracle AI Database 26 `V$DATABASE_INCARNATION`](https://docs.oracle.com/en/database/oracle/oracle-database/26/refrn/V-DATABASE_INCARNATION.html) | A new incarnation is created when a database is opened with `RESETLOGS` | Verified for 26 |
| Failover lineage | [Oracle AI Database 26 role transitions](https://docs.oracle.com/en/database/oracle/oracle-database/26/sbydb/managing-oracle-data-guard-role-transitions.html) | Oracle states that failover causes a new incarnation at the primary | Verified for 26 |
| Transport/apply/gap | [`V$ARCHIVE_DEST_STATUS`](https://docs.oracle.com/en/database/oracle/oracle-database/26/refrn/V-ARCHIVE_DEST_STATUS.html), [`V$DATAGUARD_PROCESS`](https://docs.oracle.com/en/database/oracle/oracle-database/26/refrn/V-DATAGUARD_PROCESS.html), [`V$DATAGUARD_STATS`](https://docs.oracle.com/en/database/oracle/oracle-database/26/refrn/V-DATAGUARD_STATS.html), [`V$ARCHIVE_GAP`](https://docs.oracle.com/en/database/oracle/oracle-database/26/refrn/V-ARCHIVE_GAP.html) | Existence and described purpose of destination, process, lag, and current blocking-gap evidence | Verified for 26; row cardinality and privilege behavior require separate rules below |
| Flashback/restart | [`V$FLASHBACK_DATABASE_LOG`](https://docs.oracle.com/en/database/oracle/oracle-database/26/refrn/V-FLASHBACK_DATABASE_LOG.html), [`V$INSTANCE`](https://docs.oracle.com/en/database/oracle/oracle-database/26/refrn/V-INSTANCE.html) | Flashback-log evidence and instance restart/status evidence | Verified for 26 |

### Required licensing approval record

Before an Enterprise artifact is pulled, built, retained, or used, an immutable
approval record must bind:

- the approving organizational role and approver identity;
- the exact terms document/revision or captured source and retrieval date;
- the registry account or other entitlement used;
- artifact version, platform, tag, digest, and source path;
- approved purpose and one-primary/two-standby topology;
- retention, caching, redistribution, and cleanup constraints;
- approval date, expiry or review date; and
- invalidation triggers, including changed terms, artifact digest, account,
  topology, purpose, or selected Oracle version.

### Supported lab topology

The following are **repository design choices**, not Oracle Data Guard product
minimums:

- one primary and two physical standbys;
- one externally provisioned database member per independently recorded
  deployment/storage identity;
- Linux x86-64 only until another platform is explicitly researched;
- no production data or production use;
- no automatic failover;
- no client-routing claim;
- no requirement to open a standby read-only;
- no Active Data Guard entitlement assumption;
- no authority-changing operation until fencing and exact-completion evidence
  pass a separate safety review; and
- no repository acceptance of Oracle terms on behalf of an operator.

Three members exercise primary-to-multiple-standby transport, bystander
behavior during role changes, and rebuild targeting. The additional standby
also increases image, CPU, memory, and storage cost. The count remains an
integration coverage choice enforced by
[`config.rs:59-64`](../crates/kuberic-oracle/src/config.rs#L59-L64), not a
statement that Oracle requires three databases.

## Broker-managed and brokerless tradeoffs

Oracle documents manual SQL role transitions and describes Data Guard Broker
as a management framework that automates configuration, monitoring, and role
transitions. Broker validation for Oracle AI Database 26 can report
switchover/failover readiness, flashback state, transport, apply lag, gap
status, log-file configuration, and strict validation results.

Sources:

- [Oracle AI Database 26 Data Guard Concepts and Administration](https://docs.oracle.com/en/database/oracle/oracle-database/26/sbydb/)
- [Oracle AI Database 26 Data Guard Broker](https://docs.oracle.com/en/database/oracle/oracle-database/26/dgbkr/)
- [`VALIDATE DATABASE`](https://docs.oracle.com/en/database/oracle/oracle-database/26/dgbkr/validate-database.html)
- [Failover troubleshooting](https://docs.oracle.com/en/database/oracle/oracle-database/26/dgbkr/troubleshooting-problems-failover-operation.html)

| Concern | Broker-managed | Brokerless / manual SQL |
|---|---|---|
| Configuration authority | Broker configuration and properties become an additional authority surface. | Database initialization parameters, SQL, and external orchestration remain the authority surface. |
| Readiness | `VALIDATE DATABASE` provides a consolidated, versioned command surface. | The integration must assemble and reconcile individual views and transition prerequisites. |
| Role transitions | Broker coordinates transport/apply and bystander handling around commands. | The integration must define the ordering, retries, and evidence for every SQL step. |
| Automatic failover | Broker can support fast-start failover, which is outside this repository's planned lab boundary. | No broker automatic failover, but an external controller could still be unsafe unless explicitly prohibited. |
| Reinstate | Broker has a named reinstate workflow and documented failure paths. | Manual flashback/recovery/rebuild procedures require a separate sourced design. |
| Observation | Broker health is convenient but may be periodic or stale; native views remain necessary. | Native views are direct but distributed across several samples and identities. |
| Exact completion | Command success is useful evidence but does not carry this repository's request/attempt identity. | SQL success also lacks repository request/attempt identity. |
| Privilege/dependency | Requires broker enablement, DGMGRL access, and broker-specific privileges/configuration. | Requires the least-privilege feasibility of each view and the authority to execute each transition statement. |

**Repository design choice:** this phase does not select broker-managed or
brokerless mutation. The choice is a stop condition before any role-changing
implementation. Automatic failover remains excluded in either case.

## Native evidence matrix

The matrix defines evidence that a future observer would need to retain. It
does not grant election, mutation, or completion authority. Samples spanning
multiple views must be identity-bracketed using the existing before/after
pattern in [`parser.rs:77-128`](../crates/kuberic-oracle/src/parser.rs#L77-L128).
Broker and view disagreement, stale broker health, or cross-identity samples
must produce an ambiguous or fail-closed result.

| Domain | Native Oracle evidence | Binding and freshness | Allowed claim | Forbidden inference |
|---|---|---|---|---|
| Database role | `V$DATABASE.DATABASE_ROLE`; broker database/configuration status. [26 reference](https://docs.oracle.com/en/database/oracle/oracle-database/26/refrn/V-DATABASE.html) | Bind DBID, `DB_UNIQUE_NAME`, incarnation, instance startup, and observation time. | Role observed for one bracketed sample. | Election authority or proof that a requested transition caused the role. |
| Database/member identity | `V$DATABASE.DBID`, `DB_UNIQUE_NAME`, database name, and primary unique name where applicable. [26 reference](https://docs.oracle.com/en/database/oracle/oracle-database/26/refrn/V-DATABASE.html) | Bind to external deployment/storage identity and the exact configured target. | Native database/member naming evidence. | DBID alone identifies one Data Guard member or one deployment. Whether every physical member shares DBID must be authoritatively sourced for the selected version before implementation. |
| Incarnation/resetlogs | `V$DATABASE_INCARNATION`, including current/parent status and resetlogs change/time. [26 reference](https://docs.oracle.com/en/database/oracle/oracle-database/26/refrn/V-DATABASE_INCARNATION.html) | Retain the complete tuple and before/after lineage. | Current native lineage and detected RESETLOGS transitions. | Kuberic history, causality for an operation, or storage continuity. |
| Redo transport | `V$ARCHIVE_DEST`, `V$ARCHIVE_DEST_STATUS`, destination errors/status, archived sequence, and synchronization fields. [26 reference](https://docs.oracle.com/en/database/oracle/oracle-database/26/refrn/V-ARCHIVE_DEST_STATUS.html) | Bind every destination to the observed primary/member identities and one sample window. | Destination-specific transport state. | Applied durability, zero data loss, or transition completion. |
| Redo receive/apply | `V$DATAGUARD_PROCESS`, `V$DATAGUARD_STATS`, and `V$ARCHIVE_DEST_STATUS` archived/applied sequence evidence, apply state, and lag. [process](https://docs.oracle.com/en/database/oracle/oracle-database/26/refrn/V-DATAGUARD_PROCESS.html), [statistics](https://docs.oracle.com/en/database/oracle/oracle-database/26/refrn/V-DATAGUARD_STATS.html), [destination status](https://docs.oracle.com/en/database/oracle/oracle-database/26/refrn/V-ARCHIVE_DEST_STATUS.html) | Record source member, applying member, metric time, and documented no-row behavior. | Native receive/apply state and lag for the sampled member. | A scalar Kuberic LSN, catch-up completion, or exact operation success. |
| Archive gaps | `V$ARCHIVE_GAP`. [26 reference](https://docs.oracle.com/en/database/oracle/oracle-database/26/refrn/V-ARCHIVE_GAP.html) | Bind to current recovery incarnation. An empty result must be interpreted only as documented for the selected version. | The currently reported blocking archive gap. | Proof that no other transport/apply issue exists. |
| Protection mode/level | `V$DATABASE.PROTECTION_MODE`, `PROTECTION_LEVEL`; broker configuration protection mode. [26 reference](https://docs.oracle.com/en/database/oracle/oracle-database/26/refrn/V-DATABASE.html) | Sample the full configuration and member identities together. | Configured and observed protection state. | Quorum durability or acknowledgment semantics compatible with Kuberic. |
| Flashback | `V$DATABASE.FLASHBACK_ON`, `V$FLASHBACK_DATABASE_LOG`; broker validation. [26 reference](https://docs.oracle.com/en/database/oracle/oracle-database/26/refrn/V-FLASHBACK_DATABASE_LOG.html) | Bind retention/oldest evidence to member and sample time. | Flashback state relevant to possible reinstate. | Guaranteed reinstate success or sufficient retention for a future failover. |
| Switchover readiness | `V$DATABASE.SWITCHOVER_STATUS`; broker `VALIDATE DATABASE [VERBOSE] [STRICT ...]`. [role state](https://docs.oracle.com/en/database/oracle/oracle-database/26/refrn/V-DATABASE.html), [broker validation](https://docs.oracle.com/en/database/oracle/oracle-database/26/dgbkr/validate-database.html) | Require primary and target samples in one identity-bracketed decision window. | Native readiness/warnings for the sampled configuration. | Exact completion or safety after the sample expires. |
| Failover readiness | Broker validation, role, transport/apply/gap/protection evidence, and independently verifiable old-primary fencing. | Bind candidate, old primary, bystanders, external deployment identities, and decision time. | Candidate readiness if every required proof is present. | Safety when fencing is absent or data-loss scope is unknown. |
| Reinstate/rebuild | Broker reinstate status where used, flashback evidence, new role/apply state, and external deployment/storage identity. | Bind the former primary or replacement to the exact post-failover lineage and attempt. | Current post-recovery membership state. | Attempt-bound reinstate/rebuild completion without a repository journal. |
| Restart/connection session | `V$INSTANCE.STARTUP_TIME`, instance name/host/status; observer connection/session identity retained by the future host. [26 reference](https://docs.oracle.com/en/database/oracle/oracle-database/26/refrn/V-INSTANCE.html) | Restart evidence is separate from database lineage; connection identity is separate from Kuberic process session. | Instance restart and observation-session continuity. | Database replacement, storage replacement, or Kuberic process identity. |
| Storage identity | No Oracle-native storage identity is admitted by this contract. | Require external volume/PVC/deployment identity and ownership evidence. | External evidence that a particular deployment/storage allocation was observed. | Oracle DBID/incarnation proves storage continuity or clone uniqueness. |

### Selected-version row and absence semantics

Each future query contract must define row count and absence separately. A
missing row, `NULL`, duplicate, disabled process, disconnected destination,
unsupported view, and privilege denial are different facts and must not share
one fallback.

| Source | Current readiness rule |
|---|---|
| `V$DATABASE` | Exactly one whole-database row for the bound target; zero or multiple rows fail closed. |
| Current `V$DATABASE_INCARNATION` | Exactly one `STATUS='CURRENT'`, whole-database row for the bound target; zero, multiple, or identity mismatch fails closed. |
| `V$INSTANCE` | Exactly one bound instance row for the configured member in the initial single-instance lab; RAC is out of scope. |
| `V$ARCHIVE_DEST_STATUS` | Expected destination IDs and count are **unknown** until the topology and destination policy are selected. Missing, duplicate, disabled, or error rows fail closed unless the selected-version contract assigns an explicit meaning. |
| `V$DATAGUARD_PROCESS` | Expected process set and multiplicity are **unknown** until the selected-version apply/transport mode is fixed. Absence is not interpreted as idle or healthy by default. |
| `V$DATAGUARD_STATS` | Metric names, row presence by role, `NULL`, and stale computation times require a selected-version rule. Missing metrics do not become zero lag. |
| `V$ARCHIVE_GAP` | Oracle documents the view as the current gap blocking recovery. Zero rows may be treated only as “no current blocking gap reported” after selected-version verification; it is not proof that transport/apply is healthy. Multiple or changing rows require resampling. |
| Broker output | Required sections, member count, warning/error handling, and sample age require a versioned parser contract. Missing text, unknown enum values, or database-view disagreement is ambiguous. |

Privilege denial, query error, unsupported columns, and failure to complete the
identity bracket are errors for every source; they are never equivalent to an
empty healthy result.

### Target-version rule

The URLs above are Oracle AI Database 26 sources where available. Any future
row supported only by Oracle 19c documentation must be labeled
“verified for 19c; unverified for the selected target” and must remain behind
a native-evidence stop condition. The selected 23-or-later lab artifact,
views, columns, enums, privileges, and commands require re-verification before
implementation.

## Exact operation-completion matrix

Native state is necessary but does not identify this repository's request or
attempt. Every operation row inherits one mandatory completion envelope:

- repository-generated operation ID and monotonic revision;
- canonical input digest and replay classification;
- attempt/process-session identity;
- exact target Oracle database/member and pre-operation lineage;
- exact external deployment/storage identity;
- precondition snapshot and postcondition snapshot; and
- authenticated authorization plus a durable operation journal.

A reused operation ID with a different revision, digest, target, storage
identity, or attempt cannot satisfy completion. The table below lists
operation-specific additions; none of the common envelope is optional. No such
runtime record exists today.

| Operation | Required preconditions | Required native postconditions | Required external/repository evidence | Exact outcome rules |
|---|---|---|---|---|
| Switchover | Identity-bracketed primary/target/by-stander evidence; no unresolved gap; supported protection/transport/apply state; readiness for both roles. | Old primary is the intended physical standby; intended standby is primary; required transport/apply paths are healthy; all members remain in compatible lineage. A role exchange alone does not prove or require a new RESETLOGS incarnation. | Same attempt across both role observations and routing evidence when routing is later supported. | `succeeded` only when every bound postcondition is true and no unreconciled terminal error exists. `failed` requires an attributable terminal error **and** proof that success postconditions did not occur. If success effects coexist with an error or acknowledgement is lost, the result is `ambiguous`. `cancelled` is unknown until sourced and cannot be inferred from a cancellation request. `rebuild-required` applies when a member cannot rejoin retained lineage. |
| Failover | Primary loss/unavailability evidence; selected target identity; declared data-loss policy; bystander compatibility; independently verifiable old-primary fencing proof. | Target is primary; Oracle 26 documents a new primary incarnation after failover, so pre- and post-failover incarnation/resetlogs evidence must be retained; the declared data-loss boundary is recorded; viable bystanders point to the new authority or are explicitly disabled/rebuild-required. [26 source](https://docs.oracle.com/en/database/oracle/oracle-database/26/sbydb/managing-oracle-data-guard-role-transitions.html) | Before dispatch: authenticated fencing proof bound to the exact old-primary database, deployment/storage identity, candidate, and operation attempt. After dispatch: retain that proof with post-failover evidence and operator authorization. | Required order: `fence-requested → fence-verified → promotion-authorized → promotion-dispatched`. `succeeded` requires the exact target, new-incarnation evidence, declared data-loss result, and retained fencing proof. `failed` requires an attributable terminal error with no promoted authority. A primary role alone, uncertain fencing, or lost causality is `ambiguous`. Incompatible history is `rebuild-required`; cancellation is unknown after authority may have changed. |
| Reinstate | Exact failed-over configuration; former-primary identity; flashback and retention evidence where broker reinstate is used; no conflicting authority. | Former primary returns as the intended standby, in compatible post-failover lineage, with transport/apply healthy. | Exact former-primary deployment/storage identity and reinstate-specific validation. | `succeeded` requires every bound postcondition and no unreconciled terminal error. `failed` requires an attributable terminal error and proof that success postconditions did not occur. Success effects plus an error or missing acknowledgement is `ambiguous`; incompatible lineage or insufficient flashback is `rebuild-required`; cancellation remains unknown until sourced. |
| Rebuild | Typed reason the old member cannot be retained; new empty target deployment/storage identity; authorized source and target; old member retired. | New member has the intended `DB_UNIQUE_NAME`, standby role, compatible lineage, and healthy transport/apply. | Exact source/target identities, new storage identity, copy/build validation, and retirement evidence for the old allocation. | Native rebuild-completion semantics are **unknown**. Future `succeeded` requires every bound postcondition, retirement proof, and no unreconciled terminal error. `failed` requires an attributable terminal error and proof that success effects did not occur. A healthy-looking standby, mixed success/error evidence, or lost acknowledgement is `ambiguous`; conflicting history or reused storage is `rebuild-required`; cancellation remains unknown until sourced. |

The five result classes used for fit analysis are `succeeded`, `failed`,
`ambiguous`, `cancelled`, and `rebuild-required`. These are design categories,
not implemented Oracle operations.

## Identity-domain matrix

| Domain | Identity/evidence | Purpose | Must not be substituted for |
|---|---|---|---|
| Oracle database | DBID, database name, `DB_UNIQUE_NAME` | Native database/member naming | Kuberic resource, replica, history, or external storage identity |
| Oracle lineage | Incarnation number, resetlogs ID/change/time, prior-incarnation evidence, activation number | Native lineage and RESETLOGS history | Kuberic epoch/history or operation attempt |
| Oracle progress | Current SCN, transported/archived/applied redo, lag/gap evidence | Native ordering and Data Guard state | Kuberic LSN, committed/quorum progress, or exact completion |
| Oracle instance | Instance name, host, startup time, status | Restart and instance evidence | Database lineage or deployment/storage replacement |
| Oracle observer connection | Future driver/session identifier and sample bracket | One observation attempt | Kuberic process session or durable operation attempt |
| Kuberic resource/history | Selected `PublicV1Identity`, `Epoch`, and `HistoryContext` (`28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/protocol/types.rs:142-208`) | Public V1 lifecycle, authority, and history domains | Oracle DBID/incarnation/SCN |
| Kuberic replica | Selected `ReplicaIdentity` (`28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/protocol/types.rs:241-338`) | Logical replica, process incarnation, and durable replica identity | Oracle `DB_UNIQUE_NAME` or fixture container |
| Kuberic process session | Selected `ProcessSessionEndpoint` (`28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/protocol/types.rs:241-338`) | One hosted process/Open lifetime and report authority | Oracle instance startup, Oracle connection session, or repository operation attempt |
| Kuberic storage | Selected `ReplicaStorageIncarnationContext` (`28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/protocol/types.rs:241-338`) | Durable allocation/incarnation and build target identity | Oracle DBID/incarnation |
| Fixture/deployment | Container, volume, network, endpoint, owner token, future Pod/PVC/deployment IDs | External ownership and allocation evidence | Oracle-native lineage or Kuberic history |

The Oracle Free clone result is the concrete non-substitution proof: an
image-derived fresh volume may retain indistinguishable DBID/current-
incarnation evidence while container and volume identity change.

## Principal and transport separation

**Repository design choice:** observation and mutation must use separate
principals, credential files, and access paths.

- The observation principal is read-only and must be unable to execute role
  changes, broker/DGMGRL mutations, configuration changes, or fencing actions.
- A future mutation principal is unavailable to the periodic observation path,
  loaded only for an authenticated authorized attempt, and scoped to the
  selected protocol and target identities.
- A future authorization/fencing principal is separate from both database
  principals when the fencing system is external.
- Tests for a future implementation must include a deny matrix proving that
  each principal cannot exercise the other principals' authority.

Authenticated encryption and peer-name verification are mandatory for every
authority-bearing channel in every supported profile: observation, mutation,
broker/DGMGRL, operation authorization, and fencing. Peer identity must bind to
the configured immutable database/member or infrastructure identity rather
than a mutable endpoint name alone. This phase does not implement TLS or select
the trust model.

## Selected Kuberic Public V1 fit/no-fit

Public V1 protocol `11` / local agent schema `12` is selected, and legacy or
preview selector paths are removed
(`28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/protocol/public_operations.rs:12-29`;
`28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/README.md:52-58`;
`28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/testing/public_v1_conformance.rs:663-672`).
The selected SF-aligned custom-Replicator API is sufficient as the
architectural base for a future full Data Guard integration. It is not a
truthful Oracle Free host because every `StatefulServiceReplica::open` returns
a `Replicator` and completion certifies role/progress/configuration/build or
other operation-specific postconditions
(`28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/application.rs:72-80`;
`28b181bb1846d057cce5913513a46ed72de4c977:docs/features/kuberic/service-fabric-api-semantics.md:44-67`).

| Concept | Selected Public V1 evidence | Data Guard fit | Remaining Oracle/upstream gate |
|---|---|---|---|
| Resource/history | `PublicV1Identity`, `Epoch`, and `HistoryContext` are selected public domains (`28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/protocol/types.rs:142-208`) | Architectural fit for hosted resource and authority history. | Oracle incarnation/resetlogs remains native evidence and cannot be declared equivalent to Kuberic history. |
| Replica/process/storage | `ReplicaIdentity`, `ProcessSessionEndpoint`, and `ReplicaStorageIncarnationContext` are selected identities (`28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/protocol/types.rs:241-338`) | Strong fit for independent member, process, and storage binding. | Oracle database, instance, observer, deployment, and storage identities must remain separate and mapping requires implementation evidence. |
| Progress | `INVALID_LSN`, `PublicProgress`, and role/progress operations are selected (`28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/replicator/mod.rs:184-343`; `28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/protocol/public_operations.rs:992-1055`) | A full custom Data Guard Replicator could implement these only after native mapping is proven. | Oracle evidence is multidimensional; no SCN-to-LSN or placeholder progress is allowed. |
| Replay/exact completion | Public operations carry replay disposition, data-loss authorization, typed results, and exact completion (`28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/protocol/public_operations.rs:2154-2195`; `28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/protocol/public_operations.rs:2200-2247`) | Strong architectural fit for request/attempt-bound outcomes. | Oracle-native state cannot prove causality without a durable operation journal, fencing, and native postcondition evidence. |
| Custom Replicator | PostgreSQL supplies neither `StateProvider` nor `StateReplicator`, but returns an active service-owned custom Replicator (`28b181bb1846d057cce5913513a46ed72de4c977:examples/postgres/src/service.rs:249-291`) | Confirms that an external engine may own its Public V1 callback implementation. | Oracle Free uses a separate singleton empty-history implementation; that does not establish Data Guard callbacks. |
| Hosting | The current host requires a returned `Replicator` (`28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/application.rs:72-80`) | The implemented Oracle Free custom Replicator proves fixed-API singleton hosting. A future Data Guard adapter still requires active replication semantics after every independent gate closes. | The Free adapter's rejected callbacks and empty history cannot be reused as Data Guard authority. |
| Conformance | Subjects are hard-coded as `postgresql` and `built-in-replacement` (`28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/testing/public_v1_conformance.rs:915-1002`) | Repository-owned Oracle contract tests can validate its bounded subject. | Oracle is not registered in the upstream generic conformance selector. |

### Oracle Free hosting decision

**Repository design choice:** Oracle Free is hosted through the implemented
[custom singleton Replicator](observation-only-hosting-contract.md). The
Replicator owns only Kuberic lifecycle, exact singleton configuration, and an
empty adapter-history position. Fresh Oracle admission gates supported
callbacks. Secondary, catch-up, build/copy, data-loss, switchover, failover,
election, and epoch-changing authority remain explicit errors. No Oracle
client service address is published, and the required Kuberic Replicator
address is not an Oracle or Data Guard endpoint.

## Stop conditions

Implementation or a stronger support claim must stop when any condition below
is unresolved:

1. **Licensing:** the organization-designated licensing/registry authority has
   not produced the required immutable approval record for the exact terms,
   artifact, topology, and intended use.
2. **Distribution:** the selected 23-or-later Enterprise-capable image or
   binary, platform, tag, digest, pull entitlement, and retention/redistribution
   constraints are not recorded.
3. **Topology:** the authorized resources and terms do not cover the
   repository's one-primary/two-standby lab, or the topology cannot exercise
   required bystander and rebuild evidence.
4. **Target version:** required views, columns, commands, enums, privileges,
   and behavior are sourced only from an older Oracle version.
5. **Native evidence:** evidence cannot be sampled with stable identity,
   documented freshness, and fail-closed missing/no-row semantics.
6. **Broker choice:** broker-managed versus brokerless mutation authority has
   not been selected and safety-reviewed.
7. **Fencing:** failover cannot independently prove that the old primary
   cannot continue serving as authority.
8. **Identity:** Oracle, Kuberic, and external deployment/storage identities
   cannot be retained without substitution.
9. **Exact completion:** the exact request/attempt cannot be bound to the
   required postconditions; the result must remain ambiguous.
10. **Reinstate/rebuild:** required flashback, history admission, new-storage,
    retirement, and completion evidence is unsourced or unavailable.
11. **Privilege/security:** separate observation, mutation, and external
    authorization/fencing principals; deny rules; credential handling; and
    authenticated encrypted channels with peer-name verification are not
    separately designed.
12. **Runtime activation:** the Oracle Free custom singleton Replicator,
    durable runtime binding, real server-free host lifecycle, and test-only
    worker-backed live lifecycle are implemented, but no production runtime
    executable, non-plaintext Oracle observation source, external Oracle
    deployment/storage evidence binding, or upstream Oracle conformance subject
    exists. None may be inferred from the repository proofs.
13. **Oracle Free:** any design would make Free electable, mutable,
    replicated, or Data Guard capable.

### Stop-condition closure record

A future implementation PR may close a stop condition only with a versioned,
durable record containing:

- gate identifier and current state;
- accountable owner and approving authority;
- immutable evidence identifiers or captured source;
- Oracle version, artifact digest, topology, and upstream-contract fingerprint;
- decision date and validity/review window;
- verification command or procedure and result;
- dependencies on other gates; and
- reopen triggers such as changed terms, artifact, topology, upstream
  contract, credentials, fencing mechanism, or evidence semantics.

Missing, expired, mismatched, or reopened records leave the gate blocked. PAW
artifacts are not that durable record and remain uncommitted.

## What this phase proves

The updated readiness verdict is: **the selected SF-aligned custom Replicator
API is a sufficient architectural base, but Data Guard implementation remains
blocked**. The independent blockers are Enterprise artifact and licensing
approval, selected version evidence, authorized one-primary/two-standby
topology, least-privilege principal and channel design, broker selection,
independent fencing, identity separation, and native exact-completion proof.

This phase proves only that the repository has an evidence vocabulary,
identity boundaries, selected-baseline fit analysis, and explicit stop
conditions for future research. It does not prove that an Enterprise lab can
currently be distributed or run, that any member is safe to promote, that
Oracle state can be converted to Kuberic progress, or that exact operation
completion is implemented.
