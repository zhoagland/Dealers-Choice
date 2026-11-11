//! Records the git commit the binary was built from (`DC_GIT_SHA`), so campaign summaries can
//! say which code produced them (FR-SP-8) and frozen binaries get a stable name.

use std::process::Command;

fn main() {
    let sha = Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".into());
    let dirty = Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=no"])
        .output()
        .ok()
        .is_some_and(|o| !o.stdout.is_empty());
    let sha = if dirty { format!("{sha}-dirty") } else { sha };
    println!("cargo:rustc-env=DC_GIT_SHA={sha}");
    // Rebuild when the commit or the index changes (works in worktrees too).
    if let Some(dir) = Command::new("git")
        .args(["rev-parse", "--absolute-git-dir"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    {
        println!("cargo:rerun-if-changed={dir}/HEAD");
        println!("cargo:rerun-if-changed={dir}/index");
    }
}
