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

pub fn cli(mast: App<'static, 'static>) -> App<'static, 'static> {
    mast.subcommand(
        SubCommand::with_name("edit")
            .aliases(&["e", "ed", "edi"])
            .about("open the job log in a text editor")
            .after_help("Sometimes you will")
            .display_order(7)
            .arg(
                Arg::with_name("validate")
                .long("validate")
                .help("validate the entire log, commenting out invalid lines")
                .long_help("If you have reason to believe your log has invalid lines -- if, for instance, you edited it without using this subcommand -- you can validate and clean it using --validate. This will convert invalid lines to comments preceded by comments marking them and explaining how they are invalid. It will announce upon completion whether it has found any invalid lines.")
            )
    )
}

pub fn run(matches: &ArgMatches) {
    let conf = Configuration::read();
    if matches.is_present("validate") {
        validation_messages(0, 0, &conf);
    } else {
        if let Some((editor, _)) = conf.effective_editor() {
            copy(log_path(), backup()).expect("could not make backup log");
            let status = Command::new(&editor)
                .arg(log_path().to_str().unwrap())
                .status()
                .expect("failed to start editor process");
            if status.success() {
                if let Some((offset, line_number)) = find_change_offset() {
                    validation_messages(offset, line_number, &conf);
                } else {
                    println!("no change found in log file; deleting backup...");
                    std::fs::remove_file(backup()).expect("failed to remove log.bak");
                }
            } else {
                fatal(
                    "the editor closed with an error; restoring log file from backup",
                    &conf,
                );
                copy(backup(), log_path()).expect("could restore log from backup");
                println!("done");
                std::fs::remove_file(backup()).expect("failed to remove log.bak");
            }
        } else {
            fatal(
                "no text editor available; see `job configure --help`",
                &conf,
            )
        }
    }
}

// scan for first line that differs
fn find_change_offset() -> Option<(usize, usize)> {
    let edited = File::open(log_path()).expect("could not open edited log file for reading");
    let mut edited = BufReader::new(edited);
    let backup = File::open(backup()).expect("could not backup log file to check for changes");
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
            .expect("failed to read line from backup log file");
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

// backup file
fn backup() -> PathBuf {
    let mut backup = base_dir();
    backup.push("log.bak");
    backup
}

fn validation_file() -> PathBuf {
    let mut validation_file_path = base_dir();
    validation_file_path.push("log.validation");
    validation_file_path
}

fn validation_messages(byte_offset: usize, starting_line: usize, conf: &Configuration) {
    if let Some((line_number, count)) = validate(byte_offset, starting_line) {
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
        copy(validation_file(), log_path()).expect("could not copy validation file to log");
        std::fs::remove_file(validation_file()).expect("could not remove validation file");
    } else {
        println!("log is valid")
    }
}

// returns line number and error count
fn validate(byte_offset: usize, starting_line: usize) -> Option<(usize, usize)> {
    let edited = File::open(log_path()).expect("could not open edited log file for reading");
    let mut reader = BufReader::new(edited);
    let validation_file = File::create(validation_file().as_path())
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
            let bytes: Vec<u8> = format!("# ERROR {}: {}\n# ", error_count, msg)
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
