extern crate chrono;
extern crate clap;
extern crate two_timer;

use crate::configure::Configuration;
use crate::log::{Event, Filter, LogController, Note};
use crate::util::{
    check_for_ongoing_event, common_search_or_filter_arguments, display_events, display_notes,
    fatal, remainder, warn,
};
use crate::vacation::VacationController;
use chrono::{Duration, Local};
use clap::{App, Arg, ArgMatches, SubCommand};
use two_timer::{parsable, parse};

fn after_help() -> &'static str {
    "The summary subcommand aggregates and displays all the logged events \
in a particular period:

  > job summary yesterday
  Friday, 17 January
    8:59 am - 9:23     0.50  e, o          email
       9:23 - 10:40    1.25  2609, cs, sb  Error in approved plugh foo for 14068FY19
      10:40 - 12:51    2.25  42, mr, sb    Multi-Floob Review Part 1
      12:51 - 1:10 pm  0.25  l, o          lunch
       1:10 - 5:03     4.00  42, mr, sb    Multi-Floob Review Part 1

  TOTAL HOURS 8.00
  2609        1.25
  42          6.00
  cs          1.25
  e           0.50
  l           0.25
  mr          6.00
  o           0.75
  sb          7.25

If no time period is provided, the default period is 'today'.

You can also summarize the notes in a particular period:

  > job s --notes sep 2018
  Thursday, 13 September
    9:33 am  sb             69cd01b14
    9:33     sb             3190d979c
    8:34 pm  sb             http://localhost:3000/managers/plugh_applications/14450FY18/entities
  Friday, 14 September
    12:42    sb             Z923289 Q923525 K923550
  Saturday, 15 September
    10:15    moe, birthday  sketchbook
    10:15    moe, birthday  hat
    10:15    moe, birthday  tee-shirt
    10:15    moe, birthday  mechanical pencils
    10:15    moe, birthday  nice book

You can provide the time expression as the final arguments, but sometimes you want to filter \
by tag it's convenient to be able to add tag expressions to the end of the previous command, in \
which case the time expression is in the way. For this case you can use the --date option instead.

The Perl version of Job Log, https://metacpan.org/pod/App::JobLog, provides a today subcommand, which \
provides a summary of the current day's tasks. Jobrog, the Rust version, lacks this subcommand, but \
the default time expression is 'today'. Also, the subcommand has a 'to' alias for people whose muscle \
memory causes them to keep trying to use the today subcommand. As in the Perl version, jobrog provides all \
unambiguous prefixes of 'summary' as aliases (also, 's', though the statistics subcommand also begins \
with 's')."
}

pub fn cli(mast: App<'static, 'static>, display_order: usize) -> App<'static, 'static> {
    mast.subcommand(common_search_or_filter_arguments(
        SubCommand::with_name("summary")
            .aliases(&["s", "su", "sum", "summ", "summa", "summar", "to"])
            // the last, "to", is there because I'm used to there being a today subcommand which does what summary with no further arguments does in jobrog
            .about("Says when you will have worked all the hours expected within the given period")
            .after_help(after_help())
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
            .display_order(display_order),
            None,
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
    ))
}

pub fn run(matches: &ArgMatches) {
    let mut phrase = remainder("period", matches);
    let date = matches.value_of("date").unwrap_or(&phrase);
    let configuration = Configuration::read(None);
    if let Some(expression) = matches.value_of("date") {
        if phrase != "today" {
            warn(
                format!(
                    "--date option '{}' is overriding '{}' as a time expression",
                    date, phrase
                ),
                &configuration,
            );
        }
        phrase = expression.to_owned();
    }
    if let Ok((start, end, _)) = parse(&phrase, configuration.two_timer_config()) {
        let mut reader = LogController::new(None).expect("could not read log");
        if let Some(time) = reader.first_timestamp() {
            // narrow the range in to just the dates from the beginning of the lot to the present
            // so that we don't have spurious vacation times
            let start = if time > start {
                time.date().and_hms(0, 0, 0)
            } else {
                start
            };
            let time = Local::now().naive_local().date().and_hms(0, 0, 0) + Duration::days(1);
            let end = if end > time { time } else { end };

            let filter = Filter::new(matches);
            check_for_ongoing_event(&mut reader, &configuration);
            if matches.is_present("notes") {
                let note: Vec<Note> = reader
                    .notes_in_range(&start, &end)
                    .into_iter()
                    .filter(|n| filter.matches(n))
                    .collect();
                if note.is_empty() {
                    warn("no note found", &configuration)
                } else {
                    display_notes(note, &start, &end, &configuration);
                }
            } else {
                let events = reader
                    .events_in_range(&start, &end)
                    .into_iter()
                    .filter(|n| filter.matches(n))
                    .collect();
                let events = if matches.is_present("no-merge") {
                    Event::gather_by_day(events, &end)
                } else {
                    Event::gather_by_day_and_merge(events, &end)
                };
                let events = VacationController::read(None).add_vacation_times(
                    &start,
                    &end,
                    events,
                    &configuration,
                    None,
                    &filter,
                );
                if events.is_empty() {
                    warn("no event found", &configuration)
                } else {
                    display_events(events, &start, &end, &configuration);
                }
            }
        } else {
            if matches.is_present("notes") {
                warn("no note found", &configuration)
            } else {
                warn("no event found", &configuration)
            }
        }
    } else {
        fatal(
            format!("could not parse '{}' as a time expression", phrase),
            &configuration,
        )
    }
}
