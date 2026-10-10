# Kuberic Oracle Progress Integration

## Purpose

This document defines the current boundary between Oracle native progress and
Kuberic's fixed Service Fabric-compatible replication contract.

The repository does not currently publish Oracle progress to Kuberic. It
defines a snapshot contract and server-free raw-row parser that can carry exact
native values supplied by a caller, so that a future integration can be
evaluated without prematurely treating one Oracle number as complete
replication or promotion authority.

## Standalone Free profile

Oracle AI Database Free is initially supported as one standalone `PRIMARY`
database opened `READ WRITE`.

The snapshot contract has fields for:

- `CURRENT_SCN`;
- `RESETLOGS_CHANGE#`;
- `ACTIVATION#`;
- DBID;
- `DB_UNIQUE_NAME`;
- database and instance names;
- database role and open mode;
- instance startup time; and
- Oracle product version and edition.

The server-free parser accepts caller-provided rows corresponding to the fixed
database, instance, product, and identity queries. It preserves a `NULL`
`CURRENT_SCN` for a mounted standby and rejects bracket identity changes and
unmodeled native logging values. No live observer currently executes those
queries. Product/version provenance therefore remains unproven until Oracle
supplies the parsed rows, and edition remains provisional configured input
until a documented live source is independently derived and cross-checked.

When supplied from a trusted future observer, these fields can support
identity, freshness, startup-time restart evidence, and health validation. They
do not represent a complete Oracle database incarnation, Kuberic replica
incarnation, process session, immutable resource binding, or replicated
durability.

The standalone profile therefore must not claim:

- secondary progress;
- catch-up capability;
- quorum durability;
- promotion eligibility;
- build or copy capability;
- data-loss recovery;
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

## Initial Kuberic behavior

When the Kuberic runtime is added, callbacks may validate fresh native identity
and role evidence. They must not mutate Oracle or fabricate unsupported
capabilities.

The left column is behavioral shorthand, not a promise of public Kuberic
methods with those exact names. Stage 3 must bind each behavior to the actual
lifecycle, role, progress, configuration, build/remove, data-loss, and returned
service-address contracts.

The initial mapping is:

| Kuberic behavior | Oracle standalone response |
|---|---|
| Open application | Connect and validate configured target |
| Validate primary role | Require native `PRIMARY` and `READ WRITE` |
| Validate secondary role | Reject as unsupported |
| Current progress | Reject until a safe contract is defined |
| Catch-up capability | Reject as unsupported |
| Build/remove replica | Reject as unsupported |
| Previous/current configuration | Reject mutation |
| Data-loss callback | Reject as unsupported |
| Write access | Do not publish a client service address |
| Switchover/failover | Reject as unsupported |

Failure must be explicit. Zero, last-known values, endpoint reachability, or a
healthy listener must not be returned as success-shaped substitutes.

## Future Data Guard admission

The current Rust contract admits a preliminary physical Data Guard snapshot
only when:

- edition is Enterprise Developer or Enterprise;
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

If Kuberic's fixed progress API cannot carry or reliably reference this evidence,
Oracle Data Guard must remain an external observe-only engine rather than
receiving an unsafe partial integration.

## Open research questions

Before publishing progress, the project must answer:

1. Which exact dynamic performance views provide stable, least-privileged
   primary and standby positions for supported Oracle versions?
2. Which values remain comparable across restart, switchover, failover,
   reinstatement, duplicate, flashback, and resetlogs?
3. Is one scalar configuration value available and safe for Kuberic's progress
   API, or is a durable evidence reference required?
4. How are archive gaps and real-time apply represented without race-prone
   multi-view assumptions?
5. Is broker-managed Data Guard required for a supported mutation protocol?
6. What old-primary fencing mechanism is independently verifiable?
7. Which Active Data Guard observations require separately licensed features?
8. Can a three-member laboratory exercise the complete protocol under the
   applicable development license?

Until these questions are answered and reviewed, Oracle progress remains native
observation evidence only.
