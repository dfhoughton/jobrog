#[macro_use]
extern crate clap;
extern crate jobrog;

use clap::App;
use jobrog::{
    add, configure, done, edit, first, last, note, resume, summary, truncate, util, when, vacation, parse
};

fn main() {
    util::init();
    let mut cli = App::new("testing")
        .version(crate_version!())
        .author(crate_authors!())
        .after_help("TODO: fill out detailed help")
        .about(crate_description!());
    cli = add::cli(cli);
    cli = done::cli(cli);
    cli = resume::cli(cli);
    cli = last::cli(cli);
    cli = first::cli(cli);
    cli = when::cli(cli);
    cli = summary::cli(cli);
    cli = edit::cli(cli);
    cli = note::cli(cli);
    cli = configure::cli(cli);
    cli = truncate::cli(cli);
    cli = vacation::cli(cli);
    cli = parse::cli(cli);
    // cli = tags::cli(cli);
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
        ("parse-time", Some(m)) => parse::run(m),
        _ => println!("{}", matches.usage()),
    }
}
