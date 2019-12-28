extern crate clap;
extern crate regex;

use crate::util::tag_search_arguments;
use clap::{App, ArgMatches, SubCommand};

pub fn cli(mast: App<'static, 'static>) -> App<'static, 'static> {
    mast.subcommand(
        tag_search_arguments(
            SubCommand::with_name("first")
                .aliases(&["f", "fi", "fir", "firs"])
                .about("show the first task recorded")
                .after_help("Should you want to find the first task of a particular sort in the log, this is a bit easier than scanning the log visually.")
                .display_order(14)
        )
    )
}

pub fn run(matches: &ArgMatches) {
    println!("first");
}
