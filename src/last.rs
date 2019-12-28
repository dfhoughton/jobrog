extern crate clap;
extern crate regex;

use crate::util::tag_search_arguments;
use clap::{App, ArgMatches, SubCommand};

pub fn cli(mast: App<'static, 'static>) -> App<'static, 'static> {
    mast.subcommand(
        tag_search_arguments(
            SubCommand::with_name("last")
                .aliases(&["l", "la", "las"])
                .about("show the last task recorded")
                .after_help("Frequently you want to know your current task, or the last task you worked on of a particular type. This command satisfies that want.")
                .display_order(5)
        )
    )
}

pub fn run(matches: &ArgMatches) {
    println!("last");
}
