extern crate chrono;
extern crate clap;

use crate::configure::Configuration;
use crate::log::{Event, Filter, Item, Log};
use crate::util::{
    check_for_ongoing_event, common_search_or_filter_arguments, describe, display_events, warn,
};
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
    let mut reader = Log::new(None).expect("could not read log");
    check_for_ongoing_event(&mut reader);
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
