extern crate chrono;
extern crate clap;
extern crate regex;

use crate::configure::Configuration;
use crate::log::{Event, Filter, Log, Note};
use crate::util::{common_search_or_filter_arguments, display_events, display_notes, warn};
use chrono::Local;
use clap::{App, ArgMatches, SubCommand};

pub fn cli(mast: App<'static, 'static>) -> App<'static, 'static> {
    mast.subcommand(
        common_search_or_filter_arguments(
            SubCommand::with_name("last")
                .aliases(&["l", "la", "las"])
                .about("show the last task recorded")
                .after_help("Frequently you want to know your most recently initiated task or written note, or the last task or note you worked on of a particular type. This command satisfies that want.")
                .display_order(5),
                None
        )
    )
}

pub fn run(matches: &ArgMatches) {
    let filter = Filter::new(matches);
    let mut reader = Log::new(None).expect("could not read log");
    let configuration = Configuration::read();
    if matches.is_present("notes") {
        let note: Vec<Note> = reader
            .notes_from_the_end()
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
            .events_from_the_end()
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
