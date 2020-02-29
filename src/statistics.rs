extern crate chrono;
extern crate clap;
extern crate colonnade;
extern crate two_timer;

use crate::configure::Configuration;
use crate::log::{Done, Item, ItemsAfter, LogController};
use crate::util::{fatal, log_path, remainder, Style};
use chrono::{Local, NaiveDateTime};
use clap::{App, AppSettings, Arg, ArgMatches, SubCommand};
use colonnade::{Alignment, Colonnade};
use std::collections::BTreeSet;
use two_timer::parse;

fn after_help() -> &'static str {
    "\
If you want aggregate statistics about your job log, this is your subcommand.

  > job statistics
  lines                            18,867
  first timestamp     2014-10-06 08:57:29
  last timestamp      2020-01-31 16:50:22
  hours clocked                    10,701
  events                           14,529
  notes                               202
  distinct event tags               2,337
  distinct note tags                   17
  comments                          1,333
  blank lines                           2
  errors                                0

All prefixes of 'statistics' after 's' -- 'st', 'sta', 'stat', etc. -- are aliases of \
this subcommand, as is 'stats'. The 's' prefix is reserved for the summary subcommand.
"
}

pub fn cli(mast: App<'static, 'static>, display_order: usize) -> App<'static, 'static> {
    mast.subcommand(
        SubCommand::with_name("statistics")
            .after_help(after_help())
            .aliases(&[
                "st",
                "sta",
                "stat",
                "stats",
                "stati",
                "statis",
                "statist",
                "statisti",
                "statistic",
            ])
            .arg(
                Arg::with_name("raw-numbers")
                    .long("raw-numbers")
                    .help("Shows counts without the comma group separator")
                    .display_order(1),
            )
            .about("Shows overall statistics of the log")
            .setting(AppSettings::TrailingVarArg)
            .arg(
                Arg::with_name("period")
                    .help("time expression")
                    .long_help(
                        "All the <period> arguments are concatenated to produce a time expression.",
                    )
                    .value_name("period")
                    .multiple(true),
            )
            .display_order(display_order),
    )
}

pub fn run(directory: Option<&str>, matches: &ArgMatches) {
    let no_commas = matches.is_present("raw-numbers");
    let conf = Configuration::read(None, directory);
    let style = Style::new(&conf);
    let mut colonnade =
        Colonnade::new(2, conf.width()).expect("could not build the statistics table");
    colonnade.columns[1].alignment(Alignment::Right);
    let (start_offset, end_time, mut maybe_start_time) = where_to_begin(matches, &conf);
    let items = ItemsAfter::new(
        start_offset,
        log_path(conf.directory()).as_path().to_str().unwrap(),
    );
    let mut line_count = 0;
    let mut event_count = 0;
    let mut note_count = 0;
    let mut comment_count = 0;
    let mut error_count = 0;
    let mut blank_line_count = 0;
    let mut event_tags: BTreeSet<String> = BTreeSet::new();
    let mut note_tags: BTreeSet<String> = BTreeSet::new();
    let mut first_timestamp: Option<NaiveDateTime> = None;
    let mut last_timestamp: Option<NaiveDateTime> = None;
    let mut duration = 0;
    let mut open_timetamp: Option<NaiveDateTime> = None;
    for item in items {
        if let Some((t, _)) = item.time() {
            if t > &end_time {
                break;
            }
            if maybe_start_time.is_none() {
                maybe_start_time = Some(t.clone());
            }
            if maybe_start_time.unwrap() > *t {
                continue;
            }
            last_timestamp = Some(t.clone());
            if first_timestamp.is_none() {
                first_timestamp = Some(t.clone());
            }
            if open_timetamp.is_none() {
                open_timetamp = Some(t.clone());
            }
        }
        line_count += 1;
        match item {
            Item::Event(e, _) => {
                event_count += 1;
                for t in e.tags {
                    event_tags.insert(t);
                }
            }
            Item::Note(n, _) => {
                note_count += 1;
                for t in n.tags {
                    note_tags.insert(t);
                }
            }
            Item::Blank(_) => blank_line_count += 1,
            Item::Comment(_) => comment_count += 1,
            Item::Done(Done(d), _) => {
                if let Some(t) = open_timetamp {
                    duration += (d.timestamp() - t.timestamp()) as usize;
                }
                open_timetamp = None;
            }
            Item::Error(_, _) => error_count += 1,
        }
    }
    let data = [
        [String::from("lines"), format_num(line_count, no_commas)],
        [
            String::from("first timestamp"),
            if let Some(t) = first_timestamp {
                format!("{}", t)
            } else {
                String::from("")
            },
        ],
        [
            String::from("last timestamp"),
            if let Some(t) = last_timestamp {
                format!("{}", t)
            } else {
                String::from("")
            },
        ],
        [
            String::from("hours clocked"),
            format!(
                "{}",
                format_num(
                    ((duration as f64) / (60.0 * 60.0)).round() as usize,
                    no_commas
                )
            ),
        ],
        [String::from("events"), format_num(event_count, no_commas)],
        [String::from("notes"), format_num(note_count, no_commas)],
        [
            String::from("distinct event tags"),
            format_num(event_tags.len(), no_commas),
        ],
        [
            String::from("distinct note tags"),
            format_num(note_tags.len(), no_commas),
        ],
        [
            String::from("comments"),
            format_num(comment_count, no_commas),
        ],
        [
            String::from("blank lines"),
            format_num(blank_line_count, no_commas),
        ],
        [String::from("errors"), format_num(error_count, no_commas)],
    ];
    for (i, line) in colonnade
        .tabulate(&data)
        .expect("couild not tabulate data")
        .iter()
        .enumerate()
    {
        println!(
            "{}",
            if i % 2 == 0 {
                style.paint("odd", line)
            } else {
                style.paint("even", line)
            }
        );
    }
}

fn where_to_begin(
    matches: &ArgMatches,
    conf: &Configuration,
) -> (usize, NaiveDateTime, Option<NaiveDateTime>) {
    if matches.is_present("period") {
        let period = remainder("period", matches);
        match parse(&period, conf.two_timer_config()) {
            Ok((t1, t2, _)) => {
                let mut log =
                    LogController::new(None, conf).expect("could not open log for reading");
                if let Some(item) = log.find_line(&t1) {
                    (item.offset(), t2, Some(t1))
                } else {
                    fatal("the log does not cover the period specified", conf);
                    unreachable!()
                }
            }
            _ => {
                fatal(
                    &format!("could not parse {} as a time expression", period),
                    conf,
                );
                unreachable!()
            }
        }
    } else {
        (0, Local::now().naive_local(), None)
    }
}

fn format_num(n: usize, no_commas: bool) -> String {
    let s1 = n.to_string();
    if no_commas {
        return s1;
    }
    let mut count = 0;
    let mut s = String::new();
    for c in s1.chars().rev() {
        s.push(c);
        count += 1;
        if count % 3 == 0 && count < s1.len() {
            s += ",";
        }
    }
    s.chars().rev().collect()
}
