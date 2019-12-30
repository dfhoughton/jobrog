extern crate chrono;
extern crate clap;

use crate::configure::Configuration;
use crate::log_items::{Event, Item, LogReader};
use crate::util::{describe, display_events, warn};
use chrono::Local;
use clap::{App, AppSettings, Arg, ArgMatches, SubCommand};

pub fn cli(mast: App<'static, 'static>) -> App<'static, 'static> {
    mast.subcommand(
        SubCommand::with_name("add")
            .aliases(&["a", "ad"])
            .about("add a new task")
            .after_help("This is the essential job command: adding an event to the log. Each event line in the log consists of a timestamp, with units in descending order of significance, an optional set of tags, and a description. These three parts are separated by colons. E.g.,\n\n  2019  7  6 18  1 30:doc:just documenting a log line")
            .arg(
                Arg::with_name("tag")
                .short("t")
                .long("tag")
                .multiple(true)
                .number_of_values(1)
                .help("add this tag to the event")
                .long_help("A tag is just a short description, like 'fun', or 'overhead'. Add a tag to an event to facilitate finding or grouping similar events.")
                .value_name("tag")
                .display_order(1)
            )
            .arg(
                Arg::with_name("copy-tags")
                .short("c")
                .long("copy-tags")
                .visible_alias("ct")
                .help("copy tags from preceding event")
                .long_help("Copy to this event all the tags of the immediately preceding event. These tags will be in addition to any tags added via --tag.")
                .display_order(2)
            )
            .setting(AppSettings::TrailingVarArg)
            .arg(
                Arg::with_name("description")
                    .help("what happened")
                    .long_help(
                        "All the <description> arguments are concatenated to produce a description of the event.",
                    )
                    .value_name("description")
                    .required(true)
                    .multiple(true)
            )
            .display_order(1)
    )
}

pub fn run(matches: &ArgMatches) {
    let mut reader = LogReader::new(None).expect("could not read log");
    let configuration = Configuration::read();
    if reader.forgot_to_end_last_event() {
        warn("it appears an event begun on a previous day is ongoing");
        println!();
        let last_event = reader.last_event().unwrap();
        let start = &last_event.start.clone();
        let now = Local::now().naive_local();
        let event = Event::gather_by_day(vec![last_event], &now);
        display_events(event, start, &now, &configuration);
        println!();
    }
    let description = matches
        .values_of("description")
        .unwrap()
        .collect::<Vec<&str>>()
        .join(" ");
    let mut tags: Vec<String> = if let Some(values) = matches.values_of("tag") {
        values.map(|s| s.to_owned()).collect()
    } else {
        vec![]
    };
    if matches.is_present("copy-tags") {
        if let Some(event) = reader.last_event() {
            for t in event.tags {
                tags.push(t);
            }
        }
    }
    let (event, offset) = reader.append_event(description, tags);
    describe("starting", Item::Event(event, offset));
}
