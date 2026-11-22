use crate::configure::Configuration;
use crate::log::{Item, LogController};
use crate::util::{check_for_ongoing_event, describe, valid_tag};

pub const AFTER_HELP: &str = "\
This is the essential job command: adding an event to the log.

  job add --tag doc just documenting a log line

Each event line in the log consists of a timestamp with units in \
descending order of significance, an optional set of tags, and a description. \
These three parts are separated by colons:

  2019  7  6 18  1 30:doc:just documenting a log line

Tags facilitate categorizing and searching for events. When you use the summary \
subcommand to view the events in a particular period the time is shown aggregated \
by tag as well.

All prefixes of 'add' (so just 'a' and 'ad') are aliases for the add subcommand.";

#[derive(Debug, clap::Args)]
#[command(
    aliases = ["a", "ad"],
    about = "Adds a new task",
    after_help = AFTER_HELP,
    after_long_help = AFTER_HELP,
    trailing_var_arg = true
)]
pub struct Args {
    /// add this tag to the event
    ///
    /// A tag is just a short description, like 'fun', or 'overhead'. Add a tag to an event
    /// to facilitate finding or grouping similar events.
    #[arg(short = 't', long, value_name = "tag", value_parser = valid_tag)]
    pub tag: Vec<String>,

    /// copy tags from preceding event
    ///
    /// Copy to this event all the tags of the immediately preceding event. These tags will
    /// be in addition to any tags added via --tag.
    #[arg(short = 'c', long, visible_alias = "ct")]
    pub copy_tags: bool,

    /// what happened
    ///
    /// All the <description> arguments are concatenated to produce a description of the event.
    #[arg(value_name = "description", required = true, num_args = 1..)]
    pub description: Vec<String>,
}

pub fn run(directory: Option<&str>, args: &Args) {
    let conf = Configuration::read(None, directory);
    let mut reader = LogController::new(None, &conf).expect("could not read log");
    check_for_ongoing_event(&mut reader, &conf);
    let description = args.description.join(" ");
    let mut tags: Vec<String> = args.tag.clone();
    if args.copy_tags {
        if let Some(event) = reader.last_event() {
            for t in event.tags {
                tags.push(t);
            }
        }
    }
    let (event, offset) = reader.append_event(description, tags);
    describe("starting", None, Item::Event(event, offset), &conf);
}
