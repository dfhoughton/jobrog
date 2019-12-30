extern crate chrono;
extern crate clap;
extern crate regex;

use crate::configure::Configuration;
use crate::log_items::{Event, Filter, LogReader, Note};
use crate::util::{common_search_or_filter_arguments, display_events, display_notes};
use chrono::Local;
use clap::{App, ArgMatches, SubCommand};

pub fn cli(mast: App<'static, 'static>) -> App<'static, 'static> {
    mast.subcommand(
        common_search_or_filter_arguments(
            SubCommand::with_name("first")
                .aliases(&["f", "fi", "fir", "firs"])
                .about("show the first task recorded")
                .after_help("Should you want to find the first task of a particular sort in the log, this is a bit easier than scanning the log visually.")
                .display_order(14),
                None
        )
    )
}

pub fn run(matches: &ArgMatches) {
    let filter = Filter::new(matches);
    let reader = LogReader::new(None).expect("could not read log");
    let configuration = Configuration::read();
    if matches.is_present("notes") {
        let note: Vec<Note> = reader
            .notes_from_the_beginning()
            .filter(|n| filter.matches(n))
            .take(1)
            .collect();
        if note.is_empty() {
            println!("no note found")
        } else {
            let start = &note[0].time.clone();
            let now = Local::now().naive_local();
            display_notes(note, start, &now, &configuration);
        }
    } else {
        let event: Vec<Event> = reader
            .events_from_the_beginning()
            .filter(|n| filter.matches(n))
            .take(1)
            .collect();
        if event.is_empty() {
            println!("no event found")
        } else {
            let start = &event[0].start.clone();
            let now = Local::now().naive_local();
            display_events(event, start, &now, &configuration);
        }
    }
}
