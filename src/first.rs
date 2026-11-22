use crate::configure::Configuration;
use crate::log::{Event, Filter, LogController, Note};
use crate::util::{display_events, display_notes, warn, FilterArgs};
use chrono::Local;

pub const AFTER_HELP: &str = "\
Should you want to find the first task of a particular sort in the log, the 'first' \
subcommand will find it for you. This is slightly easier than visually scanning the log. \
If you want to find the first event or note with a particular description of tag, this \
subcommand is the way to go.

  > job first --tag g
  Thursday,  4 December 2014
    10:30 am - 11:42  1.25  g  setting up to work on Gargamel

  TOTAL HOURS 1.25
  g           1.25

All prefixes of 'first' are aliases of the subcommand.
";

#[derive(Debug, clap::Args)]
#[command(
    aliases = ["f", "fi", "fir", "firs"],
    about = "Shows the first task recorded",
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
    let reader = LogController::new(None, &conf).expect("could not read log");
    if args.filter.notes {
        let note: Vec<Note> = reader
            .notes_from_the_beginning()
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
            .events_from_the_beginning()
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
