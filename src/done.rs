use crate::configure::Configuration;
use crate::log::{Event, LogController};
use crate::util::{check_for_ongoing_event, describe, display_events, warn};
use chrono::Local;

pub const AFTER_HELP: &str = "\
The done subcommand places a DONE timestamp in the job log. This is just a \
timestamp followed by a colon and the word 'DONE':

  2019  1  2 15 04 05:DONE

Generally one ends one task by beginning another, but you want to go off the clock \
you can use the done subcommand.

All prefixes of 'done' -- 'd', 'do', and 'don' -- are aliases.";

#[derive(Debug, clap::Args)]
#[command(
    aliases = ["d", "do", "don"],
    about = "Ends a currently open task",
    after_help = AFTER_HELP,
    after_long_help = AFTER_HELP
)]
pub struct Args {}

pub fn run(directory: Option<&str>, _args: &Args) {
    let conf = Configuration::read(None, directory);
    let mut reader = LogController::new(None, &conf).expect("could not read log");
    if let Some(event) = reader.last_event() {
        check_for_ongoing_event(&mut reader, &conf);
        if event.ongoing() {
            let (done, offset) = reader.close_event();
            describe(
                "ending",
                Some(&event.description),
                crate::log::Item::Done(done, offset),
                &conf,
            );
        } else {
            warn("the most recent event is not ongoing", &conf);
            let now = Local::now().naive_local();
            let start = event.start;
            let event = Event::gather_by_day(vec![event], &now);
            println!();
            display_events(event, &start, &now, &conf);
            println!();
            warn("no change to log", &conf)
        }
    } else {
        warn("there is currently no event in the log", &conf)
    }
}
