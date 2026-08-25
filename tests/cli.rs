use std::process::Command;

fn assert_runs_count(args: &[&str]) {
    let output = Command::new(env!("CARGO_BIN_EXE_prompt"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(args)
        .output()
        .expect("prompt should run");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "prompt failed with stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.starts_with("Total tokens: "),
        "Count should run, but stdout was:\n{stdout}"
    );
}

#[test]
fn parses_variadic_file_options_for_count() {
    assert_runs_count(&[
        "count",
        "-p",
        "src",
        "Cargo.toml",
        "-e",
        "never-match-one",
        "never-match-two",
    ]);
}
