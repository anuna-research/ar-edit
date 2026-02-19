//! TEST-030: Exit codes (CON-009)
//!
//! Verifies that the `ar-edit` binary returns correct exit codes:
//!   0 = SUCCESS
//!   1 = USER_ERROR
//!   2 = SYSTEM_ERROR
//!   3 = VALIDATION_ERROR
//!
//! Since command handlers are not yet wired up, we test exit code behaviour
//! via clap-level validation (invalid args, help flag, etc.) and verify
//! the binary builds and runs.

use assert_cmd::Command;

fn ar_edit() -> Command {
    #[allow(deprecated)]
    Command::cargo_bin("ar-edit").expect("binary ar-edit should be built")
}

// -- Success (exit 0) --------------------------------------------------------

#[test]
fn help_flag_exits_zero() {
    ar_edit().arg("--help").assert().success();
}

#[test]
fn subcommand_help_exits_zero() {
    ar_edit().args(["init", "--help"]).assert().success();
}

#[test]
fn edit_subcommand_help_exits_zero() {
    ar_edit().args(["edit", "--help"]).assert().success();
}

// -- Non-zero exit on invalid usage ------------------------------------------

#[test]
fn no_args_exits_nonzero() {
    ar_edit().assert().failure();
}

#[test]
fn unknown_subcommand_exits_nonzero() {
    ar_edit().arg("nonexistent").assert().failure();
}

#[test]
fn init_missing_name_exits_nonzero() {
    ar_edit().arg("init").assert().failure();
}

#[test]
fn add_missing_files_exits_nonzero() {
    ar_edit().arg("add").assert().failure();
}

#[test]
fn render_missing_required_args_exits_nonzero() {
    ar_edit().arg("render").assert().failure();
}

// -- Conflicting range flags rejected ----------------------------------------

#[test]
fn mixed_range_types_rejected() {
    ar_edit()
        .args([
            "edit", "add-segment", "my-edit",
            "--source", "src-001",
            "--from-word", "0",
            "--to-word", "10",
            "--from-scene", "1",
            "--to-scene", "3",
        ])
        .assert()
        .failure();
}

// -- JSON flag is accepted globally ------------------------------------------

#[test]
fn json_flag_accepted_with_help() {
    ar_edit().args(["--json", "--help"]).assert().success();
}

// -- Exit code 2 for clap errors (standard clap behaviour) -------------------

#[test]
fn clap_error_exits_with_code_2() {
    let output = ar_edit().output().expect("failed to run ar-edit");
    // clap exits with code 2 for usage errors
    assert_eq!(
        output.status.code(),
        Some(2),
        "clap usage errors should exit with code 2"
    );
}
