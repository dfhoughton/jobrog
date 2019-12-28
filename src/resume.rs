extern crate clap;
extern crate regex;

use crate::util::common_search_or_filter_arguments;
use clap::{App, ArgMatches, SubCommand};

pub fn cli(mast: App<'static, 'static>) -> App<'static, 'static> {
    mast.subcommand(common_search_or_filter_arguments(
        SubCommand::with_name("resume")
            .aliases(&["r", "re", "res", "resu", "resdum"])
            .about("resume the last stopped task")
            .display_order(4),
        Some(true),
    ))
}

pub fn run(matches: &ArgMatches) {
    println!("resumed");
}
