extern crate clap;
extern crate regex;

use crate::util::tag_search_arguments;
use clap::{App, ArgMatches, SubCommand};

pub fn cli(mast: App<'static, 'static>) -> App<'static, 'static> {
    mast.subcommand(tag_search_arguments(
        SubCommand::with_name("resume")
            .aliases(&["r", "re", "res", "resu", "resdum"])
            .about("resume the last stopped task")
            .display_order(4),
    ))
}

pub fn run(matches: &ArgMatches) {
    println!("resumed");
}
