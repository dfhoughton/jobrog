extern crate chrono;
extern crate clap;

use crate::configure::Configuration;
use crate::log::{Item, LogController};
use crate::util::{check_for_ongoing_event, describe, some_nws};
use clap::{App, AppSettings, Arg, ArgMatches, SubCommand};

fn after_help() -> &'static str {
    "\
This is the essential job command: adding an event to the log.

  job add --tag doc just documenting a log line

Each event line in the log consists of a timestamp with units in \
descending order of significance, an optional set of tags, and a description. \
These three parts are separated by colons:

  2019  7  6 18  1 30:doc:just documenting a log line

Tags facilitate categorizing and searching for events. When you use the summary \
subcommand to view the events in a particular period the time is shown aggregated \
by tag as well.

All prefixes of 'add' (so just 'a' and 'ad') are aliases for the add subcommand."
}

pub fn cli(mast: App<'static, 'static>, display_order: usize) -> App<'static, 'static> {
    mast.subcommand(
        SubCommand::with_name("add")
            .aliases(&["a", "ad"])
            .about("Adds a new task")
            .after_help(after_help())
            .arg(
                Arg::with_name("tag")
                .short("t")
                .long("tag")
                .multiple(true)
                .number_of_values(1)
                .help("add this tag to the event")
                .long_help("A tag is just a short description, like 'fun', or 'overhead'. Add a tag to an event to facilitate finding or grouping similar events.")
                .value_name("tag")
                .validator(|v| if some_nws(&v) {Ok(())} else {Err(format!("{:?} is not a suitable tag: it has no non-whitespace character", v))} )
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
            .display_order(display_order)
    )
}

pub fn run(directory: Option<&str>, matches: &ArgMatches) {
    let conf = Configuration::read(None, directory);
    let mut reader = LogController::new(None, &conf).expect("could not read log");
    check_for_ongoing_event(&mut reader, &conf);
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
    describe("starting", None, Item::Event(event, offset), &conf);
}
