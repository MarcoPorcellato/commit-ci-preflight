use std::collections::HashSet;
use std::path::{Path, PathBuf};

const SOCIAL_PREVIEW_PNG: &[u8] = include_bytes!("../docs/assets/social-preview.png");

fn unique_fixture_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "ccp-public-docs-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    assert!(!root.exists(), "fixture path unexpectedly exists: {root:?}");
    root
}

fn local_link_destinations(markdown: &str) -> Vec<String> {
    let mut destinations = Vec::new();
    let mut rest = markdown;
    while let Some(start) = rest.find("](") {
        let after = &rest[start + 2..];
        let Some(end) = after.find(')') else { break };
        let mut destination = after[..end].trim().to_owned();
        if destination.starts_with('<') && destination.ends_with('>') {
            destination = destination[1..destination.len() - 1].to_owned();
        }
        destinations.push(destination);
        rest = &after[end + 1..];
    }
    destinations
}

fn markdown_heading_anchors(markdown: &str) -> HashSet<String> {
    markdown
        .lines()
        .filter_map(|line| line.strip_prefix('#'))
        .filter(|line| line.starts_with([' ', '\t']))
        .map(|line| {
            line.trim()
                .trim_end_matches('#')
                .trim()
                .chars()
                .filter_map(|c| {
                    if c.is_alphanumeric() || c == '_' || c == '-' || c == ' ' {
                        Some(c.to_ascii_lowercase())
                    } else {
                        None
                    }
                })
                .collect::<String>()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join("-")
        })
        .collect()
}

fn validate_local_links(root: &Path, document: &Path) -> Vec<String> {
    let document_path = root.join(document);
    let markdown = std::fs::read_to_string(&document_path).expect("read document");
    let mut findings = Vec::new();
    for destination in local_link_destinations(&markdown) {
        if destination.starts_with("http:")
            || destination.starts_with("https:")
            || destination.starts_with("mailto:")
            || destination.starts_with('/')
        {
            continue;
        }
        if destination.contains('%') {
            findings.push(format!(
                "{}: unsupported percent-encoded local link {destination}",
                document.display()
            ));
            continue;
        }
        let (path_part, fragment) = destination
            .split_once('#')
            .map_or((destination.as_str(), None), |(path, fragment)| {
                (path, Some(fragment))
            });
        let base_depth = document
            .parent()
            .map_or(0, |parent| parent.components().count());
        let mut depth = base_depth as isize;
        let escapes_root = Path::new(path_part).components().any(|component| {
            match component {
                std::path::Component::ParentDir => depth -= 1,
                std::path::Component::Normal(_) => depth += 1,
                _ => {}
            }
            depth < 0
        });
        if escapes_root {
            findings.push(format!(
                "{}: local target escapes repository root {destination}",
                document.display()
            ));
            continue;
        }
        let target_document = if path_part.is_empty() {
            document_path.clone()
        } else {
            document_path
                .parent()
                .expect("document parent")
                .join(path_part)
        };
        let relative_target = target_document.strip_prefix(root).map_err(|_| ()).ok();
        if relative_target.is_none() {
            findings.push(format!(
                "{}: local target escapes repository root {destination}",
                document.display()
            ));
            continue;
        }
        if !target_document.is_file() {
            findings.push(format!(
                "{}: missing local target {}",
                document.display(),
                path_part
            ));
            continue;
        }
        if let Some(fragment) = fragment {
            let target = std::fs::read_to_string(&target_document).expect("read link target");
            if !markdown_heading_anchors(&target).contains(&fragment.to_ascii_lowercase()) {
                findings.push(format!(
                    "{}: missing fragment #{fragment} in {path_part}",
                    document.display()
                ));
            }
        }
    }
    findings
}

fn png_dimensions(bytes: &[u8]) -> Result<(u32, u32), String> {
    const SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    if bytes.len() < SIGNATURE.len() || &bytes[..8] != SIGNATURE {
        return Err("invalid PNG signature".into());
    }
    if bytes.len() < 16 || &bytes[12..16] != b"IHDR" {
        return Err("missing IHDR chunk".into());
    }
    let chunk_length = u32::from_be_bytes(bytes[8..12].try_into().expect("chunk length"));
    if chunk_length != 13 || bytes.len() < 8 + 4 + 4 + 13 + 4 {
        return Err("truncated IHDR chunk".into());
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().expect("width"));
    let height = u32::from_be_bytes(bytes[20..24].try_into().expect("height"));
    if width == 0 || height == 0 {
        return Err("PNG dimensions must be non-zero".into());
    }
    Ok((width, height))
}

#[test]
fn local_link_validation_reports_missing_files_and_fragments() {
    let root = unique_fixture_root("broken-links");
    std::fs::create_dir_all(root.join("docs")).expect("create docs");
    std::fs::write(
        root.join("README.md"),
        "# Root\n\n[missing](docs/missing.md)\n[bad anchor](docs/present.md#absent)\n",
    )
    .expect("write README");
    std::fs::write(root.join("docs/present.md"), "# Present heading\n").expect("write target");

    let findings = validate_local_links(&root, Path::new("README.md"));

    assert_eq!(
        findings,
        vec![
            "README.md: missing local target docs/missing.md",
            "README.md: missing fragment #absent in docs/present.md",
        ]
    );
    std::fs::remove_dir_all(root).expect("remove owned fixture");
}

#[test]
fn png_dimensions_rejects_invalid_or_truncated_bytes() {
    assert_eq!(
        png_dimensions(b"not a png"),
        Err("invalid PNG signature".into())
    );
    assert_eq!(
        png_dimensions(b"\x89PNG\r\n\x1a\n"),
        Err("missing IHDR chunk".into())
    );
    let truncated_ihdr = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR\x00\x00\x00\x01\x00\x00\x00\x01";
    assert_eq!(
        png_dimensions(truncated_ihdr),
        Err("truncated IHDR chunk".into())
    );
}

#[test]
fn local_links_reject_traversal_and_percent_encoding() {
    let root = unique_fixture_root("safety");
    std::fs::create_dir_all(&root).expect("create fixture root");
    std::fs::write(
        root.join("README.md"),
        "[escape](../outside.md) [encoded](docs/a%20b.md)",
    )
    .expect("write README");
    let findings = validate_local_links(&root, Path::new("README.md"));
    assert_eq!(findings.len(), 2);
    assert!(findings[0].contains("escapes repository root"));
    assert!(findings[1].contains("unsupported percent-encoded"));
    std::fs::remove_dir_all(root).expect("remove owned fixture");
}

#[test]
fn current_public_documents_have_valid_local_links() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    const PUBLIC_DOCUMENTS: &[&str] = &[
        "README.md",
        "SUPPORT.md",
        "docs/CASE_STUDY_PR71.md",
        "docs/INSTALLATION.md",
        "docs/TUTORIAL.md",
        "docs/ADOPTION_GUIDE.md",
        "docs/BETA_SUPPORT.md",
        "docs/REPOSITORY_PRESENTATION.md",
        "docs/THREAT_MODEL.md",
    ];
    // Task 2 adds SUPPORT.md and docs/CASE_STUDY_PR71.md to this active slice.
    let findings: Vec<_> = PUBLIC_DOCUMENTS
        .iter()
        .flat_map(|document| validate_local_links(root, Path::new(document)))
        .collect();
    assert!(findings.is_empty(), "documentation findings: {findings:?}");
}

#[test]
fn social_preview_png_is_uploadable() {
    assert_eq!(png_dimensions(SOCIAL_PREVIEW_PNG), Ok((1280, 640)));
    assert!(SOCIAL_PREVIEW_PNG.len() < 1_048_576);
}
