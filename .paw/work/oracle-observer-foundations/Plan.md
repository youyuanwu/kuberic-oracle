# Oracle Observer Foundations Plan

## Approach

Implement the server-free portion of Stage 1 without selecting an Oracle
driver or requiring Oracle client libraries. The crate will expose strict
configuration and raw-row parsing contracts that a later live observer can
populate from fixed queries.

The implementation will preserve Oracle-native uncertainty, reject unsupported
values instead of coercing them, and verify that database and instance identity
remain stable across a bracketed observation.

## Key Decisions

- Represent `CURRENT_SCN` as `Option<DecimalScn>` because Oracle may return
  `NULL` for a mounted physical standby.
- Replace the Boolean force-logging field with an explicit
  `ForceLogging::{Enabled, Disabled}` domain. The parser will reject Oracle
  standby-nologging states until the contract deliberately models them.
- Retain parsed product/version evidence in `OracleObservation`.
- Accept raw query values through driver-independent row structures. No Oracle
  driver, network connection, CLI, or container fixture is part of this work.
- Keep polling cadence separate from connect, query, whole-snapshot, and
  shutdown deadlines. All durations must be nonzero, and operations bounded by
  the whole-snapshot deadline must not exceed it.
- Model principal placement, connection container, `CONTAINER_DATA` policy, and
  exact expected row counts explicitly so a future live observer can validate
  multitenant scope before admitting a snapshot.
- Treat configured edition as provisional input to the server-free parser.
  Independent live edition provenance remains required before Stage 1 is
  complete.

## Work Items

1. **Harden native snapshot contracts**
   - Make `DatabaseSnapshot.current_scn` and `Admission.current_scn` nullable.
   - Introduce explicit force-logging and product snapshot types.
   - Add classified errors needed by configuration and parsing.

2. **Add observer configuration**
   - Define bounded credential/trust file references.
   - Define multitenant principal, connection, and row-scope configuration.
   - Define separate timeout and polling configuration with strict validation.

3. **Add bracketed raw-row parsing**
   - Define driver-independent raw database, instance, product, and identity
     rows.
   - Parse exact supported Oracle values and reject unknown or unmodeled values.
   - Preserve nullable SCN values.
   - Verify pre/post database and instance identity.
   - Retain product/version evidence in the resulting observation.

4. **Expand server-free contract tests**
   - Cover valid standalone parsing.
   - Cover mounted standby `NULL` SCN.
   - Cover `LOG_MODE=MANUAL`, standby-nologging force-logging states, malformed
     values, identity changes, timeout ordering, polling independence, secret
     paths, and multitenant scope.
   - Update existing admission tests for the strengthened types.

5. **Verify and review**
   - Run formatting, strict Clippy, workspace tests, and documentation tests.
   - Review the complete diff against the plan and design safety requirements.
   - Apply any required fixes before creating the final pull request.

## Completion Criteria

- No Oracle server, container, Oracle client, or new third-party dependency is
  required.
- Unsupported native values fail with classified errors and are never coerced.
- A mounted standby can carry `CURRENT_SCN = NULL` without a fabricated value.
- Bracket identity changes are rejected.
- Observer configuration cannot express zero or improperly ordered deadlines,
  missing credential/trust paths, or an unbounded/ambiguous row scope.
- Existing Stage 0 behavior remains intact except for the deliberate stronger
  nullable-SCN and force-logging types.
- Repository formatting, Clippy, and all tests pass.
