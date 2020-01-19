extern crate chrono;
extern crate clap;

use crate::configure::Configuration;
use crate::log::{Event, Item, LogController};
use crate::util::{check_for_ongoing_event, describe, display_events, warn};
use chrono::Local;
use clap::{App, SubCommand};

fn after_help() -> &'static str {
    "The done subcommand places a DONE timestamp in the job log. This is just a \
    timestamp followed by a colon and the word 'DONE':

  2019  1  2 15 04 05:DONE

Generally one ends one task by beginning another, but you want to go off the clock \
you can use the done subcommand.

All prefixes of 'done' -- 'd', 'do', and 'don' -- are aliases."
}

pub fn cli(mast: App<'static, 'static>, display_order: usize) -> App<'static, 'static> {
    mast.subcommand(
        SubCommand::with_name("done")
            .aliases(&["d", "do", "don"])
            .about("Ends a currently open task")
            .after_help(after_help())
            .display_order(display_order),
    )
}

pub fn run() {
    let mut reader = LogController::new(None).expect("could not read log");
    let conf = Configuration::read(None);
    if let Some(event) = reader.last_event() {
        check_for_ongoing_event(&mut reader, &conf);
        if event.ongoing() {
            let (done, offset) = reader.close_event();
            let mut message = String::from("ending ");
            message += &event.description;
            describe(&message, Item::Done(done, offset), &conf);
        } else {
            warn("the most recent event is not ongoing", &conf);
            let now = Local::now().naive_local();
            let start = &event.start.clone();
            let event = Event::gather_by_day(vec![event], &now);
            println!();
            display_events(event, start, &now, &conf);
            println!();
            warn("no change to log", &conf)
        }
    } else {
        warn("there is currently no event in the log", &conf)
    }
}
