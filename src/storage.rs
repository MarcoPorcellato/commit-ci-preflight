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

use std::path::Path;

use crate::config::NormalizedStorage;

pub trait StorageProbe: Send + Sync {
    fn available_bytes(&self, path: &Path) -> Result<u64, StorageError>;
}

#[derive(Debug, Default)]
pub struct SystemStorageProbe;

impl StorageProbe for SystemStorageProbe {
    fn available_bytes(&self, path: &Path) -> Result<u64, StorageError> {
        fs2::available_space(path).map_err(|source| StorageError::Probe {
            path: path.to_path_buf(),
            source,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoragePreflight {
    pub available_bytes: u64,
    pub required_bytes: u64,
}

pub fn preflight(
    policy: &NormalizedStorage,
    cache_root: &Path,
    probe: &dyn StorageProbe,
) -> Result<StoragePreflight, StorageError> {
    let required_bytes = required_bytes(policy)?;
    let available_bytes = probe.available_bytes(cache_root)?;
    if available_bytes < required_bytes {
        return Err(StorageError::Insufficient {
            available_bytes,
            required_bytes,
        });
    }
    Ok(StoragePreflight {
        available_bytes,
        required_bytes,
    })
}

pub fn required_bytes(policy: &NormalizedStorage) -> Result<u64, StorageError> {
    policy
        .min_free_bytes
        .checked_add(policy.receipt_journal_reserve_bytes)
        .and_then(|total| total.checked_add(policy.max_cache_growth_bytes))
        .and_then(|total| total.checked_add(policy.max_artifact_bytes))
        .ok_or(StorageError::PolicyOverflow)
}

#[derive(Debug)]
pub enum StorageError {
    Probe {
        path: std::path::PathBuf,
        source: std::io::Error,
    },
    PolicyOverflow,
    Insufficient {
        available_bytes: u64,
        required_bytes: u64,
    },
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Probe { path, source } => write!(
                formatter,
                "cannot determine free storage at {}: {source}",
                path.display()
            ),
            Self::PolicyOverflow => {
                formatter.write_str("storage policy required-byte sum overflows")
            }
            Self::Insufficient {
                available_bytes,
                required_bytes,
            } => write!(
                formatter,
                "storage preflight requires {required_bytes} free bytes but only {available_bytes} are available"
            ),
        }
    }
}

impl std::error::Error for StorageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Probe { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;
    use std::path::PathBuf;

    struct FixedProbe(Result<u64, io::ErrorKind>);

    impl StorageProbe for FixedProbe {
        fn available_bytes(&self, _path: &Path) -> Result<u64, StorageError> {
            match self.0 {
                Ok(value) => Ok(value),
                Err(kind) => Err(StorageError::Probe {
                    path: PathBuf::from("/owned-cache"),
                    source: io::Error::from(kind),
                }),
            }
        }
    }

    fn policy() -> NormalizedStorage {
        NormalizedStorage {
            min_free_bytes: 100,
            receipt_journal_reserve_bytes: 20,
            max_cache_growth_bytes: 30,
            max_artifact_bytes: 40,
        }
    }

    #[test]
    fn exact_capacity_passes_with_deterministic_required_sum() {
        let outcome = preflight(&policy(), Path::new("/owned-cache"), &FixedProbe(Ok(190)))
            .expect("exact capacity passes");
        assert_eq!(outcome.available_bytes, 190);
        assert_eq!(outcome.required_bytes, 190);
    }

    #[test]
    fn insufficient_capacity_fails_closed_without_side_effects() {
        assert!(matches!(
            preflight(&policy(), Path::new("/owned-cache"), &FixedProbe(Ok(189))),
            Err(StorageError::Insufficient {
                available_bytes: 189,
                required_bytes: 190,
            })
        ));
    }

    #[test]
    fn probe_failure_and_overflow_are_not_treated_as_capacity() {
        assert!(matches!(
            preflight(
                &policy(),
                Path::new("/owned-cache"),
                &FixedProbe(Err(io::ErrorKind::PermissionDenied))
            ),
            Err(StorageError::Probe { .. })
        ));
        let overflow = NormalizedStorage {
            min_free_bytes: u64::MAX,
            receipt_journal_reserve_bytes: 1,
            max_cache_growth_bytes: 0,
            max_artifact_bytes: 0,
        };
        assert!(matches!(
            required_bytes(&overflow),
            Err(StorageError::PolicyOverflow)
        ));
    }
}
