extern crate clap;
extern crate regex;

use crate::util::common_search_or_filter_arguments;
use clap::{App, ArgMatches, SubCommand};

pub fn cli(mast: App<'static, 'static>) -> App<'static, 'static> {
    mast.subcommand(
        common_search_or_filter_arguments(
            SubCommand::with_name("last")
                .aliases(&["l", "la", "las"])
                .about("show the last task recorded")
                .after_help("Frequently you want to know your most recently initiated task or written note, or the last task or note you worked on of a particular type. This command satisfies that want.")
                .display_order(5),
                None
        )
    )
}

pub fn run(matches: &ArgMatches) {
    println!("last");
}
