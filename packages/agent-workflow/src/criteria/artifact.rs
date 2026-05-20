//! Artifact criterion evaluation.
//!
//! Checks filesystem presence and optionally freshness.

use crate::model::ArtifactCriterion;
use crate::paths::ProjectPaths;
use super::CriterionResult;
use std::fs;
use std::time::SystemTime;

/// Evaluate an artifact criterion against the filesystem.
pub fn evaluate(c: &ArtifactCriterion, paths: &ProjectPaths) -> CriterionResult {
    let resolved = if c.path.starts_with('/') || c.path.starts_with("\\") {
        std::path::PathBuf::from(&c.path)
    } else {
        paths.root().join(&c.path)
    };

    let metadata = match fs::metadata(&resolved) {
        Ok(m) => m,
        Err(_) => {
            return CriterionResult::NotMet {
                reason: format!("Artifact not found: {}", resolved.display()),
            };
        }
    };

    if !metadata.is_file() && !metadata.is_dir() {
        return CriterionResult::NotMet {
            reason: format!("Path exists but is not a file or directory: {}", resolved.display()),
        };
    }

    if let Some(max_age) = c.max_age_seconds {
        let modified = metadata
            .modified()
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let elapsed = match SystemTime::now().duration_since(modified) {
            Ok(d) => d.as_secs(),
            Err(_) => {
                return CriterionResult::NotMet {
                    reason: "Artifact modified time is in the future".to_string(),
                };
            }
        };
        if elapsed > max_age {
            return CriterionResult::NotMet {
                reason: format!(
                    "Artifact age {}s exceeds max {}s: {}",
                    elapsed, max_age, resolved.display()
                ),
            };
        }
    }

    CriterionResult::Met
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ArtifactCriterion;

    #[test]
    fn test_artifact_exists() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let paths = ProjectPaths::from_root(tmp.path().to_path_buf());
        let file = tmp.path().join("exists.txt");
        std::fs::write(&file, "data").expect("write");

        let c = ArtifactCriterion {
            path: file.to_string_lossy().to_string(),
            must_exist: true,
            max_age_seconds: None,
        };
        assert_eq!(evaluate(&c, &paths), CriterionResult::Met);
    }

    #[test]
    fn test_artifact_missing() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let paths = ProjectPaths::from_root(tmp.path().to_path_buf());

        let c = ArtifactCriterion {
            path: "missing.txt".to_string(),
            must_exist: true,
            max_age_seconds: None,
        };
        assert!(matches!(evaluate(&c, &paths), CriterionResult::NotMet { .. }));
    }
}
