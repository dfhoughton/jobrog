extern crate clap;
extern crate dirs;
extern crate regex;

use clap::{App, Arg, ArgMatches};
use dirs::home_dir;
use regex::Regex;

// a collection of arguments used in many subcommands concerned with searching for or filtering events
pub fn common_search_or_filter_arguments(
    app: App<'static, 'static>,
    for_events: Option<bool>,
) -> App<'static, 'static> {
    if for_events.is_none() {
        app.arg(
            Arg::with_name("notes")
            .short("n")
            .long("notes")
            .help("consider notes, not events")
            .long_help("Consider only notes, not events. If this is false, only events are considered, not notes.")
            .display_order(1)
            )
    } else {
        app
    }.arg(
        Arg::with_name("tag")
        .short("t")
        .long("tag")
        .visible_alias("tag-all")
        .multiple(true)
        .number_of_values(1)
        .help(match for_events {
            Some(true) => "skip events that lack this tag",
            Some(false) => "skip notes that lack this tag",
            None => "skip events/notes that lack this tag"
        })
        .long_help(match for_events {
            Some(true) => "Skip events that lack this tag.",
            Some(false) => "Skip notes that lack this tag.",
            None => "Skip events/notes that lack this tag."
        })
        .value_name("tag")
        .display_order(1)
    )
    .arg(
        Arg::with_name("tag-none")
        .short("n")
        .long("tag-none")
        .visible_alias("tn")
        .multiple(true)
        .number_of_values(1)
        .help(match for_events {
            Some(true) => "skip events that have this tag",
            Some(false) => "skip notes that have this tag",
            None => "skip events/notes that have this tag"
        })
        .long_help(match for_events {
            Some(true) => "Skip events that have this tag.",
            Some(false) => "Skip notes that have this tag.",
            None => "Skip events/notes that have this tag."
        })
        .value_name("tag")
        .display_order(2)
    )
    .arg(
        Arg::with_name("tag-some")
        .short("s")
        .long("tag-some")
        .visible_alias("ts")
        .multiple(true)
        .number_of_values(1)
        .help(match for_events {
            Some(true) => "skip events that lack any of these tags",
            Some(false) => "skip notes that lack any of these tags",
            None => "skip events/notes that lack any of these tags"
        })
        .long_help(match for_events {
            Some(true) => "Skip events that lack any of these tags. This is identical to --tag if only one value is provided. It is useful when you are looking for the last event that is tagged with some subset of a particular set of tags.",
            Some(false) => "Skip notes that lack any of these tags. This is identical to --tag if only one value is provided. It is useful when you are looking for the last note that is tagged with some subset of a particular set of tags.",
            None => "Skip events or notes that lack any of these tags. This is identical to --tag if only one value is provided. It is useful when you are looking for the last event/note that is tagged with some subset of a particular set of tags."
        })
        .value_name("tag")
        .display_order(3)
    )
    .arg(
        Arg::with_name("rx")
        .long("rx")
        .multiple(true)
        .number_of_values(1)
        .help(match for_events {
            Some(true) => "find events whose description matches this pattern",
            Some(false) => "find notes whose text matches this pattern",
            None => "find events/notes whose description/text matches this pattern"
        })
        .long_help(match for_events {
            Some(true) => "Find events whose description matches this regular expression.",
            Some(false) => "Find notes whose text matches this regular expression.",
            None => "Find events or notes whose description or text matches this regular expression."
        })
        .value_name("pattern")
        .validator(|arg| if Regex::new(&arg).is_ok() {Ok(())} else {Err(format!("'{}' cannot be parsed as a regular expression", &arg))})
        .display_order(4)
    )
    .arg(
        Arg::with_name("rx-not")
        .long("rx-not")
        .multiple(true)
        .number_of_values(1)
        .help(match for_events {
            Some(true) => "skip events whose description matches this pattern",
            Some(false) => "skip notes whose text matches this pattern",
            None => "skip events/notes whose description/text matches this pattern"
        })
        .long_help(match for_events {
            Some(true) => "Find events whose description does not match this regular expression.",
            Some(false) => "Find notes whose text does not match this regular expression.",
            None => "Find events or notes whose description or text does not match this regular expression."
        })
        .value_name("pattern")
        .validator(|arg| if Regex::new(&arg).is_ok() {Ok(())} else {Err(format!("'{}' cannot be parsed as a regular expression", &arg))})
        .display_order(5)
    )
}

// concatenate the trailing arguments -- we need to do this often enough it seems worth DRYing up
pub fn remainder(argname: &str, matches: &ArgMatches) -> std::string::String {
    matches
        .values_of(argname)
        .unwrap()
        .collect::<Vec<&str>>()
        .join(" ")
}

pub fn base_dir() -> std::path::PathBuf {
    let mut dir = home_dir().unwrap();
    dir.push(".joblog");
    dir
}

pub fn log_path() -> std::path::PathBuf {
    let mut dir = base_dir();
    dir.push("log");
    dir
}
