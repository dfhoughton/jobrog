extern crate chrono;
extern crate clap;

use crate::configure::Configuration;
use crate::log::{parse_line, timestamp, Item, LogController};
use crate::util::{base_dir, fatal, log_path, success, warn};
use chrono::{Local, NaiveDateTime};
use clap::{App, Arg, ArgMatches, SubCommand};
use std::fs::{copy, File};
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::PathBuf;
use std::process::Command;
use std::str::FromStr;

const BUFFER_SIZE: usize = 16 * 1024;

fn after_help() -> &'static str {
    "\
Sometimes you will fail to log a change of tasks, fail to log out at the end \
of the day, or find you spent more time than is allowed at lunch. In these cases you \
need to edit the log manually. The edit subcommand will open a text editor for you and \
validate the changes once you save and close the editor. If it finds any errors, it will \
comment them out, provide a preceding explanation of the error, and notify you of the number \
of errors it found and the line number of the first error. It also creates a backup of the log \
file before it opens the editor, so if need be you can destroy the botched log file and restore \
the backup. You will have to do this manually. If it finds no errors it will destroy the backup \
and restore any pre-existing backup it may have found."
}

pub fn cli(mast: App<'static, 'static>, display_order: usize) -> App<'static, 'static> {
    mast.subcommand(
        SubCommand::with_name("edit")
            .aliases(&["e", "ed", "edi"])
            .about("Opens the job log in a text editor")
            .after_help(after_help())
            .display_order(display_order)
            .arg(
                Arg::with_name("validate")
                .long("validate")
                .help("Validates the entire log, commenting out invalid lines")
                .long_help("If you have reason to believe your log has invalid lines -- if, \
                for instance, you edited it without using this subcommand -- \
                you can validate and clean it using --validate.")
                .conflicts_with("error-comments")
            )
            .arg(
                Arg::with_name("error-comments")
                .long("error-comments")
                .short("e")
                .help("Finds comment lines marking errors")
                .long_help("After validation some lines may be marked as invalid. This means the lines themselves \
                are converted into comments preceded by comments beginning '# ERROR'. Ideally one \
                immediately fixes these errors, removing the error markers. --error-comments checks whether any remain.")
                .conflicts_with("validate")
            )
    )
}

pub fn run(directory: Option<&str>, matches: &ArgMatches) {
    let conf = Configuration::read(None, directory);
    if matches.is_present("validate") {
        validation_messages(0, 0, &conf, None, None, None);
    } else if matches.is_present("error-comments") {
        let mut log = LogController::new(None, &conf).expect("could not open log for validation");
        let mut error_lines: Vec<String> = vec![];
        for item in log.items() {
            match item {
                Item::Comment(line_offset) => {
                    let line = log
                        .larry
                        .get(line_offset)
                        .expect(&format!("failed to read line {}", line_offset + 1));
                    if line.starts_with("# ERROR") {
                        error_lines.push((line_offset + 1).to_string());
                    }
                }
                _ => (),
            }
        }
        if error_lines.is_empty() {
            success("no error comments found", &conf);
        } else {
            if error_lines.len() == 1 {
                warn(
                    format!("found an error comment at line {}", error_lines[0]),
                    &conf,
                )
            } else {
                let list = error_lines.join(", ");
                warn(
                    format!("found error comments at these lines: {}", list),
                    &conf,
                );
            }
        }
    } else {
        if let Some((mut args, _)) = conf.effective_editor() {
            let editor = args.remove(0);
            let mut command = Command::new(&editor);
            while !args.is_empty() {
                command.arg(args.remove(0));
            }
            let backed_up_backup = backup_backup(conf.directory());
            copy(log_path(conf.directory()), backup(None, conf.directory()))
                .expect("could not make backup log");
            let status = command
                .arg(
                    log_path(conf.directory())
                        .to_str()
                        .expect("failed to obtain log path"),
                )
                .status()
                .expect("failed to start editor process");
            if status.success() {
                if let Some((offset, line_number)) =
                    find_change_offset(None, None, conf.directory())
                {
                    validation_messages(offset, line_number, &conf, None, None, None);
                } else {
                    success("no change found in log file; deleting backup...", &conf);
                    restore_backup(backed_up_backup, conf.directory());
                }
            } else {
                fatal(
                    "the editor closed with an error; restoring log file from backup",
                    &conf,
                );
                copy(backup(None, conf.directory()), log_path(conf.directory()))
                    .expect("could not restore log from backup");
                restore_backup(backed_up_backup, conf.directory());
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

fn restore_backup(backed_up_backup: bool, directory: Option<&str>) {
    std::fs::remove_file(backup(None, directory)).expect("failed to remove log.bak");
    if backed_up_backup {
        copy(backup_backup_file(directory), backup(None, directory))
            .expect("could not restore pre-existing backup file");
        std::fs::remove_file(backup_backup_file(directory))
            .expect("could not removed backup of backup file");
    }
}

// backup the backup if it exists and return whether you did so
fn backup_backup(directory: Option<&str>) -> bool {
    if backup(None, directory).as_path().exists() {
        copy(backup(None, directory), backup_backup_file(directory))
            .expect("could not make backup log");
        true
    } else {
        false
    }
}

// scan for first line that differs
// returns byte count and line count
fn find_change_offset(
    log: Option<&str>,
    backup_file: Option<&str>,
    directory: Option<&str>,
) -> Option<(usize, usize)> {
    let edited =
        File::open(log_file(log, directory)).expect("could not open edited log file for reading");
    let mut edited = BufReader::new(edited);
    let backup = File::open(backup(backup_file, directory))
        .expect("could not backup log file to check for changes");
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
fn backup(file: Option<&str>, directory: Option<&str>) -> PathBuf {
    if let Some(file) = file {
        PathBuf::from_str(file).expect(&format!("could not create path from {}", file))
    } else {
        let mut backup = base_dir(directory);
        backup.push("log.bak");
        backup
    }
}

// a backup of the backup in case (this should get cleaned up at the end of the process)
fn backup_backup_file(directory: Option<&str>) -> PathBuf {
    let mut backup = base_dir(directory);
    backup.push("log.bak.bak");
    backup
}

fn validation_file(file: Option<&str>, directory: Option<&str>) -> PathBuf {
    if let Some(file) = file {
        PathBuf::from_str(file).expect(&format!("could not create path from {}", file))
    } else {
        let mut validation_file_path = base_dir(directory);
        validation_file_path.push("log.validation");
        validation_file_path
    }
}

fn log_file(file: Option<&str>, directory: Option<&str>) -> PathBuf {
    if let Some(path) = file {
        PathBuf::from_str(path).expect(&format!("could not create a path with {}", path))
    } else {
        log_path(directory)
    }
}

fn validation_messages(
    byte_offset: usize,
    starting_line: usize,
    conf: &Configuration,
    log: Option<&str>,
    validation_file_name: Option<&str>,
    now: Option<NaiveDateTime>,
) {
    let testing = log.is_some();
    if let Some((line_number, count)) = validate(
        byte_offset,
        starting_line,
        log,
        validation_file_name,
        now,
        conf,
    ) {
        if count > 1 {
            if !testing {
                warn(
                    format!(
                        "{} errors were found starting at line {}",
                        count, line_number
                    ),
                    conf,
                )
            }
        } else {
            if !testing {
                warn(format!("one error was found at line {}", line_number), conf)
            }
        }
        copy(
            validation_file(validation_file_name, conf.directory()),
            log_file(log, conf.directory()),
        )
        .expect("could not copy validation file to log");
    } else {
        if !testing {
            success("log is valid", conf);
        }
    }
    if backup_backup_file(conf.directory()).as_path().exists() {
        std::fs::remove_file(backup_backup_file(conf.directory()))
            .expect("could not remove backup backup file");
    }
    std::fs::remove_file(validation_file(validation_file_name, conf.directory()))
        .expect("could not remove validation file");
}

// returns line number and error count
fn validate(
    byte_offset: usize,
    starting_line: usize,
    log: Option<&str>,
    validation: Option<&str>,
    now: Option<NaiveDateTime>,
    conf: &Configuration,
) -> Option<(usize, usize)> {
    let edited = File::open(log_file(log, conf.directory()))
        .expect("could not open edited log file for reading");
    let mut reader = BufReader::new(edited);
    let validation_file = File::create(validation_file(validation, conf.directory()).as_path())
        .expect("could not open file to receive validation output");
    let mut writer = BufWriter::new(validation_file);
    let mut bytes_written: usize = 0;
    // fill up the validation file up to the offset without validating
    while bytes_written < byte_offset {
        let delta = byte_offset - bytes_written;
        let mut buffer: Vec<u8> = if delta < BUFFER_SIZE {
            vec![0; delta]
        } else {
            vec![0; BUFFER_SIZE]
        };
        reader
            .read_exact(&mut buffer)
            .expect("could not read from log file");
        bytes_written += buffer.len();
        writer
            .write_all(&buffer)
            .expect("could not write to validation file");
    }
    // now start validating
    let mut buffer = String::new();
    let mut seen_done = false;
    let mut line_number = starting_line;
    let mut first_error = 0;
    let mut error_count = 0;
    let mut open_task = false;
    let now = now.unwrap_or(Local::now().naive_local());
    let mut log = LogController::new(Some(log_file(log, conf.directory())), conf)
        .expect("could not open edited log file");
    let mut last_timestamp = log
        .items_before(starting_line)
        .find(|i| i.has_time())
        .and_then(|i| Some(i.time().unwrap().0.clone()));
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
                open_task = true;
                time = Some(e.start);
            }
            Item::Done(d, _) => {
                if !open_task {
                    if seen_done || starting_line == 0 {
                        error_message = Some("DONE without preceding event".to_owned());
                    } else {
                        // look back to see if there are events before the first change
                        let prev = log.items_before(starting_line).find(|i| match i {
                            Item::Done(_, _) | Item::Event(_, _) => true,
                            _ => false,
                        });
                        let bad = match prev {
                            Some(Item::Event(_, _)) | None => false,
                            _ => true,
                        };
                        if bad {
                            error_message = Some("DONE without preceding event".to_owned());
                        }
                    }
                }
                seen_done = true;
                open_task = false;
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
    use crate::log::{Done, Event, LogLine, Note};
    use chrono::{Duration, NaiveDate};

    enum Stub<'a> {
        E(i64),
        N(i64),
        D(i64),
        C,
        B,
        Error(&'a str),
    }

    impl<'a> Stub<'a> {
        fn make(&self, base_time: &NaiveDateTime) -> String {
            match self {
                Stub::E(duration) => {
                    let mut event = Event::coin("event".to_owned(), vec![]);
                    event.start = *base_time + Duration::hours(*duration);
                    event.to_line()
                }
                Stub::N(duration) => {
                    let mut note = Note::coin("note".to_owned(), vec![]);
                    note.time = *base_time + Duration::hours(*duration);
                    note.to_line()
                }
                Stub::D(duration) => Done(*base_time + Duration::hours(*duration)).to_line(),
                Stub::C => "# comment".to_owned(),
                Stub::B => "".to_owned(),
                Stub::Error(s) => s.to_string(),
            }
        }
    }

    // returns log name, log path, and byte offsets per line
    fn create_log(
        disambiguator: &str,
        base_time: &NaiveDateTime,
        lines: &[Stub],
    ) -> (String, PathBuf, Vec<usize>) {
        let disambiguator = format!("log_{}", disambiguator);
        let log_buffer = PathBuf::from_str(&disambiguator)
            .expect(&format!("could not create log file for {}", disambiguator));
        let file = File::create(log_buffer.as_path())
            .expect("could not open file to receive validation output");
        let mut writer = BufWriter::new(file);
        let mut offsets = vec![];
        let mut offset = 0;
        let newline_length = "\n".len();
        for line in lines {
            offsets.push(offset.clone());
            let line = line.make(base_time);
            offset += line.len() + newline_length;
            writeln!(writer, "{}", line)
                .expect(&format!("faile to write {} into {}", line, disambiguator));
        }
        (disambiguator, log_buffer, offsets)
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

    fn test_configuration(disambiguator: &str) -> (PathBuf, Configuration) {
        let conf_path = configuration_path(disambiguator);
        File::create(conf_path.as_path()).expect("could not create configuration file path");
        let conf = Configuration::read(Some(conf_path), Some("."));
        (configuration_path(disambiguator), conf)
    }

    fn lines(file: &PathBuf) -> Vec<String> {
        let file = File::open(file).expect(&format!("could not open {}", file.to_str().unwrap()));
        let mut reader = BufReader::new(file);
        let mut ret = Vec::new();
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).expect("failed to read line") == 0 {
                break;
            }
            ret.push(line);
        }
        ret
    }

    #[test]
    fn test_find_change_offset_when_no_change() {
        let disambiguator1 = "test_find_change_offset_when_no_change1";
        let disambiguator2 = "test_find_change_offset_when_no_change2";
        let t = NaiveDate::from_ymd(2000, 1, 1).and_hms(0, 0, 0);
        let lines = [Stub::C, Stub::B, Stub::E(1), Stub::N(2), Stub::D(3)];
        let (n1, log1, _) = create_log(disambiguator1, &t, &lines);
        let (n2, log2, _) = create_log(disambiguator2, &t, &lines);
        let diff = find_change_offset(Some(&n1), Some(&n2), Some("."));
        assert!(diff.is_none(), "no difference found");
        cleanup(vec![log1, log2]);
    }

    #[test]
    fn test_find_change_offset_with_change() {
        let disambiguator1 = "test_find_change_offset_with_change1";
        let disambiguator2 = "test_find_change_offset_with_change2";
        let t = NaiveDate::from_ymd(2000, 1, 1).and_hms(0, 0, 0);
        let lines = [Stub::E(1), Stub::N(2), Stub::D(3)];
        let (n1, log1, _) = create_log(disambiguator1, &t, &lines);
        let (n2, log2, _) = create_log(disambiguator2, &t, &lines[0..2]);
        let diff = find_change_offset(Some(&n1), Some(&n2), Some("."));
        assert!(diff.is_some(), "difference found");
        assert_eq!(2, diff.unwrap().1, "difference at third line");
        cleanup(vec![log1, log2]);
    }

    #[test]
    fn test_validation_messages_all_good() {
        let disambiguator = "test_validation_messages_all_good";
        let validation = format!("validation_{}", disambiguator);
        let validation_path = PathBuf::from_str(&validation).expect("could not make path");
        let (conf_path, conf) = test_configuration(disambiguator);
        let t = NaiveDate::from_ymd(2000, 1, 1).and_hms(0, 0, 0);
        let events = [Stub::E(0), Stub::E(1), Stub::E(2)];
        let now = t + Duration::weeks(1);
        let (name, buff, _) = create_log(disambiguator, &t, &events);
        let backup_name = format!("{}.bak", disambiguator);
        let (backup_name, backup_buff, _) = create_log(&backup_name, &t, &events);
        validation_messages(0, 0, &conf, Some(&name), Some(&backup_name), Some(now));
        let lines = lines(&buff);
        assert!(lines.iter().find(|&s| s.contains("ERROR")).is_none());
        cleanup(vec![buff, backup_buff, conf_path, validation_path]);
    }

    #[test]
    fn test_validation_messages_garbled_line() {
        let disambiguator = "test_validation_messages_garbled_line";
        let validation = format!("validation_{}", disambiguator);
        let validation_path = PathBuf::from_str(&validation).expect("could not make path");
        let (conf_path, conf) = test_configuration(disambiguator);
        let t = NaiveDate::from_ymd(2000, 1, 1).and_hms(0, 0, 0);
        let events = [Stub::E(0), Stub::E(1), Stub::E(2), Stub::Error("foo")];
        let now = t + Duration::weeks(1);
        let (name, buff, _) = create_log(disambiguator, &t, &events);
        let backup_name = format!("{}.bak", disambiguator);
        let (backup_name, backup_buff, _) = create_log(&backup_name, &t, &events);
        validation_messages(0, 0, &conf, Some(&name), Some(&backup_name), Some(now));
        let lines = lines(&buff);
        assert!(lines.iter().find(|&s| s.contains("ERROR")).is_some());
        assert!(lines[3].contains("unexpected line format"));
        cleanup(vec![buff, backup_buff, conf_path, validation_path]);
    }

    #[test]
    fn test_validation_messages_done_without_event() {
        let disambiguator = "test_validation_messages_done_without_event";
        let validation = format!("validation_{}", disambiguator);
        let validation_path = PathBuf::from_str(&validation).expect("could not make path");
        let (conf_path, conf) = test_configuration(disambiguator);
        let t = NaiveDate::from_ymd(2000, 1, 1).and_hms(0, 0, 0);
        let events = [Stub::D(0), Stub::D(1), Stub::D(2)];
        let now = t + Duration::weeks(1);
        let (name, buff, _) = create_log(disambiguator, &t, &events);
        let backup_name = format!("{}.bak", disambiguator);
        let (backup_name, backup_buff, _) = create_log(&backup_name, &t, &events);
        validation_messages(0, 0, &conf, Some(&name), Some(&backup_name), Some(now));
        let lines = lines(&buff);
        assert!(lines.iter().find(|&s| s.contains("ERROR")).is_some());
        assert!(lines[0].contains("DONE without preceding event"));
        cleanup(vec![buff, backup_buff, conf_path, validation_path]);
    }

    #[test]
    fn test_validation_messages_done_with_offset() {
        let disambiguator = "test_validation_messages_done_with_offset";
        let validation = format!("validation_{}", disambiguator);
        let validation_path = PathBuf::from_str(&validation).expect("could not make path");
        let (conf_path, conf) = test_configuration(disambiguator);
        let t = NaiveDate::from_ymd(2000, 1, 1).and_hms(0, 0, 0);
        let events = [Stub::E(0), Stub::D(1), Stub::E(2)];
        let now = t + Duration::weeks(1);
        let (name, buff, offsets) = create_log(disambiguator, &t, &events);
        let backup_name = format!("{}.bak", disambiguator);
        let (backup_name, backup_buff, _) = create_log(&backup_name, &t, &events);
        validation_messages(
            offsets[1],
            1,
            &conf,
            Some(&name),
            Some(&backup_name),
            Some(now),
        );
        let lines = lines(&buff);
        assert!(lines.iter().find(|&s| s.contains("ERROR")).is_none());
        cleanup(vec![buff, backup_buff, conf_path, validation_path]);
    }

    #[test]
    fn test_validation_messages_done_with_offset_error() {
        let disambiguator = "test_validation_messages_done_with_offset_error";
        let validation = format!("validation_{}", disambiguator);
        let validation_path = PathBuf::from_str(&validation).expect("could not make path");
        let (conf_path, conf) = test_configuration(disambiguator);
        let t = NaiveDate::from_ymd(2000, 1, 1).and_hms(0, 0, 0);
        let events = [Stub::E(1), Stub::D(0), Stub::E(2)];
        let now = t + Duration::weeks(1);
        let (name, buff, offsets) = create_log(disambiguator, &t, &events);
        let backup_name = format!("{}.bak", disambiguator);
        let (backup_name, backup_buff, _) = create_log(&backup_name, &t, &events);
        validation_messages(
            offsets[1],
            1,
            &conf,
            Some(&name),
            Some(&backup_name),
            Some(now),
        );
        let lines = lines(&buff);
        assert!(lines.iter().find(|&s| s.contains("ERROR")).is_some());
        cleanup(vec![buff, backup_buff, conf_path, validation_path]);
    }

    #[test]
    fn test_validation_messages_done_after_done() {
        let disambiguator = "test_validation_messages_done_after_done";
        let validation = format!("validation_{}", disambiguator);
        let validation_path = PathBuf::from_str(&validation).expect("could not make path");
        let (conf_path, conf) = test_configuration(disambiguator);
        let t = NaiveDate::from_ymd(2000, 1, 1).and_hms(0, 0, 0);
        let events = [Stub::E(0), Stub::D(1), Stub::D(2)];
        let now = t + Duration::weeks(1);
        let (name, buff, _) = create_log(disambiguator, &t, &events);
        let backup_name = format!("{}.bak", disambiguator);
        let (backup_name, backup_buff, _) = create_log(&backup_name, &t, &events);
        validation_messages(0, 0, &conf, Some(&name), Some(&backup_name), Some(now));
        let lines = lines(&buff);
        assert!(lines.iter().find(|&s| s.contains("ERROR")).is_some());
        cleanup(vec![buff, backup_buff, conf_path, validation_path]);
    }

    #[test]
    fn test_events_out_of_order() {
        let disambiguator = "test_events_out_of_order";
        let validation = format!("validation_{}", disambiguator);
        let validation_path = PathBuf::from_str(&validation).expect("could not make path");
        let (conf_path, conf) = test_configuration(disambiguator);
        let t = NaiveDate::from_ymd(2000, 1, 1).and_hms(0, 0, 0);
        let events = [Stub::E(1), Stub::E(0), Stub::E(2)];
        let now = t + Duration::weeks(1);
        let (name, buff, _) = create_log(disambiguator, &t, &events);
        let backup_name = format!("{}.bak", disambiguator);
        let (backup_name, backup_buff, _) = create_log(&backup_name, &t, &events);
        validation_messages(0, 0, &conf, Some(&name), Some(&backup_name), Some(now));
        let lines = lines(&buff);
        assert!(lines.iter().find(|&s| s.contains("ERROR")).is_some());
        assert!(lines[1].contains("timestamp out of order with earlier timestamp"));
        cleanup(vec![buff, backup_buff, conf_path, validation_path]);
    }

    #[test]
    fn test_events_out_of_order_with_offset() {
        let disambiguator = "test_events_out_of_order_with_offset";
        let validation = format!("validation_{}", disambiguator);
        let validation_path = PathBuf::from_str(&validation).expect("could not make path");
        let (conf_path, conf) = test_configuration(disambiguator);
        let t = NaiveDate::from_ymd(2000, 1, 1).and_hms(0, 0, 0);
        let events = [Stub::E(1), Stub::E(0), Stub::E(2)];
        let now = t + Duration::weeks(1);
        let (name, buff, offsets) = create_log(disambiguator, &t, &events);
        let backup_name = format!("{}.bak", disambiguator);
        let (backup_name, backup_buff, _) = create_log(&backup_name, &t, &events);
        validation_messages(
            offsets[1],
            1,
            &conf,
            Some(&name),
            Some(&backup_name),
            Some(now),
        );
        let lines = lines(&buff);
        assert!(lines.iter().find(|&s| s.contains("ERROR")).is_some());
        assert!(lines[1].contains("timestamp out of order with earlier timestamp"));
        cleanup(vec![buff, backup_buff, conf_path, validation_path]);
    }

    #[test]
    fn test_events_in_future() {
        let disambiguator = "test_events_in_future";
        let validation = format!("validation_{}", disambiguator);
        let validation_path = PathBuf::from_str(&validation).expect("could not make path");
        let (conf_path, conf) = test_configuration(disambiguator);
        let t = NaiveDate::from_ymd(2000, 1, 1).and_hms(0, 0, 0);
        let now = t - Duration::weeks(1);
        let events = [Stub::E(0), Stub::E(1), Stub::E(2)];
        let (name, buff, _) = create_log(disambiguator, &t, &events);
        let backup_name = format!("{}.bak", disambiguator);
        let (backup_name, backup_buff, _) = create_log(&backup_name, &t, &events);
        validation_messages(0, 0, &conf, Some(&name), Some(&backup_name), Some(now));
        let lines = lines(&buff);
        assert!(lines.iter().find(|&s| s.contains("ERROR")).is_some());
        assert!(lines[0].contains("timestamp in future"));
        cleanup(vec![buff, backup_buff, conf_path, validation_path]);
    }
}
