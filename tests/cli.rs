//! Integration tests that exercise the compiled `job` binary end to end.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// A unique temporary directory for one test, removed on drop.
struct TestDir(PathBuf);

impl TestDir {
    fn new(name: &str) -> TestDir {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "jobrog-cli-test-{}-{}-{}",
            name,
            std::process::id(),
            id
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        TestDir(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn job(dir: &TestDir, args: &[&str]) -> Output {
    let mut full_args = vec!["-d", dir.path().to_str().unwrap()];
    full_args.extend_from_slice(args);
    Command::new(env!("CARGO_BIN_EXE_job"))
        .args(&full_args)
        .output()
        .expect("failed to run job binary")
}

/// Strip ANSI escape sequences so assertions don't depend on terminal styling.
fn stdout(output: &Output) -> String {
    let s = String::from_utf8_lossy(&output.stdout).into_owned();
    strip_ansi(&s)
}

fn stderr(output: &Output) -> String {
    let s = String::from_utf8_lossy(&output.stderr).into_owned();
    strip_ansi(&s)
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            // skip the escape sequence: ESC [ ... letter
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn assert_ok(output: &Output) {
    assert!(
        output.status.success(),
        "expected success, got {:?}\nstdout: {}\nstderr: {}",
        output.status.code(),
        stdout(output),
        stderr(output)
    );
}

#[test]
fn help_lists_all_subcommands() {
    let dir = TestDir::new("help");
    let output = job(&dir, &["--help"]);
    assert_ok(&output);
    let out = stdout(&output);
    for cmd in [
        "add",
        "summary",
        "done",
        "resume",
        "last",
        "first",
        "note",
        "when",
        "tag",
        "edit",
        "configure",
        "vacation",
        "parse-time",
        "truncate",
        "statistics",
    ] {
        assert!(out.contains(cmd), "help output should mention {cmd}");
    }
}

#[test]
fn subcommand_aliases_work() {
    let dir = TestDir::new("aliases");
    // 'a' is an alias of 'add'
    let output = job(&dir, &["a", "-t", "work", "did", "stuff"]);
    assert_ok(&output);
    assert!(stdout(&output).contains("did stuff"));
}

#[test]
fn add_note_done_and_query_round_trip() {
    let dir = TestDir::new("round-trip");
    assert_ok(&job(
        &dir,
        &["add", "-t", "work", "-t", "project", "wrote", "code"],
    ));
    assert_ok(&job(&dir, &["note", "-t", "lunch", "ate", "lunch"]));
    assert_ok(&job(&dir, &["done"]));
    assert_ok(&job(&dir, &["add", "-t", "overhead", "standup"]));

    // the log file records tags and description in colon-separated form
    let log = fs::read_to_string(dir.path().join("log")).unwrap();
    assert!(
        log.contains("project work:wrote code"),
        "log has tags: {}",
        log
    );
    assert!(
        log.contains("<NOTE>lunch:ate lunch"),
        "log has note: {}",
        log
    );
    assert!(log.contains(":DONE"), "log has DONE marker: {}", log);

    // last shows the most recent event
    let out = stdout(&job(&dir, &["last"]));
    assert!(out.contains("standup"), "last shows latest event: {}", out);

    // last with a tag filter shows the matching event instead
    let out = stdout(&job(&dir, &["last", "--tag", "work"]));
    assert!(out.contains("wrote code"), "filtered last: {}", out);
    assert!(
        !out.contains("standup"),
        "filtered last excludes others: {}",
        out
    );

    // first shows the earliest event
    let out = stdout(&job(&dir, &["first"]));
    assert!(
        out.contains("wrote code"),
        "first shows earliest event: {}",
        out
    );

    // summary lists both events and the note
    let out = stdout(&job(&dir, &["summary"]));
    assert!(
        out.contains("wrote code"),
        "summary has first event: {}",
        out
    );
    assert!(out.contains("standup"), "summary has second event: {}", out);
    let out = stdout(&job(&dir, &["summary", "--notes"]));
    assert!(out.contains("ate lunch"), "summary --notes: {}", out);
}

#[test]
fn resume_repeats_last_event() {
    let dir = TestDir::new("resume");
    assert_ok(&job(&dir, &["add", "-t", "work", "first", "task"]));
    assert_ok(&job(&dir, &["done"]));
    assert_ok(&job(&dir, &["resume"]));
    let log = fs::read_to_string(dir.path().join("log")).unwrap();
    let count = log.matches("work:first task").count();
    assert_eq!(count, 2, "resume should duplicate the event line: {}", log);
}

#[test]
fn parse_time_reports_range() {
    let dir = TestDir::new("parse-time");
    let output = job(&dir, &["parse-time", "9am-5pm"]);
    assert_ok(&output);
    let out = stdout(&output);
    assert!(out.contains("09:00:00"), "parse-time start: {}", out);
    assert!(out.contains("17:00"), "parse-time end: {}", out);

    // an unparsable expression fails with a nonzero exit status
    let output = job(&dir, &["parse-time", "not a time at all !!!"]);
    assert!(!output.status.success());
}

#[test]
fn configure_sets_precision() {
    let dir = TestDir::new("configure");
    assert_ok(&job(&dir, &["configure", "--precision", "3"]));
    let conf = fs::read_to_string(dir.path().join("config.ini")).unwrap();
    assert!(conf.contains("precision=3"), "config.ini: {}", conf);
    // setting it again warns that it is already set but still succeeds
    let output = job(&dir, &["configure", "--precision", "3"]);
    assert_ok(&output);
    let all = format!("{}{}", stdout(&output), stderr(&output));
    assert!(all.contains("already"), "warns about no-op: {}", all);
}

#[test]
fn vacation_add_and_list() {
    let dir = TestDir::new("vacation");
    assert_ok(&job(
        &dir,
        &["vacation", "--add", "--when", "2024-12-25", "Christmas"],
    ));
    let output = job(&dir, &["vacation", "--list"]);
    assert_ok(&output);
    let out = stdout(&output);
    assert!(out.contains("Christmas"), "vacation list: {}", out);
    assert!(out.contains("2024-12-25"), "vacation date: {}", out);
    let vac = fs::read_to_string(dir.path().join("vacation")).unwrap();
    assert!(vac.contains("2024 12 25"), "vacation file: {}", vac);
}

#[test]
fn tag_modifies_event() {
    let dir = TestDir::new("tag");
    assert_ok(&job(&dir, &["add", "-t", "work", "tagged", "task"]));
    assert_ok(&job(&dir, &["done"]));
    // tag the only event in today's period
    let output = job(&dir, &["tag", "--add", "extra"]);
    assert_ok(&output);
    let log = fs::read_to_string(dir.path().join("log")).unwrap();
    assert!(
        log.contains("extra work:tagged task"),
        "tagged log: {}",
        log
    );
}

#[test]
fn truncate_moves_old_lines_to_backup() {
    let dir = TestDir::new("truncate");
    // write a log directly: several old lines, then recent lines
    let mut contents = String::from("# job log\n");
    for day in 1..=5 {
        contents.push_str(&format!("2020 1 {} 9 0 0:work:old task {}\n", day, day));
        contents.push_str(&format!("2020 1 {} 17 0 0:DONE\n", day));
    }
    contents.push_str("2026 11 21 9 0 0:work:recent task\n");
    contents.push_str("2026 11 21 17 0 0:DONE\n");
    fs::write(dir.path().join("log"), contents).unwrap();

    // events earlier than 2021-01-01 are moved to the head file, the rest stay
    assert_ok(&job(&dir, &["truncate", "2021-01-01"]));
    let backups: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("log.head-to-"))
        .collect();
    assert_eq!(backups.len(), 1, "expected one backup file");
    let backup = fs::read_to_string(backups[0].path()).unwrap();
    assert!(
        backup.contains("old task"),
        "backup holds removed lines: {}",
        backup
    );
    let log = fs::read_to_string(dir.path().join("log")).unwrap();
    assert!(
        log.contains("recent task"),
        "log keeps recent lines: {}",
        log
    );

    // truncating at a time after every line must not hang or crash
    assert_ok(&job(&dir, &["truncate", "tomorrow"]));
}

#[test]
fn statistics_reports_counts() {
    let dir = TestDir::new("statistics");
    assert_ok(&job(&dir, &["add", "-t", "work", "task", "one"]));
    assert_ok(&job(&dir, &["done"]));
    assert_ok(&job(&dir, &["note", "a", "note"]));
    let output = job(&dir, &["statistics"]);
    assert_ok(&output);
    let out = stdout(&output);
    assert!(out.contains("lines"), "statistics: {}", out);
    assert!(out.contains("events"), "statistics: {}", out);
    assert!(out.contains("notes"), "statistics: {}", out);
}

#[test]
fn invalid_subcommand_fails() {
    let dir = TestDir::new("invalid");
    let output = job(&dir, &["frobnicate"]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("unrecognized subcommand"));
}

#[test]
fn missing_description_fails() {
    let dir = TestDir::new("missing-desc");
    let output = job(&dir, &["add"]);
    assert!(!output.status.success());
}
