# Observation-Only Hosting Contract

> **Status:** kuberic-oracle proposal only. The selected Kuberic Public V1
> baseline does not contain this hosting interface. No Oracle runtime adapter,
> Kuberic dependency, transport, credential, TLS, election, mutation, or Data
> Guard implementation is added by this document.

## Selected upstream facts

The independently reviewable upstream baseline for this document is
`youyuanwu/kuberic` commit
`28b181bb1846d057cce5913513a46ed72de4c977`. Citations use the immutable form
`<commit>:<path>:<line-range>`.

| Selected fact | Immutable upstream evidence |
|---|---|
| Public V1 protocol `11` and local agent schema `12` are the selected production contract. | `28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/protocol/public_operations.rs:12-29`; `28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/README.md:52-58` |
| The built-in path is the sole Public V1 path; legacy/preview engines, selectors, and fallback are removed or rejected. | `28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/README.md:34-36`; `28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/README.md:68-75`; `28b181bb1846d057cce5913513a46ed72de4c977:docs/features/kuberic/stateless-default-replicator.md:5-8`; `28b181bb1846d057cce5913513a46ed72de4c977:docs/features/kuberic/stateless-default-replicator.md:16-23`; `28b181bb1846d057cce5913513a46ed72de4c977:docs/features/kuberic/replicator-boundary.md:17-20`; `28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/testing/public_v1_conformance.rs:500-525`; `28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/testing/public_v1_conformance.rs:663-672` |
| The public SF-shaped application surface includes `StatefulServiceReplica`, `StateProvider`, `Replicator`, `PrimaryReplicator`, optional `StateReplicator`, and `ReplicatorFactory`; the runtime carries `ReplicatorInterfaces` and immutable `ReplicatorCapabilities`. | `28b181bb1846d057cce5913513a46ed72de4c977:docs/features/kuberic/replicator-boundary.md:6-15`; `28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/application.rs:72-107`; `28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/replicator/mod.rs:128-168`; `28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/replicator/mod.rs:184-343`; `28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/replicator/mod.rs:955-961` |
| Public identities and semantics include `PublicV1Identity`, `Epoch`, `ReplicaRole`, `ReplicaIdentity`, `ReplicaStorageIncarnationContext`, `ProcessSessionEndpoint`, `HistoryContext`, `INVALID_LSN`, configuration and replica-set shapes, role/epoch/progress/configuration/build/remove callbacks, data-loss authorization, replay classification, and exact operation completion. | `28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/application.rs:20-24`; `28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/protocol/types.rs:142-208`; `28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/protocol/types.rs:241-338`; `28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/protocol/types.rs:340-406`; `28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/protocol/public_operations.rs:12-29`; `28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/protocol/public_operations.rs:339-417`; `28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/protocol/public_operations.rs:420-500`; `28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/protocol/public_operations.rs:775-796`; `28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/protocol/public_operations.rs:992-1055`; `28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/protocol/public_operations.rs:2154-2195`; `28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/protocol/public_operations.rs:2200-2247`; `28b181bb1846d057cce5913513a46ed72de4c977:docs/features/kuberic/service-fabric-api-semantics.md:11-29`; `28b181bb1846d057cce5913513a46ed72de4c977:docs/features/kuberic/service-fabric-api-semantics.md:44-83` |
| `StatefulServiceReplica::open` must return a `Replicator`; completion certifies operation-specific postconditions rather than transport acknowledgement. There is no truthful observation-only hosted interface. | `28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/application.rs:72-80`; `28b181bb1846d057cce5913513a46ed72de4c977:docs/features/kuberic/replicator-boundary.md:45-59`; `28b181bb1846d057cce5913513a46ed72de4c977:docs/features/kuberic/service-fabric-api-semantics.md:13-30`; `28b181bb1846d057cce5913513a46ed72de4c977:docs/features/kuberic/service-fabric-api-semantics.md:44-67` |
| PostgreSQL is the native custom-Replicator reference. It returns a service-owned `PgReplicator`, supplies neither a `StateProvider` nor a `StateReplicator`, and owns its replication/copy state. It is an active replication implementation, not an observation-only adapter. | `28b181bb1846d057cce5913513a46ed72de4c977:docs/features/postgres/design.md:21-28`; `28b181bb1846d057cce5913513a46ed72de4c977:examples/postgres/src/service.rs:109-165`; `28b181bb1846d057cce5913513a46ed72de4c977:examples/postgres/src/service.rs:249-250`; `28b181bb1846d057cce5913513a46ed72de4c977:examples/postgres/src/service.rs:260-291`; `28b181bb1846d057cce5913513a46ed72de4c977:docs/features/kuberic/replicator-boundary.md:79-89` |
| Public V1 conformance recognizes only `postgresql` and `built-in-replacement`; literal selector and scenario matches provide no normal registration path for a third custom subject. | `28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/testing/public_v1_conformance.rs:915-1002`; `28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/tests/public_v1_conformance.rs:1-12` |
| The built-in Public V1 replication path has no credential contract. Custom Replicators may own their own transport and storage formats; the PostgreSQL example's credentials and transport are PostgreSQL-specific. | `28b181bb1846d057cce5913513a46ed72de4c977:docs/features/kuberic/replicator-boundary.md:70-82`; `28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/README.md:73-83`; `28b181bb1846d057cce5913513a46ed72de4c977:docs/features/kuberic/stateless-default-replicator.md:129-137`; `28b181bb1846d057cce5913513a46ed72de4c977:docs/features/postgres/design.md:286-296`; `28b181bb1846d057cce5913513a46ed72de4c977:examples/postgres/src/public_transport.rs:23-50`; `28b181bb1846d057cce5913513a46ed72de4c977:examples/postgres/src/public_transport.rs:64-125`; `28b181bb1846d057cce5913513a46ed72de4c977:examples/postgres/src/public_transport.rs:185-193` |
| Kuberic remains experimental and source-only, without published production images, a supported installation, or a production activation/traffic-movement procedure. | `28b181bb1846d057cce5913513a46ed72de4c977:README.md:3-5`; `28b181bb1846d057cce5913513a46ed72de4c977:docs/features/kuberic/stateless-default-replicator.md:160-170` |

These are selected upstream facts. The remainder of this document is a
kuberic-oracle proposal and must not be read as an API that exists in Kuberic.

## Proposed generic hosting boundary

The proposal is for external engines that can report trustworthy bounded
observations but cannot truthfully implement replication, election, or
configuration authority. Oracle Free is the motivating consumer, not a
special-case API. The same boundary must be usable by any external
observe-only engine.

### Lifecycle

The complete lifecycle contains exactly four operations:

1. **Open** — bind one hosted observation session to the complete identity
   tuple and immutable capabilities. Open returns an opaque in-process session
   handle, not a service or replication address.
2. **Observe** — execute one fresh, bounded observation and publish explicit
   health plus evidence through exact completion.
3. **Close** — reject new observations, resolve every accepted observation,
   release session resources, and complete only after no result from that
   session can still be published.
4. **Abort** — immediately make the session terminal, cancel or abandon
   in-flight work, and prevent every later result from being published.
   Repeated Abort calls are idempotent.

There are no role, epoch, progress, configuration, build, catch-up, remove,
data-loss, election, promotion, switchover, failover, or replication
callbacks. Unsupported behavior is absent from the interface rather than
represented by a callback that returns success.

### Identity binding

Open binds four Kuberic hosting identities as independent fields:

- resource identity;
- logical replica identity;
- process-session identity; and
- storage-incarnation identity.

Every Observe request and completion repeats the exact tuple. A completion is
stale and unusable when any field differs from the currently open session.
The storage-incarnation identity changes on replacement or re-creation even
when the deployment name or engine-native identity appears unchanged.

Engine-native and external identities remain separate evidence domains:

- Oracle database identity: DBID, database name, and `DB_UNIQUE_NAME`;
- Oracle lineage identity: incarnation and resetlogs evidence;
- Oracle instance identity: instance name, host, and startup time;
- Oracle observer identity: the process/connection/sample attempt producing
  evidence;
- deployment identity: Pod, container, VM, or other allocation identity; and
- external storage identity: PVC, volume, disk, or equivalent allocation.

No Oracle, Kuberic, observer, deployment, or storage identity may substitute
for another. A stable DBID does not prove stable storage; an Oracle startup
time is not a Kuberic process session; a deployment name is not a replica or
database identity.

### Bounded observation and publication

An Observe request contains:

- a caller-generated observation request ID;
- the bound resource, replica, process-session, and storage-incarnation tuple;
- an absolute completion deadline;
- a maximum acceptable evidence age; and
- the expected engine target identity.

One completion contains:

- the same request and identity tuple;
- observer attempt identity;
- started, observed, and completed timestamps;
- explicit engine health and evidence health;
- separate `deadline_met` and `maximum_age_met` decisions;
- the exact engine-native identity and lineage snapshot;
- an evidence digest or durable reference; and
- one exact disposition: `succeeded`, `failed`, `cancelled`, or `ambiguous`.

`succeeded` is valid only when the evidence belongs to the bound identities,
both bounds are met, publication wins all cancellation/Close/Abort races, and
the complete evidence payload is committed atomically. A deadline miss or
over-age sample publishes the failed bound and cannot reuse cached success.

`failed` requires an attributable terminal fault and proof that no success was
published. `cancelled` requires proof that cancellation won before
publication. `ambiguous` is mandatory when the host cannot prove whether a
result was published; ambiguous evidence cannot update health, readiness, or
authority state.

### Cancellation, restart, replacement, and faults

- **Cancellation:** request-scoped cancellation is monotonic. A completion
  arriving after cancellation may succeed only when publication was already
  atomically committed before cancellation won; otherwise it is cancelled or
  ambiguous, never late success.
- **Process restart:** every Open uses a new process-session identity.
  Results from an earlier process session are stale even if all other
  identities match.
- **Stale session:** after Close or Abort completes, the host rejects all new
  requests and discards every late completion from that session.
- **Storage replacement:** a changed storage-incarnation identity invalidates
  prior evidence. Matching database or deployment names do not override that
  invalidation.
- **Fault:** a session fault stops new observations and makes unresolved
  attempts failed or ambiguous according to publication evidence. Fault
  handling cannot convert partial work into success.
- **Close race:** Close completes only after every accepted request has an
  exact terminal disposition and the publication channel is sealed.
- **Abort race:** Abort seals publication first, then cancels and releases
  work. It does not wait for engine cleanup to claim observation success.

### Addresses and capability absence

Open publishes neither a client service address nor a replication endpoint.
An implementation may use a private in-process control handle, but that handle
is not routable application or peer-replication metadata.

Capabilities are immutable for the Open session and declare only bounded
observation and health/evidence publication. The following capabilities are
explicitly absent:

- role and epoch management;
- generic LSN or progress reporting;
- previous/current replica-set configuration;
- catch-up capability or quorum waiting;
- build, copy, remove, or replacement authority;
- data-loss callbacks or authorization;
- election, promotion, switchover, or failover;
- client routing; and
- replication transport.

Absence is not encoded as `INVALID_LSN`, zero, last-known progress, an empty
endpoint, a healthy listener, or a successful no-op. No Oracle Free adapter may
implement a fake `Replicator` to enter this hosting path.

## Minimal proposed upstream changes

This section is proposal-only. It does not claim that any item exists at the
selected Kuberic commit.

1. **Add a separate generic observation-only hosted interface.** Reuse the
   selected resource, replica, process-session, and storage-incarnation
   identity types, but return an observation session rather than a
   `Replicator`. The only operations are Open, Observe, Close, and Abort.
2. **Add an immutable hosting-capability variant.** Represent replicated and
   observation-only hosting as distinct variants. The observation-only variant
   exposes bounded observation and health/evidence publication while making
   replication, authority, address, and progress capabilities structurally
   absent.
3. **Make conformance subjects registrable.** Replace the literal
   `postgresql` / `built-in-replacement` selection with a subject descriptor
   registry and add generic observation-only scenarios for identity binding,
   freshness, cancellation, restart, storage replacement, fault, capability
   absence, and exact completion.

The proposal does not change Public V1 replication semantics, add a built-in
credential contract, choose transport or TLS, define Oracle-specific types,
map SCN to LSN, or add Data Guard behavior. A custom active Replicator remains
free to own its transport; an observation-only subject has no replication
transport at all.

## Oracle Free application

Oracle Free may use this proposal only as one standalone, observe-only,
non-electable, mutation-free, non-replicating, Data Guard-free subject. It may
publish bounded native health and identity/lineage evidence. It may not
publish Kuberic progress, accept replication or authority callbacks, expose a
client service address or replication endpoint, or become Data Guard capable.

The selected Public V1 API is therefore a sufficient architectural base for a
future full custom Data Guard Replicator, but it is not the truthful hosting
surface for Oracle Free. Data Guard implementation remains separately blocked
by the readiness contract.
