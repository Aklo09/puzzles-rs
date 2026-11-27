//! The `puzzles-cli` binary end to end: arguments, stdin, output and
//! exit codes.

use std::io::Write;
use std::process::{Command, Output, Stdio};

fn cli(args: &[&str], stdin: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_puzzles-cli"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("runs");
    child
        .stdin
        .take()
        .expect("piped")
        .write_all(stdin)
        .expect("writes");
    child.wait_with_output().expect("finishes")
}

fn stdout(o: &Output) -> String {
    String::from_utf8(o.stdout.clone()).expect("UTF-8")
}

#[test]
fn generate_prints_ids() {
    let o = cli(&["generate", "flip", "3x3c#42"], b"");
    assert!(o.status.success());
    assert_eq!(stdout(&o), "3x3:d074191345d1644c17058,f80\n");

    let o = cli(&["generate", "Flip", "-n", "4"], b"");
    assert!(o.status.success());
    assert_eq!(stdout(&o).lines().count(), 4);
    assert!(stdout(&o).lines().all(|id| id.starts_with("5x5:")));
}

#[test]
fn solve_reads_arguments_or_stdin() {
    let id = "3x3:d074191345d1644c17058,f80";
    let o = cli(&["solve", "flip", "--move", id], b"");
    assert_eq!(stdout(&o), "S011011001\n");
    let o = cli(
        &["solve", "flip", "--move"],
        format!("{id}\n\n{id}\n").as_bytes(),
    );
    assert_eq!(stdout(&o), "S011011001\nS011011001\n");

    let o = cli(&["solve", "flip", "--move", "2x1:a,4", id], b"");
    assert!(!o.status.success(), "one failed");
    assert_eq!(stdout(&o), "S011011001\n", "the other still solved");
    assert!(String::from_utf8_lossy(&o.stderr).contains("No solution exists"));
}

#[test]
fn roundtrip_rewrites_a_save_file() {
    let save = b"SAVEFILE:41:Simon Tatham's Portable Puzzle Collection\n\
VERSION :1:1\nGAME    :4:Flip\nPARAMS  :4:3x3c\nCPARAMS :4:3x3c\n\
DESC    :25:d074191345d1644c17058,f80\nNSTATES :1:2\nSTATEPOS:1:2\nMOVE    :4:M1,1\n";
    let o = cli(&["roundtrip"], save);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(o.stdout, save);

    let o = cli(&["roundtrip"], b"junk");
    assert!(!o.status.success());
    assert_eq!(
        String::from_utf8_lossy(&o.stderr),
        "Data does not appear to be a saved game file\n"
    );
}

#[test]
fn unknown_games_list_the_known_ones() {
    let o = cli(&["generate", "chess"], b"");
    assert!(!o.status.success());
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("games: ") && err.contains("Flip"), "{err}");
}
