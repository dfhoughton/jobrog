extern crate clap;
extern crate dirs;
extern crate regex;

use clap::{App, Arg, ArgMatches};
use dirs::home_dir;
use regex::Regex;

// a collection of arguments used in many subcommands concerned with search for or filtering events
pub fn tag_search_arguments(app: App<'static, 'static>) -> App<'static, 'static> {
    app.arg(
        Arg::with_name("tag")
        .short("t")
        .long("tag")
        .visible_alias("tag-all")
        .multiple(true)
        .number_of_values(1)
        .help("skip events that lack this tag")
        .long_help("Skip events that lack this tag.")
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
        .help("skip events that have this tag")
        .long_help("Skip events that have this tag.")
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
        .help("skip events that lack any of these tags")
        .long_help("Skip events that lack any of these tags. This is identical to --tag if only one value is provided. It is useful when you are looking for the last event that is tagged with some subset of a particular set of tags.")
        .value_name("tag")
        .display_order(3)
    )
    .arg(
        Arg::with_name("rx")
        .long("rx")
        .multiple(true)
        .number_of_values(1)
        .help("find events whose description matches this pattern")
        .long_help("Find events whose description matches this regular expression.")
        .value_name("pattern")
        .validator(|arg| if Regex::new(&arg).is_ok() {Ok(())} else {Err(format!("'{}' cannot be parsed as a regular expression", &arg))})
        .display_order(4)
    )
    .arg(
        Arg::with_name("rx-not")
        .long("rx-not")
        .multiple(true)
        .number_of_values(1)
        .help("skip events whose description matches this pattern")
        .long_help("Find events whose description does not match this regular expression.")
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