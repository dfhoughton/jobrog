extern crate chrono;
extern crate clap;
extern crate colonnade;

use crate::configure::Configuration;
use crate::log::{Item, ItemsAfter};
use crate::util::log_path;
use chrono::NaiveDateTime;
use clap::{App, SubCommand};
use colonnade::{Alignment, Colonnade};
use std::collections::BTreeSet;

pub fn cli(mast: App<'static, 'static>) -> App<'static, 'static> {
    mast.subcommand(
        SubCommand::with_name("statistics")
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
            .about("Show overall statistics of the log.")
            .display_order(14),
    )
}

pub fn run() {
    let conf = Configuration::read(None);
    let mut colonnade =
        Colonnade::new(2, conf.width()).expect("could not build the statistics table");
    colonnade.columns[1].alignment(Alignment::Right);
    let items = ItemsAfter::new(0, log_path().as_path().to_str().unwrap());
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
        [String::from("lines"), format_num(line_count)],
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
        [String::from("events"), format_num(event_count)],
        [String::from("notes"), format_num(note_count)],
        [
            String::from("distinct event tags"),
            format_num(event_tags.len()),
        ],
        [
            String::from("distinct note tags"),
            format_num(note_tags.len()),
        ],
        [String::from("comments"), format_num(comment_count)],
        [String::from("blank lines"), format_num(blank_line_count)],
        [String::from("errors"), format_num(error_count)],
    ];
    for line in colonnade.tabulate(&data).expect("couild not tabulate data") {
        println!("{}", line);
    }
}

fn format_num(n: usize) -> String {
    let mut count = 0;
    let mut s = String::new();
    let s1 = n.to_string();
    for c in s1.chars().rev() {
        s.push(c);
        count += 1;
        if count % 3 == 0 && count < s1.len() {
            s += ",";
        }
    }
    s.chars().rev().collect()
}
