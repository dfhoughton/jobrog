#[macro_use]
extern crate clap;
extern crate jobrog;

use clap::App;
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
  starting tags facilitate searching and aggregation (tags: foo)
  > job note you can take notes as well
  noted you can take notes as well (no tags)
  > job note notes are events without a duration
  noted notes are events without a duration (no tags)
  > job add you can go off the clock
  starting you can go off the clock (no tags)
  > job done
  ending you can go off the clock at 11:13 am
  > job resume --tag foo
  starting tags facilitate searching and aggregation (tags: foo)
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
