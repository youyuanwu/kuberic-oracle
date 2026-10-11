# Oracle Data Guard Readiness Contract

> **Status:** Research and design contract only. No Data Guard fixture,
> production Oracle connection, Kuberic runtime adapter, role mutation, or
> authority-changing operation is implemented.
>
> **Oracle Free boundary:** Oracle AI Database Free remains one standalone,
> observe-only, non-electable, mutation-free database. It is not a Data Guard
> member and does not implement replication-runtime callbacks.
>
> **Licensing:** This document records technical research, not legal advice.
> Image access and use require repository-owner approval of the exact Oracle
> terms that apply to the selected artifact.

## Claim labels

Every conclusion in this document uses one of these labels:

- **Verified fact** — supported by repository code at the cited lines or by an
  authoritative Oracle source for the cited version.
- **Repository design choice** — a support boundary selected by this
  repository; it is not presented as an Oracle product requirement.
- **Unknown** — insufficiently sourced for implementation or a stronger
  support claim. Each unknown has a stop condition.

Facts about the dormant Kuberic preview are verified only for local commits
`5c202ec` and `b0e96aa`. They remain facts about pinned, incompatible reference
code, not facts about a production or stable Kuberic API.

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
| OTN developer use | Oracle's agreement permits internal development, testing, prototyping, and demonstration, and excludes data-processing, business, commercial, and production use. It also prohibits third-party distribution and does not include technical support. [Source](https://www.oracle.com/downloads/licenses/standard-license.html) | The repository owner must confirm that the exact selected binary or image is offered under this agreement and that the intended lab use complies. |
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
| Redo receive/apply | `V$DATAGUARD_PROCESS`, `V$DATAGUARD_STATS`, applied/received sequence evidence, apply and transport lag. [process](https://docs.oracle.com/en/database/oracle/oracle-database/26/refrn/V-DATAGUARD_PROCESS.html), [statistics](https://docs.oracle.com/en/database/oracle/oracle-database/26/refrn/V-DATAGUARD_STATS.html) | Record source member, applying member, metric time, and documented no-row behavior. | Native receive/apply state and lag for the sampled member. | A scalar Kuberic LSN, catch-up completion, or exact operation success. |
| Archive gaps | `V$ARCHIVE_GAP`. [26 reference](https://docs.oracle.com/en/database/oracle/oracle-database/26/refrn/V-ARCHIVE_GAP.html) | Bind to current recovery incarnation. An empty result must be interpreted only as documented for the selected version. | The currently reported blocking archive gap. | Proof that no other transport/apply issue exists. |
| Protection mode/level | `V$DATABASE.PROTECTION_MODE`, `PROTECTION_LEVEL`; broker configuration protection mode. [26 reference](https://docs.oracle.com/en/database/oracle/oracle-database/26/refrn/V-DATABASE.html) | Sample the full configuration and member identities together. | Configured and observed protection state. | Quorum durability or acknowledgment semantics compatible with Kuberic. |
| Flashback | `V$DATABASE.FLASHBACK_ON`, `V$FLASHBACK_DATABASE_LOG`; broker validation. [26 reference](https://docs.oracle.com/en/database/oracle/oracle-database/26/refrn/V-FLASHBACK_DATABASE_LOG.html) | Bind retention/oldest evidence to member and sample time. | Flashback state relevant to possible reinstate. | Guaranteed reinstate success or sufficient retention for a future failover. |
| Switchover readiness | `V$DATABASE.SWITCHOVER_STATUS`; broker `VALIDATE DATABASE [VERBOSE] [STRICT ...]`. [role state](https://docs.oracle.com/en/database/oracle/oracle-database/26/refrn/V-DATABASE.html), [broker validation](https://docs.oracle.com/en/database/oracle/oracle-database/26/dgbkr/validate-database.html) | Require primary and target samples in one identity-bracketed decision window. | Native readiness/warnings for the sampled configuration. | Exact completion or safety after the sample expires. |
| Failover readiness | Broker validation, role, transport/apply/gap/protection evidence, and independently verifiable old-primary fencing. | Bind candidate, old primary, bystanders, external deployment identities, and decision time. | Candidate readiness if every required proof is present. | Safety when fencing is absent or data-loss scope is unknown. |
| Reinstate/rebuild | Broker reinstate status where used, flashback evidence, new role/apply state, and external deployment/storage identity. | Bind the former primary or replacement to the exact post-failover lineage and attempt. | Current post-recovery membership state. | Attempt-bound reinstate/rebuild completion without a repository journal. |
| Restart/connection session | `V$INSTANCE.STARTUP_TIME`, instance name/host/status; observer connection/session identity retained by the future host. [26 reference](https://docs.oracle.com/en/database/oracle/oracle-database/26/refrn/V-INSTANCE.html) | Restart evidence is separate from database lineage; connection identity is separate from Kuberic process session. | Instance restart and observation-session continuity. | Database replacement, storage replacement, or Kuberic process identity. |
| Storage identity | No Oracle-native storage identity is admitted by this contract. | Require external volume/PVC/deployment identity and ownership evidence. | External evidence that a particular deployment/storage allocation was observed. | Oracle DBID/incarnation proves storage continuity or clone uniqueness. |

### Target-version rule

The URLs above are Oracle AI Database 26 sources where available. Any future
row supported only by Oracle 19c documentation must be labeled
“verified for 19c; unverified for the selected target” and must remain behind
a native-evidence stop condition. The selected 23-or-later lab artifact,
views, columns, enums, privileges, and commands require re-verification before
implementation.

## Exact operation-completion matrix

Native state is necessary but does not identify this repository's request or
attempt. A future completion record would need a repository-generated request
ID, monotonic revision, input digest, attempt/session identity, target native
identity, external deployment/storage identity, precondition snapshot, and
postcondition snapshot. None exists today.

| Operation | Required preconditions | Required native postconditions | Required external/repository evidence | Exact outcome rules |
|---|---|---|---|---|
| Switchover | Identity-bracketed primary/target/by-stander evidence; no unresolved gap; supported protection/transport/apply state; readiness for both roles. | Old primary is the intended physical standby; intended standby is primary; required transport/apply paths are healthy; all members remain in compatible lineage. | Exact request/revision/input digest; operation journal; same attempt across both role observations; target deployment/storage identities. | `succeeded` only when every bound postcondition is met. `failed` requires an attributable terminal error. `cancelled` is unknown until sourced. Any unexplained role state, stale sample, or missing causality is `ambiguous`. |
| Failover | Primary loss/unavailability evidence; selected target identity; declared data-loss policy; bystander compatibility; independent old-primary fencing plan. | Target is primary; current incarnation/resetlogs and data-loss boundary are retained; viable bystanders point to the new authority or are explicitly disabled/rebuild-required. | Exact request/attempt; fencing proof; pre-failover evidence; post-failover deployment/storage identities; recorded operator authorization. | No success without fencing. `succeeded` requires the exact target and declared data-loss result. A primary role alone is `ambiguous`. Incompatible retained history is `rebuild-required`. Cancellation semantics are unknown. |
| Reinstate | Exact failed-over configuration; former-primary identity; flashback and retention evidence where broker reinstate is used; no conflicting authority. | Former primary returns as the intended standby, in compatible post-failover lineage, with transport/apply healthy. | Exact reinstate request/attempt and former-primary deployment/storage identity. | `succeeded` requires attributable membership and apply health. Broker/database command failure is `failed` only when tied to the attempt. Missing causality or unexpected lineage is `ambiguous` or `rebuild-required`. |
| Rebuild | Typed reason the old member cannot be retained; new empty target deployment/storage identity; authorized source and target; old member retired. | New member has the intended `DB_UNIQUE_NAME`, standby role, compatible lineage, and healthy transport/apply. | Exact source/target identities, new storage identity, copy/build attempt journal, validation sample, and retirement evidence for the old allocation. | Native rebuild-completion semantics are **unknown**. Until sourced and implemented, a healthy-looking standby is not exact success. Conflicting history or reused storage is `rebuild-required`; unattributed completion is `ambiguous`. |

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
| Kuberic resource/history | Dormant `ResourceUid`, `Epoch` | Preview lifecycle and authority domains | Oracle DBID/incarnation/SCN |
| Kuberic replica | Dormant logical replica, instance ID, agent generation | Exact preview replica incarnation | Oracle `DB_UNIQUE_NAME` or fixture container |
| Kuberic process session | Dormant `ProcessSessionId` | One preview process/Open/report authority session | Oracle instance startup or SQL connection |
| Kuberic storage | Dormant `PvcUid` and build authorization | Preview durable allocation and copy target | Oracle DBID/incarnation |
| Fixture/deployment | Container, volume, network, endpoint, owner token, future Pod/PVC/deployment IDs | External ownership and allocation evidence | Oracle-native lineage or Kuberic history |

The Oracle Free clone result is the concrete non-substitution proof: an
image-derived fresh volume may retain indistinguishable DBID/current-
incarnation evidence while container and volume identity change.

## Dormant Kuberic Public V1 fit/no-fit

This section uses only local commits `5c202ec` and `b0e96aa`, inspected as
`<sha>:<path>`. The preview is protocol 11/schema 8, while production remains
protocol 9/schema 6. Production normalization rejects Public V1 status
([`b0e96aa:kuberic-controller/src/normalize.rs:87-97`]) and preview storage has
no migration contract
([`b0e96aa:kuberic-runtime/src/host/sqlite_store.rs:157-162`]). Later preview
changes were not reviewed; targeting them requires a new analysis.

| Concept | Pinned preview evidence | Data Guard fit | Blocker / upstream decision |
|---|---|---|---|
| Resource | `ResourceUid` and lifecycle generation (`b0e96aa:kuberic-runtime/src/protocol/public_operations.rs:57-77`) | Fit for binding one hosted integration resource. | No production Public V1 resource contract is selected. |
| History | `Epoch { data_loss_number, configuration_number }` (`b0e96aa:kuberic-runtime/src/protocol/types.rs:121-136`) | Partial fit: Data Guard failover/configuration changes need separate authority dimensions. | Oracle incarnation/resetlogs cannot be declared equivalent to `Epoch`; admission rules need upstream definition. |
| Replica | `ReplicaIdentity` separates logical replica, instance, and durable generation (`b0e96aa:kuberic-runtime/src/protocol/types.rs:181-186`) | Conceptual fit for primary/standby member incarnations. | Mapping from Oracle member plus deployment identity is unselected. |
| Process | Public reports bind a process session (`b0e96aa:kuberic-runtime/src/protocol/public_operations.rs:1067-1086`) | Fit for one hosted process/Open attempt. | Oracle instance identity is not this identity. |
| Session | `ProcessSessionId` also binds intents and reports (`b0e96aa:kuberic-runtime/src/protocol/public_operations.rs:351-365`) | Partial fit for exact attempt/report authority. | Oracle connection sessions and repository operation attempts remain separate. |
| Storage | `PvcUid` binds build source and target storage (`b0e96aa:kuberic-runtime/src/protocol/public_operations.rs:576-610`) | Strong conceptual fit for external storage evidence and rebuild targets. | The preview storage model is dormant and non-migratable. |
| Evidence | Public V1 rejects legacy/native evidence fields (`b0e96aa:kuberic-runtime/src/control/convert.rs:182-277,344-379`) | No fit for transporting the full Oracle evidence matrix directly. | A future extension contract or separate evidence channel is required. |
| Progress | `PublicProgress { role, first, last }` (`b0e96aa:kuberic-runtime/src/protocol/public_operations.rs:998-1026`) | No safe direct fit: Oracle evidence is multidimensional. | No SCN-to-LSN/range conversion is allowed. |
| Exact completion | Completion retains operation ID, revision, input digest, replay disposition, and a typed result (`b0e96aa:kuberic-runtime/src/protocol/public_operations.rs:1030-1046`) | Strong conceptual fit for repository attempt-bound outcomes. | Oracle-native state cannot populate exact completion without a repository journal and a selected upstream contract. |
| Rebuild | Typed rebuild-required reasons and exact local storage (`b0e96aa:kuberic-runtime/src/protocol/public_operations.rs:935-992`) | Conceptual fit for incompatible history and replacement storage. | Oracle-specific history admission and source/target authorization remain undesigned. |
| Hosting | No selected replacement replication implementation; production rejects the preview. | No fit for Oracle Free replication callbacks. | Upstream must provide or select a stable observation-only hosting boundary before Oracle Free integration. |

### Oracle Free hosting decision

**Repository design choice:** Oracle Free requires a separate
observation-only hosting boundary. It must not pretend to implement
replication callbacks, secondary progress, catch-up, build/copy, data-loss,
switchover, failover, or election. The benefit is truthful capability and
authority separation. The cost is a distinct future upstream hosting path.
Because no stable upstream boundary is selected, runtime integration remains
stopped.

## Stop conditions

Implementation or a stronger support claim must stop when any condition below
is unresolved:

1. **Licensing:** the repository owner has not approved the exact terms and
   intended use for the selected artifact.
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
11. **Privilege/security:** least-privilege observation and mutation grants,
    credential handling, and production TLS are not separately designed.
12. **Upstream contract:** the replacement Kuberic public contract and
    observation-only hosting boundary are not stable and selected.
13. **Oracle Free:** any design would make Free electable, mutable,
    replicated, or Data Guard capable.

## What this phase proves

This phase proves only that the repository has an evidence vocabulary,
identity boundaries, fit analysis, and explicit stop conditions for future
research. It does not prove that an Enterprise lab can currently be
distributed or run, that any member is safe to promote, that Oracle state can
be converted to Kuberic progress, or that exact operation completion is
implemented.
