extern crate chrono;
extern crate clap;

use crate::configure::Configuration;
use crate::log::{Event, Filter, Item, LogController};
use crate::util::{check_for_ongoing_event, common_search_or_filter_arguments, describe, warn};
use clap::{App, ArgMatches, SubCommand};

fn after_help() -> &'static str {
    "If you start the day by returning to what you were doing and the end of the previous \
day, you can simply type

  job resume

to start back up. If you start the day reading email, a task you tag with 'e', you can \
type

  job resume --tag e

To log the first task of the days as the email task. Any time you switch tasks back to \
one you've done befoer you can resume the old task rather than type out its full description \
and tags.

All prefixes of 'resume' are aliases of the subcommand."
}

pub fn cli(mast: App<'static, 'static>, display_order: usize) -> App<'static, 'static> {
    mast.subcommand(common_search_or_filter_arguments(
        SubCommand::with_name("resume")
            .aliases(&["r", "re", "res", "resu", "resum"])
            .about("Resumes a stopped task")
            .after_help(after_help())
            .display_order(display_order),
        Some(true),
    ))
}

pub fn run(directory: Option<&str>, matches: &ArgMatches) {
    let filter = Filter::new(matches);
    let conf = Configuration::read(None, directory);
    let mut reader = LogController::new(None, &conf).expect("could not read log");
    check_for_ongoing_event(&mut reader, &conf);
    let event: Vec<Event> = reader
        .events_from_the_end()
        .filter(|n| filter.matches(n))
        .take(1)
        .collect();
    if event.is_empty() {
        warn("no event found", &conf)
    } else if event[0].ongoing() {
        warn("event ongoing", &conf)
    } else {
        let (event, offset) =
            reader.append_event(event[0].description.clone(), event[0].tags.clone());
        describe("resuming", None, Item::Event(event, offset), &conf);
    }
}
