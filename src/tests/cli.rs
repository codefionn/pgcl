use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn test_nothing() -> anyhow::Result<()> {
    let mut cmd = Command::cargo_bin("pgcl")?;

    cmd.assert().success().stdout(predicate::str::is_empty());

    Ok(())
}

#[test]
fn test_two_values() -> anyhow::Result<()> {
    let mut cmd = Command::cargo_bin("pgcl")?;

    cmd.write_stdin("1\n2")
        .assert()
        .success()
        .stdout(predicate::str::is_match(r"^1\n2\n$").unwrap());

    Ok(())
}

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

#[test]
fn test_import_fib() -> anyhow::Result<()> {
    let mut cmd = Command::cargo_bin("pgcl")?;
    println!("{:?}", std::env::current_dir());

    cmd.write_stdin("fib = import \"./examples/fib.pgcl\"\nfib.fib 10")
        .assert()
        .success()
        .stdout(predicate::str::is_match(r"^_\n55\n$").unwrap());

    Ok(())
}

#[test]
fn test_import_time_fib() -> anyhow::Result<()> {
    let mut cmd = Command::cargo_bin("pgcl")?;
    println!("{:?}", std::env::current_dir());

    cmd.write_stdin(
        "time = (import sys).time\nfib = (import \"./examples/fib.pgcl\").fib\ntime (fib 10)",
    )
    .assert()
    .success()
    .stdout(predicate::str::is_match(r"^_\n_\n\([0-9]+\.[0-9]+, 55\)\n$").unwrap());

    Ok(())
}

#[test]
fn test_import_id_with_map_match() -> anyhow::Result<()> {
    let mut cmd = Command::cargo_bin("pgcl")?;
    println!("{:?}", std::env::current_dir());

    cmd.write_stdin("{ id } = import std\nid (12 + 10)")
        .assert()
        .success()
        .stdout(predicate::str::is_match(r"^_\n22\n$").unwrap());

    Ok(())
}
