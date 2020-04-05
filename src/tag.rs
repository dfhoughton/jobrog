extern crate chrono;
extern crate clap;
extern crate two_timer;

use crate::configure::Configuration;
use crate::log::{parse_line, Filter, Item, LogController, LogLine};
use crate::util::{
    common_search_or_filter_arguments, display_events, display_notes, fatal, remainder, some_nws,
    warn,
};
use chrono::{Duration, Local};
use clap::{App, Arg, ArgMatches, SubCommand};
use std::fs::{copy, remove_file, File};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::PathBuf;
use std::str::FromStr;
use two_timer::parse;

fn after_help() -> &'static str {
    "\
If you are interrupted in the middle of the task you may want to add a timestamp to \
the log and delay tagging the task until a quieter moment:
    
    job a talking to Captain Distraction

When you are done with this interruption you can return to your prior task, but now you \
need to categorize the interruption. You can `job edit` to add the missing tags, but the \
tag subcommand makes this a little easier. With `job tag --empty --last --add overhead --add communication` or \
perhaps `job t -el -a o -a c` you're back on your way.

All prefixes of 'tag', so 't' and 'ta', are aliases of the subcommand.
"
}

const BUFFER_SIZE: usize = 16 * 1024;

pub fn cli(mast: App<'static, 'static>, display_order: usize) -> App<'static, 'static> {
    mast.subcommand(common_search_or_filter_arguments(
        SubCommand::with_name("tag")
            .aliases(&["t", "ta"])
            .about("Modifies the tags for specified events/notes")
            .after_help(after_help())
            .arg(
                Arg::with_name("period")
                    .help("description of time period of interest")
                    .long_help(
                        "Words describing the period of interest. E.g., 'last week' or '2016-10-2'.",
                    )
                    .value_name("word")
                    .default_value("today")
                    .multiple(true)
            )
            .display_order(display_order),
            None,
    ).arg(
        Arg::with_name("last")
        .long("last")
        .short("l")
        .conflicts_with("first")
        .help("Applies changes only to the last line found")
    ).arg(
        Arg::with_name("first")
        .long("first")
        .short("f")
        .conflicts_with("last")
        .help("Applies changes only to the first line found")
    ).arg(
        Arg::with_name("clear")
        .long("clear")
        .short("c")
        .conflicts_with("remove")
        .help("Removes all existing tags")
    ).arg(
        Arg::with_name("add")
        .long("add")
        .short("a")
        .visible_alias("add-tag")
        .validator(|v| if some_nws(&v) {Ok(())} else {Err(format!("tag {:?} needs some non-whitespace character", v))})
        .multiple(true)
        .number_of_values(1)
        .help("Adds tag")
        .value_name("tag")
    ).arg(
        Arg::with_name("remove")
        .long("remove")
        .short("r")
        .conflicts_with("clear")
        .visible_alias("remove-tag")
        .validator(|v| if some_nws(&v) {Ok(())} else {Err(format!("tag {:?} needs some non-whitespace character", v))})
        .multiple(true)
        .number_of_values(1)
        .help("Removes tag, if present")
        .value_name("tag")
    )
)
}

pub fn run(directory: Option<&str>, matches: &ArgMatches) {
    let conf = Configuration::read(None, directory);
    let mut to_add = if let Some(values) = matches.values_of("add") {
        values.collect::<Vec<_>>()
    } else {
        vec![]
    };
    to_add.sort_unstable();
    to_add.dedup();
    let mut to_remove = if let Some(values) = matches.values_of("remove") {
        values.collect::<Vec<_>>()
    } else {
        vec![]
    };
    to_remove.sort_unstable();
    to_remove.dedup();
    let clear = matches.is_present("clear");
    // some sanity checking
    if clear {
        if matches.is_present("no-tags") {
            warn(
                "there is no point in --clear if you are seeking only items that are --empty",
                &conf,
            );
            if to_add.is_empty() {
                fatal("no tag changes specified: you must --add a tag if you are seeking only items that are --empty", &conf);
            }
        }
    } else {
        let mut common = vec![];
        let filtered_to_add = to_add
            .iter()
            .filter(|s| {
                if to_remove.contains(s) {
                    common.push(**s);
                    false
                } else {
                    true
                }
            })
            .map(|s| *s)
            .collect::<Vec<_>>();
        let filtered_to_remove = to_remove
            .iter()
            .filter(|s| !common.contains(s))
            .map(|s| *s)
            .collect::<Vec<_>>();
        if !common.is_empty() {
            to_add = filtered_to_add;
            to_remove = filtered_to_remove;
            warn(
                format!(
                    "the following tags are to be both added and removed: {}",
                    common.join(", ")
                ),
                &conf,
            );
        }
        if to_add.is_empty() && to_remove.is_empty() {
            fatal(
                "no tag changes specified: you must --clear tags, --add a tag, or --remove a tag",
                &conf,
            );
        }
    }
    let phrase = remainder("period", matches);
    if let Ok((start, end, _)) = parse(&phrase, conf.two_timer_config()) {
        let mut reader = LogController::new(None, &conf).expect("could not read log");
        let now = Local::now().naive_local();
        if let Some(time) = reader.first_timestamp() {
            // narrow the range in to just the dates from the beginning of the log to the present
            // so that we don't have spurious vacation times
            let start = if time > start {
                time.date().and_hms(0, 0, 0)
            } else {
                start
            };
            let time = now.date().and_hms(0, 0, 0) + Duration::days(1);
            let end = if end > time { time } else { end };

            let filter = Filter::new(matches);
            let notes_only = matches.is_present("notes");
            let mut items = reader
                .tagable_items_in_range(&start, &end)
                .into_iter()
                .filter(|i| match i {
                    Item::Note(n, _) => {
                        if notes_only {
                            filter.matches(n)
                        } else {
                            false
                        }
                    }
                    Item::Event(e, _) => {
                        if notes_only {
                            false
                        } else {
                            filter.matches(e)
                        }
                    }
                    _ => false,
                })
                .collect::<Vec<_>>();
            if items.is_empty() {
                fatal(
                    format!("no {} found", if notes_only { "note" } else { "event" }),
                    &conf,
                );
            } else if matches.is_present("last") {
                items = vec![items.remove(items.len() - 1)];
            }
            let mut changed = false;
            items = items
                .into_iter()
                .map(|i| match &i {
                    Item::Note(n, offset) => {
                        let mut tags = vec![];
                        if clear {
                            changed = changed || !n.tags.is_empty();
                        } else {
                            for s in &n.tags {
                                if to_remove.contains(&s.as_str()) {
                                    changed = true;
                                } else {
                                    tags.push(s.clone());
                                }
                            }
                        }
                        for s in &to_add {
                            let s = s.to_string();
                            if !tags.contains(&s) {
                                changed = true;
                                tags.push(s);
                            }
                        }
                        let mut n = n.clone();
                        n.tags = tags;
                        Item::Note(n, *offset)
                    }
                    Item::Event(e, offset) => {
                        let mut tags = vec![];
                        if clear {
                            changed = changed || !e.tags.is_empty();
                        } else {
                            for s in &e.tags {
                                if to_remove.contains(&s.as_str()) {
                                    changed = true;
                                } else {
                                    tags.push(s.clone());
                                }
                            }
                        }
                        for s in &to_add {
                            let s = s.to_string();
                            if !tags.contains(&s) {
                                changed = true;
                                tags.push(s);
                            }
                        }
                        let mut e = e.clone();
                        e.tags = tags;
                        Item::Event(e, *offset)
                    }
                    _ => unreachable!(),
                })
                .collect();
            if changed {
                // create a copy of the log with the desired changes and replace the current log
                // this could be more efficient; maybe some day it will be
                let mut modified_copy = BufWriter::new(modified_copy(&conf));
                let mut buf_reader = BufReader::new(log_file(&conf));
                let byte_offset = reader
                    .larry
                    .offset(items[0].offset())
                    .expect("could not obtain line offset of first item")
                    as usize;
                let mut bytes_written: usize = 0;
                // fill up the log copy up to the offset without parsing bytes
                while bytes_written < byte_offset {
                    let delta = byte_offset - bytes_written;
                    let mut buffer: Vec<u8> = if delta < BUFFER_SIZE {
                        vec![0; delta]
                    } else {
                        vec![0; BUFFER_SIZE]
                    };
                    buf_reader
                        .read_exact(&mut buffer)
                        .expect("could not read from log file");
                    bytes_written += buffer.len();
                    modified_copy
                        .write_all(&buffer)
                        .expect("could not write to validation file");
                }
                // now add the changes and any other lines
                let mut item_offset = 0;
                for line_offset in items[0].offset()..reader.larry.len() {
                    if item_offset == items.len() || items[item_offset].offset() != line_offset {
                        modified_copy
                            .write(
                                reader
                                    .larry
                                    .get(line_offset)
                                    .expect("could not obtain log line")
                                    .as_bytes(),
                            )
                            .expect("could not write log line to log copy");
                    } else {
                        let line = match &items[item_offset] {
                            Item::Event(e, _) => e.to_line(),
                            Item::Note(n, _) => n.to_line(),
                            _ => unreachable!(),
                        };
                        modified_copy
                            .write(line.as_bytes())
                            .expect("could not write log line to log copy");
                        modified_copy
                            .write("\n".as_bytes())
                            .expect("could not add newline to log copy");
                        item_offset += 1;
                    }
                }
                modified_copy
                    .flush()
                    .expect("could not flush log copy buffer");
                copy(copy_path(&conf), log_path(&conf))
                    .expect("could not replace old log with new");
                remove_file(copy_path(&conf)).expect("could not remove log copy");
                // now display the items
                if notes_only {
                    let notes = items
                        .iter()
                        .map(|i| match i {
                            Item::Note(n, _) => n.clone(),
                            _ => unreachable!(),
                        })
                        .collect::<Vec<_>>();
                    display_notes(notes, &start, &end, &conf);
                } else {
                    // we need to create events *with end times*
                    let events = items
                        .iter()
                        .map(|i| match i {
                            Item::Event(e, offset) => {
                                let mut e = e.clone();
                                // look for end time
                                for i in offset + 1..reader.larry.len() {
                                    let i = parse_line(
                                        &reader.larry.get(i).expect("larry failed us"),
                                        0,
                                    );
                                    match &i {
                                        Item::Event(_, _) | Item::Done(_, _) => {
                                            e.end = Some(i.time().unwrap().0.clone())
                                        }
                                        _ => (),
                                    }
                                    if e.end.is_some() {
                                        break;
                                    }
                                }
                                e
                            }
                            _ => unreachable!(),
                        })
                        .collect::<Vec<_>>();
                    display_events(events, &start, &end, &conf);
                }
            } else {
                warn("no change", &conf);
            }
        } else {
            if matches.is_present("notes") {
                warn("no note found", &conf)
            } else {
                warn("no event found", &conf)
            }
        }
    } else {
        fatal(
            format!("could not parse '{}' as a time expression", phrase),
            &conf,
        )
    }
}

fn copy_path(conf: &Configuration) -> PathBuf {
    let mut p = PathBuf::from_str(conf.directory().unwrap())
        .expect("could not obtain JobLog base directory");
    p.push("log.copy");
    p
}

fn modified_copy(conf: &Configuration) -> File {
    File::create(copy_path(conf)).expect("could not produce file into which to write changes")
}

fn log_path(conf: &Configuration) -> PathBuf {
    let mut p = PathBuf::from_str(conf.directory().unwrap())
        .expect("could not obtain JobLog base directory");
    p.push("log");
    p
}

fn log_file(conf: &Configuration) -> File {
    File::open(log_path(conf)).expect("could not produce log file")
}
