extern crate clap;

use clap::{App, SubCommand};

pub fn cli(mast: App<'static, 'static>) -> App<'static, 'static> {
    mast.subcommand(
        SubCommand::with_name("edit")
            .aliases(&["e", "ed", "edi"])
            .about("open the job log in a text editor")
            .after_help("Sometimes you will")
            .display_order(7),
    )
}

pub fn run() {
    // see https://doc.rust-lang.org/std/process/struct.Child.html
    println!("edit");
}
