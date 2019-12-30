extern crate chrono;
extern crate clap;

use crate::configure::Configuration;
use crate::log_items::{Event, Item, LogReader};
use crate::util::{describe, display_events, warn};
use chrono::Local;
use clap::{App, SubCommand};

pub fn cli(mast: App<'static, 'static>) -> App<'static, 'static> {
    mast.subcommand(
        SubCommand::with_name("done")
            .aliases(&["d", "do", "don"])
            .about("end the current open task")
            .after_help("Place a DONE timestamp in the job log. E.g.,\n\n  2019  1  2 15 04 05:DONE\n\nIf the last log line is a DONE timestamp, there is no task ongoing.")
            .display_order(2)
    )
}

pub fn run() {
    let mut reader = LogReader::new(None).expect("could not read log");
    if let Some(event) = reader.last_event() {
        if event.ongoing() {
            let (done, offset) = reader.close_event();
            let mut message = String::from("ending ");
            message += &event.description;
            describe(&message, Item::Done(done, offset));
        } else {
            warn("the most recent event is not ongoing");
            let configuration = Configuration::read();
            let now = Local::now().naive_local();
            let start = &event.start.clone();
            let event = Event::gather_by_day(vec![event], &now);
            println!();
            display_events(event, start, &now, &configuration);
            println!();
            warn("no change to log")
        }
    } else {
        warn("there is currently no event in the log")
    }
}
