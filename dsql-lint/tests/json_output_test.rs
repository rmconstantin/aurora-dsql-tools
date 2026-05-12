use std::process::{Command, Stdio};

fn dsql_lint_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_dsql-lint"))
}

#[test]
fn json_lint_clean_file() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("clean.sql");
    std::fs::write(&input, "CREATE TABLE t (id UUID PRIMARY KEY);").unwrap();

    let output = dsql_lint_bin()
        .arg("--format")
        .arg("json")
        .arg(input.to_str().unwrap())
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(
        output.stderr.is_empty(),
        "JSON mode must not write to stderr: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be valid JSON");

    assert_eq!(json["schema_version"], 1);

    let files = json["files"].as_array().expect("should have 'files' array");
    assert_eq!(files.len(), 1);

    let diags = files[0]["diagnostics"].as_array().unwrap();
    assert!(diags.is_empty());
    assert_eq!(json["summary"]["errors"], 0);
    assert_eq!(json["summary"]["warnings"], 0);
}

#[test]
fn json_lint_with_errors() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("bad.sql");
    std::fs::write(&input, "CREATE TABLE t (id SERIAL PRIMARY KEY);").unwrap();

    let output = dsql_lint_bin()
        .arg("--format")
        .arg("json")
        .arg(input.to_str().unwrap())
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(output.stderr.is_empty(), "JSON mode must not write to stderr");

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be valid JSON");

    let files = json["files"].as_array().unwrap();
    assert_eq!(files.len(), 1);

    let diags = files[0]["diagnostics"].as_array().unwrap();
    assert!(!diags.is_empty());

    let d = &diags[0];
    assert_eq!(d["rule"], "serial_type");
    assert!(d["line"].is_number());
    assert!(d["message"].as_str().unwrap().contains("SERIAL"));
    assert!(d["suggestion"].as_str().is_some());
    assert!(d["fix_result"]["status"].as_str().is_some());

    let preview = d["statement_preview"].as_str().unwrap();
    assert!(!preview.contains('\n'), "Preview should not contain newlines");

    assert_eq!(json["summary"]["warnings"], 1);
    assert_eq!(json["summary"]["errors"], 0);
}

#[test]
fn json_fix_mode_file_has_output_file() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("fix.sql");
    std::fs::write(&input, "CREATE INDEX idx ON t(col);").unwrap();

    let output = dsql_lint_bin()
        .arg("--fix")
        .arg("--format")
        .arg("json")
        .arg(input.to_str().unwrap())
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(output.stderr.is_empty(), "JSON mode must not write to stderr");

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be valid JSON");

    let file_entry = &json["files"][0];
    let diags = file_entry["diagnostics"].as_array().unwrap();
    assert!(!diags.is_empty());
    assert_eq!(diags[0]["fix_result"]["status"], "fixed");

    assert!(file_entry["output_file"].as_str().is_some());
    assert!(file_entry["fixed_sql"].is_null());
    assert!(file_entry["error"].is_null());

    assert_eq!(json["summary"]["fixed"], 1);
}

#[test]
fn json_file_read_error_appears_in_files_array() {
    let output = dsql_lint_bin()
        .arg("--format")
        .arg("json")
        .arg("/nonexistent/file.sql")
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(output.stderr.is_empty(), "JSON mode must not write to stderr");

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be valid JSON");

    let files = json["files"].as_array().unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0]["file"], "/nonexistent/file.sql");
    assert!(files[0]["error"].as_str().is_some());
    assert!(files[0]["diagnostics"].as_array().unwrap().is_empty());
}

#[test]
fn json_lint_nullable_fields_always_present() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("clean.sql");
    std::fs::write(&input, "CREATE TABLE t (id UUID PRIMARY KEY);").unwrap();

    let output = dsql_lint_bin()
        .arg("--format")
        .arg("json")
        .arg(input.to_str().unwrap())
        .output()
        .unwrap();

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be valid JSON");
    let file_entry = &json["files"][0];

    assert!(file_entry.get("error").is_some());
    assert!(file_entry["error"].is_null());
    assert!(file_entry.get("output_file").is_some());
    assert!(file_entry["output_file"].is_null());
    assert!(file_entry.get("fixed_sql").is_some());
    assert!(file_entry["fixed_sql"].is_null());
}

#[test]
fn json_multiple_files_grouped() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.sql");
    let b = dir.path().join("b.sql");
    std::fs::write(&a, "CREATE TABLE t (id SERIAL PRIMARY KEY);").unwrap();
    std::fs::write(&b, "CREATE TABLE u (id UUID PRIMARY KEY);").unwrap();

    let output = dsql_lint_bin()
        .arg("--format")
        .arg("json")
        .arg(a.to_str().unwrap())
        .arg(b.to_str().unwrap())
        .output()
        .unwrap();

    assert!(output.stderr.is_empty(), "JSON mode must not write to stderr");

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("should be valid JSON");

    let files = json["files"].as_array().expect("should have 'files' array");
    assert_eq!(files.len(), 2);
    assert!(!files[0]["diagnostics"].as_array().unwrap().is_empty());
    assert!(files[1]["diagnostics"].as_array().unwrap().is_empty());
}

#[test]
fn json_broken_pipe_exits_0() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.sql");
    std::fs::write(&path, "CREATE TABLE t (id SERIAL);").unwrap();

    let mut child = dsql_lint_bin()
        .arg("--format")
        .arg("json")
        .arg(&path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take().unwrap());

    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(0));
}
