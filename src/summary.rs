extern crate clap;
extern crate two_timer;

use crate::util::tag_search_arguments;
use clap::{App, Arg, ArgMatches, SubCommand};
use two_timer::{parsable, parse};

pub fn cli(mast: App<'static, 'static>) -> App<'static, 'static> {
    mast.subcommand(tag_search_arguments(
        SubCommand::with_name("summary")
            .aliases(&["s", "su", "sum", "summ", "summa", "summar"])
            .about("says when you will have worked all the hours expected within the given period")
            .after_help(".")
            .arg(
                Arg::with_name("period")
                    .help("description of time period summarized")
                    .long_help(
                        "Words describing the period summarized. E.g., 'last week' or '2016-10-2'.",
                    )
                    .value_name("word")
                    .default_value("today")
                    .multiple(true)
            )
            .display_order(3),
    ).arg(
        Arg::with_name("date")
        .long("date")
        .short("d")
        .help("the time expression as an option rather than an argument")
        .long_help("If you are frequently reviewing the tasks done in a particular pay period, filtering them by tag, say, it may be convenient for the date not to be at the end of the command line -- better to add filters here. In this case you can use the --date option.")
        .validator(|v| if parsable(&v) {Ok(())} else {Err(format!("cannot parse '{}' as a time expression", v))} )
        .value_name("phrase")
    ).arg(
        Arg::with_name("no-merge")
        .long("no-merge")
        .help("don't merge contiguous events with the same tags")
        .long_help("By default contiguous events with the same tags are displayed as a single event with the sub-events' descriptions joined with '; '. --no-merge prevents this.")
        .conflicts_with("merge-all")
    ).arg(
        Arg::with_name("merge-all")
        .long("merge-all")
        .help("merge contiguous events regardless of tags")
        .long_help("Display contiguous events as single events with merged tag sets and descriptions. Merged descriptions are joined with '; '.")
        .conflicts_with("no-merge")
    ))
}

pub fn run(matches: &ArgMatches) {
    let phrase = matches
        .values_of("period")
        .unwrap()
        .collect::<Vec<&str>>()
        .join(" ");
    let date = matches.value_of("date").unwrap_or(&phrase);
    println!("when: {}", date);
}
