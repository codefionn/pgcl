use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn test_value_1() -> anyhow::Result<()> {
    let mut cmd = Command::cargo_bin("pgcl")?;

    cmd.write_stdin("1")
        .assert()
        .success()
        .stdout(predicate::str::ends_with("1\n"));

    Ok(())
}

#[test]
fn test_addition() -> anyhow::Result<()> {
    let mut cmd = Command::cargo_bin("pgcl")?;

    cmd.write_stdin("1 + 3")
        .assert()
        .success()
        .stdout(predicate::str::ends_with("4\n"));

    Ok(())
}
