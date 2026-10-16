# Oracle Free Custom Replicator Contract

> **Status:** Implemented as the optional `kuberic` feature in the production
> crate. The implementation supplies a custom singleton Kuberic Replicator
> around an injected observation source. It adds no production Oracle driver,
> runtime executable, plaintext production connection, container authority,
> Oracle mutation, election, failover, or Data Guard behavior.

## Selected upstream facts

The independently reviewable upstream baseline is `youyuanwu/kuberic` commit
`28b181bb1846d057cce5913513a46ed72de4c977`. The Cargo dependency is pinned to
that exact revision. Citations use the immutable form
`<commit>:<path>:<line-range>`.

| Selected fact | Immutable upstream evidence |
|---|---|
| Public V1 protocol `11` and local agent schema `12` are the selected production contract. | `28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/protocol/public_operations.rs:12-29`; `28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/README.md:52-58` |
| The built-in path is the sole selected built-in Public V1 path; legacy/preview selectors and fallback are removed or rejected. | `28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/README.md:34-36`; `28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/README.md:68-75`; `28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/testing/public_v1_conformance.rs:500-525`; `28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/testing/public_v1_conformance.rs:663-672` |
| Every hosted `StatefulServiceReplica::open` returns a `Replicator`; custom implementations may provide a `PrimaryReplicator` while omitting `StateProvider` and `StateReplicator`. | `28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/application.rs:72-107`; `28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/replicator/mod.rs:128-168`; `28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/replicator/mod.rs:609-688`; `28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/replicator/mod.rs:955-961` |
| Zero is a valid empty-history boundary; `INVALID_LSN = -1` means progress is unknown or inapplicable and cannot be reported as completed progress. | `28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/application.rs:20-24`; `28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/protocol/public_operations.rs:2154-2195` |
| PostgreSQL is the selected application-owned custom-Replicator reference. It returns a service-owned Replicator and supplies neither a `StateProvider` nor a `StateReplicator`. | `28b181bb1846d057cce5913513a46ed72de4c977:examples/postgres/src/service.rs:109-165`; `28b181bb1846d057cce5913513a46ed72de4c977:examples/postgres/src/service.rs:249-291`; `28b181bb1846d057cce5913513a46ed72de4c977:docs/features/kuberic/replicator-boundary.md:79-89` |
| Public V1 conformance subject selection recognizes only `postgresql` and `built-in-replacement`. Oracle therefore uses repository-owned contract tests rather than claiming registration in the upstream generic suite. | `28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/testing/public_v1_conformance.rs:915-1002`; `28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/tests/public_v1_conformance.rs:1-12` |
| Kuberic remains experimental and source-only, without published production images, a supported installation, or a production activation procedure. | `28b181bb1846d057cce5913513a46ed72de4c977:README.md:3-5`; `28b181bb1846d057cce5913513a46ed72de4c977:docs/features/kuberic/stateless-default-replicator.md:160-170` |

## Implemented boundary

The adapter treats Oracle Free as an external observed database and the
Kuberic Replicator as a singleton host-lifecycle contract. The Replicator does
not claim that Oracle Free performs native replication.

The implementation has four components:

- `OracleObservationSource` supplies one fresh `OracleObservation`. No
  production implementation is included.
- `OracleService` implements `StatefulServiceReplica`, validates resource
  identity, owns terminal lifecycle, and returns no client service address.
- `OracleReplicatorFactory` binds exact replica and process-session identity,
  rejects a supplied `StateProvider`, and returns a conservative primary
  interface bundle without a `StateReplicator`.
- `OracleReplicator` validates every supported callback against fresh Oracle
  admission within the configured observation timeout and rejects unsupported
  replication and authority operations.

The optional `kuberic` feature is disabled by default. Enabling it adds the
exact Kuberic source dependency and adapter types, but it does not add an
Oracle driver or executable.

## Callback contract

| Public V1 callback | Oracle Free behavior |
|---|---|
| Application Open | Validate Kuberic resource identity, allocate one service-owned custom Replicator, and bind the exact Open replica identity. |
| Factory creation | Bind the exact replica and process-session identities plus the configured Kuberic Replicator address; reject a `StateProvider`. |
| Replicator Open | Enter the open generation and return the configured Kuberic Replicator address. This is not an Oracle listener, client address, or Data Guard endpoint. |
| Primary role | Require a fresh admitted Oracle Free observation for the configured DBID, `DB_UNIQUE_NAME`, incarnation, edition, version, `PRIMARY` role, and `READ WRITE` mode. |
| None role | Revoke published application role without changing Oracle. |
| Active/idle secondary | Reject explicitly; Oracle Free is not a Kuberic secondary or build target. |
| Current configuration | Accept only one exact local Primary member, write quorum one, and zero remote replica descriptions. Exact replay is allowed; replacement is rejected. |
| Current progress | Return `0` only after fresh Oracle admission. Zero represents empty adapter-owned Kuberic history, not Oracle SCN or redo progress. |
| Catch-up capability | Return `INVALID_LSN` after fresh admission because peer catch-up is inapplicable. |
| Epoch update | Admit only the already-bound epoch; reject an epoch change that could grant new authority. |
| Catch-up configuration/quorum | Reject explicitly. |
| Data loss | Reject explicitly rather than certifying Oracle data-loss safety. |
| Build/remove | Reject explicitly. |
| Close/Abort | Advance the terminal generation so an in-flight observation cannot publish late success. |

Callbacks never issue SQL mutation. The injected source may obtain native
evidence only through the fixed read-only observation contract.

## Progress domains

The adapter has an empty Kuberic application-history domain. Its completed
progress is therefore zero.

Oracle-native values remain separate evidence:

- `CURRENT_SCN`;
- resetlogs change and resetlogs ID;
- activation number;
- database incarnation;
- DBID and `DB_UNIQUE_NAME`; and
- instance startup identity.

None is cast, truncated, hashed, saturated, or otherwise converted into
Kuberic LSN, epoch, history, quorum, catch-up, or promotion authority.
`INVALID_LSN` is used only for the inapplicable catch-up capability; it is not
reported as completed progress.

## Identity and configuration binding

Application Open binds the Kuberic resource and exact replica identity.
Factory creation independently binds the admitted process session and requires
the same replica identity and Replicator address. A changed identity, session,
or address fails closed.

Current configuration admission requires:

- exactly one member;
- the member is the locally bound exact replica identity;
- the member role is `Primary`;
- the descriptor primary ID is the local replica ID;
- write quorum is one; and
- there are no remote peer descriptions.

The host retains its own storage-incarnation and Public V1 history domains.
Oracle DBID, incarnation, startup identity, deployment identity, and storage
identity remain separate and cannot substitute for one another.

## Freshness and lifecycle fencing

Primary role, epoch, progress, catch-up-capability, and current-configuration
callbacks obtain a new injected observation and run the existing
`admit_observation` checks. Failed, stale, future-dated, wrong-identity,
wrong-incarnation, wrong-edition, wrong-version, standby, or non-`READ WRITE`
evidence rejects the callback. The adapter wraps acquisition in its own
nonzero timeout; a stalled source fails as a deadline error even if the source
does not enforce its own bound.

Every asynchronous callback captures the current open generation. Close or
Abort changes that generation before terminal completion. A delayed
observation may finish its external work, but it cannot publish callback
success after the generation changes.

## Addresses and publication

`RoleChange.service_address` is always `None`; the adapter publishes no Oracle
client endpoint and no Oracle client service address.

Public V1 nevertheless requires `Replicator::open` to return the configured
Kuberic Replicator address. For this singleton profile it is host metadata,
not an Oracle address and not evidence of a replication listener. Any
configuration containing a remote peer is rejected before catch-up, build, or
peer communication can be admitted.

## Production and test boundary

Production code now contains the feature-gated adapter and runtime-host
library. The runtime layer provides:

- validated resource, replica, Pod, PVC, root, endpoint, token, deadline, and
  transport configuration;
- a versioned durable application binding for the exact Oracle support profile,
  immutable image, DBID, `DB_UNIQUE_NAME`, incarnation, logical replica, and
  canonical application root;
- Kuberic named application-path binding and exact local endpoint resolution;
- binding initialization for `OpenMode::New` and exact validation for
  `OpenMode::Existing`;
- bounded `ReplicaHost` startup cancellation, shutdown, wait, application
  close, and abort containment; and
- no-follow, owner-only, atomic application-binding persistence.

The server-free proof initializes a real agent, executes Open, Primary, epoch,
singleton configuration, progress, and Close, restarts from the same metadata
and application roots, observes a new process session, and rejects a command
from the superseded session. It also rejects changed Oracle target/support
binding and a moved application path.

Production still has:

- no Oracle driver;
- no runtime executable;
- no credential reader;
- no live connection path;
- no container or package authority; and
- no plaintext production mode.

The existing `oracledb` dependency and plaintext connection remain in the test
crate and are restricted to the fixture-owned loopback worker. The test-only
worker source drives the same real host lifecycle inside the existing live
fixture, waits for worker idleness, and stops the host before fixture cleanup.
A future production observation source requires a separately reviewed
non-plaintext transport and deployment design.

## Data Guard separation

This singleton adapter keeps Oracle Free mutation-free and Data Guard-free. It
does not make Oracle Free electable, replicated, or Data Guard capable and
supplies no secondary, catch-up, build, removal, data-loss, promotion,
switchover, failover, reinstate, or routing behavior.

A future Enterprise Data Guard Replicator remains a separate implementation
subject to the complete readiness, licensing, transport, fencing, identity,
and exact-completion gates in
[the Data Guard readiness contract](data-guard-readiness.md).
