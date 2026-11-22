use crate::configure::Configuration;
use crate::log::{Item, LogController};
use crate::util::{check_for_ongoing_event, describe, valid_tag};

pub const AFTER_HELP: &str = "\
You can record notes in your job log as well as events. But they are \
reported by the summary subcommand with only one timestamp, not two, and they \
aren't reported at all unless you specifically ask for them. E.g.,

  > job note -t paula -t birthday install Job Log

A note line in the log is identical to an event line, except '<NOTE>' is used as the separator \
between the timestamp and the tags:

  2020  1 18 12 10 26<NOTE>birthday paula:install Job Log

All prefixes of 'note' are aliases of the subcommand.";

#[derive(Debug, clap::Args)]
#[command(
    aliases = ["n", "no", "not"],
    about = "Adds a new note",
    after_help = AFTER_HELP,
    after_long_help = AFTER_HELP,
    trailing_var_arg = true
)]
pub struct Args {
    /// Adds this tag to the note
    ///
    /// A tag is just a short description, like 'fun', or 'Louis'. Add a tag to a note to
    /// facilitate finding or grouping similar notes.
    #[arg(short = 't', long, value_name = "tag", value_parser = valid_tag)]
    pub tag: Vec<String>,

    /// Copies tags from preceding note
    ///
    /// Copy to this note all the tags of the immediately preceding note. These tags will
    /// be in addition to any tags added via --tag.
    #[arg(short = 'c', long, visible_alias = "ct")]
    pub copy_tags: bool,

    /// text to record
    ///
    /// All the <note> arguments are concatenated to produce the text of the note.
    #[arg(value_name = "note", required = true, num_args = 1..)]
    pub note: Vec<String>,
}

pub fn run(directory: Option<&str>, args: &Args) {
    let conf = Configuration::read(None, directory);
    let mut reader = LogController::new(None, &conf).expect("could not read log");
    check_for_ongoing_event(&mut reader, &conf);
    let description = args.note.join(" ");
    let mut tags: Vec<String> = args.tag.clone();
    if args.copy_tags {
        if let Some(event) = reader.last_event() {
            for t in event.tags {
                tags.push(t);
            }
        }
    }
    let (note, offset) = reader.append_note(description, tags);
    describe("noted", None, Item::Note(note, offset), &conf);
}
