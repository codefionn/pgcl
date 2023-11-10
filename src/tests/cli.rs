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

    cmd.write_stdin("fib = import \"./examples/fib.pgcl\"\nfib.fib 10")
        .assert()
        .success()
        .stdout(predicate::str::is_match(r"^55\n$").unwrap());

    Ok(())
}

#[test]
fn test_import_time_fib() -> anyhow::Result<()> {
    let mut cmd = Command::cargo_bin("pgcl")?;

    cmd.write_stdin(
        "time = (import sys).time\nfib = (import \"./examples/fib.pgcl\").fib\ntime (fib 10)",
    )
    .assert()
    .success()
    .stdout(predicate::str::is_match(r"^\([0-9]+\.[0-9]+, 55\)\n$").unwrap());

    Ok(())
}

#[test]
fn test_import_id_with_map_match() -> anyhow::Result<()> {
    let mut cmd = Command::cargo_bin("pgcl")?;
    cmd.write_stdin("{ id } = import std;id (12 + 10)")
        .assert()
        .success()
        .stdout(predicate::str::is_match(r"^22\n$").unwrap());

    let mut cmd = Command::cargo_bin("pgcl")?;
    cmd.write_stdin("{ id } = import std\nid (12 + 10)\n")
        .assert()
        .success()
        .stdout(predicate::str::is_match(r"^22\n$").unwrap());

    let mut cmd = Command::cargo_bin("pgcl")?;
    cmd.write_stdin("{ id } = import std\nid (12 + 10)")
        .assert()
        .success()
        .stdout(predicate::str::is_match(r"^22\n$").unwrap());

    Ok(())
}

#[test]
fn test_sys_println() -> anyhow::Result<()> {
    let mut cmd = Command::cargo_bin("pgcl")?;

    cmd.write_stdin("sys = import sys\nsys.println \"Hello, world\"")
        .assert()
        .success()
        .stdout(predicate::str::is_match(r"Hello, world").unwrap());

    Ok(())
}

#[test]
fn test_sys_println_with_program() -> anyhow::Result<()> {
    let mut cmd = Command::cargo_bin("pgcl")?;

    cmd.write_stdin("sys = import sys;sys.println \"Hello, world\"")
        .assert()
        .success()
        .stdout(predicate::str::is_match(r"^Hello, world\n$").unwrap());

    Ok(())
}

#[test]
fn test_sys_println_with_program_with_newline() -> anyhow::Result<()> {
    let mut cmd = Command::cargo_bin("pgcl")?;

    cmd.write_stdin("sys = import sys;\nsys.println \"Hello, world\"")
        .assert()
        .success()
        .stdout(predicate::str::is_match(r"^Hello, world\n$").unwrap());

    Ok(())
}

#[test]
fn test_httprequest() -> anyhow::Result<()> {
    let mut cmd = Command::cargo_bin("pgcl")?;

    cmd.write_stdin("((import sys).fetch \"https://github.com/codefionn/pgcl\").ok")
        .assert()
        .success()
        .stdout(predicate::str::is_match(r"^@true\n$")?);

    Ok(())
}

#[test]
fn test_execute_script() -> anyhow::Result<()> {
    let mut cmd = Command::cargo_bin("pgcl")?;

    cmd.arg("examples/fib10.pgcl")
        .assert()
        .success()
        .stdout(predicate::str::is_match(r"^55\n$")?);

    Ok(())
}

#[test]
fn test_execute_script_with_actor() -> anyhow::Result<()> {
    let mut cmd = Command::cargo_bin("pgcl")?;

    cmd.arg("examples/actor_fib10.pgcl")
        .assert()
        .success()
        .stdout(predicate::str::is_match(r"^55\n$")?);

    Ok(())
}

#[test]
fn test_foldr() -> anyhow::Result<()> {
    let mut cmd = Command::cargo_bin("pgcl")?;

    cmd.write_stdin(r"(import std).foldl (\x \y x - y) 0 [1, 2, 3, 4, 5]")
        .assert()
        .success()
        .stdout(predicate::str::is_match(r"^-15\n$")?);

    Ok(())
}

#[test]
fn test_exit_0() -> anyhow::Result<()> {
    let mut cmd = Command::cargo_bin("pgcl")?;

    cmd.pipe_stdin("tests/exit_0.pgcl")?
        .assert()
        .code(predicate::eq(0))
        .stdout(predicate::str::is_match(r"^$")?);

    Ok(())
}

#[test]
fn test_exit_1() -> anyhow::Result<()> {
    let mut cmd = Command::cargo_bin("pgcl")?;

    cmd.pipe_stdin("tests/exit_1.pgcl")?
        .assert()
        .code(predicate::eq(1))
        .stdout(predicate::str::is_match(r"^$")?);

    Ok(())
}

#[test]
fn test_exit_2() -> anyhow::Result<()> {
    let mut cmd = Command::cargo_bin("pgcl")?;

    cmd.pipe_stdin("tests/exit_2.pgcl")?
        .assert()
        .code(predicate::eq(2))
        .stdout(predicate::str::is_match(r"^$")?);

    Ok(())
}

#[test]
fn test_exit_0_direct() -> anyhow::Result<()> {
    let mut cmd = Command::cargo_bin("pgcl")?;

    cmd.arg("tests/exit_0.pgcl")
        .assert()
        .code(predicate::eq(0))
        .stdout(predicate::str::is_match(r"^$")?);

    Ok(())
}

#[test]
fn test_exit_1_direct() -> anyhow::Result<()> {
    let mut cmd = Command::cargo_bin("pgcl")?;

    cmd.arg("tests/exit_1.pgcl")
        .assert()
        .code(predicate::eq(1))
        .stdout(predicate::str::is_match(r"^$")?);

    Ok(())
}

#[test]
fn test_exit_2_direct() -> anyhow::Result<()> {
    let mut cmd = Command::cargo_bin("pgcl")?;

    cmd.arg("tests/exit_2.pgcl")
        .assert()
        .code(predicate::eq(2))
        .stdout(predicate::str::is_match(r"^$")?);

    Ok(())
}

#[test]
fn test_exit_0_import() -> anyhow::Result<()> {
    let mut cmd = Command::cargo_bin("pgcl")?;

    cmd.write_stdin("import \"tests/exit_0.pgcl\"")
        .assert()
        .code(predicate::eq(0))
        .stdout(predicate::str::is_match(r"^$")?);

    Ok(())
}

#[test]
fn test_exit_1_import() -> anyhow::Result<()> {
    let mut cmd = Command::cargo_bin("pgcl")?;

    cmd.write_stdin("import \"tests/exit_1.pgcl\"")
        .assert()
        .code(predicate::eq(1))
        .stdout(predicate::str::is_match(r"^$")?);

    Ok(())
}

#[test]
fn test_exit_2_import() -> anyhow::Result<()> {
    let mut cmd = Command::cargo_bin("pgcl")?;

    cmd.write_stdin("import \"tests/exit_2.pgcl\"")
        .assert()
        .code(predicate::eq(2))
        .stdout(predicate::str::is_match(r"^$")?);

    Ok(())
}

#[test]
fn test_immediate() -> anyhow::Result<()> {
    let mut cmd = Command::cargo_bin("pgcl")?;

    cmd.write_stdin(r"(std = $ import std); std.map (\x x * x) [0, 1, 2, 3]")
        .assert()
        .success()
        .stdout(predicate::str::is_match(r"^\[0, 1, 4, 9\]\n$")?);

    let mut cmd = Command::cargo_bin("pgcl")?;

    cmd.write_stdin(r"std = $ import std; std.map (\x x * x) [0, 1, 2, 3]")
        .assert()
        .success()
        .stdout(predicate::str::is_match(r"^\[0, 1, 4, 9\]\n$")?);

    let mut cmd = Command::cargo_bin("pgcl")?;

    cmd.write_stdin(r"(std = $ import std; std.map (\x x * x) [0, 1, 2, 3])")
        .assert()
        .success()
        .stdout(predicate::str::is_match(r"^\[0, 1, 4, 9\]\n$")?);

    Ok(())
}

#[test]
fn test_pipe() -> anyhow::Result<()> {
    let mut cmd = Command::cargo_bin("pgcl")?;

    cmd.write_stdin(r"4 | (+) 1")
        .assert()
        .success()
        .stdout(predicate::str::is_match(r"^5\n$")?);

    Ok(())
}

#[test]
fn test_import_natural() -> anyhow::Result<()> {
    let mut cmd = Command::cargo_bin("pgcl")?;

    cmd.write_stdin("nat = import \"./examples/natural.pgcl\"\nnat.num.ten")
        .assert()
        .success()
        .stdout(predicate::str::is_match(r"^\(@succ \(@succ \(@succ \(@succ \(@succ \(@succ \(@succ \(@succ \(@succ \(@succ @zero\)\)\)\)\)\)\)\)\)\)\n$").unwrap());

    Ok(())
}

#[test]
fn test_assert() -> anyhow::Result<()> {
    let mut cmd = Command::cargo_bin("pgcl")?;
    cmd.arg("./examples/assert_success.pgcl").assert().success();

    let mut cmd = Command::cargo_bin("pgcl")?;
    cmd.arg("./examples/assert_failure.pgcl").assert().failure();

    let mut cmd = Command::cargo_bin("pgcl")?;
    cmd.arg("./examples/assert_failure_msg.pgcl")
        .assert()
        .failure()
        .stderr(predicate::str::is_match(r"This is a test").unwrap());

    Ok(())
}

#[test]
fn test_assert_import() -> anyhow::Result<()> {
    let mut cmd = Command::cargo_bin("pgcl")?;

    cmd.write_stdin("import \"./examples/assert_success.pgcl\"\n")
        .assert()
        .success();

    let mut cmd = Command::cargo_bin("pgcl")?;
    cmd.write_stdin("import \"./examples/assert_failure.pgcl\"\n")
        .assert()
        .failure();

    let mut cmd = Command::cargo_bin("pgcl")?;
    cmd.write_stdin("import \"./examples/assert_failure_msg.pgcl\"\n")
        .assert()
        .failure()
        .stderr(predicate::str::is_match(r"This is a test").unwrap());

    Ok(())
}

#[test]
fn test_assert_stdin() -> anyhow::Result<()> {
    let mut cmd = Command::cargo_bin("pgcl")?;
    cmd.pipe_stdin("./examples/assert_success.pgcl")
        .unwrap()
        .assert()
        .success();

    let mut cmd = Command::cargo_bin("pgcl")?;
    cmd.pipe_stdin("./examples/assert_failure.pgcl")
        .unwrap()
        .assert()
        .failure();

    let mut cmd = Command::cargo_bin("pgcl")?;
    cmd.pipe_stdin("./examples/assert_failure_msg.pgcl")
        .unwrap()
        .assert()
        .failure()
        .stderr(predicate::str::is_match(r"This is a test").unwrap());

    Ok(())
}

#[test]
fn test_debug() -> anyhow::Result<()> {
    let mut cmd = Command::cargo_bin("pgcl")?;
    cmd.arg("./examples/debug_assert_success.pgcl")
        .assert()
        .success();

    let mut cmd = Command::cargo_bin("pgcl")?;
    cmd.arg("./examples/debug_assert_failure.pgcl")
        .assert()
        .success();

    let mut cmd = Command::cargo_bin("pgcl")?;
    cmd.arg("--debug")
        .arg("./examples/debug_assert_success.pgcl")
        .assert()
        .success();

    let mut cmd = Command::cargo_bin("pgcl")?;
    cmd.arg("--debug")
        .arg("./examples/debug_assert_failure.pgcl")
        .assert()
        .failure();

    Ok(())
}

#[test]
fn test_debug_stdin() -> anyhow::Result<()> {
    let mut cmd = Command::cargo_bin("pgcl")?;
    cmd.pipe_stdin("./examples/debug_assert_success.pgcl")
        .unwrap()
        .assert()
        .success();

    let mut cmd = Command::cargo_bin("pgcl")?;
    cmd.pipe_stdin("./examples/debug_assert_failure.pgcl")
        .unwrap()
        .assert()
        .success();

    let mut cmd = Command::cargo_bin("pgcl")?;
    cmd.arg("--debug")
        .pipe_stdin("./examples/debug_assert_success.pgcl")
        .unwrap()
        .assert()
        .success();

    let mut cmd = Command::cargo_bin("pgcl")?;
    cmd.arg("--debug")
        .pipe_stdin("./examples/debug_assert_failure.pgcl")
        .unwrap()
        .assert()
        .failure();

    Ok(())
}

#[test]
fn test_std() -> anyhow::Result<()> {
    let mut cmd = Command::cargo_bin("pgcl")?;
    cmd.arg("./tests/std.pgcl").assert().success();

    Ok(())
}

#[test]
fn test_std_stdin() -> anyhow::Result<()> {
    let mut cmd = Command::cargo_bin("pgcl")?;
    cmd.pipe_stdin("./tests/std.pgcl")
        .unwrap()
        .assert()
        .success();

    Ok(())
}

#[test]
fn test_str_syscall_stdin() -> anyhow::Result<()> {
    let mut cmd = Command::cargo_bin("pgcl")?;
    cmd.pipe_stdin("./tests/str_syscall.pgcl")
        .unwrap()
        .assert()
        .success()
        .stdout(predicate::str::is_match(r"true\nfalse\n").unwrap());

    Ok(())
}

#[test]
fn test_str_syscall() -> anyhow::Result<()> {
    let mut cmd = Command::cargo_bin("pgcl")?;
    cmd.arg("./tests/str_syscall.pgcl")
        .assert()
        .success()
        .stdout(predicate::str::is_match(r"true\nfalse\n").unwrap());

    Ok(())
}

#[test]
fn test_str_import() -> anyhow::Result<()> {
    let mut cmd = Command::cargo_bin("pgcl")?;
    cmd.write_stdin("import \"./tests/str_syscall.pgcl\"")
        .assert()
        .success()
        .stdout(predicate::str::is_match(r"true\nfalse\n").unwrap());

    Ok(())
}
