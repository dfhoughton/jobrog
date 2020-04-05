#[macro_use]
extern crate clap;
extern crate jobrog;

use clap::{App, Arg};
use jobrog::{
    add, configure, done, edit, first, last, note, parse, resume, statistics, summary, tag,
    truncate, util, vacation, when,
};

fn after_help() -> &'static str {
    "The 'job' executable allows one to maintain and view a log of daily activity."
}

fn main() {
    let mut cli = App::new("testing")
        .version(crate_version!())
        .author(crate_authors!())
        .after_help(after_help())
        .about(crate_description!())
        .arg(
            Arg::with_name("directory")
                .long("directory")
                .short("d")
                .value_name("dir")
                .help("Looks in this directory for the log rather than ~/.joblog")
                .long_help(
                    "If you need or want to use a directory other than .joblog \
            in your home directory to store job log's log, vacation file, configuration \
            file, and so forth, specify this alternative directory with --directory. \
            As with .joblog, if it does not exist it will be created as needed.",
                ),
        );
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
        tag::cli,
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
    let directory = matches.value_of("directory");
    util::init(directory);
    match matches.subcommand() {
        ("add", Some(m)) => add::run(directory, m),
        ("note", Some(m)) => note::run(directory, m),
        ("done", _) => done::run(directory),
        ("edit", Some(m)) => edit::run(directory, m),
        ("resume", Some(m)) => resume::run(directory, m),
        ("last", Some(m)) => last::run(directory, m),
        ("tag", Some(m)) => tag::run(directory, m),
        ("first", Some(m)) => first::run(directory, m),
        ("when", Some(m)) => when::run(directory, m),
        ("summary", Some(m)) => summary::run(directory, m),
        ("truncate", Some(m)) => truncate::run(directory, m),
        ("configure", Some(m)) => configure::run(directory, m),
        ("vacation", Some(m)) => vacation::run(directory, m),
        ("statistics", Some(m)) => statistics::run(directory, m),
        ("parse-time", Some(m)) => parse::run(directory, m),
        _ => println!("{}", matches.usage()),
    }
}
