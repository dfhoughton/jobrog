extern crate clap;

use clap::{App, AppSettings, Arg, ArgMatches, SubCommand};

pub fn cli(mast: App<'static, 'static>) -> App<'static, 'static> {
    mast.subcommand(
        SubCommand::with_name("note")
            .aliases(&["n", "no", "not"])
            .about("add a new note")
            .after_help("This is the essential job command: adding an event to the log. Like event lines, a note line in the log consists of a timestamp, with units in descending order of significance, an optional set of tags, and some text. Unlike an event line, for a note the separator between the first and second part is the string '<NOTE>' rather than a colon. A colon separates the second and third parts. E.g.,\n\n  2019  7  6 18  1 30<NOTE>birthday paula:Paula said the main thing she wants is a hibachi\n\nUnlike events, notes have no duration. Notes are ignored when summarizing the log unless you explicitly ask for a summary of notes instead of events.")
            .arg(
                Arg::with_name("tag")
                .short("t")
                .long("tag")
                .multiple(true)
                .number_of_values(1)
                .help("add this tag to the note")
                .long_help("A tag is just a short description, like 'fun', or 'Louis'. Add a tag to a note to facilitate finding or grouping similar notes.")
                .value_name("tag")
                .display_order(1)
            )
            .arg(
                Arg::with_name("copy-tags")
                .short("c")
                .long("copy-tags")
                .visible_alias("ct")
                .help("copy tags from preceding note")
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
            .display_order(10)
    )
}

pub fn run(matches: &ArgMatches) {
    let note = matches
        .values_of("note")
        .unwrap()
        .collect::<Vec<&str>>()
        .join(" ");
    let tags = matches
        .values_of("tag")
        .unwrap()
        .collect::<Vec<&str>>()
        .join(", ");
    println!("noted: {}; tags: {}", note, tags);
}
