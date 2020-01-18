#[macro_use]
extern crate clap;
extern crate jobrog;

use clap::App;
use jobrog::{
    add, configure, done, edit, first, last, note, parse, resume, statistics, summary, truncate,
    util, vacation, when,
};

fn after_help() -> &'static str {
    "TODO: fill out detailed help"
}

fn main() {
    util::init();
    let mut cli = App::new("testing")
        .version(crate_version!())
        .author(crate_authors!())
        .after_help(after_help())
        .about(crate_description!());
    // for determining the listing order
    let order = [
        add::cli,
        summary::cli,
        done::cli,
        resume::cli,
        last::cli,
        first::cli,
        note::cli,
        when::cli,
        edit::cli,
        configure::cli,
        vacation::cli,
        parse::cli,
        truncate::cli,
        statistics::cli,
    ];
    for (i, command) in order.iter().enumerate() {
        cli = command(cli, i);
    }
    let matches = cli.get_matches();
    match matches.subcommand() {
        ("add", Some(m)) => add::run(m),
        ("note", Some(m)) => note::run(m),
        ("done", _) => done::run(),
        ("edit", Some(m)) => edit::run(m),
        ("resume", Some(m)) => resume::run(m),
        ("last", Some(m)) => last::run(m),
        ("first", Some(m)) => first::run(m),
        ("when", Some(m)) => when::run(m),
        ("summary", Some(m)) => summary::run(m),
        ("truncate", Some(m)) => truncate::run(m),
        ("configure", Some(m)) => configure::run(m),
        ("vacation", Some(m)) => vacation::run(m),
        ("statistics", Some(m)) => statistics::run(m),
        ("parse-time", Some(m)) => parse::run(m),
        _ => println!("{}", matches.usage()),
    }
}
