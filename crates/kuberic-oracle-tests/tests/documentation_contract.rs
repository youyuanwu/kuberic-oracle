use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use std::collections::HashSet;
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
                    anchors.insert(github_heading_anchor(&heading));
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
