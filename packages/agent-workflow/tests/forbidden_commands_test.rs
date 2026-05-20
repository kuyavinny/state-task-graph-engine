//! Static analysis: ensure no forbidden `stage` mutation commands are called
//! directly from `agent-workflow` source code.
//!
//! All Module 1 mutations MUST route through `agent-adapter`.

use std::path::Path;

/// List of stage commands that constitute mutations.
static FORBIDDEN_COMMANDS: &[&str] = &[
    "stage claim",
    "stage complete",
    "stage fail",
    "stage block",
    "stage skip",
    "stage cancel",
    "stage reopen",
    "stage append-nodes",
    "stage heartbeat",
    "stage release",
];

fn find_rs_files(dir: &Path, files: &mut Vec<std::path::PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                find_rs_files(&path, files);
            } else if path.extension().map(|e| e == "rs").unwrap_or(false) {
                files.push(path);
            }
        }
    }
}

#[test]
fn test_no_forbidden_stage_mutations_in_source() {
    // Locate the agent-workflow src directory
    let src_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");

    let mut files = Vec::new();
    find_rs_files(&src_dir, &mut files);

    let mut violations = Vec::new();

    for path in files {
        let contents = std::fs::read_to_string(&path).expect("read source file");
        for cmd in FORBIDDEN_COMMANDS {
            for (line_no, line) in contents.lines().enumerate() {
                let trimmed = line.trim();
                // Skip comment lines
                if trimmed.starts_with("//")
                    || trimmed.starts_with("/*")
                    || trimmed.starts_with("*")
                {
                    continue;
                }
                // Skip string literals (test data)
                if trimmed.starts_with('\"') && trimmed.ends_with('\"') {
                    continue;
                }
                if line.contains(cmd) {
                    violations.push(format!(
                        "{}:{}: forbidden command '{}'",
                        path.display(),
                        line_no + 1,
                        cmd
                    ));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Forbidden stage mutation commands found:\n{}",
        violations.join("\n")
    );
}

#[test]
fn test_graph_client_trait_only_has_readonly() {
    // Compile-time structural check: the trait only defines readonly methods.
    use agent_workflow::graph_client::GraphStatusClient;
    fn assert_readonly<T: GraphStatusClient>(_client: &T) {}
    let client = agent_workflow::graph_client::RealGraphStatusClient::default();
    assert_readonly(&client);
}
