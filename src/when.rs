extern crate chrono;
extern crate clap;
extern crate two_timer;

use crate::configure::Configuration;
use crate::log::{Event, Filter, LogController};
use crate::util::fatal;
use crate::vacation::VacationController;
use chrono::{Duration, Local, NaiveDateTime};
use clap::{App, AppSettings, Arg, ArgMatches, SubCommand};
use two_timer::parse;

fn after_help() -> &'static str {
    "If you are expected to log a certain number of hours a day this command allows you \
to discover how many addional hours you will have to work to meet this expectation.

Without any additional arguments the assumed period is the current day. Perhaps more useful \
is the pay period, but to use 'pay period' (abbreviated 'pp') as your time expression, \
you must have configured a pay period for the job log. See the configure subcommand.

  > job when
  when: today
  you were done at  4:16:52 PM

All prefixes of 'when' are aliases of the subcommand.
"
}

pub fn cli(mast: App<'static, 'static>, display_order: usize) -> App<'static, 'static> {
    mast.subcommand(
        SubCommand::with_name("when")
            .aliases(&["w", "wh", "whe"])
            .about("Says when you will have worked all the hours expected within the given period")
            .after_help(after_help())
            .setting(AppSettings::TrailingVarArg)
            .arg(
                Arg::with_name("period")
                    .help("time expression")
                    .long_help(
                        "All the <period> arguments are concatenated to produce a time expression.",
                    )
                    .value_name("period")
                    .default_value("today")
                    .multiple(true),
            )
            .display_order(display_order),
    )
}

pub fn run(directory: Option<&str>, matches: &ArgMatches) {
    let conf = Configuration::read(None, directory);
    let phrase = matches
        .values_of("period")
        .unwrap()
        .collect::<Vec<&str>>()
        .join(" ");
    println!("when: {}", phrase);
    match parse(&phrase, conf.two_timer_config()) {
        Ok((start, end, _)) => {
            let now = Local::now().naive_local();
            if now <= start {
                fatal(
                    format!(
                        "the current moment, {}, must be after the first moment sought: {}.",
                        now, start
                    ),
                    &conf,
                )
            } else if start >= end {
                fatal(
                    format!(
                        "the current moment, {}, must be before the last moment sought: {}.",
                        now, end
                    ),
                    &conf,
                )
            } else {
                let mut reader = LogController::new(None, &conf).expect("could not read log");
                let events = reader.events_in_range(&start, &now);
                // first figure out how much you *should* work during the period
                let mut start_date = start.date();
                let end_time = if now < end { now } else { end };
                let mut hours_required = 0.0;
                while start_date.and_hms(0, 0, 0) < end_time {
                    if conf.is_workday(&start_date) {
                        hours_required += conf.day_length;
                    }
                    start_date += Duration::days(1);
                }
                // then figure out how much you have worked
                let events = Event::gather_by_day(events, &end);
                let filter = Filter::dummy();
                let events = VacationController::read(None, conf.directory())
                    .add_vacation_times(&start, &end, events, &conf, None, &filter);
                let mut seconds_worked = 0.0;
                let mut last_moment = None;
                for e in events {
                    seconds_worked += e.duration(&now);
                    last_moment = e.end.clone();
                }
                // now do the math
                let seconds_required = hours_required * (60.0 * 60.0);
                let delta = seconds_required - seconds_worked;
                if delta > 0.0 {
                    let completion_time = now + Duration::seconds(delta as i64);
                    let delta_hours = delta / (60.0 * 60.0);
                    println!(
                        "you will be finished at {}, {:.2} hours from now",
                        tell_time(&now, &completion_time),
                        delta_hours
                    );
                } else {
                    let completion_time = last_moment.unwrap_or(now) + Duration::seconds(delta as i64);
                    println!("you were done at {}", tell_time(&now, &completion_time));
                }
            }
        }
        Err(e) => fatal(e.msg(), &conf),
    }
}

fn tell_time(now: &NaiveDateTime, then: &NaiveDateTime) -> String {
    if now.date() == then.date() {
        format!("{}", then.format("%l:%M:%S %p"))
    } else {
        format!("{}", then.format("%l:%M:%S %p on %A, %e %B %Y"))
    }
}
