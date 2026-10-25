use assert_cmd::Command;
use predicates::prelude::*;

fn bin() -> Command {
    Command::cargo_bin("rexboard").expect("binary")
}

#[test]
fn version_prints_name() {
    bin()
        .arg("--version")
        .assert()
        .success()
        .stdout("rexboard 0.0.0\n");
}

#[test]
fn unknown_flag_with_json_is_usage_json() {
    bin()
        .args(["--nope", "--json"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("\"code\":\"USAGE\""))
        .stderr(predicate::str::contains("--nope"));
}

#[test]
fn no_command_prints_help_and_exits_2() {
    bin()
        .assert()
        .code(2)
        .stderr(predicate::str::contains("Usage:"));
}

#[test]
fn no_command_with_json_is_usage_json() {
    bin()
        .arg("--json")
        .assert()
        .code(2)
        .stderr(
            "{\"error\":{\"code\":\"USAGE\",\"message\":\"a command is required\",\"next_steps\":[\"rexboard --help\"]},\"ok\":false}\n",
        );
}
