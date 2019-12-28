extern crate clap;

use clap::{App, SubCommand};

pub fn cli(mast: App<'static, 'static>) -> App<'static, 'static> {
    mast.subcommand(
        SubCommand::with_name("done")
            .aliases(&["d", "do", "don"])
            .about("end the current open task")
            .after_help("Place a DONE timestamp in the job log. E.g.,\n\n  2019  1  2 15 04 05:DONE\n\nIf the last log line is a DONE timestamp, there is no task ongoing.")
            .display_order(2)
    )
}

pub fn run() {
    println!("done");
}
