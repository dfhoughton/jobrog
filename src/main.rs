#[macro_use]
extern crate clap;
extern crate jobrog;

use clap::{App, Arg};
use jobrog::{
    add, configure, done, edit, first, last, note, parse, resume, statistics, summary, truncate,
    util, vacation, when,
};

fn after_help() -> &'static str {
    "\
The 'job' executable allows one to maintain and view a log of daily activity.

  > job add creating demonstration events in the log
  starting creating demonstration events in the log (no tags)
  > job add events have a duration
  starting events have a duration (no tags)
  > sleep 60
  > job add --tag foo tags facilitate searching and aggregation
  starting tags facilitate searching and aggregation (foo)
  > job note you can take notes as well
  noted you can take notes as well (no tags)
  > job note notes are events without a duration
  noted notes are events without a duration (no tags)
  > job add you can go off the clock
  starting you can go off the clock (no tags)
  > job done
  ending you can go off the clock at 11:13 am
  > job resume --tag foo
  starting tags facilitate searching and aggregation (foo)
  > job note you can resume an earlier event
  noted you can resume an earlier event (no tags)
  > job note you can summarize the log
  noted you can summarize the log (no tags)
  > job summary today
  Sunday, 19 January
    11:11 am - 11:12    0.021       creating demonstration events in the log; events have a duration
       11:12 - 11:13    0.006  foo  tags facilitate searching and aggregation
       11:13 - 11:13    0.001       you can go off the clock
       11:13 - ongoing  0.007  foo  tags facilitate searching and aggregation
  
  TOTAL HOURS 0.036
  UNTAGGED    0.022
  foo         0.013
  > job summary --notes today
  Sunday, 19 January
    11:12 am    you can take notes as well
    11:12       notes are events without a duration
    11:13       you can resume an earlier event
    11:13       you can summarize the log
  > job note you can configure job
  noted you can configure job (no tags)
  > job configure --precision quarter
  setting precision to quarter!
  > job summary today
  Sunday, 19 January
    11:11 am - 11:12    0.00       creating demonstration events in the log; events have a duration
       11:12 - 11:13    0.00  foo  tags facilitate searching and aggregation
       11:13 - 11:13    0.00       you can go off the clock
       11:13 - ongoing  0.00  foo  tags facilitate searching and aggregation
  
  TOTAL HOURS 0.00
  UNTAGGED    0.00
  foo         0.00

This version of job is a Rust implementation: https://github.com/dfhoughton/jobrog. \
The original implementation was in Perl: https://metacpan.org/pod/App::JobLog."
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
