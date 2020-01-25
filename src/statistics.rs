extern crate chrono;
extern crate clap;
extern crate colonnade;

use crate::configure::Configuration;
use crate::log::{Item, ItemsAfter};
use crate::util::log_path;
use chrono::NaiveDateTime;
use clap::{App, Arg, ArgMatches, SubCommand};
use colonnade::{Alignment, Colonnade};
use std::collections::BTreeSet;

fn after_help() -> &'static str {
    "\
If you want aggregate statistics about your job log, this is your subcommand.

  > job statistics
  lines                            18,731
  first timestamp     2014-10-06 08:57:29
  last timestamp      2020-01-17 17:03:46
  events                           14,419
  notes                               202
  distinct event tags               2,326
  distinct note tags                   17
  comments                          1,323
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
            .display_order(display_order),
    )
}

pub fn run(directory: Option<&str>, matches: &ArgMatches) {
    let no_commas = matches.is_present("raw-numbers");
    let conf = Configuration::read(None, directory);
    let mut colonnade =
        Colonnade::new(2, conf.width()).expect("could not build the statistics table");
    colonnade.columns[1].alignment(Alignment::Right);
    let items = ItemsAfter::new(0, log_path(conf.directory()).as_path().to_str().unwrap());
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
    for item in items {
        line_count += 1;
        if let Some((t, _)) = item.time() {
            last_timestamp = Some(t.clone());
            if first_timestamp.is_none() {
                first_timestamp = Some(t.clone());
            }
        }
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
            Item::Done(_, _) => (),
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
    for line in colonnade.tabulate(&data).expect("couild not tabulate data") {
        println!("{}", line);
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
