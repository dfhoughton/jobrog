extern crate clap;
extern crate deflate;

use crate::util::remainder;
use clap::{App, AppSettings, Arg, ArgMatches, SubCommand};

pub fn cli(mast: App<'static, 'static>) -> App<'static, 'static> {
    mast.subcommand(
        SubCommand::with_name("truncate")
            .aliases(&["tr", "tru", "trun", "trunc", "trunca", "truncat"])
            .about("truncate the log to only recent events")
            .after_help("Over time your log will fill with cruft: work no one is interested in any longer, tags whose meaning you've forgotten. What you want to do at this point is chop off all the old stuff, stash it somewhere you can find it if need be, and 
retain in your active log only the more recent events. This is what truncate is for. You give it a starting date and it splits your log into two with the active portion containing all moments on that date or after. The older portion is 
retained in the hidden directory.")
            .arg(
                Arg::with_name("zip")
                .short("z")
                .long("zip")
                .help("compress truncated head of log with zip")
                .long_help("To conserve space, compress the truncated head of the log with Zlib.")
            )
            .arg(
                Arg::with_name("gzip")
                .short("g")
                .long("gzip")
                .help("compress truncated head of log with gzip")
                .long_help("To conserve space, compress the truncated head of the log with Gzip.")
            )
            .arg(
                Arg::with_name("deflate")
                .short("d")
                .long("deflate")
                .help("compress truncated head of log with deflate")
                .long_help("To conserve space, compress the truncated head of the log with DEFLATE.")
            )
            .setting(AppSettings::TrailingVarArg)
            .arg(
                Arg::with_name("date")
                    .help("earliest time to preserve in log")
                    .long_help(
                        "All the <date> arguments are concatenated to produce the cutoff date. Events earlier than this moment will be preserved in the truncated head of the log. Events on or after this date will remain in the active log.",
                    )
                    .value_name("description")
                    .required(true)
                    .multiple(true)
            )
            .display_order(13)
    )
}

pub fn run(matches: &ArgMatches) {
    println!("truncated; date: {}", remainder("date", matches));
}
