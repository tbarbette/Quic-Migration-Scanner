//! End-to-end tests of the compiled binary: input parsing, progress bar flag,
//! `-p/--multipath` flag and JSON output file.
use std::process::Command;

fn run(extra: &[&str], csv: &str, name: &str) -> (std::process::Output, String) {
    let dir = std::env::temp_dir().join(format!("qms-cli-{}-{}", std::process::id(), name));
    std::fs::create_dir_all(&dir).unwrap();
    let input = dir.join("in.csv");
    let output = dir.join("out.json");
    std::fs::write(&input, csv).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_migration_checker"))
        .args(extra)
        .args(["--threads", "2", "-o"])
        .arg(&output)
        .arg(&input)
        .output()
        .unwrap();
    let json = std::fs::read_to_string(&output).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);
    (out, json)
}

// Unroutable-in-practice local targets: nothing listens on UDP port 1.
const CSV: &str = "ip,domain,port\n127.0.0.1,,1\n::1,,1\n";

#[test]
fn writes_one_json_entry_per_target_skipping_header() {
    let (out, json) = run(&["--no-progress"], CSV, "basic");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(json.trim_start().starts_with('[') && json.trim_end().ends_with(']'), "{json}");
    assert_eq!(json.matches("\"url\"").count(), 2, "{json}");
    assert!(json.contains("https://127.0.0.1:1"), "{json}");
    assert!(json.contains("https://[::1]:1"), "{json}");
}

#[test]
fn no_progress_flag_silences_progress_bar() {
    let (out, _) = run(&["--no-progress"], CSV, "noprog");
    assert!(out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!stderr.contains("/2"), "progress bar drawn despite --no-progress: {stderr}");
}

#[test]
fn multipath_flag_is_accepted() {
    for flag in ["-p", "--multipath"] {
        let (out, json) = run(&[flag, "--no-progress"], CSV, "mp");
        assert!(out.status.success(), "{flag}: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(json.matches("\"url\"").count(), 2, "{flag}: {json}");
    }
}

#[test]
fn unknown_flag_is_rejected_and_help_lists_multipath() {
    let bin = env!("CARGO_BIN_EXE_migration_checker");
    assert!(!Command::new(bin).arg("--bogus").output().unwrap().status.success());
    let help = Command::new(bin).arg("--help").output().unwrap();
    let text = String::from_utf8_lossy(&help.stdout).to_string() + &String::from_utf8_lossy(&help.stderr);
    assert!(text.contains("--multipath") && text.contains("--no-progress"), "{text}");
}
