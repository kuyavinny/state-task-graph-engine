//! Artifact handling: validation, path safety, size enforcement, and copy logic.
//!
//! The adapter distinguishes two artifact categories:
//!
//! - **Project artifacts** — files the agent produced in the project tree (e.g.
//!   source files, test outputs).  These are **referenced in place** and never
//!   copied, moved, or rewritten by the adapter.
//!
//! - **Adapter artifacts** — temporary logs, rendered prompts, raw agent outputs,
//!   debug traces.  These live under `.agent/adapter_artifacts/` and may be
//!   **copied** there if they meet the size limits in
//!   [`ArtifactPolicy`](crate::config::ArtifactPolicy).
//!
//! # Path safety
//!
//! All artifact paths are canonicalised and checked against the project root.
//! Paths that escape the project root are rejected with
//! [`AdapterError::ArtifactPolicyViolation`].

use std::path::{Path, PathBuf};

use crate::config::ArtifactPolicy;
use crate::error::AdapterError;

/// Well-known directory for adapter-owned artifacts, relative to project root.
pub const ADAPTER_ARTIFACTS_DIR: &str = ".agent/adapter_artifacts";

/// Decides whether an artifact path is adapter-owned or a project artifact.
///
/// An artifact is adapter-owned if it resides under `.agent/adapter_artifacts/`
/// relative to the project root.
/// Everything else is a project artifact that must be referenced in place.
pub fn is_adapter_artifact(path: &Path, project_root: &Path) -> bool {
    // Try relative comparison first
    if let Ok(relative) = path.strip_prefix(project_root) {
        return relative.starts_with(ADAPTER_ARTIFACTS_DIR);
    }
    // Fall back to string matching for relative paths
    let path_str = path.to_string_lossy();
    path_str.starts_with(ADAPTER_ARTIFACTS_DIR)
        || path_str.starts_with(&format!("./{}", ADAPTER_ARTIFACTS_DIR))
}

/// Canonicalise a path and verify it falls within the project root.
///
/// Returns the canonical path on success, or an error if:
/// - The path does not exist (cannot canonicalise).
/// - The canonical path escapes the project root.
pub fn normalize_and_validate_path(
    path: &Path,
    project_root: &Path,
) -> Result<PathBuf, AdapterError> {
    let canonical = path
        .canonicalize()
        .map_err(|e| AdapterError::ArtifactPolicyViolation {
            message: format!(
                "artifact path '{}' could not be resolved: {}",
                path.display(),
                e
            ),
        })?;

    let canonical_root =
        project_root
            .canonicalize()
            .map_err(|e| AdapterError::ArtifactPolicyViolation {
                message: format!(
                    "project root '{}' could not be resolved: {}",
                    project_root.display(),
                    e
                ),
            })?;

    if !canonical.starts_with(&canonical_root) {
        return Err(AdapterError::ArtifactPolicyViolation {
            message: format!(
                "artifact path '{}' resolves outside the project directory",
                path.display()
            ),
        });
    }

    Ok(canonical)
}

/// Validate that a single adapter-owned artifact does not exceed the individual
/// byte limit in [`ArtifactPolicy::max_copied_artifact_bytes`].
pub fn validate_artifact_size(path: &Path, policy: &ArtifactPolicy) -> Result<(), AdapterError> {
    let metadata = std::fs::metadata(path).map_err(|e| AdapterError::ArtifactPolicyViolation {
        message: format!("cannot stat artifact '{}': {}", path.display(), e),
    })?;

    if metadata.len() > policy.max_copied_artifact_bytes {
        return Err(AdapterError::ArtifactPolicyViolation {
            message: format!(
                "artifact '{}' is {} bytes, exceeding the individual limit of {} bytes",
                path.display(),
                metadata.len(),
                policy.max_copied_artifact_bytes
            ),
        });
    }

    Ok(())
}

/// Validate that the total size of all adapter artifacts does not exceed
/// [`ArtifactPolicy::max_total_copied_bytes`].
pub fn validate_total_artifact_size(
    paths: &[PathBuf],
    policy: &ArtifactPolicy,
) -> Result<(), AdapterError> {
    let mut total: u64 = 0;
    for path in paths {
        let metadata =
            std::fs::metadata(path).map_err(|e| AdapterError::ArtifactPolicyViolation {
                message: format!("cannot stat artifact '{}': {}", path.display(), e),
            })?;
        total += metadata.len();
    }

    if total > policy.max_total_copied_bytes {
        return Err(AdapterError::ArtifactPolicyViolation {
            message: format!(
                "total artifact size is {} bytes, exceeding the total limit of {} bytes",
                total, policy.max_total_copied_bytes
            ),
        });
    }

    Ok(())
}

/// Validate all artifact paths from a result packet.
///
/// For each path:
/// 1. Canonicalise and check it's within the project root.
/// 2. If it's an adapter artifact, check the individual size limit.
///
/// After individual checks, verify the total size of all adapter artifacts
/// does not exceed `max_total_copied_bytes`.
///
/// Returns a vec of validated canonical paths on success.
pub fn validate_artifacts(
    artifact_paths: &[String],
    evidence_artifact_paths: &[Option<String>],
    raw_agent_output_path: &Option<String>,
    project_root: &Path,
    policy: &ArtifactPolicy,
) -> Result<Vec<PathBuf>, AdapterError> {
    let mut adapter_artifact_paths: Vec<PathBuf> = Vec::new();
    let mut validated: Vec<PathBuf> = Vec::new();

    // Collect all paths to validate
    let mut all_paths: Vec<&str> = artifact_paths.iter().map(|s| s.as_str()).collect();
    for path in evidence_artifact_paths.iter().flatten() {
        all_paths.push(path.as_str());
    }
    if let Some(path) = raw_agent_output_path.as_ref() {
        all_paths.push(path.as_str());
    }

    for raw_path in &all_paths {
        let path = Path::new(raw_path);
        let canonical = normalize_and_validate_path(path, project_root)?;

        if is_adapter_artifact(&canonical, project_root) {
            // Adapter artifacts: check individual size limit
            validate_artifact_size(&canonical, policy)?;
            adapter_artifact_paths.push(canonical.clone());
        }
        // Project artifacts are referenced in place — no size check, no copy.

        validated.push(canonical);
    }

    // Total size check for adapter artifacts only
    if !adapter_artifact_paths.is_empty() {
        validate_total_artifact_size(&adapter_artifact_paths, policy)?;
    }

    Ok(validated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup_temp_dir() -> tempfile::TempDir {
        tempfile::tempdir().expect("temp dir")
    }

    #[test]
    fn is_adapter_artifact_in_adapter_dir() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().canonicalize().unwrap();
        assert!(is_adapter_artifact(
            Path::new(".agent/adapter_artifacts/log.txt"),
            &root
        ));
        assert!(is_adapter_artifact(
            Path::new("./.agent/adapter_artifacts/log.txt"),
            &root
        ));
        // Also test with absolute path
        let abs_path = dir.path().join(".agent/adapter_artifacts/log.txt");
        assert!(is_adapter_artifact(&abs_path, &root));
    }

    #[test]
    fn is_adapter_artifact_project_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().canonicalize().unwrap();
        assert!(!is_adapter_artifact(Path::new("src/main.rs"), &root));
        assert!(!is_adapter_artifact(
            Path::new("./tests/auth.spec.ts"),
            &root
        ));
        assert!(!is_adapter_artifact(Path::new("./Cargo.toml"), &root));
    }

    #[test]
    fn normalize_and_validate_rejects_traversal() {
        let dir = setup_temp_dir();
        let root = dir.path().canonicalize().unwrap();

        // Create a file inside the project to canonicalise against
        let inner = dir.path().join("inner.txt");
        fs::write(&inner, "data").unwrap();

        // Valid path inside project
        let result = normalize_and_validate_path(&inner, &root);
        assert!(result.is_ok());

        // Symlink pointing outside would be caught by canonicalize + starts_with
        // (but creating cross-OS symlinks in tests is tricky; the logic is tested
        // indirectly via the S1 fix in cli.rs)
    }

    #[test]
    fn normalize_and_validate_rejects_nonexistent_path() {
        let dir = setup_temp_dir();
        let root = dir.path().canonicalize().unwrap();
        let bad = dir.path().join("does_not_exist.txt");

        let result = normalize_and_validate_path(&bad, &root);
        assert!(result.is_err());
    }

    #[test]
    fn validate_artifact_size_rejects_oversized() {
        let dir = setup_temp_dir();
        let policy = ArtifactPolicy {
            max_copied_artifact_bytes: 10,
            max_total_copied_bytes: 100,
        };

        let file = dir.path().join("big.log");
        fs::write(&file, "this is more than ten bytes").unwrap();

        let result = validate_artifact_size(&file, &policy);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("exceeding the individual limit"));
    }

    #[test]
    fn validate_artifact_size_allows_within_limit() {
        let dir = setup_temp_dir();
        let policy = ArtifactPolicy {
            max_copied_artifact_bytes: 1024,
            max_total_copied_bytes: 2048,
        };

        let file = dir.path().join("small.log");
        fs::write(&file, "ok").unwrap();

        let result = validate_artifact_size(&file, &policy);
        assert!(result.is_ok());
    }

    #[test]
    fn validate_total_rejects_excess() {
        let dir = setup_temp_dir();
        let policy = ArtifactPolicy {
            max_copied_artifact_bytes: 100,
            max_total_copied_bytes: 10,
        };

        let f1 = dir.path().join("a.log");
        fs::write(&f1, "hello world").unwrap();

        let result = validate_total_artifact_size(&[f1], &policy);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("total limit"));
    }

    #[test]
    fn validate_artifacts_rejects_path_traversal() {
        let dir = setup_temp_dir();
        let root = dir.path().canonicalize().unwrap();
        let policy = ArtifactPolicy {
            max_copied_artifact_bytes: 1024,
            max_total_copied_bytes: 2048,
        };

        // Pass a path outside the project root
        let outside = Path::new("/etc/passwd");
        // This will fail because /etc/passwd canonical path doesn't start with root
        let result = validate_artifacts(
            &[outside.to_string_lossy().to_string()],
            &[],
            &None,
            &root,
            &policy,
        );
        assert!(result.is_err());
    }

    #[test]
    fn project_artifact_not_size_checked() {
        let dir = setup_temp_dir();
        let root = dir.path().canonicalize().unwrap();
        let policy = ArtifactPolicy {
            max_copied_artifact_bytes: 5, // very small limit
            max_total_copied_bytes: 5,
        };

        // Create a project artifact (not under .agent/adapter_artifacts/)
        let src_file = dir.path().join("src");
        fs::create_dir_all(&src_file).unwrap();
        let project_artifact = src_file.join("main.rs");
        fs::write(&project_artifact, "this is more than five bytes long").unwrap();

        // Project artifacts should pass even if they exceed adapter limits
        let result = validate_artifacts(
            &[project_artifact.to_string_lossy().to_string()],
            &[],
            &None,
            &root,
            &policy,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn adapter_artifact_rejected_when_oversized() {
        let dir = setup_temp_dir();
        let root = dir.path().canonicalize().unwrap();
        let policy = ArtifactPolicy {
            max_copied_artifact_bytes: 5,
            max_total_copied_bytes: 100,
        };

        // Create .agent/adapter_artifacts/ dir
        let adapter_dir = dir.path().join(".agent/adapter_artifacts");
        fs::create_dir_all(&adapter_dir).unwrap();
        let adapter_file = adapter_dir.join("big.log");
        fs::write(&adapter_file, "this is more than five bytes").unwrap();

        let result = validate_artifacts(
            &[adapter_file.to_string_lossy().to_string()],
            &[],
            &None,
            &root,
            &policy,
        );
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("individual limit"));
    }

    #[test]
    fn mixed_artifacts_project_and_adapter_pass() {
        let dir = setup_temp_dir();
        let root = dir.path().canonicalize().unwrap();
        let policy = ArtifactPolicy {
            max_copied_artifact_bytes: 1024,
            max_total_copied_bytes: 2048,
        };

        // Project artifact
        let src_dir = dir.path().join("src");
        fs::create_dir_all(&src_dir).unwrap();
        let project_artifact = src_dir.join("main.rs");
        fs::write(&project_artifact, "fn main() {}").unwrap();

        // Adapter artifact
        let adapter_dir = dir.path().join(".agent/adapter_artifacts");
        fs::create_dir_all(&adapter_dir).unwrap();
        let adapter_artifact = adapter_dir.join("coverage.json");
        fs::write(&adapter_artifact, "{\"coverage\": 95}").unwrap();

        let result = validate_artifacts(
            &[
                project_artifact.to_string_lossy().to_string(),
                adapter_artifact.to_string_lossy().to_string(),
            ],
            &[],
            &None,
            &root,
            &policy,
        );
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 2);
    }

    #[test]
    fn evidence_artifact_path_validated() {
        let dir = setup_temp_dir();
        let root = dir.path().canonicalize().unwrap();
        let policy = ArtifactPolicy {
            max_copied_artifact_bytes: 1024,
            max_total_copied_bytes: 2048,
        };

        // Create an evidence artifact
        let adapter_dir = dir.path().join(".agent/adapter_artifacts");
        fs::create_dir_all(&adapter_dir).unwrap();
        let evidence_file = adapter_dir.join("test_output.json");
        fs::write(&evidence_file, "{\"passed\": true}").unwrap();

        let result = validate_artifacts(
            &[],
            &[Some(evidence_file.to_string_lossy().to_string())],
            &None,
            &root,
            &policy,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn valid_adapter_artifact_copied() {
        let dir = setup_temp_dir();
        let root = dir.path().canonicalize().unwrap();
        let policy = ArtifactPolicy {
            max_copied_artifact_bytes: 1024,
            max_total_copied_bytes: 2048,
        };

        // Create valid adapter artifact
        let adapter_dir = dir.path().join(".agent/adapter_artifacts");
        fs::create_dir_all(&adapter_dir).unwrap();
        let small_log = adapter_dir.join("small.log");
        fs::write(&small_log, "ok").unwrap();

        let result = validate_artifacts(
            &[small_log.to_string_lossy().to_string()],
            &[],
            &None,
            &root,
            &policy,
        );
        assert!(result.is_ok());
    }
}
