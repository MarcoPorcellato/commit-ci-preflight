// Copyright 2026 Marco Porcellato
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::error::Error;
use std::ffi::OsString;
use std::fs;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value as JsonValue, json};
use sha2::{Digest, Sha256};
use toml::Value as TomlValue;

const CREATED_UTC: &str = "2026-08-10T00:00:00Z";
const SBOM_PATH: &str = "SBOM.spdx.json";
const NOTICES_PATH: &str = "THIRD_PARTY_NOTICES.md";
const MAX_LICENSE_BYTES: u64 = 1_048_576;

type LockPackageKey = (String, String, String);
type LockChecksums = BTreeMap<LockPackageKey, String>;

#[derive(Clone, Debug)]
struct Package {
    id: String,
    name: String,
    version: String,
    license: String,
    source: String,
    repository: Option<String>,
    authors: Vec<String>,
    manifest_dir: PathBuf,
    checksum: Option<String>,
}

#[derive(Debug)]
struct Generated {
    sbom: Vec<u8>,
    notices: Vec<u8>,
}

#[derive(Debug)]
struct LicenseDocument {
    text: String,
    packages: BTreeSet<String>,
    file_names: BTreeSet<String>,
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut arguments = env::args().skip(1);
    let mode = arguments.next().unwrap_or_else(|| "--check".to_owned());
    if arguments.next().is_some() || !matches!(mode.as_str(), "--check" | "--write") {
        return Err(invalid(
            "usage: cargo run --example generate_release_metadata -- [--check|--write]",
        )
        .into());
    }

    let generated = generate(Path::new("."))?;
    if mode == "--write" {
        fs::write(SBOM_PATH, &generated.sbom)?;
        fs::write(NOTICES_PATH, &generated.notices)?;
        println!("wrote {SBOM_PATH} and {NOTICES_PATH}");
    } else {
        check_exact(Path::new(SBOM_PATH), &generated.sbom)?;
        check_exact(Path::new(NOTICES_PATH), &generated.notices)?;
        println!("release metadata is current");
    }
    Ok(())
}

fn generate(root: &Path) -> Result<Generated, Box<dyn Error>> {
    let metadata = load_metadata(root)?;
    let lock_bytes = fs::read(root.join("Cargo.lock"))?;
    let lock_digest = hex_digest(&lock_bytes);
    let checksums = load_lock_checksums(&lock_bytes)?;
    let packages = load_packages(&metadata, &checksums)?;
    let root_id = metadata
        .pointer("/resolve/root")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| invalid("cargo metadata did not identify the root package"))?;
    let root_package = packages
        .iter()
        .find(|package| package.id == root_id)
        .ok_or_else(|| invalid("root package is absent from cargo metadata packages"))?;

    Ok(Generated {
        sbom: generate_sbom(&metadata, &packages, root_package, &lock_digest)?,
        notices: generate_notices(&packages, root_package)?,
    })
}

fn load_metadata(root: &Path) -> Result<JsonValue, Box<dyn Error>> {
    let cargo = env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
    let output = Command::new(cargo)
        .args(["metadata", "--locked", "--format-version", "1"])
        .current_dir(root)
        .output()?;
    if !output.status.success() {
        return Err(invalid(format!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
        .into());
    }
    Ok(serde_json::from_slice(&output.stdout)?)
}

fn load_lock_checksums(lock_bytes: &[u8]) -> Result<LockChecksums, Box<dyn Error>> {
    let lock_text = std::str::from_utf8(lock_bytes)?;
    let parsed: TomlValue = toml::from_str(lock_text)?;
    let entries = parsed
        .get("package")
        .and_then(TomlValue::as_array)
        .ok_or_else(|| invalid("Cargo.lock does not contain a package array"))?;
    let mut checksums = BTreeMap::new();

    for entry in entries {
        let table = entry
            .as_table()
            .ok_or_else(|| invalid("Cargo.lock package entry is not a table"))?;
        let Some(checksum) = table.get("checksum").and_then(TomlValue::as_str) else {
            continue;
        };
        let name = required_toml_string(table, "name")?;
        let version = required_toml_string(table, "version")?;
        let source = table
            .get("source")
            .and_then(TomlValue::as_str)
            .unwrap_or_default()
            .to_owned();
        checksums.insert((name, version, source), checksum.to_owned());
    }
    Ok(checksums)
}

fn required_toml_string(
    table: &toml::map::Map<String, TomlValue>,
    key: &str,
) -> Result<String, io::Error> {
    table
        .get(key)
        .and_then(TomlValue::as_str)
        .map(str::to_owned)
        .ok_or_else(|| invalid(format!("Cargo.lock package is missing {key}")))
}

fn load_packages(
    metadata: &JsonValue,
    checksums: &BTreeMap<(String, String, String), String>,
) -> Result<Vec<Package>, Box<dyn Error>> {
    let entries = metadata
        .get("packages")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| invalid("cargo metadata packages is not an array"))?;
    let mut packages = Vec::with_capacity(entries.len());

    for entry in entries {
        let id = required_json_string(entry, "id")?;
        let name = required_json_string(entry, "name")?;
        let version = required_json_string(entry, "version")?;
        let source = entry
            .get("source")
            .and_then(JsonValue::as_str)
            .unwrap_or_default()
            .to_owned();
        let manifest_path = PathBuf::from(required_json_string(entry, "manifest_path")?);
        let manifest_dir = manifest_path
            .parent()
            .ok_or_else(|| invalid("package manifest path has no parent"))?
            .to_path_buf();
        let license = entry
            .get("license")
            .and_then(JsonValue::as_str)
            .map(normalize_license)
            .unwrap_or_else(|| "NOASSERTION".to_owned());
        let repository = entry
            .get("repository")
            .and_then(JsonValue::as_str)
            .map(str::to_owned);
        let authors = entry
            .get("authors")
            .and_then(JsonValue::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(JsonValue::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        let checksum = checksums
            .get(&(name.clone(), version.clone(), source.clone()))
            .cloned();

        packages.push(Package {
            id,
            name,
            version,
            license,
            source,
            repository,
            authors,
            manifest_dir,
            checksum,
        });
    }

    packages.sort_by(|left, right| {
        (&left.name, &left.version, &left.source).cmp(&(&right.name, &right.version, &right.source))
    });
    Ok(packages)
}

fn required_json_string(value: &JsonValue, key: &str) -> Result<String, io::Error> {
    value
        .get(key)
        .and_then(JsonValue::as_str)
        .map(str::to_owned)
        .ok_or_else(|| invalid(format!("cargo metadata package is missing {key}")))
}

fn generate_sbom(
    metadata: &JsonValue,
    packages: &[Package],
    root_package: &Package,
    lock_digest: &str,
) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut id_map = BTreeMap::new();
    let mut used_ids = BTreeSet::new();
    for package in packages {
        let spdx_id = format!(
            "SPDXRef-Package-{}-{}",
            sanitize_spdx(&package.name),
            sanitize_spdx(&package.version)
        );
        if !used_ids.insert(spdx_id.clone()) {
            return Err(invalid(format!("duplicate SPDX identifier {spdx_id}")).into());
        }
        id_map.insert(package.id.clone(), spdx_id);
    }

    let package_values: Vec<JsonValue> = packages
        .iter()
        .map(|package| {
            let mut value = json!({
                "SPDXID": id_map[&package.id],
                "name": package.name,
                "versionInfo": package.version,
                "downloadLocation": download_location(package),
                "filesAnalyzed": false,
                "licenseConcluded": package.license,
                "licenseDeclared": package.license,
                "copyrightText": "NOASSERTION",
                "externalRefs": [{
                    "referenceCategory": "PACKAGE-MANAGER",
                    "referenceType": "purl",
                    "referenceLocator": format!(
                        "pkg:cargo/{}@{}",
                        percent_encode(&package.name),
                        percent_encode(&package.version)
                    )
                }]
            });
            if let Some(checksum) = &package.checksum {
                value
                    .as_object_mut()
                    .expect("package JSON is an object")
                    .insert(
                        "checksums".to_owned(),
                        json!([{"algorithm": "SHA256", "checksumValue": checksum}]),
                    );
            }
            value
        })
        .collect();

    let mut relationships = BTreeSet::new();
    relationships.insert((
        "SPDXRef-DOCUMENT".to_owned(),
        "DESCRIBES".to_owned(),
        id_map[&root_package.id].clone(),
    ));
    if let Some(nodes) = metadata
        .pointer("/resolve/nodes")
        .and_then(JsonValue::as_array)
    {
        for node in nodes {
            let Some(from_id) = node.get("id").and_then(JsonValue::as_str) else {
                continue;
            };
            let Some(from_spdx) = id_map.get(from_id) else {
                continue;
            };
            let Some(dependencies) = node.get("dependencies").and_then(JsonValue::as_array) else {
                continue;
            };
            for dependency in dependencies.iter().filter_map(JsonValue::as_str) {
                if let Some(to_spdx) = id_map.get(dependency) {
                    relationships.insert((
                        from_spdx.clone(),
                        "DEPENDS_ON".to_owned(),
                        to_spdx.clone(),
                    ));
                }
            }
        }
    }
    let relationship_values: Vec<JsonValue> = relationships
        .into_iter()
        .map(|(from, relationship, to)| {
            json!({
                "spdxElementId": from,
                "relationshipType": relationship,
                "relatedSpdxElement": to
            })
        })
        .collect();

    let document = json!({
        "SPDXID": "SPDXRef-DOCUMENT",
        "spdxVersion": "SPDX-2.3",
        "dataLicense": "CC0-1.0",
        "name": format!("{}-{}", root_package.name, root_package.version),
        "documentNamespace": format!(
            "https://github.com/MarcoPorcellato/commit-ci-preflight/sbom/{}/{}",
            root_package.version,
            lock_digest
        ),
        "creationInfo": {
            "created": CREATED_UTC,
            "creators": [
                "Person: Marco Porcellato",
                format!("Tool: commit-ci-preflight-release-metadata/{}", root_package.version)
            ],
            "comment": format!("Generated from locked Cargo metadata; Cargo.lock SHA-256: {lock_digest}")
        },
        "packages": package_values,
        "relationships": relationship_values
    });
    let mut bytes = serde_json::to_vec_pretty(&document)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn generate_notices(
    packages: &[Package],
    root_package: &Package,
) -> Result<Vec<u8>, Box<dyn Error>> {
    let dependencies: Vec<&Package> = packages
        .iter()
        .filter(|package| package.id != root_package.id)
        .collect();
    let mut documents: BTreeMap<String, LicenseDocument> = BTreeMap::new();
    let mut packages_without_text = Vec::new();

    for package in &dependencies {
        let found = collect_license_documents(package)?;
        if found.is_empty() {
            packages_without_text.push(format!("{} {}", package.name, package.version));
        }
        for (file_name, text) in found {
            let digest = hex_digest(text.as_bytes());
            let document = documents.entry(digest).or_insert_with(|| LicenseDocument {
                text,
                packages: BTreeSet::new(),
                file_names: BTreeSet::new(),
            });
            document
                .packages
                .insert(format!("{} {}", package.name, package.version));
            document.file_names.insert(file_name);
        }
    }

    let mut output = String::new();
    output.push_str("# Third-party notices\n\n");
    output.push_str("Generated deterministically from the locked Rust dependency graph for ");
    output.push_str(&format!(
        "Commit CI Preflight {}. This file is an inventory and bundled copy of license or notice texts found in the packaged crates; it is not legal advice.\n\n",
        root_package.version
    ));
    output.push_str("| Crate | Version | Authors | Declared license | Source | Cargo checksum |\n");
    output.push_str("|---|---|---|---|---|---|\n");
    for package in &dependencies {
        let authors = if package.authors.is_empty() {
            "Not declared".to_owned()
        } else {
            package.authors.join(", ")
        };
        let source = package
            .repository
            .as_deref()
            .or_else(|| (!package.source.is_empty()).then_some(package.source.as_str()))
            .unwrap_or("Not declared");
        output.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            escape_table(&package.name),
            escape_table(&package.version),
            escape_table(&authors),
            escape_table(&package.license),
            escape_table(source),
            package.checksum.as_deref().unwrap_or("Not present")
        ));
    }

    if !packages_without_text.is_empty() {
        output.push_str("\n## Packages without a bundled license text file\n\n");
        output.push_str("The following packaged crates declared a license expression but did not contain a UTF-8 file whose name begins with LICENSE, COPYING, NOTICE, or UNLICENSE. Consult the source link and declared expression above before redistribution.\n\n");
        for package in packages_without_text {
            output.push_str(&format!("- {package}\n"));
        }
    }

    output.push_str("\n## Deduplicated license and notice texts\n");
    for (digest, document) in documents {
        output.push_str(&format!("\n### SHA-256 {digest}\n\n"));
        output.push_str(&format!(
            "Packages: {}  \nSource filenames: {}\n\n",
            document.packages.into_iter().collect::<Vec<_>>().join(", "),
            document
                .file_names
                .into_iter()
                .collect::<Vec<_>>()
                .join(", ")
        ));
        for line in document.text.lines() {
            output.push_str("    ");
            output.push_str(line);
            output.push('\n');
        }
        if !document.text.ends_with('\n') {
            output.push('\n');
        }
    }
    Ok(output.into_bytes())
}

fn collect_license_documents(package: &Package) -> Result<Vec<(String, String)>, Box<dyn Error>> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(&package.manifest_dir)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if !file_type.is_file() {
            continue;
        }
        let file_name = entry.file_name().to_string_lossy().into_owned();
        let lowered = file_name.to_ascii_lowercase();
        if !(lowered.starts_with("license")
            || lowered.starts_with("copying")
            || lowered.starts_with("notice")
            || lowered.starts_with("unlicense"))
        {
            continue;
        }
        if entry.metadata()?.len() > MAX_LICENSE_BYTES {
            return Err(invalid(format!(
                "license text exceeds {MAX_LICENSE_BYTES} bytes: {} {} {file_name}",
                package.name, package.version
            ))
            .into());
        }
        let bytes = fs::read(entry.path())?;
        let text = String::from_utf8(bytes).map_err(|_| {
            invalid(format!(
                "license text is not UTF-8: {} {} {file_name}",
                package.name, package.version
            ))
        })?;
        entries.push((file_name, text));
    }
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(entries)
}

fn download_location(package: &Package) -> String {
    if package.source.starts_with("registry+") {
        format!(
            "https://crates.io/api/v1/crates/{}/{}/download",
            percent_encode(&package.name),
            percent_encode(&package.version)
        )
    } else if package.source.is_empty() {
        "NOASSERTION".to_owned()
    } else {
        package.source.clone()
    }
}

fn normalize_license(license: &str) -> String {
    match license {
        "MIT/Apache-2.0" => "MIT OR Apache-2.0".to_owned(),
        other => other.to_owned(),
    }
}

fn sanitize_spdx(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-') {
                character
            } else {
                '-'
            }
        })
        .collect()
}

fn percent_encode(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn escape_table(value: &str) -> String {
    value.replace('|', "\\|").replace(['\r', '\n'], " ")
}

fn hex_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn check_exact(path: &Path, expected: &[u8]) -> Result<(), Box<dyn Error>> {
    let actual = fs::read(path).map_err(|error| {
        invalid(format!(
            "{} is missing or unreadable: {error}; run with --write",
            path.display()
        ))
    })?;
    if actual != expected {
        return Err(invalid(format!(
            "{} is stale; run with --write and review the diff",
            path.display()
        ))
        .into());
    }
    Ok(())
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
mod tests {
    use super::{normalize_license, percent_encode, sanitize_spdx};

    #[test]
    fn legacy_dual_license_is_normalized_to_spdx_expression() {
        assert_eq!(normalize_license("MIT/Apache-2.0"), "MIT OR Apache-2.0");
    }

    #[test]
    fn spdx_identifier_is_stable_and_bounded_to_safe_characters() {
        assert_eq!(sanitize_spdx("toml@1.1.4+spec"), "toml-1.1.4-spec");
    }

    #[test]
    fn cargo_purl_segments_are_percent_encoded() {
        assert_eq!(percent_encode("1.1.4+spec"), "1.1.4%2Bspec");
    }
}
