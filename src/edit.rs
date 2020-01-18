extern crate chrono;
extern crate clap;

use crate::configure::Configuration;
use crate::log::{parse_line, timestamp, Item};
use crate::util::{base_dir, fatal, log_path, warn};
use chrono::{Local, NaiveDateTime};
use clap::{App, Arg, ArgMatches, SubCommand};
use std::fs::{copy, File};
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::PathBuf;
use std::process::Command;
use std::str::FromStr;

pub fn cli(mast: App<'static, 'static>, display_order: usize) -> App<'static, 'static> {
    mast.subcommand(
        SubCommand::with_name("edit")
            .aliases(&["e", "ed", "edi"])
            .about("open the job log in a text editor")
            .after_help("Sometimes you will")
            .display_order(display_order)
            .arg(
                Arg::with_name("validate")
                .long("validate")
                .help("validate the entire log, commenting out invalid lines")
                .long_help("If you have reason to believe your log has invalid lines -- if, for instance, you edited it without using this subcommand -- you can validate and clean it using --validate. This will convert invalid lines to comments preceded by comments marking them and explaining how they are invalid. It will announce upon completion whether it has found any invalid lines.")
            )
    )
}

pub fn run(matches: &ArgMatches) {
    let conf = Configuration::read(None);
    if matches.is_present("validate") {
        validation_messages(0, 0, &conf, None, None);
    } else {
        if let Some((editor, _)) = conf.effective_editor() {
            let backed_up_backup = backup_backup();
            copy(log_path(), backup(None)).expect("could not make backup log");
            let status = Command::new(&editor)
                .arg(log_path().to_str().unwrap())
                .status()
                .expect("failed to start editor process");
            if status.success() {
                if let Some((offset, line_number)) = find_change_offset(None, None) {
                    validation_messages(offset, line_number, &conf, None, None);
                } else {
                    println!("no change found in log file; deleting backup...");
                    restore_backup(backed_up_backup);
                }
            } else {
                fatal(
                    "the editor closed with an error; restoring log file from backup",
                    &conf,
                );
                copy(backup(None), log_path()).expect("could not restore log from backup");
                restore_backup(backed_up_backup);
                println!("done");
            }
        } else {
            fatal(
                "no text editor available; see `job configure --help`",
                &conf,
            )
        }
    }
}

fn restore_backup(backed_up_backup: bool) {
    std::fs::remove_file(backup(None)).expect("failed to remove log.bak");
    if backed_up_backup {
        copy(backup_backup_file(), backup(None))
            .expect("could not restore pre-existing backup file");
        std::fs::remove_file(backup_backup_file())
            .expect("could not removed backup of backup file");
    }
}

// backup the backup if it exists and return whether you did so
fn backup_backup() -> bool {
    if log_path().as_path().exists() {
        copy(log_path(), backup_backup_file()).expect("could not make backup log");
        true
    } else {
        false
    }
}

// scan for first line that differs
// returns byte count and line count
fn find_change_offset(log: Option<&str>, backup_file: Option<&str>) -> Option<(usize, usize)> {
    let edited = File::open(log_file(log)).expect("could not open edited log file for reading");
    let mut edited = BufReader::new(edited);
    let backup =
        File::open(backup(backup_file)).expect("could not backup log file to check for changes");
    let mut backup = BufReader::new(backup);
    let mut buf1 = String::new();
    let mut buf2 = String::new();
    let mut line_count: usize = 0;
    let mut byte_count: usize = 0;
    loop {
        let bytes = backup
            .read_line(&mut buf1)
            .expect("failed to read line from backup log file");
        edited
            .read_line(&mut buf2)
            .expect("failed to read line from edited log file");
        if buf1 != buf2 {
            return Some((byte_count, line_count));
        }
        if bytes == 0 {
            break;
        }
        byte_count += bytes;
        line_count += 1;
        buf1.clear();
        buf2.clear();
    }
    None
}

// backup log file
fn backup(file: Option<&str>) -> PathBuf {
    if let Some(file) = file {
        PathBuf::from_str(file).expect(&format!("could not create path from {}", file))
    } else {
        let mut backup = base_dir();
        backup.push("log.bak");
        backup
    }
}

// a backup of the backup in case (this should get cleaned up at the end of the process)
fn backup_backup_file() -> PathBuf {
    let mut backup = base_dir();
    backup.push("log.bak.bak");
    backup
}

fn validation_file(file: Option<&str>) -> PathBuf {
    if let Some(file) = file {
        PathBuf::from_str(file).expect(&format!("could not create path from {}", file))
    } else {
        let mut validation_file_path = base_dir();
        validation_file_path.push("log.validation");
        validation_file_path
    }
}

fn log_file(file: Option<&str>) -> PathBuf {
    if let Some(path) = file {
        PathBuf::from_str(path).expect(&format!("could not create a path with {}", path))
    } else {
        log_path()
    }
}

fn validation_messages(
    byte_offset: usize,
    starting_line: usize,
    conf: &Configuration,
    log: Option<&str>,
    validation_file_name: Option<&str>,
) {
    if let Some((line_number, count)) =
        validate(byte_offset, starting_line, log, validation_file_name)
    {
        if count > 1 {
            warn(
                format!(
                    "{} errors were found starting at line {}",
                    count, line_number
                ),
                conf,
            )
        } else {
            warn(format!("one error was found at line {}", line_number), conf)
        }
        copy(validation_file(validation_file_name), log_file(log))
            .expect("could not copy validation file to log");
        std::fs::remove_file(validation_file(validation_file_name))
            .expect("could not remove validation file");
    } else {
        println!("log is valid")
    }
}

// returns line number and error count
fn validate(
    byte_offset: usize,
    starting_line: usize,
    log: Option<&str>,
    validation: Option<&str>,
) -> Option<(usize, usize)> {
    let edited = File::open(log_file(log)).expect("could not open edited log file for reading");
    let mut reader = BufReader::new(edited);
    let validation_file = File::create(validation_file(validation).as_path())
        .expect("could not open file to receive validation output");
    let mut writer = BufWriter::new(validation_file);
    let mut bytes_written: usize = 0;
    let mut buffer: Vec<u8> = Vec::with_capacity(1024);
    // fill up the validation file up to the offset without validating
    while bytes_written < byte_offset {
        let delta = byte_offset - bytes_written;
        if delta < buffer.capacity() {
            buffer = Vec::with_capacity(delta);
        }
        reader
            .read_exact(&mut buffer)
            .expect("could not read from log file");
        bytes_written += buffer.capacity();
        writer
            .write_all(&buffer)
            .expect("could not write to validation file");
        buffer.clear();
    }
    // now start validating
    let mut buffer = String::new();
    let mut last_timestamp: Option<NaiveDateTime> = None;
    let mut line_number = starting_line;
    let mut first_error = 0;
    let mut error_count = 0;
    let now = Local::now().naive_local();
    loop {
        let bytes_read = reader.read_line(&mut buffer).expect("could not read line");
        if bytes_read == 0 {
            break;
        }
        let mut error_message: Option<String> = None;
        let mut time: Option<NaiveDateTime> = None;
        let item = parse_line(&buffer, line_number);
        match item {
            Item::Note(n, _) => {
                time = Some(n.time);
            }
            Item::Event(e, _) => {
                time = Some(e.start);
            }
            Item::Done(d, _) => {
                time = Some(d.0);
            }
            Item::Error(e, _) => {
                error_message = Some(e);
            }
            _ => (),
        }
        if error_message.is_none() {
            if let Some(t) = time {
                if t > now {
                    error_message = Some(String::from("timestamp in future"));
                } else if let Some(ot) = last_timestamp {
                    if ot > t {
                        error_message = Some(format!(
                            "timestamp out of order with earlier timestamp {}",
                            timestamp(&ot)
                        ));
                    }
                }
            }
            if error_message.is_none() {
                last_timestamp = time;
            }
        }
        if let Some(msg) = error_message {
            error_count += 1;
            if error_count == 1 {
                first_error = line_number + 1;
            }
            let bytes: Vec<u8> = format!(
                "# ERROR {} on {}: {}\n# ",
                error_count,
                now.format("%F at %r"),
                msg
            )
            .bytes()
            .collect();
            writer
                .write_all(&bytes)
                .expect("failed to write error message to validation file");
        }
        let bytes: Vec<u8> = buffer.bytes().collect();
        writer
            .write_all(&bytes)
            .expect("failed to write line to validation file");
        line_number += 1;
        buffer.clear();
    }
    if error_count > 0 {
        Some((first_error, error_count))
    } else {
        None
    }
}

// find_change_offset(log: Option<&str>, backup_file: Option<&str>)
// validation_messages(byte_offset: usize, starting_line: usize, conf: &Configuration, log: Option<&str>, validation_file_name: Option<&str>)
#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::{Event, LogLine};
    use chrono::{Duration, NaiveDate};

    fn create_log<T: ToString>(disambiguator: &str, lines: &[T]) -> (String, PathBuf) {
        let disambiguator = format!("log_{}", disambiguator);
        let log_buffer = PathBuf::from_str(&disambiguator)
            .expect(&format!("could not create log file for {}", disambiguator));
        let file = File::create(log_buffer.as_path())
            .expect("could not open file to receive validation output");
        let mut writer = BufWriter::new(file);
        for line in lines {
            let line = line.to_string();
            writeln!(writer, "{}", line).expect(&format!(
                "faile to write {} into log_{}",
                line, disambiguator
            ));
        }
        (disambiguator, log_buffer)
    }

    fn cleanup(files: Vec<PathBuf>) {
        for pb in files {
            if pb.as_path().exists() {
                std::fs::remove_file(pb).expect("could not cleanup file");
            }
        }
    }

    fn configuration_path(disambiguator: &str) -> PathBuf {
        PathBuf::from_str(&format!("{}_configuration", disambiguator)).expect("could not make path")
    }

    #[test]
    fn test_find_change_offset_when_no_change() {
        let disambiguator1 = "test_find_change_offset_when_no_change1";
        let disambiguator2 = "test_find_change_offset_when_no_change2";
        let lines = ["foo", "bar", "baz"];
        let (n1, log1) = create_log(disambiguator1, &lines);
        let (n2, log2) = create_log(disambiguator2, &lines);
        let diff = find_change_offset(Some(&n1), Some(&n2));
        assert!(diff.is_none(), "no difference found");
        cleanup(vec![log1, log2]);
    }

    #[test]
    fn test_find_change_offset_with_change() {
        let disambiguator1 = "test_find_change_offset_with_change1";
        let disambiguator2 = "test_find_change_offset_with_change2";
        let (n1, log1) = create_log(disambiguator1, &["foo", "bar", "baz"]);
        let (n2, log2) = create_log(disambiguator2, &["foo", "bar"]);
        let diff = find_change_offset(Some(&n1), Some(&n2));
        println!("{:?}", diff);
        assert!(diff.is_some(), "difference found");
        assert_eq!(2, diff.unwrap().1, "difference at third line");
        cleanup(vec![log1, log2]);
    }

    #[test]
    fn test_validation_messages_all_good() {
        let disambiguator = "test_validation_messages_all_good";
        let validation = format!("validation_{}", disambiguator);
        let validation_path = PathBuf::from_str(&validation).expect("could not make path");
        let conf_path = configuration_path(disambiguator);
        let conf = Configuration::read(Some(conf_path));
        let mut t = NaiveDate::from_ymd(2000, 1, 1).and_hms(0, 0, 0);
        let mut events = vec![];
        for duration in &[2, 1, 3] {
            let end_time = t + Duration::hours(*duration as i64);
            let mut event = Event::coin(format!("event {}", duration), vec![]);
            event.start = t.clone();
            event.end = Some(end_time.clone());
            t = end_time;
            events.push(event.to_line());
        }
        let (name, buff) = create_log(disambiguator, &events);
        let backup_name = format!("{}.bak", disambiguator);
        let (backup_name, backup_buff) = create_log(&backup_name, &events);
        validation_messages(0, 0, &conf, Some(&name), Some(&backup_name));
        cleanup(vec![
            buff,
            backup_buff,
            configuration_path(disambiguator),
            validation_path,
        ]);
    }
}
