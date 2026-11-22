use crate::configure::Configuration;
use crate::log::{Event, Filter, LogController, Note};
use crate::util::{display_events, display_notes, warn, FilterArgs};
use chrono::Local;

pub const AFTER_HELP: &str = "\
Frequently you want to know your most recently initiated task or written note, \
or the last task or note you worked on of a particular type. This command satisfies that want.

  > job last
  Friday, 17 January
    1:10 pm - 5:03  4.00  42, mr, sb  Multi-Floob Review Part 1

  TOTAL HOURS 4.00
  42          4.00
  mr          4.00
  sb          4.00

All prefixes of 'last' are aliases of the subcommand.";

#[derive(Debug, clap::Args)]
#[command(
    aliases = ["l", "la", "las"],
    about = "Shows the last task recorded",
    after_help = AFTER_HELP,
    after_long_help = AFTER_HELP
)]
pub struct Args {
    #[command(flatten)]
    pub filter: FilterArgs,
}

pub fn run(directory: Option<&str>, args: &Args) {
    let filter = Filter::new(&args.filter);
    let conf = Configuration::read(None, directory);
    let mut reader = LogController::new(None, &conf).expect("could not read log");
    if args.filter.notes {
        let note: Vec<Note> = reader
            .notes_from_the_end()
            .filter(|n| filter.matches(n))
            .take(1)
            .collect();
        if note.is_empty() {
            warn("no note found", &conf)
        } else {
            let start = note[0].time;
            let now = Local::now().naive_local();
            display_notes(note, &start, &now, &conf);
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
            let start = event[0].start;
            let now = Local::now().naive_local();
            let event = Event::gather_by_day(event, &now);
            display_events(event, &start, &now, &conf);
        }
    }
}
