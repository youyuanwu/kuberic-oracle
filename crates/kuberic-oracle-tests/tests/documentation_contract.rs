use std::fs;
use std::path::{Path, PathBuf};

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
        "## Dormant Kuberic Public V1 fit/no-fit",
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
}

#[test]
fn readiness_contract_cites_stable_identity_guards_and_preview_pins() {
    let readiness = read("docs/data-guard-readiness.md");
    for evidence in [
        "target_identity_must_match",
        "target_incarnation_must_match_even_when_dbid_matches",
        "5c202ec53104e536568d664dafadfb05f89bdd1a",
        "b0e96aa2a4dc3872b55f2485cdd51b9f0d5ead20",
    ] {
        assert!(readiness.contains(evidence), "{evidence}");
    }
}

#[test]
fn relative_markdown_links_resolve() {
    let root = workspace_root();
    let documents = [
        root.join("README.md"),
        root.join("docs/data-guard-readiness.md"),
        root.join("docs/design.md"),
        root.join("docs/kuberic-progress.md"),
    ];

    for document in documents {
        let content = fs::read_to_string(&document).unwrap();
        for target in markdown_link_targets(&content) {
            if target.contains("://") || target.starts_with('#') || target.starts_with("mailto:") {
                continue;
            }
            let path = target.split_once('#').map_or(target, |(path, _)| path);
            if path.is_empty() {
                continue;
            }
            let resolved = document.parent().unwrap().join(path);
            assert!(
                resolved.exists(),
                "{} references missing {}",
                document.display(),
                resolved.display()
            );
        }
    }
}

fn markdown_link_targets(content: &str) -> impl Iterator<Item = &str> {
    content
        .split("](")
        .skip(1)
        .filter_map(|suffix| suffix.split_once(')').map(|(target, _)| target))
}
