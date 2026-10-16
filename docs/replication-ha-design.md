# Oracle Replication, Replica Build, and HA Join Design

> **Status:** Proposed design, not implemented or approved for activation.
> This document does not close the
> [Data Guard readiness gates](data-guard-readiness.md#stop-conditions).
> Production remains observe-only. Oracle Free remains a standalone,
> mutation-free, non-electable profile with empty adapter-history progress.
>
> **Decision:** Use Oracle Data Guard physical replication behind a custom
> Kuberic Replicator. Report a certified, lineage-bound redo-prefix watermark
> as Kuberic LSN, not raw `CURRENT_SCN`, a redo sequence number, or a synthetic
> counter. Durable receipt, apply, quorum commitment, and promotion authority
> are separate facts.

## Scope and support profile

The proposed Enterprise profile supports an externally provisioned primary
and two externally provisioned physical standbys. Three members are this
repository's laboratory/support invariant, not an Oracle product minimum.
An exact Enterprise-capable artifact, version, digest, platform, terms, and
topology require recorded approval. `EnterpriseDeveloper` is a repository
profile name, not an Oracle-published edition or license tier.

Initial replication work covers physical standby build, redo transport,
recovery, and admission as a Kuberic secondary. Switchover and failover are
specified here to establish the safety requirements for that foundation;
their implementation requires a later, separate safety review.

The initial native scope is one instance and one enabled redo thread per
database, whole-CDB replication, identical admitted PDB coverage, `ARCHIVELOG`,
and `FORCE LOGGING=YES`. Standbys remain `MOUNTED` with managed recovery.
Read-only queries during apply require separate Active Data Guard entitlement;
the design does not depend on querying application tables on a standby.
RAC, logical/snapshot standbys, far sync, cascades, excluded PDBs, asynchronous
HA writes, arbitrary quorum policies, and automatic failover are out of scope.

Production never creates containers, allocates volumes, installs Oracle,
accepts terms, or chooses unrecorded destructive targets. Database seeding and
Data Guard configuration are new, privileged mutation capabilities, enabled
only after explicit approval. External provisioning and ownership evidence
must exist before an adapter can use them.

## Fixed Kuberic API and ownership

The selected baseline remains
`28b181bb1846d057cce5913513a46ed72de4c977`, Public V1 protocol `11`, local agent
schema `12`. No Kuberic API change is proposed. The
[selected upstream evidence](observation-only-hosting-contract.md#selected-upstream-facts)
and immutable references below distinguish this baseline from a newer sibling
checkout.

The custom service supplies `Replicator` and `PrimaryReplicator`, but neither
`StateProvider` nor `StateReplicator`, following the PostgreSQL native-engine
pattern. Oracle owns redo generation, transport, recovery, and transaction
commit. The adapter owns native-operation coordination, evidence, journals,
and the mapping to existing callbacks. Kuberic owns role intent, identities,
epoch, history, PC/CC configuration, access decisions, and exact public
completion. An adapter must not create a second controller or election loop.

The pinned API explicitly defines primary progress as locally durable
quorum-committed progress, secondary progress as highest contiguous durably
accepted progress, and recovery progress as election-only with access closed
[K1]. PostgreSQL separately certifies primary commitment and observes standby
receive/replay positions [K3]. Copying only its callback names would miss
these semantics.

| Callback | Proposed Enterprise behavior |
|---|---|
| Open | Validate durable bindings, native lineage, storage, history, and journals; keep access closed until authority is admitted. |
| ChangeRole IdleSecondary | Admit an exact build target; no client access, vote, or completed progress before build. |
| BuildReplica | Execute the exact authorized source/target build and obtain a durable, attempt-bound receipt. |
| ChangeRole ActiveSecondary | Require the completed build or exact existing-member admission, compatible history, and healthy recovery; never create Oracle primary authority. |
| CurrentProgress | Return the role-specific certified LSN defined below. |
| CatchUpCapability | Return the earliest provably retained catch-up boundary, or `INVALID_LSN`. |
| UpdateCatchUpReplicaSetConfiguration | Stage exact PC/CC authority and native policy with writes fenced; reject unsupported quorum translations. |
| WaitForCatchUpQuorum | Wait for one frozen boundary under exact configuration/session bindings; distinguish WriteQuorum from All. |
| UpdateCurrentReplicaSetConfiguration | Activate only the proved configuration and acknowledgment policy, then retire obsolete authority. |
| ChangeRole Primary / UpdateEpoch | Validate exact role-transition authorization; an epoch alone never authorizes Oracle promotion. |
| OnDataLoss | Reconcile an authorized history change; never use a constant boolean as proof that Oracle data was preserved or discarded. |
| RemoveReplica | Revoke exact peer/build authority and destinations without violating the surviving configuration; never delete storage implicitly. |
| Close / Abort | Fence publication, revoke grants, and contain workers; do not infer native rollback from cancellation. |

Service and Replicator role callbacks must converge on one durable transition
record, not independently dispatch the same Oracle mutation. A Primary callback
without an authorized native transition may validate an already established
primary, but must not silently promote a standby. No Oracle client address is
published until a separately reviewed access/routing fence is integrated.

## Architecture

```text
Kuberic controller / local agent
    | existing Public V1 authority and lifecycle
    v
Oracle service + custom PrimaryReplicator
    |-- durable binding, progress certificates, operation journal
    |-- read-only, identity-bracketed native observer
    |-- separately authorized, process-isolated native executor
    |-- attempt-bound source/target build coordination
    `-- external access and old-primary fencing verification
                 |
                 v
Externally provisioned Oracle Enterprise databases
    primary ---- native redo transport ----> standby A
            `--- native redo transport ----> standby B
```

The proposed first mutation backend is brokerless whole-database Data Guard
with a bounded, allowlisted RMAN/SQL executor. This minimizes competing
authority surfaces for the initial physical build. It is a proposal requiring
the existing broker-choice safety gate, not a resolved approval.

Broker and Fast-Start Failover must not independently change roles. A future
broker backend would replace the mutation owner rather than run alongside
brokerless control. Standalone tools and out-of-band administrator changes
invalidate admission until reconciled.

## Identity, history, and durable records

Each observation/certificate binds:

- Kuberic resource UID, exact replica/storage incarnation, process session,
  `HistoryContext`, epoch, and configuration revision;
- Oracle database DBID, admitted current/parent incarnation and resetlogs
  identity, member `DB_UNIQUE_NAME`, activation evidence, and instance startup;
- external database allocation and storage identity, not just the adapter's
  Pod/PVC, listener name, DBID, or filesystem path;
- support/artifact fingerprint, whole-CDB/PDB coverage, redo-thread inventory,
  observer/executor generation, and sample times.

Physical standbys belong to the source database lineage but are distinct
members and allocations. RMAN standby creation must retain the required
source identity; ordinary independent-database duplication is not equivalent
[O6]. Per-member incarnation record numbers and activation observations must be
validated against documented lineage, not assumed identical by numeric value.

Persist three owner-only, no-follow, atomic record families:

| Record | Required contents |
|---|---|
| HA binding | All identity domains above; approved native policy; admitted history-to-lineage relationship; schema version. |
| Progress certificate | Mapping version; base recovery certificate; contiguous redo inventory; durable/apply frontiers; exact quorum evidence; configuration and history; evidence digest and observation bracket. |
| Operation journal | Public operation ID/revision, canonical input digest, build/transition attempt, source/target sessions and storage, authorization, immutable copy manifest, frozen boundaries, dispatch intent, native results, reconciliation and terminal receipt. |

A journal is not a replacement for Oracle redo. Persisting a larger number
does not make database bytes durable. Journals must be revalidated against
current storage and native recovery evidence after restart. A stale process
session cannot resume authority merely because it finds its old journal.

## Kuberic LSN contract

### Representation and comparability

**Proposed mapping version 1: certified closed-redo-prefix SCN.**

```text
LSN = checked_i64(certified inclusive SCN boundary)
```

The value measures a complete recoverable prefix of the admitted Oracle
database history. It is not an Oracle redo byte address, a transaction count,
or an encoding of identity. SCNs need not be consecutive transaction numbers.
Prefix commitment refers to replication of redo, not a claim that every SQL
transaction encountered in that prefix committed; Oracle recovery retains
transaction commit/rollback semantics.
No subtraction of a resource-specific origin, hashing, bit-packing of
incarnation, truncation, saturation, or fallback to a local counter is allowed.
Parse Oracle `NUMBER` exactly, reject fractional/negative values, and reject a
boundary above `i64::MAX`.

The numeric representation is permitted only after the native-prefix and
fixed-API conformance proofs below pass. Until then, the Enterprise adapter
must not advertise usable replication progress. This is a proposed,
evidence-backed mapping for a new Enterprise profile, not a change to the
current Free adapter's no-SCN-conversion contract.

Comparison requires the same admitted Kuberic history and compatible native
redo lineage. DBID alone is insufficient. A RESETLOGS/failover branch must go
through explicit history admission; a larger number on another branch cannot
win an election. Kuberic epochs are never derived from SCNs or resetlogs
numbers. A restart without history/storage change preserves the numeric
domain but renews the process session.

### Complete-prefix certificates

Version 1 deliberately certifies only completed archived-log boundaries.
Unarchived online or standby-redo tails remain separate native evidence.
This is coarser than real-time redo progress and can delay a build or an
election. It must not be represented as the latest acknowledged application
commit, and promotion must account for the uncertified tail.

Start from an admitted recovery base `B0`, proved by a validated database
recovery/checkpoint and its required backup/control-file/redo inventory. An
empty target has no such base. Never invent `B0=0`, use an image's DBID as a
base certificate, or require control-file history back to database creation.

For the single enabled redo thread, consider a completed archive with
exclusive upper boundary `NEXT_CHANGE#=N`. Its inclusive boundary is:

```text
B = N - 1
```

`FIRST_CHANGE#` and `NEXT_CHANGE#` describe the log interval [O2]. Extend a
certificate only through a validated sequence chain covering everything
needed after `B0`, in the same resetlogs lineage. Check actual interval
coverage as well as sequence continuity; sparse SCNs are not missing redo.
Require finite, exact interval values with `NEXT_CHANGE# > FIRST_CHANGE#`;
open-log sentinel values are not completed boundaries.
Stop at the first missing, contradictory, unvalidated, or unusable segment.
Do not select `MAX(NEXT_CHANGE#)` across the view.

The inventory must handle duplicate rows from multiple archive destinations,
restores, and copies. Deduplicate equivalent native log identities, retain
provenance, and reject conflicting metadata. A control-file row is not proof
that the file exists, is complete, or can be recovered. Cleared, unavailable,
deleted, expired, or placeholder records cannot extend a received prefix
without independently verified durable replacement evidence. Pin the needed
redo while an operation relies on it.

Define these distinct frontiers:

| Symbol | Meaning | Minimum proof |
|---|---|---|
| `L_p` | Primary's locally durable complete prefix | Valid recovery base plus complete, validated local archived-redo chain. |
| `D_i` | Member i's durably accepted complete prefix | Valid recovery base plus the complete local redo needed to recover through the boundary; not merely primary destination status or an RFS network acknowledgment. |
| `A_i` | Standby i's datafile-applied complete prefix | Same contiguous coverage with standby `REGISTRAR=RFS`, `APPLIED=YES` evidence, or an explicitly validated replacement recovery-base certificate. |
| `Q_C` | Quorum-certified prefix for configuration C | Local durability plus exact member certificates satisfying C's write predicate. |

Oracle documents `APPLIED=NO` as received but not applied,
`APPLIED=IN-MEMORY` as not yet reflected in datafiles, and `APPLIED=YES` as
applied with datafiles updated; these meanings apply to physical-standby rows
registered by RFS [O2]. Neither `IN-MEMORY` nor a local-primary `APPLIED` value
proves standby datafile durability.

`D_i` may exceed `A_i`: a secondary may possess durable redo it has not yet
applied. Retain that redo until recovery/checkpoint evidence replaces it.
The latest recovery-progress statistic is diagnostic unless its selected
version's crash-durability semantics are independently proved. No application
table query or Active Data Guard option is needed for this archived-log
certificate design; mounted-view privileges still require a live proof.

### Role-specific values

| Kuberic state | `current_progress()` | Additional rule |
|---|---|---|
| Current Oracle Free singleton | `0` | Existing empty adapter-owned history only; never claims Oracle replication. |
| Enterprise unbuilt IdleSecondary | `INVALID_LSN` (`-1`) | No completed build or electable progress; never substitute zero. |
| Enterprise built IdleSecondary | `D_i`, only after exact build receipt admission | Still non-voting until the controller changes role/configuration. |
| Enterprise ActiveSecondary | Highest certified `D_i` | Applied progress `A_i` is retained separately for recovery and join readiness. |
| Enterprise Primary | `Q_C`, or `min(Q_PC, Q_CC)` under admitted joint authority | Never local `CURRENT_SCN` or `L_p` alone. |
| Recovery / role None | Revalidated local `D_i` (or `L_p` for a former primary) | Election evidence only, with read/write access closed; does not certify quorum commitment or authorize promotion. |

An unknown/unavailable value and an observation error are different.
`INVALID_LSN` is allowed for genuinely unknown/not-applicable progress, but
never as completed build/catch-up progress [K2], [K4]. Query failures, stale
samples, privilege denial, identity changes, conflicting lineage, and a
regression below an already certified boundary must surface an error and
close admission, not become `-1`, zero, or cached success.

Progress is monotonic within its exact admitted history/storage/configuration
domain. A durable earlier certificate may survive restart only when its
bytes/recovery base still verify. Never mask storage rollback with
`max(last_reported, newly_observed)`. Before changing configuration or role,
establish the new role's required boundary at least through the retained
authoritative boundary; otherwise keep access closed and catch up or rebuild.
Mapping-version changes require explicit migration/history admission, not
mixed-version comparison.

### Primary quorum calculation

For an ordinary configuration `C` containing the local primary with write
quorum `w`, let its eligible standby certificates be ordered by decreasing
`D_i`. Only exact admitted member/storage/session/history bindings count.

```text
w = 1: Q_C = L_p
w > 1: Q_C = min(L_p, (w-1)-th largest eligible standby D_i)
```

With fewer than `w-1` valid standby certificates, there is no new quorum
certificate. Do not reduce the quorum, count an idle build target, or count an
unrelated synchronous destination. This formula describes a threshold quorum;
more specific predicates require their exact member-set proof. During joint
authority both PC and CC predicates must hold, not a majority of their union.

Example, all values in the same admitted history:

```text
C = {P, S1, S2}, write quorum = 2
L_p = 1500
D_1 = 1400, A_1 = 1200
D_2 = 1100, A_2 = 1000

P.current_progress()  = 1400
S1.current_progress() = 1400
S2.current_progress() = 1100
```

S1 is not yet ready for an apply-required join boundary of 1400. If the
primary's `CURRENT_SCN` is 1800, reporting 1800 would overclaim the observed
quorum prefix. A candidate reporting 1600 on a conflicting history is
ineligible, not a better candidate.

### Catch-up capability

`catch_up_capability()` is a lower retention boundary, not latest progress.
Return the earliest inclusive public boundary `R` from which an admitted peer
can advance using a complete pinned redo chain through the required target.
If the oldest retained log begins at SCN `F`, a possible predecessor boundary
is `F-1`, but only after proving complete lineage-compatible coverage.
`MIN(FIRST_CHANGE#)` alone is insufficient.

Report `INVALID_LSN` when that capability is unknown, not implemented, expired,
or requires a new base copy. Prove peer-specific recovery compatibility again
at catch-up time. A scalar retention boundary cannot attest to every possible
peer's base files or replace a frozen build manifest.

## Oracle commit policy and Kuberic quorum

The quorum calculation above proves historical prefix durability. It does
**not** by itself make externally connected Oracle client commits obey
Kuberic's write predicate. Closing the adapter's service address is also
insufficient if clients can still write directly to Oracle.

The proposed first writable HA configuration is exactly three admitted voters
with write quorum two, `MAXIMUM PROTECTION`, and `SYNC AFFIRM` to the eligible
standbys. Oracle maximum protection stops the primary instead of continuing
unprotected when no qualifying standby can acknowledge; `AFFIRM` requires
standby-redo-log persistence [O3], [O4]. Verify the exact qualifying destination
set and native behavior before claiming this implements local-plus-one-standby
durability.

Require system-level `COMMIT_WAIT=FORCE_WAIT`, including the selected CDB/PDB
scope, and deny uncoordinated changes. Oracle documents that a system
`FORCE_WAIT` ignores session/transaction overrides, and changing the parameter
can invalidate that guarantee [O5]. The lab must prove synchronous commit
behavior under session overrides, destination failures, reconnects, and
native restarts.

Reject these shortcuts:

- `MAXIMUM AVAILABILITY` as an unconditional quorum guarantee: it may continue
  without a synchronized standby.
- `SYNC NOAFFIRM` as disk durability: acknowledgment need not wait for disk.
- `MANDATORY` or `LOG_ARCHIVE_MIN_SUCCEED_DEST` as a general per-commit quorum.
- lag zero, destination `VALID`, or an empty gap view as commitment.
- a synchronous peer outside the admitted configuration as a quorum voter.

Data Guard does not automatically implement arbitrary PC/CC quorums. The first
version must reject unsupported writable configurations. During bootstrap,
join, replacement, or configuration change, keep external writes verifiably
fenced until the supported native and Kuberic policies agree. No polling loop
can close a window in which Oracle already acknowledged an unprotected write.
Reconfiguration that cannot enforce both old and new write predicates must
remain write-closed; it must not return success and temporarily fall back to
asynchronous operation.

## Replica build and HA join

### Authority and frozen copy

Build is invoked on the primary for an exact `ReplicaInformation`. The idle
descriptor carries `INVALID_LSN` for both progress fields and `must_catchup`
false [K1]. It is not the carrier for invented copy-boundary progress.

Bind the source and target to the existing Public V1 build authority:
operation ID/revision, build ID, epoch, resource, history, configuration,
replica/storage identities, process sessions, and target address [K4].
Adapter-private source/target coordination must validate this authority at
every dispatch and publication point, like the native PostgreSQL pattern
[K3]. It may not introduce an alternate lifecycle or controller endpoint.
An address or `DB_UNIQUE_NAME` match alone is not authorization.

Use an immutable backup-based RMAN standby build for the first design.
Freeze a validated backup/control-file manifest and retained redo rather than
rerunning a moving active duplicate under the same build ID. The manifest
records piece identities/digests, file inventory and destination mapping,
encryption/key references, source lineage, checkpoint/recovery requirements,
and exclusive/inclusive SCN conventions. Never use ordinary duplicate/open
RESETLOGS to produce an unrelated database and call it a joined standby.

Choose `B_copy` only after proving the frozen backup can recover to that
boundary without requiring later, absent redo. Online/fuzzy backup files
cannot be treated as a point-in-time snapshot merely by sampling an SCN.
For the initial lab, quiesce/fence application writes during seed capture;
capture and validate the backup and requisite archives, certify `B_copy`,
then freeze the manifest. Oracle/RMAN command syntax and `UNTIL SCN` boundary
semantics must be version-verified before implementation; no operational
recipe is authorized by this design.

### State machine

```text
authorized -> target-claimed -> manifest-frozen -> copy-dispatched
           -> standby-mounted -> transport-configured -> recovery-running
           -> copy-boundary-applied -> catch-up-boundary-applied
           -> receipt-durable -> built-idle
           -> configuration-admitted -> active-secondary
```

1. **Admit the attempt.** Check authorization, exact current source authority,
   target session/storage, Enterprise approval, encrypted channels, space, and
   source retention. Validate the target is empty or an exact resumable
   attempt. Conflicting existing data requires replacement, not overwrite.
2. **Freeze the source.** Journal dispatch intent, establish the immutable
   manifest and quorum-certified `B_copy`, and pin backup/redo retention.
   The source must already have an admitted configuration. Bootstrap may use
   explicitly authorized singleton authority with external application writes
   closed; do not claim HA quorum while constructing its first standbys.
3. **Seed only claimed storage.** Execute the approved RMAN standby restore/
   duplicate into the exact externally allocated target roots. Validate file
   paths and prevent source/target aliasing or primary-file reuse. RMAN exit
   zero is execution evidence, not completed build.
4. **Mount and attach.** Prove source-compatible database lineage and distinct
   member/allocation identity, intended PDB coverage, standby control file,
   standby redo-log suitability, transport identity, and managed recovery.
   No client access is granted to the target.
5. **Recover the frozen copy.** Require target `D_t >= B_copy` and
   `A_t >= B_copy`, including every required log interval. Validate datafiles
   and the recoverable base before admitting a numeric public prefix.
6. **Catch up to one later boundary.** Freeze and journal `B_join >= B_copy`
   from fresh source quorum-certified evidence, with required redo retained.
   Require `D_t >= B_join` and `A_t >= B_join`. Do not chase an indefinitely
   moving primary head, and do not lower the boundary on timeout.
7. **Persist and acknowledge.** Revalidate source/target authority, history,
   sessions, configuration, recovery, and storage; atomically persist the
   terminal target receipt and bind the source completion to it.
8. **Join the replica set.** Build success leaves a built IdleSecondary.
   Only controller-issued role/configuration operations admit ActiveSecondary
   membership. Apply-required catch-up, PC/CC proofs, exact native policy,
   and fencing precede voting/write-access grants.

The existing inbound receipt requires nonnegative boundaries and
`durable_lsn >= catch_up_boundary_lsn >= replication_boundary_lsn` [K4].
Populate them with certified target durability, `B_join`, and `B_copy`;
retain applied-frontier and native evidence alongside the receipt. The
adapter's success condition is stronger: target apply must also cover
`B_join`. `final_sequence` is the validated build protocol's terminal sequence,
not Oracle's redo sequence and not a fabricated completion token.
Receipt publication through the pinned host admission path must be proved;
writing an adapter-local JSON receipt alone is insufficient.

Replaying the same operation resumes/reconciles the frozen manifest and exact
attempt. A changed digest, target storage, source history, or superseded
session cannot reuse a terminal result. A lost response after native success
is ambiguous until the journal and target evidence establish the same
operation's postconditions. A new process session requires fresh authorized
admission; it does not silently inherit an old transport grant.
If the source loses the frozen copy or required retained history, follow the
pinned source-history-loss/replacement semantics [K5]; do not generate a new
backup under the same attempt or misclassify source loss as target corruption.

Cancellation fences future dispatch and public success, bounds/contains owned
workers, and records reconciliation-needed state. It does not automatically
delete target files, undo Oracle recovery, or prove that no mutation occurred.
Cleanup may remove only exact claimed resources under explicit retirement
authority; member removal never implies volume deletion.

### Joining an existing physical standby

An explicitly authorized import may skip seed transfer, not admission.
The target must already be the intended physical standby in the admitted
lineage, with externally verified allocation ownership and no conflicting
controller, broker, recovery, or writer authority. A matching DBID/name,
listener, or high SCN is insufficient.

Freeze an existing-base manifest and recovery certificate, retain the redo
required to reach `B_copy` and `B_join`, then execute the same receive/apply,
receipt, built-idle, and controller-driven membership steps. Record that the
copy representation is an admitted existing base, not bytes transferred by
this attempt. Verify that this import representation satisfies pinned build
admission before enabling it. Missing provenance or incompatible history
requires a new-storage build, not an automatic adoption fallback.

## Catch-up, configuration changes, and replacement

For `WaitForCatchUpQuorum`, freeze a complete source-local boundary `B_wait`
and the exact PC/CC/member-session set for that attempt. This may be ahead of
current `Q_C`: the purpose is to wait for the quorum to reach it. Produce the
archive boundary through a separately authorized executor, not by hiding a
log switch inside a read-only observation callback.

- **WriteQuorum:** require locally durable redo and both admitted write
  predicates through `B_wait`.
- **All:** require every member specified by the admitted catch-up contract
  to durably receive and apply through `B_wait`, not just a write quorum.
- Preserve `must_catchup` constraints; do not advertise specific-quorum support
  until the exact selected API semantics and native proof are implemented.

The call fails on changed history, configuration, storage, session,
authorization, or deadline. A lag metric, observed peer report, or a high
unbound SCN does not complete it.

Configuration changes first revoke external writes and drain/fence existing
writers. Stage native destinations and exact peer grants while preserving old
authority and retention. Catch up the new set through a boundary covering the
previous authoritative progress, prove PC and CC predicates, then atomically
admit CC and retire PC/grants. Grant writes only for a supported native policy.
An unsupported live joint policy stays write-closed or is rejected.

For replacement, retire the old allocation and build to a new storage
incarnation. A stopped or stale member cannot rejoin merely because its SCN is
higher. Reuse of an existing allocation requires explicit proof of compatible
recovery state; divergence requires reinstate or rebuild. Removal must not
silently reduce the quorum or leave an old destination able to acknowledge
outside current authority.

## HA transitions and data-loss boundaries

### Planned switchover

Revoke external writes, drain transactions, and capture the old primary's
final recoverable redo boundary, including its formerly unarchived tail.
Require target receive/apply through that boundary and bystander compatibility.
Then execute the selected native switchover under one operation journal.
Verify the old primary is a standby, the target is the intended primary,
native protection/transport policy is valid, and exact Kuberic role/history/
configuration authority agrees before granting new write access.

Do not assume every role exchange creates RESETLOGS. Retain before/after
Oracle lineage and establish the admitted relation explicitly [O7].

### Unplanned failover

Required order:

```text
access-revoked -> old-primary-fence-verified -> candidate/history-admitted
              -> data-loss-policy-authorized -> recovery-complete
              -> promotion-dispatched -> postconditions-verified
              -> new-write-access-granted
```

A missing/partitioned primary, revoked Kuberic role, or failed health check is
not fencing. Fencing evidence must independently prevent the exact old
database allocation from serving writes, including direct client connections,
and bind the candidate and attempt. Keep fencing effective through recovery,
promotion, routing, and reconciliation.

Kuberic may use comparable `D_i` values as recovery/election evidence, but
numeric ranking is only one predicate. Recover the chosen candidate's durable
redo tail and validate apply/corruption state before promotion. A higher
received prefix is not an immediately writable database.

In particular, maximum protection can acknowledge commits held in a standby
redo log that has not yet become a certified archive. Therefore `Q_C` is a
proved lower boundary, **not** proof that all acknowledged writes stop there.
To claim zero data loss, recover/prove the relevant synchronized redo tail and
the complete acknowledged-commit boundary for the old authority. If that
boundary is unknown, or a required voter holding redo is unavailable, zero
loss is unproved even when a candidate has the largest reported LSN.
Keep promotion blocked unless a separately authorized lossy recovery policy
explicitly accepts the uncertainty; never relabel an unknown loss range as
zero.

Retain failover-specific native incarnation/resetlogs evidence and authorize
the Kuberic history change separately. Reconcile bystanders and fence/retire
incompatible members before admitting their reports. Reinstate requires
proved flashback/history conditions; otherwise use a new-storage rebuild.
`OnDataLoss` results must describe the authorized application/history effect,
not mere detection that an Oracle failover happened.

Automatic failover and automatic client routing remain disabled in the first
build/join implementation.

### Exact outcomes

Every build/join and role-transition result uses the existing
[operation-completion envelope](data-guard-readiness.md#exact-operation-completion-matrix).
Report `succeeded` only for every attempt-bound postcondition with no
unreconciled terminal error. Report `failed` only for an attributable error
and evidence that success effects did not occur. Lost acknowledgments, mixed
success/error evidence, unknown native effects, or uncertain fencing remain
`ambiguous`. A cancellation request is not a verified `cancelled` outcome.
Incompatible retained history/storage is `rebuild-required`.
These are reconciliation categories, not new Public V1 result variants.

## Native observation and security contract

| Evidence | Use | Rejected shortcut |
|---|---|---|
| `V$DATABASE`, `V$DATABASE_INCARNATION`, `V$INSTANCE` | Bracket database/member lineage, role, logging, startup, and scope. | `CURRENT_SCN` equals committed or durable accepted progress. |
| `V$ARCHIVED_LOG` plus validated file/recovery inventory | Contiguous durable and applied archive prefixes. | Maximum sequence/SCN or metadata row alone proves recoverability. |
| `V$ARCHIVE_DEST_STATUS`, destination parameters | Exact transport policy, destination identity, errors and recovery mode. | Healthy destination proves exact target process/storage or quorum. |
| `V$DATAGUARD_PROCESS`, `V$DATAGUARD_STATS` | Recovery/transport health and freshness diagnostics. | Running MRP or zero lag means build or catch-up completed. |
| `V$ARCHIVE_GAP` | Current blocking recovery gap [O8]. | Empty rows prove all required redo is present. |
| RMAN manifest, validation and recovery results | Frozen seed files and required redo through copy/join boundaries. | Successful command proves target history and public completion. |

Oracle documents nullable/not-open and standby-checkpoint semantics for
`CURRENT_SCN` [O1]. Do not replace null with zero or use it as a flush LSN.
Every view has a selected-version row-count, scope, duplicate, null, missing,
privilege, freshness, and unknown-enum contract. Sample across views with
before/after identity and authority checks; persist provenance and reject
cross-generation mixtures.

Observation is read-only and cannot invoke RMAN, switch logs, alter
destinations, start recovery, or change roles. Use separate observation,
RMAN/backup, Data Guard mutation, and external fencing credentials with a
tested deny matrix. If mounted fixed-view access requires an administrative
connection mode, prove its minimum privilege or isolate a narrowly scoped
privileged helper; never silently give the observer `SYSDBA`.

TCPS/authenticated encryption and peer-name verification cover database
observation, RMAN/redo transport, peer build coordination, authorization, and
fencing. Native local IPC is acceptable only with an independently reviewed
same-host identity/permission boundary, not as a plaintext network escape.
The pinned Kuberic transport must also be confined/protected by reviewed
deployment controls; no claim of built-in transport authentication is added.
No secrets appear in argv, journals, connection descriptors in logs, receipts,
or diagnostics. Retain only secret references and sanitized typed failures.

## Validation and activation gates

The proposed mapping and mutation paths require live, version-specific proof.
These are implementation requirements, not claims established by this document.

| Area | Required evidence |
|---|---|
| Native prefix semantics | Exact inclusive/exclusive archive boundaries, single-thread coverage, duplicate/conflicting rows, cleared/missing files, resetlogs filtering, fuzzy-backup base recovery, numeric overflow, and archived versus active-tail behavior. |
| Crash durability | Crash between receive, archive registration, disk persistence, in-memory apply, datafile update, and certificate persistence; no advertised prefix may survive without reconstructable bytes/base state. |
| Fixed-API progress | Coarse closed-log progress satisfies selected Public V1 conformance; role-specific receive/apply/commit distinctions, monotonic transitions, `INVALID_LSN`, history mismatch, restart and rollback. If coarse certification is incompatible, activation stops rather than fabricating a finer position. |
| Quorum and commits | Exact three-voter/quorum-two native policy, FORCE_WAIT/override behavior, both standbys lost, one lost, unauthorized destination, protection downgrade, and PC/CC predicates; acknowledged writes must never bypass the admitted policy. |
| Build/join | Same immutable manifest on retry, deterministic logical copy boundary, source retention loss, wrong source/target/session/storage, target aliasing, interrupted RMAN/recovery, delayed apply, frozen join boundary, and real host receipt admission. |
| Reconfiguration | No write window between policies, old-session rejection, non-voting idle targets, required All/WriteQuorum behavior, replacement storage retirement and delayed stale completion. |
| HA safety | Network-partitioned but writable old primary, unavailable synchronized redo holder, stale/unknown commit boundary, corrupt tail, lossy versus zero-loss authorization, switchover role exchange, failover history change and incompatible bystanders. |
| Security/ownership | Privilege deny matrix, mounted observation, TLS trust/name/redirect failures, credential redaction, bounded workers, explicit authority and exact-owned cleanup. |

All existing [readiness stop conditions](data-guard-readiness.md#stop-conditions)
remain in force. Closure requires durable reviewable records for the exact
artifact/version, authority, test procedure/results, validity, and reopen
conditions. A design document, PAW artifact, passing Free test, or mocked
Enterprise snapshot is not a closure record.

## Implementation sequence

1. Approve the exact Enterprise lab artifact/terms, brokerless design, secure
   channels, external allocation/access/fencing boundaries, and native
   single-thread/whole-CDB scope.
2. Prove mounted observation and the closed-redo-prefix mapping with durable
   certificates and crash tests. Keep production mutation disabled.
3. Implement the bounded native executor and immutable RMAN build journal;
   prove real Public V1 build receipts and built-idle admission.
4. Implement secondary join, exact native commit policy, catch-up, and
   write-closed configuration/replacement. Demonstrate three-member behavior
   before enabling the supported writable configuration.
5. Separately review and implement planned switchover, then explicit failover/
   reinstate with independent fencing and data-loss authorization. Automatic
   failover/routing requires another review.

The existing Oracle Free adapter remains unchanged throughout. Enterprise
history, journals, capabilities, and storage bindings must never be inferred
from or silently migrated out of its empty-history singleton binding.

## Sources

Oracle references describe the cited 26 release documentation, not approval
of a particular downloadable artifact. Reverify against the selected binary
and capture evidence before activation. Kuberic links are immutable.

- [K1] Pinned Replicator contract, quorum modes, and idle target semantics.
- [K2] Pinned `Lsn=i64` and `INVALID_LSN` contract.
- [K3] Pinned PostgreSQL Public V1 native progress, catch-up, and build.
- [K4] Pinned Public V1 build authority and inbound receipt validation.
- [K5] Pinned SF-compatible lifecycle, configuration, build, and history rules.
- [O1] Oracle `V$DATABASE`, including `CURRENT_SCN`.
- [O2] Oracle `V$ARCHIVED_LOG`, interval, duplicate-row, and apply semantics.
- [O3] Oracle Data Guard protection modes.
- [O4] Oracle redo transport, `SYNC`, `AFFIRM`, and destination identity.
- [O5] Oracle `COMMIT_WAIT`.
- [O6] Oracle RMAN duplication and physical standby creation.
- [O7] Oracle Data Guard role transitions.
- [O8] Oracle `V$ARCHIVE_GAP`.

[K1]: https://github.com/youyuanwu/kuberic/blob/28b181bb1846d057cce5913513a46ed72de4c977/kuberic-runtime/src/replicator/mod.rs#L126-L275
[K2]: https://github.com/youyuanwu/kuberic/blob/28b181bb1846d057cce5913513a46ed72de4c977/kuberic-runtime/src/application.rs#L19-L24
[K3]: https://github.com/youyuanwu/kuberic/blob/28b181bb1846d057cce5913513a46ed72de4c977/examples/postgres/src/public_v1.rs#L1183-L1535
[K4]: https://github.com/youyuanwu/kuberic/blob/28b181bb1846d057cce5913513a46ed72de4c977/kuberic-runtime/src/protocol/public_operations.rs#L625-L708
[K5]: https://github.com/youyuanwu/kuberic/blob/28b181bb1846d057cce5913513a46ed72de4c977/docs/features/kuberic/service-fabric-api-semantics.md
[O1]: https://docs.oracle.com/en/database/oracle/oracle-database/26/refrn/V-DATABASE.html
[O2]: https://docs.oracle.com/en/database/oracle/oracle-database/26/refrn/V-ARCHIVED_LOG.html
[O3]: https://docs.oracle.com/en/database/oracle/oracle-database/26/sbydb/oracle-data-guard-protection-modes.html
[O4]: https://docs.oracle.com/en/database/oracle/oracle-database/26/sbydb/oracle-data-guard-redo-transport-services.html
[O5]: https://docs.oracle.com/en/database/oracle/oracle-database/26/refrn/COMMIT_WAIT.html
[O6]: https://docs.oracle.com/en/database/oracle/oracle-database/26/sbydb/creating-oracle-data-guard-physical-standby.html
[O7]: https://docs.oracle.com/en/database/oracle/oracle-database/26/sbydb/managing-oracle-data-guard-role-transitions.html
[O8]: https://docs.oracle.com/en/database/oracle/oracle-database/26/refrn/V-ARCHIVE_GAP.html
