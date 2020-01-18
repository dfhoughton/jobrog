extern crate chrono;
extern crate clap;
extern crate regex;

use crate::configure::Configuration;
use crate::log::{Event, Filter, LogController, Note};
use crate::util::{common_search_or_filter_arguments, display_events, display_notes, warn};
use chrono::Local;
use clap::{App, ArgMatches, SubCommand};

fn after_help() -> &'static str {
    "Should you want to find the first task of a particular sort in the log, the 'first' \
subcommand will find it for you. This is slightly easier than visually scanning the log. \
If you want to find the first event or note with a particular description of tag, this \
subcommand is the way to go.

  > job first --tag g
  Thursday,  4 December 2014
    10:30 am - 11:42  1.25  g  setting up to work on Gargamel

  TOTAL HOURS 1.25
  g           1.25

All prefixes of 'first' are aliases of the subcommand.
"
}

pub fn cli(mast: App<'static, 'static>, display_order: usize) -> App<'static, 'static> {
    mast.subcommand(
        common_search_or_filter_arguments(
            SubCommand::with_name("first")
                .aliases(&["f", "fi", "fir", "firs"])
                .about("Shows the first task recorded")
                .after_help(after_help())
                .display_order(display_order),
                None
        )
    )
}

pub fn run(matches: &ArgMatches) {
    let filter = Filter::new(matches);
    let reader = LogController::new(None).expect("could not read log");
    let configuration = Configuration::read(None);
    if matches.is_present("notes") {
        let note: Vec<Note> = reader
            .notes_from_the_beginning()
            .filter(|n| filter.matches(n))
            .take(1)
            .collect();
        if note.is_empty() {
            warn("no note found", &configuration)
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
            warn("no event found", &configuration)
        } else {
            let start = &event[0].start.clone();
            let now = Local::now().naive_local();
            let event = Event::gather_by_day(event, &now);
            display_events(event, start, &now, &configuration);
        }
    }
}
