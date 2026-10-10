# Copilot Instructions - Kuberic Oracle

This repository contains the Oracle database observation and integration
contracts for Kuberic.

The initial supported profile is observe-only Oracle AI Database Free running
in an externally provisioned, digest-pinned container. Do not add database
deployment, Data Guard mutation, promotion, switchover, or license-acceptance
behavior to production code without an explicit design and safety review.

Run `just check` for routine changes. Live Oracle container tests, when added,
must remain ignored by default and must clean up only resources recorded in an
exact ownership journal.
