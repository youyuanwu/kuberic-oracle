# Kuberic Oracle Progress Integration

## Purpose

This document defines the current boundary between Oracle native progress and
Kuberic's fixed Service Fabric-compatible replication contract.

The repository does not currently publish Oracle progress to Kuberic. It
defines a snapshot contract and raw-row parser, and a test-only PoC has
populated those contracts from one real Oracle Free member. Repository test
configuration explicitly authorizes that default live proof; the server-free
override disables it. This evidence does
not turn any Oracle number into complete replication or promotion authority.

## Standalone Free profile

Oracle AI Database Free is initially supported as one standalone `PRIMARY`
database opened `READ WRITE`. It is observe-only and non-electable: the
repository exposes no promotion, role/configuration, catch-up, build/copy,
data-loss, or client-routing operation for this profile.

The snapshot contract has fields for:

- `CURRENT_SCN`;
- `RESETLOGS_CHANGE#`;
- `ACTIVATION#`;
- current `V$DATABASE_INCARNATION` record ID;
- resetlogs ID, change, time, prior-incarnation evidence, status, and
  whole-database `CON_ID=0` scope;
- DBID;
- `DB_UNIQUE_NAME`;
- database and instance names;
- database role and open mode;
- instance startup time; and
- Oracle product version and edition.

The server-free parser accepts rows corresponding to the fixed identity,
current-incarnation, database, instance, and product queries. It preserves a
`NULL` `CURRENT_SCN` for a mounted standby and rejects bracket identity or
lineage changes and unmodeled native logging values. The test-only PoC executes
that bracket and independently reads `V$INSTANCE.EDITION`, native version 23,
and the Oracle AI Database 26ai Free product/release evidence. Production still
has no live observer.

When supplied from a trusted future observer, these fields can support
database identity, current Oracle incarnation/resetlogs lineage, freshness,
startup-time restart evidence, and health validation. They do not represent
Kuberic history context, Kuberic replica incarnation, process/storage
incarnation, immutable service authority, or replicated durability.

The pinned Oracle Free image seeds a fresh owned volume with the same DBID and
current-incarnation tuple. The live proof keeps changed container/volume
identity separate and records the result as an indistinguishable clone; it does
not convert fixture replacement into an Oracle-native rejection claim.

The standalone profile therefore must not claim:

- secondary progress;
- catch-up capability;
- quorum durability;
- promotion eligibility;
- build or copy capability;
- data-loss recovery;
- configuration mutation;
- client-service routing;
- switchover; or
- failover.

## Why `CURRENT_SCN` is not enough

An Oracle system change number orders database changes within compatible native
lineage. It is useful evidence, but a single SCN does not answer all questions
required by the Kuberic replication contract.

In particular:

- primary current SCN and standby applied SCN describe different positions;
- received redo may not be archived;
- archived redo may not be applied;
- applied redo may belong to an incompatible resetlogs incarnation;
- transport and apply can have gaps;
- a standby can be open read-only with or without managed recovery;
- a candidate can be reachable but unsafe to promote;
- protection mode and destination requirements affect acknowledged durability;
- broker configuration and runtime health are separate from SCN ordering; and
- SCN evidence does not fence the old primary.

Hashing, truncating, saturating, or otherwise forcing a compound native state
into Kuberic's scalar progress would hide safety-relevant distinctions.

## Data Guard progress domains

A future physical Data Guard adapter must keep at least these domains separate.

### Database lineage

- DBID;
- database incarnation;
- resetlogs change number and time;
- activation number;
- database unique name; and
- primary/standby relationship.

### Redo generation and transport

- primary current SCN;
- current and archived redo sequence;
- destination status and error;
- transmitted/received redo evidence;
- transport lag; and
- unresolved archive gaps.

### Recovery and apply

- standby applied SCN;
- applied redo sequence;
- managed recovery process state;
- real-time apply state;
- apply lag;
- recovery mode; and
- apply or corruption errors.

### Configuration authority

- native database role;
- open mode;
- protection mode and level;
- switchover/failover readiness;
- broker configuration identity and status when broker-managed;
- enabled destinations and required acknowledgements; and
- exact Kuberic epoch, replica incarnation, and process session.

These values are related but not interchangeable.

The complete source, freshness, allowed-claim, and forbidden-inference rules
are in the
[Data Guard native evidence matrix](data-guard-readiness.md#native-evidence-matrix).
The
[identity-domain matrix](data-guard-readiness.md#identity-domain-matrix)
defines the non-substitution rules for Oracle, Kuberic, and external
deployment/storage identities.

## Selected Kuberic baseline and initial behavior

The selected upstream baseline is commit
`28b181bb1846d057cce5913513a46ed72de4c977`. Public V1 protocol `11` and local
agent schema `12` are the sole selected production contract; the legacy and
preview selectors and fallback are removed or rejected
(`28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/protocol/public_operations.rs:12-29`;
`28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/README.md:52-58`;
`28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/README.md:68-75`;
`28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/testing/public_v1_conformance.rs:500-525`).

The selected API is SF-shaped: every hosted `StatefulServiceReplica` returns a
`Replicator`, and the public surface includes role, epoch, progress,
configuration, catch-up, build/remove, data-loss authorization, replay, and
exact completion semantics
(`28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/application.rs:72-80`;
`28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/replicator/mod.rs:184-343`;
`28b181bb1846d057cce5913513a46ed72de4c977:docs/features/kuberic/service-fabric-api-semantics.md:44-83`).
Successful role or lifecycle callbacks cannot be observational no-ops: public
completion is semantic evidence that the exact operation-specific
postcondition was established.

PostgreSQL is the selected custom-Replicator reference. It supplies neither a
`StateProvider` nor a `StateReplicator`, but it still returns an active
service-owned `PgReplicator`; it is not an observe-only adapter
(`28b181bb1846d057cce5913513a46ed72de4c977:examples/postgres/src/service.rs:109-165`;
`28b181bb1846d057cce5913513a46ed72de4c977:examples/postgres/src/service.rs:249-291`;
`28b181bb1846d057cce5913513a46ed72de4c977:docs/features/kuberic/replicator-boundary.md:79-89`).

Oracle Free therefore still lacks a truthful hosted surface. It must not
pretend to implement replication callbacks, mutate Oracle, or fabricate
unsupported capabilities. The generic
[observation-only hosting contract](observation-only-hosting-contract.md)
proposes exactly Open, Observe, Close, and Abort with explicit capability
absence. That proposal does not exist at the selected upstream commit.

The
[operation-completion matrix](data-guard-readiness.md#exact-operation-completion-matrix)
separates Oracle-native state from repository request/attempt binding and
defaults unattributable outcomes to ambiguous.

The observation-only proposal maps Oracle standalone behavior without
implementing Replicator-shaped placeholders:

| Proposed observation-only behavior | Oracle standalone response |
|---|---|
| Open | Bind resource, replica, process-session, storage-incarnation, and configured Oracle target identities |
| Observe | Require fresh bounded native identity, lineage, `PRIMARY`, and `READ WRITE` evidence |
| Close | Stop new observations, resolve accepted attempts, and seal publication |
| Abort | Seal publication, cancel in-flight work, and remain idempotently terminal |
| Replication/election/progress/configuration/build/remove/data-loss | Capability absent; no callback exists |
| Addresses | Publish neither a client service address nor a replication endpoint |

Failure must be explicit. Zero, last-known values, endpoint reachability, or a
healthy listener must not be returned as success-shaped substitutes.

## Future Data Guard admission

The current Rust contract admits a preliminary physical Data Guard snapshot
only when:

- the repository profile is `EnterpriseDeveloper` or `Enterprise`, with an
  Enterprise-capable artifact whose exact terms are separately approved;
- the configured topology contains exactly three replicas as this repository's
  supported laboratory invariant, not an Oracle Data Guard minimum;
- role is `PRIMARY` or `PHYSICAL STANDBY`;
- the primary is `READ WRITE`;
- the current caller-populated Rust contract accepts a physical standby as
  `MOUNTED`, `READ ONLY`, or `READ ONLY WITH APPLY`;
- the database uses `ARCHIVELOG`; and
- `FORCE LOGGING` is enabled.

Live `MOUNTED` observation remains unresolved because `CURRENT_SCN` may be
`NULL` while the database is not open. Before that state is supported by a live
parser, the snapshot must preserve nullability, use a justified alternate
native source, or exclude `MOUNTED`; zero and last-known substitutes are
prohibited.

`READ ONLY WITH APPLY` is only a native-state admission value here. It does not
establish Active Data Guard entitlement for the selected image, option, or
CDB/PDB use; that remains Stage 4 research.

The current logging representation is also a subset. A live parser must reject
or explicitly model `LOG_MODE=MANUAL`, standby-nologging `FORCE_LOGGING`
states, and every other unmodeled native value rather than coercing them into a
supported value.

This admission is intentionally insufficient for Kuberic progress or
promotion. It establishes only that the snapshot belongs to the broad native
profile that later research will evaluate.

## Promotion authority

No SCN threshold alone grants promotion authority.

A future promotion operation must bind:

- a durable Kuberic operation ID and epoch;
- exact candidate and old-primary database identities;
- exact Kuberic process sessions;
- compatible lineage;
- a declared data-loss policy;
- required transport/apply/gap evidence;
- an old-primary fence receipt;
- native transition commands and their complete results; and
- post-transition role, open-mode, incarnation, and routing evidence.

For failover, independently verifiable fencing proof must be authenticated,
bound to the exact old-primary and external deployment/storage identities plus
the operation attempt, and validated **before** any promotion command is
dispatched. The proof remains part of the final completion evidence; a fencing
plan or post-dispatch receipt is not sufficient authorization.

If Kuberic's fixed progress API cannot carry or reliably reference this evidence,
Oracle Data Guard must remain an external observe-only engine rather than
receiving an unsafe partial integration.

## Research status

The readiness phase classifies the previous open questions without converting
partial evidence into implementation authority:

| Question | Status | Remaining blocker |
|---|---|---|
| Stable least-privilege primary/standby views | Partially answered | Oracle 26 sources identify candidate views, but least-privilege grants and the selected lab version require verification. |
| Comparability across restart and role/lineage transitions | Partially answered | Identity and incarnation domains are defined; operation-specific lineage and causality require lab proof. |
| Scalar Kuberic progress | Answered: no direct scalar mapping | SCN and compound Data Guard evidence must not be converted to Kuberic LSN/history. A future evidence-reference contract remains upstream work. |
| Gaps and real-time apply without sampling races | Partially answered | Native sources are cataloged, but multi-view identity bracketing, freshness, expected no-row behavior, and broker/view disagreement require implementation proof. |
| Broker-managed versus brokerless mutation | Blocked | Tradeoffs are documented; selection requires a separate safety review. |
| Old-primary fencing | Blocked | No independently verifiable fencing mechanism is selected. |
| Active Data Guard licensing | Blocked | `READ ONLY WITH APPLY` remains a native admission value only; entitlement for the selected artifact and use is unsourced. |
| Three-member licensed lab | Blocked | Exact 23-or-later artifact, terms, platform, registry access, and topology authorization are unverified. |

The full [stop-condition list](data-guard-readiness.md#stop-conditions) is
normative for future implementation. Until those conditions pass, Oracle
progress remains native observation evidence only.
