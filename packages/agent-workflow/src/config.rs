/// Returns the workflow controller version string.
///
/// Format: `<major>.<minor>.<patch>`
///
/// # Examples
///
/// ```
/// assert_eq!(agent_workflow::version(), "1.0.0");
/// ```
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
