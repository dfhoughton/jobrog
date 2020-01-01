extern crate chrono;
extern crate clap;
extern crate two_timer;

use crate::configure::Configuration;
use crate::log::Log;
use crate::util::fatal;
use chrono::{Duration, Local, NaiveDate, NaiveDateTime};
use clap::{App, AppSettings, Arg, ArgMatches, SubCommand};
use two_timer::{parse, Config};

pub fn cli(mast: App<'static, 'static>) -> App<'static, 'static> {
    mast.subcommand(
        SubCommand::with_name("when")
            .aliases(&["w", "wh", "whe"])
            .about("says when you will have worked all the hours expected within the given period")
            .after_help("You are expected to log a certain number of hours a day. This command allows you to discover how many addional hours you will have to work to meet this expectation.\n\nWithout any additional arguments the assumed period is the current day. Perhaps more useful is the pay period, but to use 'pay period' (abbreviated 'pp') as your time expression, you must have configured a pay period for the job log.")
            .setting(AppSettings::TrailingVarArg)
            .arg(
                Arg::with_name("period")
                    .help("time expression")
                    .long_help(
                        "All the <period> arguments are concatenated to produce a time expression.",
                    )
                    .value_name("period")
                    .default_value("today")
                    .multiple(true)
            )
            .display_order(6)
    )
}

pub fn run(matches: &ArgMatches) {
    let configuration = Configuration::read();
    let conf = Config::new()
        .monday_starts_week(!configuration.sunday_begins_week)
        .pay_period_start(configuration.start_pay_period)
        .pay_period_length(configuration.length_pay_period);
    let phrase = matches
        .values_of("period")
        .unwrap()
        .collect::<Vec<&str>>()
        .join(" ");
    println!("when: {}", phrase);
    match parse(&phrase, Some(conf)) {
        Ok((start, end, _)) => {
            let now = Local::now().naive_local();
            if now <= start {
                fatal(
                    format!(
                        "the current moment, {}, must be after the first moment sought: {}.",
                        now, start
                    ),
                    &configuration,
                )
            } else if start >= end {
                fatal(
                    format!(
                        "the current moment, {}, must be before the last moment sought: {}.",
                        now, end
                    ),
                    &configuration,
                )
            } else {
                let mut reader = Log::new(None).expect("could not read log");
                let events = reader.events_in_range(&start, &now);
                let mut hours_required = 0.0;
                let mut hours_worked = 0.0;
                let mut last_workday: Option<NaiveDate> = None;
                for e in events {
                    let date = e.start.date();
                    if configuration.is_workday(date) {
                        if last_workday.is_none() || last_workday.unwrap() != date {
                            hours_required += configuration.day_length;
                            last_workday = Some(date);
                        }
                    }
                    hours_worked += e.duration(&now);
                }
                let delta = hours_required - hours_worked;
                let completion_time = now + Duration::seconds((delta * (60.0 * 60.0)) as i64);
                if completion_time > now {
                    println!(
                        "you will be finished at {}, {:.2} hours from now",
                        tell_time(&now, &completion_time),
                        delta
                    );
                } else {
                    println!("you were done at {}", tell_time(&now, &completion_time));
                }
            }
        }
        Err(e) => fatal(e.msg(), &configuration),
    }
}

fn tell_time(now: &NaiveDateTime, then: &NaiveDateTime) -> String {
    if now.date() == then.date() {
        format!("{}", then.format("%k:%M:%S %p"))
    } else {
        format!("{}", then.format("%k:%M:%S %p on %A, %e %B %Y"))
    }
}
