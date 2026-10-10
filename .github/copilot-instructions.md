# Copilot Instructions - Kuberic Oracle

This repository contains the Oracle database observation and integration
contracts for Kuberic.

The initial supported profile is observe-only Oracle AI Database Free running
in an externally provisioned, digest-pinned container. Do not add database
deployment, Data Guard mutation, promotion, switchover, or license-acceptance
behavior to production code without an explicit design and safety review.

Run `just check` for routine changes. The live Oracle PoC is an ordinary,
authorization-gated Rust test; GitHub Actions enables it in the default
validation job. It is Linux x86-64/test-only and may use plaintext only through
its owned loopback fixture. Production must not expose plaintext. Live cleanup
may remove only resources recorded and revalidated through the exact ownership
journal.
