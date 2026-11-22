use std::collections::BTreeMap;

use crate::configure::Configuration;
use crate::log::{Event, Filter, LogController};
use crate::util::{duration_string, fatal, Style};
use crate::vacation::VacationController;
use chrono::{Duration, Local, NaiveDateTime};

use colonnade::{Alignment, Colonnade};
use two_timer::parse;

pub const AFTER_HELP: &str = "\
If you are expected to log a certain number of hours a day this command allows you \
to discover how many addional hours you will have to work to meet this expectation.

Without any additional arguments the assumed period is the current day. Perhaps more useful \
is the pay period, but to use 'pay period' (abbreviated 'pp') as your time expression, \
you must have configured a pay period for the job log. See the configure subcommand.

  > job when
  when: today
  you were done at  4:16:52 PM

If you have configured time budgets (again, see the configure command), your the time \
completed in each budget will also be displayed.

 > job when pp
 when: pp
 you were done at  2:51:07 PM on Friday,  5 August 2022

 budget  budgeted  completed
 insp       15.00       1.00

All prefixes of 'when' are aliases of the subcommand.
";

#[derive(Debug, clap::Args)]
#[command(
    aliases = ["w", "wh", "whe"],
    about = "Says when you will have worked all the hours expected within the given period",
    after_help = AFTER_HELP,
    after_long_help = AFTER_HELP,
    trailing_var_arg = true
)]
pub struct Args {
    /// time expression
    ///
    /// All the <period> arguments are concatenated to produce a time expression.
    #[arg(value_name = "period", default_value = "today", num_args = 1..)]
    pub period: Vec<String>,
}

pub fn run(directory: Option<&str>, args: &Args) {
    let conf = Configuration::read(None, directory);
    let phrase = args.period.join(" ");
    println!("when: {}", phrase);
    match parse(&phrase, conf.two_timer_config()) {
        Ok(range) => {
            let start = range.start;
            let end = range.end;
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
                while start_date.and_hms_opt(0, 0, 0).unwrap() < end_time {
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
                let mut budget_counter: Option<BTreeMap<String, (f32, f32)>> =
                    if let Some(budgets) = &conf.budgets {
                        let mut bc: BTreeMap<String, (f32, f32)> = BTreeMap::new();
                        for pair in budgets {
                            bc.insert(pair.0.clone(), (pair.1 * 60.0 * 60.0, 0.0));
                        }
                        Some(bc)
                    } else {
                        None
                    };
                let mut next_threshold = conf.next_start_pay_period(&start.date());
                for e in events {
                    let seconds = e.duration(&now);
                    if let Some(bc) = &mut budget_counter {
                        let d = e.start.date();
                        if d >= next_threshold.unwrap() {
                            // fresh budgets
                            for tuple in bc.values_mut() {
                                tuple.1 = 0.0
                            }
                            next_threshold = conf.next_start_pay_period(&d)
                        }
                        for tag in &e.tags {
                            if let Some(tuple) = bc.get_mut(tag.as_str()) {
                                tuple.1 += seconds
                            }
                        }
                    }
                    seconds_worked += seconds;
                    last_moment = e.end;
                }
                // now do the math
                let seconds_required = hours_required * (60.0 * 60.0);
                let delta = seconds_required - seconds_worked;
                let style = Style::new(&conf);
                if delta > 0.0 {
                    let completion_time = now + Duration::seconds(delta as i64);
                    let delta_hours = delta / (60.0 * 60.0);
                    println!(
                        "you will be finished at {}, {:.2} hours from now",
                        style.paint("important", tell_time(&now, &completion_time)),
                        delta_hours
                    );
                } else {
                    let completion_time =
                        last_moment.unwrap_or(now) + Duration::seconds(delta as i64);
                    println!(
                        "you were done at {}",
                        style.paint("important", tell_time(&now, &completion_time))
                    );
                }
                if let Some(bc) = budget_counter {
                    let style = Style::new(&conf);
                    let mut lines = vec![vec![
                        "budget".to_owned(),
                        "budgeted".to_owned(),
                        "completed".to_owned(),
                    ]];
                    let mut table = Colonnade::new(3, conf.width()).unwrap();
                    for (tag, (budgeted, completed)) in bc {
                        lines.push(vec![
                            style.paint("tags", tag),
                            style.paint("duration", duration_string(budgeted, &conf)),
                            style.paint("duration", duration_string(completed, &conf)),
                        ]);
                    }
                    table.columns[1]
                        .set_alignment(Alignment::Right)
                        .set_left_margin(2);
                    table.columns[2]
                        .set_alignment(Alignment::Right)
                        .set_left_margin(2);
                    println!();
                    for (offset, row) in table
                        .macerate(&lines)
                        .expect("failed to macerate data")
                        .iter()
                        .enumerate()
                    {
                        if offset == 0 {
                            for line in row.iter() {
                                for (margin, content) in line.iter() {
                                    print!("{}{}", margin, style.paint("header", content))
                                }
                            }
                        } else {
                            for line in row.iter() {
                                for (col, (margin, content)) in line.iter().enumerate() {
                                    if col == 0 {
                                        print!("{}{}", margin, style.paint("tags", content))
                                    } else {
                                        print!("{}{}", margin, style.paint("duration", content))
                                    }
                                }
                            }
                        }
                        println!()
                    }
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
