extern crate chrono;
extern crate clap;
extern crate regex;

use crate::configure::Configuration;
use crate::log::{Event, Filter, LogController, Note};
use crate::util::{common_search_or_filter_arguments, display_events, display_notes, warn};
use chrono::Local;
use clap::{App, ArgMatches, SubCommand};

fn after_help() -> &'static str {
    "\
Frequently you want to know your most recently initiated task or written note, \
or the last task or note you worked on of a particular type. This command satisfies that want.

  > job last
  Friday, 17 January
    1:10 pm - 5:03  4.00  42, mr, sb  Multi-Floob Review Part 1

  TOTAL HOURS 4.00
  42          4.00
  mr          4.00
  sb          4.00

All prefixes of 'last' are aliases of the subcommand."
}

pub fn cli(mast: App<'static, 'static>, display_order: usize) -> App<'static, 'static> {
    mast.subcommand(common_search_or_filter_arguments(
        SubCommand::with_name("last")
            .aliases(&["l", "la", "las"])
            .about("Shows the last task recorded")
            .after_help(after_help())
            .display_order(display_order),
        None,
    ))
}

pub fn run(directory: Option<&str>, matches: &ArgMatches) {
    let filter = Filter::new(matches);
    let conf = Configuration::read(None, directory);
    let mut reader = LogController::new(None, &conf).expect("could not read log");
    if matches.is_present("notes") {
        let note: Vec<Note> = reader
            .notes_from_the_end()
            .filter(|n| filter.matches(n))
            .take(1)
            .collect();
        if note.is_empty() {
            warn("no note found", &conf)
        } else {
            let start = &note[0].time.clone();
            let now = Local::now().naive_local();
            display_notes(note, start, &now, &conf);
        }
    } else {
        let event: Vec<Event> = reader
            .events_from_the_end()
            .filter(|n| filter.matches(n))
            .take(1)
            .collect();
        if event.is_empty() {
            warn("no event found", &conf)
        } else {
            let start = &event[0].start.clone();
            let now = Local::now().naive_local();
            let event = Event::gather_by_day(event, &now);
            display_events(event, start, &now, &conf);
        }
    }
}
