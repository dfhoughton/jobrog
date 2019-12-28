extern crate clap;
extern crate two_timer;

use clap::{App, AppSettings, Arg, ArgMatches, SubCommand};
use two_timer::parse;

pub fn cli(mast: App<'static, 'static>) -> App<'static, 'static> {
    mast.subcommand(
        SubCommand::with_name("when")
            .aliases(&["w", "wh", "whe"])
            .about("says when you will have worked all the hours expected within the given period")
            .after_help("You are expected to log a certain number of hours a day. This command allows you to discover how many addional hours you will have to work to meet this expectation.\n\nWithout any additional arguments the assumed period is the current day. Perhaps more useful is the pay period, but to use 'pay period' (abbreviated 'pp') as your time expression, you must have configured a pay period for the job log.")
            .setting(AppSettings::TrailingVarArg)
            .arg(
                Arg::with_name("period")
                    .help("time expression")
                    .long_help(
                        "All the <period> arguments are concatenated to produce a time expression.",
                    )
                    .value_name("period")
                    .default_value("today")
                    .multiple(true)
            )
            .display_order(6)
    )
}

pub fn run(matches: &ArgMatches) {
    let phrase = matches
        .values_of("period")
        .unwrap()
        .collect::<Vec<&str>>()
        .join(" ");
    println!("when: {}", phrase);
}
