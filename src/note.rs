extern crate clap;

use crate::configure::Configuration;
use crate::log::{Item, LogController};
use crate::util::{check_for_ongoing_event, describe, remainder, some_nws};
use clap::{App, AppSettings, Arg, ArgMatches, SubCommand};

fn after_help() -> &'static str {
    "You can record notes in your job log as well as events. But they are \
reported by the summary subcommand with only one timestamp, not two, and they \
aren't reported at all unless you specifically ask for them. E.g.,

  > job note -t paula -t birthday install Job Log

A note line in the log is identical to an event line, except '<NOTE>' is used as the separator \
between the timestamp and the tags:

  2020  1 18 12 10 26<NOTE>birthday paula:install Job Log

All prefixes of 'note' are aliases of the subcommand."
}

pub fn cli(mast: App<'static, 'static>, display_order: usize) -> App<'static, 'static> {
    mast.subcommand(
        SubCommand::with_name("note")
            .aliases(&["n", "no", "not"])
            .about("Adds a new note")
            .after_help(after_help())
            .arg(
                Arg::with_name("tag")
                .short("t")
                .long("tag")
                .multiple(true)
                .number_of_values(1)
                .help("Adds this tag to the note")
                .long_help("A tag is just a short description, like 'fun', or 'Louis'. Add a tag to a note to facilitate finding or grouping similar notes.")
                .value_name("tag")
                .validator(|v| if some_nws(&v) {Ok(())} else {Err(format!("tag {:?} needs some non-whitespace character", v))})
                .display_order(1)
            )
            .arg(
                Arg::with_name("copy-tags")
                .short("c")
                .long("copy-tags")
                .visible_alias("ct")
                .help("Copies tags from preceding note")
                .long_help("Copy to this note all the tags of the immediately preceding note. These tags will be in addition to any tags added via --tag.")
                .display_order(2)
            )
            .setting(AppSettings::TrailingVarArg)
            .arg(
                Arg::with_name("note")
                    .help("text to record")
                    .long_help(
                        "All the <note> arguments are concatenated to produce the text of the note.",
                    )
                    .value_name("note")
                    .required(true)
                    .multiple(true)
            )
            .display_order(display_order)
    )
}

pub fn run(matches: &ArgMatches) {
    let mut reader = LogController::new(None).expect("could not read log");
    let conf = Configuration::read(None);
    check_for_ongoing_event(&mut reader, &conf);
    let description = remainder("note", matches);
    let mut tags: Vec<String> = if let Some(values) = matches.values_of("tag") {
        values.map(|s| s.to_owned()).collect()
    } else {
        vec![]
    };
    if matches.is_present("copy-tags") {
        if let Some(event) = reader.last_event() {
            for t in event.tags {
                tags.push(t);
            }
        }
    }
    let (note, offset) = reader.append_note(description, tags);
    describe("noted", Item::Note(note, offset));
}
