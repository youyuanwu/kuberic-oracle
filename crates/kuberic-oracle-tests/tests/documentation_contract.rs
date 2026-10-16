use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

const SELECTED_KUBERIC_COMMIT: &str = "28b181bb1846d057cce5913513a46ed72de4c977";
const DOCUMENTS: [&str; 5] = [
    "README.md",
    "docs/data-guard-readiness.md",
    "docs/design.md",
    "docs/kuberic-progress.md",
    "docs/observation-only-hosting-contract.md",
];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: &str) -> String {
    fs::read_to_string(workspace_root().join(path)).unwrap()
}

#[test]
fn readiness_contract_has_required_sections_and_integrations() {
    let readiness = read("docs/data-guard-readiness.md");
    for heading in [
        "## Enterprise lab feasibility",
        "## Broker-managed and brokerless tradeoffs",
        "## Native evidence matrix",
        "## Exact operation-completion matrix",
        "## Identity-domain matrix",
        "## Selected Kuberic Public V1 fit/no-fit",
        "## Stop conditions",
    ] {
        assert!(readiness.contains(heading), "{heading}");
    }

    assert!(read("README.md").contains("(docs/data-guard-readiness.md)"));
    assert!(read("docs/design.md").contains("(data-guard-readiness.md"));
    assert!(
        read("docs/kuberic-progress.md")
            .contains("(data-guard-readiness.md#identity-domain-matrix)")
    );
    assert!(
        read("README.md")
            .contains("(docs/observation-only-hosting-contract.md#selected-upstream-facts)")
    );
    assert!(
        read("docs/design.md")
            .contains("(observation-only-hosting-contract.md#selected-upstream-facts)")
    );
    assert!(
        read("docs/kuberic-progress.md")
            .contains("(observation-only-hosting-contract.md#selected-upstream-facts)")
    );
}

#[test]
fn selected_public_v1_baseline_is_pinned_with_immutable_evidence() {
    let readiness = read("docs/data-guard-readiness.md");
    for evidence in [
        "target_identity_must_match",
        "target_incarnation_must_match_even_when_dbid_matches",
    ] {
        assert!(readiness.contains(evidence), "{evidence}");
    }

    let required_citations = [
        (
            "README.md",
            "28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/protocol/public_operations.rs:12-29",
        ),
        (
            "README.md",
            "28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/README.md:52-58",
        ),
        (
            "README.md",
            "28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/README.md:34-36",
        ),
        (
            "docs/design.md",
            "28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/application.rs:72-80",
        ),
        (
            "docs/kuberic-progress.md",
            "28b181bb1846d057cce5913513a46ed72de4c977:examples/postgres/src/service.rs:249-291",
        ),
        (
            "docs/data-guard-readiness.md",
            "28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/testing/public_v1_conformance.rs:915-1002",
        ),
        (
            "docs/observation-only-hosting-contract.md",
            "28b181bb1846d057cce5913513a46ed72de4c977:docs/features/kuberic/stateless-default-replicator.md:160-170",
        ),
        (
            "docs/observation-only-hosting-contract.md",
            "28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/application.rs:72-107",
        ),
        (
            "docs/observation-only-hosting-contract.md",
            "28b181bb1846d057cce5913513a46ed72de4c977:kuberic-runtime/src/replicator/mod.rs:955-961",
        ),
    ];
    for (path, citation) in required_citations {
        assert!(read(path).contains(citation), "{path}: {citation}");
    }

    for path in DOCUMENTS {
        let content = normalize_whitespace(&read(path));
        assert!(
            content.contains(SELECTED_KUBERIC_COMMIT),
            "{path} must cite the selected baseline"
        );
        assert!(
            content.contains("protocol `11`") || content.contains("Protocol `11`"),
            "{path} must identify Public V1 protocol 11"
        );
        assert!(
            content.contains("schema `12`"),
            "{path} must identify local agent schema 12"
        );
    }
}

#[test]
fn stale_preview_baselines_cannot_return() {
    let forbidden = [
        "5c202ec53104e536568d664dafadfb05f89bdd1a",
        "b0e96aa2a4dc3872b55f2485cdd51b9f0d5ead20",
        "protocol-11/schema-8",
        "production protocol 9/schema 6",
        "production remains protocol 9/schema 6",
        "Dormant Kuberic Public V1",
        "Phase 5's future public V1",
        "has not selected a replacement Replicator",
    ];

    for path in DOCUMENTS {
        let content = read(path);
        for stale in forbidden {
            assert!(!content.contains(stale), "{path}: {stale}");
        }
    }
}

#[test]
fn custom_singleton_contract_is_bounded_and_has_no_oracle_replication_claims() {
    let contract = normalize_whitespace(&read("docs/observation-only-hosting-contract.md"));
    for required in [
        "Implemented as the optional `kuberic` feature",
        "`OracleObservationSource`",
        "`OracleService`",
        "`OracleReplicatorFactory`",
        "`OracleReplicator`",
        "one exact local Primary member",
        "write quorum one",
        "zero remote replica descriptions",
        "empty adapter-owned Kuberic history",
        "`INVALID_LSN`",
        "Catch-up configuration/quorum",
        "Data loss",
        "Build/remove",
        "Close/Abort",
        "cannot publish callback success after the generation changes",
        "`RoleChange.service_address` is always `None`",
        "not an Oracle listener",
        "no Oracle driver",
        "no runtime executable",
        "no plaintext production mode",
        "versioned durable application binding",
        "initializes a real agent",
        "observes a new process session",
        "fresh worker-produced evidence",
        "does not make Oracle Free electable, replicated, or Data Guard capable",
    ] {
        assert!(contract.contains(required), "{required}");
    }

    for forbidden_claim in [
        "converted into Kuberic LSN",
        "Oracle Free performs native replication",
        "no Oracle client service address",
        "Data Guard endpoint",
    ] {
        assert!(contract.contains(forbidden_claim), "{forbidden_claim}");
    }
}

#[test]
fn oracle_free_boundary_and_data_guard_blockers_remain_explicit() {
    let per_document = [
        (
            "README.md",
            [
                "standalone, observe-only, non-electable",
                "optional Public V1 custom singleton Replicator",
                "runtime-host library",
            ],
        ),
        (
            "docs/design.md",
            [
                "an optional exact-revision Kuberic dependency",
                "progress zero as the adapter's empty Kuberic history",
                "a real server-free initialization/lifecycle/restart proof",
            ],
        ),
        (
            "docs/kuberic-progress.md",
            [
                "Return zero for empty adapter-owned Kuberic history",
                "Publish no Oracle client service address",
                "a command fenced to the old session is rejected",
            ],
        ),
        (
            "docs/observation-only-hosting-contract.md",
            ["mutation-free", "Data Guard-free", "runtime-host library"],
        ),
    ];
    for (path, required) in per_document {
        let content = normalize_whitespace(&read(path));
        for value in required {
            assert!(content.contains(value), "{path}: {value}");
        }
    }

    let readiness = normalize_whitespace(&read("docs/data-guard-readiness.md"));
    for required in [
        "the selected SF-aligned custom Replicator API is a sufficient architectural base, but Data Guard implementation remains blocked",
        "Enterprise artifact and licensing approval",
        "selected version evidence",
        "authorized one-primary/two-standby topology",
        "least-privilege principal and channel design",
        "broker selection",
        "independent fencing",
        "identity separation",
        "native exact-completion proof",
        "no production runtime executable, non-plaintext Oracle observation source, external Oracle deployment/storage evidence binding, or upstream Oracle conformance subject exists",
    ] {
        assert!(
            readiness.contains(required),
            "docs/data-guard-readiness.md: {required}"
        );
    }
}

fn normalize_whitespace(content: &str) -> String {
    content.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn relative_markdown_links_resolve() {
    let root = workspace_root();
    let documents = DOCUMENTS.map(|path| root.join(path));

    for document in documents {
        let content = fs::read_to_string(&document).unwrap();
        for target in markdown_link_targets(&content) {
            if target.contains("://") || target.starts_with("mailto:") {
                continue;
            }

            let (path, anchor) = target
                .split_once('#')
                .map_or((target.as_str(), None), |(path, anchor)| {
                    (path, Some(anchor))
                });
            let resolved = if path.is_empty() {
                document.clone()
            } else {
                document.parent().unwrap().join(path)
            };
            assert!(
                resolved.exists(),
                "{} references missing {}",
                document.display(),
                resolved.display()
            );

            if let Some(anchor) = anchor.filter(|_| {
                resolved
                    .extension()
                    .is_some_and(|extension| extension == "md")
            }) {
                let anchors = markdown_heading_anchors(&fs::read_to_string(&resolved).unwrap());
                assert!(
                    anchors.contains(anchor),
                    "{} references missing heading #{anchor} in {}",
                    document.display(),
                    resolved.display()
                );
            }
        }
    }
}

#[test]
fn duplicate_headings_receive_github_style_suffixes() {
    let anchors = markdown_heading_anchors("# Section\n\n## Section\n");
    assert!(anchors.contains("section"));
    assert!(anchors.contains("section-1"));
}

fn markdown_link_targets(content: &str) -> Vec<String> {
    Parser::new_ext(content, Options::all())
        .filter_map(|event| match event {
            Event::Start(Tag::Link { dest_url, .. }) => Some(dest_url.into_string()),
            _ => None,
        })
        .collect()
}

fn markdown_heading_anchors(content: &str) -> HashSet<String> {
    let mut anchors = HashSet::new();
    let mut occurrences = HashMap::new();
    let mut heading = None;

    for event in Parser::new_ext(content, Options::all()) {
        match event {
            Event::Start(Tag::Heading { .. }) => heading = Some(String::new()),
            Event::Text(text) | Event::Code(text) => {
                if let Some(heading) = &mut heading {
                    heading.push_str(&text);
                }
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some(heading) = heading.take() {
                    let base = github_heading_anchor(&heading);
                    let occurrence = occurrences.entry(base.clone()).or_insert(0usize);
                    let anchor = if *occurrence == 0 {
                        base
                    } else {
                        format!("{base}-{occurrence}")
                    };
                    *occurrence += 1;
                    anchors.insert(anchor);
                }
            }
            _ => {}
        }
    }

    anchors
}

fn github_heading_anchor(heading: &str) -> String {
    let mut anchor = String::new();
    for character in heading.chars().flat_map(char::to_lowercase) {
        if character.is_alphanumeric() || character == '-' || character == '_' {
            anchor.push(character);
        } else if character.is_whitespace() {
            anchor.push('-');
        }
    }
    anchor
}
