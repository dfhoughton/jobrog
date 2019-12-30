extern crate chrono;
extern crate clap;

use crate::configure::Configuration;
use crate::log_items::{Event, Filter, Item, LogReader};
use crate::util::{common_search_or_filter_arguments, describe, display_events, warn};
use chrono::Local;
use clap::{App, ArgMatches, SubCommand};

pub fn cli(mast: App<'static, 'static>) -> App<'static, 'static> {
    mast.subcommand(common_search_or_filter_arguments(
        SubCommand::with_name("resume")
            .aliases(&["r", "re", "res", "resu", "resum"])
            .about("resume the last stopped task")
            .display_order(4),
        Some(true),
    ))
}

pub fn run(matches: &ArgMatches) {
    let filter = Filter::new(matches);
    let mut reader = LogReader::new(None).expect("could not read log");
    let configuration = Configuration::read();
    let now = Local::now().naive_local();
    if reader.forgot_to_end_last_event() {
        warn("it appears an event begun on a previous day is ongoing");
        println!();
        let last_event = reader.last_event().unwrap();
        let start = &last_event.start.clone();
        let event = Event::gather_by_day(vec![last_event], &now);
        display_events(event, start, &now, &configuration);
        println!();
    }
    let event: Vec<Event> = reader
        .events_from_the_end()
        .filter(|n| filter.matches(n))
        .take(1)
        .collect();
    if event.is_empty() {
        warn("no event found")
    } else if event[0].ongoing() {
        warn("event ongoing")
    } else {
        let (event, offset) =
            reader.append_event(event[0].description.clone(), event[0].tags.clone());
        describe("starting", Item::Event(event, offset));
    }
}
