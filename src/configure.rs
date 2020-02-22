extern crate chrono;
extern crate clap;
extern crate colonnade;
extern crate ini;
extern crate regex;
extern crate term_size;
extern crate two_timer;

use crate::util::{base_dir, fatal, success, warn, Style, STYLE_MATCHER};
use chrono::{Datelike, NaiveDate};
use clap::{App, Arg, ArgMatches, SubCommand};
use colonnade::{Alignment, Colonnade};
use ini::Ini;
use regex::Regex;
use std::collections::BTreeMap;
use std::env;
use std::fs::File;
use std::path::PathBuf;
use two_timer::{parsable, parse, Config};

pub const PRECISION: &str = "2";
pub const SUNDAY_BEGINS_WEEK: &str = "true";
pub const LENGTH_PAY_PERIOD: &str = "14";
pub const DAY_LENGTH: &str = "8";
pub const BEGINNING_WORK_DAY: (usize, usize) = (9, 0);
pub const WORKDAYS: &str = "MTWHF";
pub const COLOR: &str = "true";
pub const TRUNCATION: &str = "round";
pub const CLOCK: &str = "12";
pub const STYLES: &'static [[&'static str; 4]; 10] = &[
    [
        "alert",
        "purple",
        "something salient",
        "ongoing end time in summary",
    ],
    [
        "duration",
        "green",
        "event duration in summaries",
        "summary",
    ],
    [
        "error",
        "bold red",
        "something went wrong",
        "parse-time with no time expression provided",
    ],
    [
        "even",
        "cyan",
        "even row in a striped table",
        "configure --list",
    ],
    [
        "header",
        "bold blue",
        "header row in vacation table",
        "vacation --list",
    ],
    [
        "important",
        "red",
        "important information",
        "TOTAL_HOURS in summary",
    ],
    ["odd", "", "odd row in a striped table", "configure --list"],
    [
        "success",
        "bold green",
        "everything is okay",
        "confirmation of configuration changes",
    ],
    ["tags", "blue", "tags in summaries", "summary"],
    [
        "warning",
        "bold purple",
        "something needs attention",
        "alert given by summary when previous day's final task was not closed",
    ],
];

fn after_help() -> &'static str {
    lazy_static! {
        static ref INTRO: &'static str = "\
    Set or display configuration parameters that control date interpretation, log summarization, etc. \
    Some configuration may be taken from environment variables -- VISUAL, EDITOR, NO_COLOR. \
If this is occurring, this will be explained when you list the configuration.

The ansi_term crate is used to provide the optional styling. One can find a list of the fixed color \
    values at https://en.wikipedia.org/wiki/ANSI_escape_code#8-bit. Style specifications are parsed \
by the following grammar:

  TOP        -> spec* 

  spec       -> non_color | foreground | background
  non_color  -> \"bold\" | \"italic\" | \"underline\" | \"dimmed\" | \"blink\" | \"reverse\" | \"hidden\"
  foreground -> fg? color
  background -> bg  color
  fg         -> \"fg\" | \"foreground\"
  bg         -> \"bg\" | \"background\"
  color      -> named | fixed
  named      -> \"black\" | \"red\" | \"green\" | \"yellow\" | \"blue\" | \"purple\" | \"cyan\" | \"white\"
  fixed      -> 0 - 255

Examples:

  red
  bold dimmed bg cyan
  foreground 16

The specifiable styles and more sample style specifications can be found in the table below.

";
        static ref OUTRO: &'static str = "\
All prefixes of 'configure' are aliases of the subcommand.
";
        static ref TEXT: String = {
            let mut s = INTRO.to_string();
            s.push_str(&describe_styles());
            s.push_str("\n");
            s.push_str(&OUTRO);
            s
        };
    }
    &TEXT
}

fn describe_styles() -> String {
    let mut data = vec![["IDENTIFIER", "DEFAULT STYLE", "DESCRIPTION", "EXAMPLE"]
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>()];
    for row in STYLES {
        data.push(row.iter().map(|s| s.to_string()).collect());
    }
    let max_width = term_size::dimensions().unwrap_or((100, 0)).0;
    let width = if max_width > 100 { 100 } else { max_width };
    let mut colonnade = Colonnade::new(4, width).expect("could not tabulate styles");
    colonnade
        .spaces_between_rows(1)
        .padding_left(2)
        .expect("insufficient space to tabulate styles");
    colonnade.columns[0].priority(0);
    colonnade.columns[1].priority(0);
    colonnade.columns[2].priority(1);
    colonnade.columns[3].priority(1);
    colonnade
        .tabulate(data)
        .expect("could not tabulate data")
        .join("\n")
        + "\n"
}

fn valid_length_pay_period(v: String) -> Result<(), String> {
    let n = v.parse::<u32>();
    if n.is_ok() {
        let n = n.unwrap();
        if n > 0 {
            Ok(())
        } else {
            Err(format!("a pay period must have some positive length"))
        }
    } else {
        Err(format!("some (small) whole number of days expected"))
    }
}

fn valid_day_length(v: String) -> Result<(), String> {
    let n = v.parse::<f32>();
    if n.is_ok() {
        let n = n.unwrap();
        if n > 0.0 {
            if n > 24.0 {
                Err(format!("one cannot work more than 24 hours in a day"))
            } else {
                Ok(())
            }
        } else {
            Err(format!("a positive number of hours expected"))
        }
    } else {
        Err(format!("some (small) number of hours expected"))
    }
}

fn valid_max_width(v: String) -> Result<(), String> {
    let n = v.parse::<usize>();
    if n.is_ok() {
        if n.unwrap() < 40 {
            Err(format!(
                "summaries in less than 40 columns will be unreadable"
            ))
        } else {
            Ok(())
        }
    } else {
        Err(format!("some whole number of columns expected"))
    }
}

fn valid_beginning_work_day(v: String) -> Result<(), String> {
    let rx = Regex::new(r"\A([1-9]\d?)(?::([0-6]\d))?\z").unwrap();
    if let Some(captures) = rx.captures(&v) {
        let hour = captures[1].to_owned();
        let hour = hour.parse::<usize>().unwrap();
        if hour < 24 {
            if let Some(m) = captures.get(2) {
                let minute = m.as_str().parse::<usize>().unwrap();
                if minute < 60 {
                    Ok(())
                } else {
                    Err(format!(
                        "minute in beginning work day expression '{}' must be less than 60",
                        v
                    ))
                }
            } else {
                Ok(())
            }
        } else {
            Err(format!(
                "hour in beginning work day expression '{}' must be less than 24",
                v
            ))
        }
    } else {
        Err(String::from(""))
    }
}

pub fn cli(mast: App<'static, 'static>, display_order: usize) -> App<'static, 'static> {
    mast.subcommand(
        SubCommand::with_name("configure")
            .aliases(&["c", "co", "con", "conf", "confi", "config", "configu", "configur"])
            .about("Sets or displays configuration parameters")
            .after_help(after_help())
            // NOTE I'm not using default_value here so we can identify when the user misuses the subcommand and should be prompted
            .arg(
                Arg::with_name("precision") // remember to keep in sync with option in summary
                .long("precision")
                .help("Sets decimal places of precision in display of time; default value: 2")
                .long_help("The number of decimal places of precision used in the display of lengths of periods in numbers of hours. \
                If the number is 0, probably not what you want, all periods will be rounded to a whole number of hours. \
                The default value is 2. If the precision is a fraction like 'quarter' times will be rounded to the closest fraction that size of the hour for display.")
                .possible_values(&["0", "1", "2", "3", "half", "third", "quarter", "sixth", "twelfth", "sixtieth"])
                .value_name("precision")
            )
            .arg(
                Arg::with_name("truncation") // remember to keep in sync with option in summary
                .long("truncation")
                .help("Sets how fractional parts of a duration too small to display for the given precision are handled; default value: round")
                .long_help("When an events duration is displayed, there is generally some amount of information not \
                displayed given the precision. By default this portion is rounded, so if the precision is a quarter \
                hour and the duration is 7.5 minutes, this will be displayed as 0.25 hours. Alternatively, one could \
                use the floor, in which case this would be 0.00 hours, or the ceiling, in which case even a single \
                second task would be shown as taking 0.25 hours.")
                .possible_values(&["round", "floor", "ceiling"])
                .value_name("function")
            )
            .arg(
                Arg::with_name("start-pay-period")
                .long("start-pay-period")
                .help("Sets the first day of some pay period")
                .long_help("A day relative to which all pay periods will be calculated. See --length-pay-period.")
                .validator(|v| if parsable(&v) {Ok(())} else {Err(format!("cannot parse '{}' as a time expression", v))} )
                .value_name("date")
            )
            .arg(
                Arg::with_name("sunday-begins-week")
                .long("sunday-begins-week")
                .help("Sets whether Sunday should be considered the first day of the week; default value; true")
                .possible_values(&["true", "false"])
                .value_name("bool")
            )
            .arg(
                Arg::with_name("clock")
                .long("clock")
                .help("Sets times should be displayed with a 12-hour or a 24-hour clock; default value; 12")
                .possible_values(&["12", "24"])
                .value_name("type")
            )
            .arg(
                Arg::with_name("length-pay-period")
                .long("length-pay-period")
                .help("Sets the number of days in a pay period; default value: 14")
                .validator(valid_length_pay_period)
                .value_name("int")
            )
            .arg(
                Arg::with_name("day-length")
                .long("day-length")
                .help("Sets expected number of hours in a workday; default value: 8")
                .validator(valid_day_length)
                .value_name("num")
            )
            .arg(
                Arg::with_name("beginning-work-day")
                .long("beginning-work-day")
                .help("Sets when a work day typically begins; default value: 9:00")
                .validator(valid_beginning_work_day)
                .value_name("hours[:minutes]")
            )
            .arg(
                Arg::with_name("workdays")
                .long("workdays")
                .help("Sets which days you are expected to work; default value: MTWHF")
                .long_help("Workdays during the week represented as a subset of SMTWHFA, where S is Sunday and A is Saturday, etc. Default value: MTWHF.")
                .validator(|v| if Regex::new(r"\A[SMTWHFA]+\z").unwrap().is_match(&v) {Ok(())} else {Err(format!("must contain only the letters SMTWHFA, \
                where S means Sunday and A, Saturday, etc."))})
                .value_name("days")
            )
            .arg(
                Arg::with_name("editor")
                .long("editor")
                .help("Sets text editor to use when manually editing the log")
                .long_help("A text editor that the edit command will invoke. E.g., /usr/bin/vim. \
                If no editor is set, job falls back to the environment variables VISUAL and EDITOR in that order. \
                If there is still no editor, you cannot use the edit command to edit the log. \
                Note, whatever editor you use must be invocable from the shell as <editor> <file>. \
                If you need to pass additional arguments to the executable, provide them delimited by spaces \
                in the same argument. E.g., --editor='/usr/bin/open -W -n -t'")
                .value_name("path")
            )
            .arg(
                Arg::with_name("max-width")
                .long("max-width")
                .help("Sets maximum number of columns when summarizing data")
                .validator(valid_max_width)
                .value_name("num")
            )
            .arg(
                Arg::with_name("color")
                .long("color")
                .help("Sets whether to use colors; default value: true")
                .long_help("Color variation helps one parse information quickly, but if you don't want it, \
                or the ANSI color codes that produce it cause you trouble, you can turn it off. \
                If you haven't set this parameter and you don't have the NO_COLOR environment variable, Job Log will use color.")
                .possible_values(&["true", "false"])
                .value_name("bool")
            )
            .arg(
                Arg::with_name("style")
                .long("style")
                .help("Sets the style for a particular style identifier")
                .value_name("id spec")
                .multiple(true)
                .number_of_values(2)
            )
            .arg(
                Arg::with_name("unset")
                .short("u")
                .long("unset")
                .help("Returns a configurable parameter to its default; to unset styles you need to provide both \
                'style' and the parameter you wish to unset; e.g., --unset 'style even'")
                .value_name("param")
                .multiple(true)
                .number_of_values(1)
            )
            .arg(
                Arg::with_name("list")
                .short("l")
                .long("list")
                .help("Lists all configuration parameters")
                .long_help("List all configuration parameters and their values.")
            )
            .display_order(display_order)
    )
}

pub fn run(directory: Option<&str>, matches: &ArgMatches) {
    let mut did_something = false;
    let mut write = false;
    let mut conf = Configuration::read(None, directory);
    if let Some(v) = matches.value_of("start-pay-period") {
        did_something = true;
        let tt_conf = Config::new()
            .monday_starts_week(!conf.sunday_begins_week)
            .pay_period_length(conf.length_pay_period)
            .pay_period_start(conf.start_pay_period);
        let (start_date_time, _, _) = parse(v, Some(tt_conf)).unwrap();
        let year = start_date_time.year();
        let month = start_date_time.month();
        let day = start_date_time.day();
        let start_date = NaiveDate::from_ymd(year, month, day);
        if conf.start_pay_period.is_some() && &start_date == conf.start_pay_period.as_ref().unwrap()
        {
            warn(
                format!("start-pay-period is already {} {} {}!", year, month, day),
                &conf,
            );
        } else {
            println!("setting start-pay-period to {} {} {}!", year, month, day);
            conf.start_pay_period = Some(start_date);
            write = true;
        }
    }
    if matches.is_present("sunday-begins-week") {
        did_something = true;
        if let Some(v) = matches.value_of("sunday-begins-week") {
            let v: bool = v.parse().unwrap();
            if v == conf.sunday_begins_week {
                warn(format!("sunday-begins-week is already {}!", v), &conf);
            } else {
                success(format!("setting sunday-begins-week to {}!", v), &conf);
                conf.sunday_begins_week = v;
                write = true;
            }
        }
    }
    if matches.is_present("clock") {
        did_something = true;
        if let Some(v) = matches.value_of("clock") {
            if (v == CLOCK) == conf.h12 {
                warn(format!("clock is already {}!", v), &conf);
            } else {
                success(format!("setting clock to {}!", v), &conf);
                conf.h12 = v == CLOCK;
                write = true;
            }
        }
    }
    if matches.is_present("color") {
        did_something = true;
        if let Some(v) = matches.value_of("color") {
            let v: bool = v.parse().unwrap();
            conf.color = Some(v);
            // demonstrate that we've set the color
            success(format!("set color to {}!", v), &conf);
            write = true;
        }
    }
    if matches.is_present("length-pay-period") {
        did_something = true;
        if let Some(v) = matches.value_of("length-pay-period") {
            let v: u32 = v.parse().unwrap();
            if v == conf.length_pay_period {
                warn(format!("length-pay-period is already {}!", v), &conf);
            } else {
                success(format!("setting length-pay-period to {}!", v), &conf);
                conf.length_pay_period = v;
                write = true;
            }
        }
    }
    if matches.is_present("beginning-work-day") {
        did_something = true;
        let v = matches.value_of("beginning-work-day").unwrap();
        let rx = Regex::new(r"\A(\d+)(?::0*(\d+))?\z").unwrap();
        let captures = rx.captures(&v).unwrap();
        let hour = captures[1].parse::<usize>().unwrap();
        let minute = if let Some(m) = captures.get(2) {
            m.as_str().parse::<usize>().unwrap()
        } else {
            0
        };
        let beginning_work_day = (hour, minute);
        if conf.beginning_work_day == beginning_work_day {
            warn(
                format!("beginning-work-day is already {}:{:02}!", hour, minute),
                &conf,
            );
        } else {
            success(
                format!("setting beginning-work-day to {}:{:02}!", hour, minute),
                &conf,
            );
            conf.beginning_work_day = beginning_work_day;
            write = true;
        }
    }
    if matches.is_present("day-length") {
        did_something = true;
        if let Some(v) = matches.value_of("day-length") {
            let v: f32 = v.parse().unwrap();
            if v == conf.day_length {
                warn(format!("day-length is already {}!", v), &conf);
            } else {
                success(format!("setting day-length to {}!", v), &conf);
                conf.day_length = v;
                write = true;
            }
        }
    }
    if matches.is_present("precision") {
        did_something = true;
        if let Some(v) = matches.value_of("precision") {
            let v = Precision::from_s(v);
            if v == conf.precision {
                warn(format!("precision is already {}!", v.to_s()), &conf);
            } else {
                success(format!("setting precision to {}!", v.to_s()), &conf);
                conf.precision = v;
                write = true;
            }
        }
    }
    if matches.is_present("truncation") {
        did_something = true;
        if let Some(v) = matches.value_of("truncation") {
            let v = Truncation::from_s(v);
            if v == conf.truncation {
                warn(format!("truncation is already {}!", v.to_s()), &conf);
            } else {
                success(format!("setting truncation to {}!", v.to_s()), &conf);
                conf.truncation = v;
                write = true;
            }
        }
    }
    if matches.is_present("workdays") {
        did_something = true;
        if let Some(v) = matches.value_of("workdays") {
            if v == &conf.serialize_workdays() {
                warn(format!("workdays is already {}!", v), &conf);
            } else {
                success(format!("setting workdays to {}!", v), &conf);
                conf.workdays(v);
                write = true;
            }
        }
    }
    if let Some(v) = matches.value_of("editor") {
        did_something = true;
        if conf.editor.is_some() && v == conf.editor.as_ref().unwrap().join(" ") {
            warn(format!("editor is already {}!", v), &conf);
        } else {
            success(format!("setting editor to {}!", v), &conf);
            conf.editor(v);
            write = true;
        }
    }
    if let Some(v) = matches.value_of("max-width") {
        did_something = true;
        let v = v.parse::<usize>().unwrap();
        if conf.max_width.is_some() && v == conf.max_width.unwrap() {
            warn(format!("max-width is already {}!", v), &conf);
        } else {
            success(format!("setting max-width to {}!", v), &conf);
            conf.max_width = Some(v);
            write = true;
        }
    }
    if let Some(vs) = matches.values_of("style") {
        let values = vs.map(|s| s.to_string()).collect::<Vec<_>>();
        for v in values.windows(2) {
            let identifier = v[0].clone();
            let style = v[1].clone();
            if !STYLE_MATCHER.is_match(&style) {
                fatal(
                    format!("cannot parse '{}' as a style specification", style),
                    &conf,
                );
            }
            if conf.style_map.contains_key(&identifier) {
                conf.style_map.insert(identifier, style);
            } else {
                fatal(
                    format!("there is no configurable style named '{}'", identifier),
                    &conf,
                );
            }
            success(format!("set {} to {}", v[0], v[1]), &conf);
            did_something = true;
            write = true;
        }
    }
    if let Some(vs) = matches.values_of("unset") {
        for v in vs {
            did_something = true;
            let mut set = true;
            match v {
                "day-length" => {
                    conf.day_length = DAY_LENGTH.parse().unwrap();
                    write = true;
                }
                "editor" => {
                    conf.editor = None;
                    write = true;
                }
                "color" => {
                    conf.color = None;
                    write = true;
                }
                "clock" => {
                    conf.h12 = "12" == CLOCK;
                    write = true;
                }
                "length-pay-period" => {
                    conf.length_pay_period = LENGTH_PAY_PERIOD.parse().unwrap();
                    write = true;
                }
                "max-width" => {
                    conf.max_width = None;
                    write = true;
                }
                "precision" => {
                    conf.precision = Precision::from_s(PRECISION);
                    write = true;
                }
                "truncation" => {
                    conf.truncation = Truncation::from_s(TRUNCATION);
                    write = true;
                }
                "start-pay-period" => {
                    conf.start_pay_period = None;
                    write = true;
                }
                "sunday-begins-week" => {
                    conf.sunday_begins_week = SUNDAY_BEGINS_WEEK.parse().unwrap();
                    write = true;
                }
                "workdays" => {
                    conf.workdays(WORKDAYS);
                    write = true;
                }
                _ => {
                    let parts = v.split_whitespace().collect::<Vec<_>>();
                    if parts.len() == 2 && parts[0] == "style" {
                        write = true;
                        set = true;
                        if conf.style_map.contains_key(parts[1]) {
                            conf.style_map
                                .insert(parts[1].to_owned(), default_style(parts[1]).to_owned());
                        } else {
                            set = false;
                        }
                    } else {
                        set = false
                    }
                }
            }
            if set {
                success(format!("unset {}", v), &conf);
            } else {
                warn(format!("unknown configuration parameter!: {}", v), &conf);
            }
        }
    }
    if write {
        conf.write()
    }
    if matches.is_present("list") {
        let mut footnotes: Vec<String> = Vec::new();
        if did_something {
            println!("");
        } else {
            did_something = true;
        }
        let mut attributes = vec![
            vec![
                String::from("precision"),
                format!("{}", conf.precision.to_s()),
            ],
            vec![
                String::from("truncation"),
                format!("{}", conf.truncation.to_s()),
            ],
            vec![
                String::from("max-width"),
                if conf.max_width.is_some() {
                    format!("{}", conf.max_width.unwrap())
                } else {
                    String::from("")
                },
            ],
            vec![
                String::from("length-pay-period"),
                format!("{}", conf.length_pay_period),
            ],
            vec![
                String::from("start-pay-period"),
                format!(
                    "{}",
                    if conf.start_pay_period.is_some() {
                        let spp = conf.start_pay_period.unwrap();
                        format!("{} {} {}", spp.year(), spp.month(), spp.day())
                    } else {
                        String::from("")
                    }
                ),
            ],
            vec![
                String::from("sunday-begins-week"),
                format!("{}", conf.sunday_begins_week),
            ],
            vec![
                String::from("clock"),
                format!("{}", if conf.h12 { "12" } else { "24" }),
            ],
            vec![String::from("workdays"), conf.serialize_workdays()],
            vec![
                String::from("beginning-work-day"),
                format!(
                    "{}:{:02}",
                    conf.beginning_work_day.0, conf.beginning_work_day.1
                ),
            ],
            vec![String::from("day-length"), format!("{}", conf.day_length)],
            vec![String::from("editor"), {
                match conf.effective_editor() {
                    Some((editor, source)) => {
                        let mut editor = editor.join(" ");
                        if let Some(source) = source {
                            for _ in 0..footnotes.len() + 1 {
                                editor.push_str("*");
                            }
                            footnotes.push(source);
                        }
                        editor
                    }
                    _ => String::from(""),
                }
            }],
            vec![String::from("color"), {
                let (c, source) = conf.effective_color();
                let mut color = format!("{}", c);
                if let Some(source) = source {
                    for _ in 0..footnotes.len() + 1 {
                        color.push_str("*");
                    }
                    footnotes.push(source);
                }
                color
            }],
        ];
        for style in &conf.style_map {
            attributes.push(vec![style.0.clone(), style.1.clone()]);
        }
        let mut table = Colonnade::new(2, conf.width()).unwrap();
        table.columns[1].alignment(Alignment::Right).left_margin(2);
        let style = Style::new(&conf);
        for (i, line) in table.tabulate(&attributes).unwrap().iter().enumerate() {
            if i % 2 == 1 {
                println!("{}", style.paint("even", line)) // even in a one-indexed table
            } else {
                println!("{}", style.paint("odd", line));
            }
        }
        if !footnotes.is_empty() {
            println!("\nenvironment variable sources:");
            let data: Vec<Vec<String>> = footnotes
                .into_iter()
                .enumerate()
                .map(|(i, s)| {
                    let asterisks = std::iter::repeat("*").take(i + 1).collect::<String>();
                    vec![asterisks, s]
                })
                .collect();
            table = Colonnade::new(2, conf.width()).unwrap();
            table.columns[0].alignment(Alignment::Right).left_margin(2);
            for line in table.tabulate(data).expect("data too wide") {
                println!("{}", line);
            }
        }
    }
    if !did_something {
        println!("{}", matches.usage());
    }
}

#[derive(Debug, Clone)]
pub enum Truncation {
    Round,
    Floor,
    Ceiling,
}

impl Truncation {
    fn to_s(&self) -> &str {
        match self {
            Truncation::Round => "round",
            Truncation::Floor => "floor",
            Truncation::Ceiling => "ceiling",
        }
    }
    fn from_s(s: &str) -> Truncation {
        match s {
            "round" => Truncation::Round,
            "ceiling" => Truncation::Ceiling,
            "floor" => Truncation::Floor,
            _ => unreachable!(),
        }
    }
    pub fn prepare(&self, n: f32, precision: &Precision) -> f32 {
        match self {
            Truncation::Round => match precision {
                // these ones will be taken care of by the formatter
                Precision::P0 | Precision::P1 | Precision::P2 | Precision::P3 => n,
                _ => (n * precision.multiplier()).round() / precision.multiplier(),
            },
            _ => {
                let mut n = n * precision.multiplier();
                n = match self {
                    Truncation::Ceiling => n.ceil(),
                    Truncation::Floor => n.floor(),
                    _ => unreachable!(),
                };
                n / precision.multiplier()
            }
        }
    }
}

impl PartialEq for Truncation {
    fn eq(&self, other: &Self) -> bool {
        match self {
            Truncation::Round => match other {
                Truncation::Round => true,
                _ => false,
            },
            Truncation::Floor => match other {
                Truncation::Floor => true,
                _ => false,
            },
            Truncation::Ceiling => match other {
                Truncation::Ceiling => true,
                _ => false,
            },
        }
    }
}

#[derive(Debug, Clone)]
pub enum Precision {
    P0,
    P1,
    P2,
    P3,
    Half,
    Third,
    Quarter,
    Sixth,
    Twelfth,
    Sixtieth,
}

impl Precision {
    fn to_s(&self) -> &str {
        match self {
            Precision::P0 => "0",
            Precision::P1 => "1",
            Precision::P2 => "2",
            Precision::P3 => "3",
            Precision::Half => "half",
            Precision::Third => "third",
            Precision::Quarter => "quarter",
            Precision::Sixth => "sixth",
            Precision::Twelfth => "twelfth",
            Precision::Sixtieth => "sixtieth",
        }
    }
    fn from_s(s: &str) -> Precision {
        match s {
            "0" => Precision::P0,
            "1" => Precision::P1,
            "2" => Precision::P2,
            "3" => Precision::P3,
            "half" => Precision::Half,
            "third" => Precision::Third,
            "quarter" => Precision::Quarter,
            "sixth" => Precision::Sixth,
            "twelfth" => Precision::Twelfth,
            "sixtieth" => Precision::Sixtieth,
            _ => unreachable!(),
        }
    }
    pub fn multiplier(&self) -> f32 {
        match self {
            Precision::P0 => 1.0,
            Precision::P1 => 10.0,
            Precision::P2 => 100.0,
            Precision::P3 => 1000.0,
            Precision::Half => 2.0,
            Precision::Third => 3.0,
            Precision::Quarter => 4.0,
            Precision::Sixth => 6.0,
            Precision::Twelfth => 12.0,
            Precision::Sixtieth => 60.0,
        }
    }
    pub fn precision(&self) -> usize {
        match self {
            Precision::P0 => 0,
            Precision::P1 => 1,
            Precision::P2 => 2,
            Precision::P3 => 3,
            Precision::Half => 1,
            _ => 2,
        }
    }
}

impl PartialEq for Precision {
    fn eq(&self, other: &Self) -> bool {
        match self {
            Precision::P0 => match other {
                Precision::P0 => true,
                _ => false,
            },
            Precision::P1 => match other {
                Precision::P1 => true,
                _ => false,
            },
            Precision::P2 => match other {
                Precision::P2 => true,
                _ => false,
            },
            Precision::P3 => match other {
                Precision::P3 => true,
                _ => false,
            },
            Precision::Half => match other {
                Precision::Half => true,
                _ => false,
            },
            Precision::Third => match other {
                Precision::Third => true,
                _ => false,
            },
            Precision::Quarter => match other {
                Precision::Quarter => true,
                _ => false,
            },
            Precision::Sixth => match other {
                Precision::Sixth => true,
                _ => false,
            },
            Precision::Twelfth => match other {
                Precision::Twelfth => true,
                _ => false,
            },
            Precision::Sixtieth => match other {
                Precision::Sixtieth => true,
                _ => false,
            },
        }
    }
}

#[derive(Clone)]
pub struct Configuration {
    pub day_length: f32,
    pub editor: Option<Vec<String>>,
    pub length_pay_period: u32,
    pub precision: Precision,
    pub truncation: Truncation,
    pub start_pay_period: Option<NaiveDate>,
    pub sunday_begins_week: bool,
    pub beginning_work_day: (usize, usize),
    color: Option<bool>,
    pub workdays: u8, // bit flags
    pub max_width: Option<usize>,
    ini: Option<Ini>,
    dir: String,
    pub h12: bool,
    pub style_map: BTreeMap<String, String>,
}

fn default_style(identifier: &str) -> &'static str {
    let row = STYLES
        .iter()
        .find(|r| r[0] == identifier)
        .expect(&format!("there is no {} style", identifier));
    row[1]
}

impl Configuration {
    fn max_term_size() -> usize {
        term_size::dimensions().unwrap_or((80, 0)).0 // if term_size fails us, use a default of 80
    }
    // the minimum of the current terminal width or the configured width, if any
    pub fn width(&self) -> usize {
        let t = Configuration::max_term_size();
        if self.max_width.is_some() {
            let n = self.max_width.unwrap();
            if n > t {
                t
            } else {
                n
            }
        } else {
            t
        }
    }
    // option parameter facilitates testing
    pub fn read(path: Option<PathBuf>, directory: Option<&str>) -> Configuration {
        let path = path.unwrap_or(Configuration::config_file(directory));
        if !path.as_path().exists() {
            File::create(path.to_str().unwrap()).expect(&format!(
                "could not create configuration file {}",
                path.to_str().unwrap()
            ));
        }
        let directory = path
            .as_path()
            .canonicalize()
            .expect(&format!(
                "could not canonicalize the path {}",
                path.as_path().to_str().unwrap()
            ))
            .parent()
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        if let Ok(ini) = Ini::load_from_file(path.as_path()) {
            let editor = if let Some(s) = ini.get_from(Some("external"), "editor") {
                Some(s.split_whitespace().map(|s| s.to_owned()).collect())
            } else {
                None
            };
            let color = if let Some(s) = ini.get_from(Some("color"), "color") {
                Some(s == COLOR)
            } else {
                None
            };
            let start_pay_period = if let Some(s) = ini.get_from(Some("time"), "start-pay-period") {
                let parts = s.split(" ").collect::<Vec<&str>>();
                Some(NaiveDate::from_ymd(
                    parts[0].parse().unwrap(),
                    parts[1].parse().unwrap(),
                    parts[2].parse().unwrap(),
                ))
            } else {
                None
            };
            let beginning_work_day = if let Some(s) =
                ini.get_from(Some("time"), "beginning-work-day")
            {
                let parts: Vec<usize> = s.split(":").map(|s| s.parse::<usize>().unwrap()).collect();
                (parts[0], parts[1])
            } else {
                BEGINNING_WORK_DAY.clone()
            };
            let mut map = BTreeMap::new();
            for style in STYLES {
                map.insert(
                    style[0].to_owned(),
                    ini.get_from_or(Some("style"), style[0], style[1])
                        .to_string(),
                );
            }
            Configuration {
                beginning_work_day,
                day_length: ini
                    .get_from_or(Some("time"), "day-length", DAY_LENGTH)
                    .parse()
                    .unwrap(),
                editor: editor,
                length_pay_period: ini
                    .get_from_or(Some("time"), "pay-period-length", LENGTH_PAY_PERIOD)
                    .parse()
                    .unwrap(),
                precision: Precision::from_s(ini.get_from_or(
                    Some("summary"),
                    "precision",
                    PRECISION,
                )),
                truncation: Truncation::from_s(ini.get_from_or(
                    Some("summary"),
                    "truncation",
                    TRUNCATION,
                )),
                start_pay_period: start_pay_period,
                sunday_begins_week: ini.get_from_or(
                    Some("time"),
                    "sunday-begins-week",
                    SUNDAY_BEGINS_WEEK,
                ) == "true",
                h12: ini.get_from_or(Some("summary"), "clock", CLOCK) == "12",
                color: color,
                workdays: Configuration::parse_workdays(ini.get_from_or(
                    Some("time"),
                    "workdays",
                    WORKDAYS,
                )),
                max_width: ini
                    .get_from(Some("summary"), "max-width")
                    .and_then(|s| Some(s.parse().unwrap())),
                dir: directory,
                ini: Some(ini),
                style_map: map,
            }
        } else {
            let mut map = BTreeMap::new();
            for style in STYLES {
                map.insert(style[0].to_owned(), style[1].to_owned());
            }
            Configuration {
                ini: None,
                day_length: DAY_LENGTH.parse().unwrap(),
                editor: None,
                length_pay_period: LENGTH_PAY_PERIOD.parse().unwrap(),
                beginning_work_day: BEGINNING_WORK_DAY.clone(),
                precision: Precision::from_s(PRECISION),
                truncation: Truncation::from_s(TRUNCATION),
                start_pay_period: None,
                color: None,
                sunday_begins_week: SUNDAY_BEGINS_WEEK == "true",
                workdays: Configuration::parse_workdays(WORKDAYS),
                max_width: None,
                dir: directory,
                h12: CLOCK == "12",
                style_map: map,
            }
        }
    }
    pub fn write(&self) {
        let mut ini = Ini::new();
        if self.day_length != DAY_LENGTH.parse::<f32>().unwrap() {
            ini.with_section(Some("time"))
                .set("day-length", format!("{}", self.day_length));
        }
        if self.beginning_work_day != BEGINNING_WORK_DAY {
            ini.with_section(Some("time")).set(
                "beginning-work-day",
                format!(
                    "{}:{}",
                    self.beginning_work_day.0, self.beginning_work_day.1
                ),
            );
        }
        if let Some(s) = self.editor.as_ref() {
            let s = s.join(" ");
            ini.with_section(Some("external")).set("editor", s);
        }
        if self.length_pay_period != LENGTH_PAY_PERIOD.parse::<u32>().unwrap() {
            ini.with_section(Some("time"))
                .set("pay-period-length", format!("{}", self.length_pay_period));
        }
        if self.precision != Precision::from_s(PRECISION) {
            ini.with_section(Some("summary"))
                .set("precision", format!("{}", self.precision.to_s()));
        }
        if self.truncation != Truncation::from_s(TRUNCATION) {
            ini.with_section(Some("summary"))
                .set("truncation", format!("{}", self.truncation.to_s()));
        }
        if self.start_pay_period.is_some() {
            let spp = self.start_pay_period.unwrap();
            ini.with_section(Some("time")).set(
                "start-pay-period",
                format!("{} {} {}", spp.year(), spp.month(), spp.day()),
            );
        }
        if self.sunday_begins_week != SUNDAY_BEGINS_WEEK.parse::<bool>().unwrap() {
            ini.with_section(Some("time"))
                .set("sunday-begins-week", format!("{}", self.sunday_begins_week));
        }
        if self.h12 != (CLOCK == "12") {
            ini.with_section(Some("summary"))
                .set("clock", format!("{}", if self.h12 { "12" } else { "24" }));
        }
        if let Some(c) = self.color {
            ini.with_section(Some("color"))
                .set("color", format!("{}", c));
        }
        let s = self.serialize_workdays();
        if s != WORKDAYS {
            ini.with_section(Some("time")).set("workdays", s);
        }
        if self.max_width.is_some() {
            ini.with_section(Some("summary"))
                .set("max-width", format!("{}", self.max_width.unwrap()));
        }
        for style in &self.style_map {
            if style.1 != default_style(&style.0) {
                ini.with_section(Some("style")).set(style.0, style.1);
            }
        }
        ini.write_to_file(Configuration::config_file(Some(&self.dir)))
            .expect("could not write config.ini");
    }
    pub fn directory(&self) -> Option<&str> {
        Some(&self.dir)
    }
    // public for testing purposes
    pub fn workdays(&mut self, workdays: &str) {
        self.workdays = Configuration::parse_workdays(workdays);
    }
    fn editor(&mut self, editor: &str) {
        self.editor = Some(editor.split_whitespace().map(|s| s.to_owned()).collect());
    }
    // returns value and its environment variable source, if any
    pub fn effective_editor(&self) -> Option<(Vec<String>, Option<String>)> {
        if let Some(vec) = self.editor.clone() {
            Some((vec, None))
        } else {
            let mut var = String::from("VISUAL");
            match env::var(&var) {
                Ok(s) => Some((
                    s.split_whitespace().map(|s| s.to_owned()).collect(),
                    Some(var),
                )),
                _ => {
                    var = String::from("EDITOR");
                    match env::var(&var) {
                        Ok(s) => Some((
                            s.split_whitespace().map(|s| s.to_owned()).collect(),
                            Some(var),
                        )),
                        _ => None,
                    }
                }
            }
        }
    }
    pub fn effective_color(&self) -> (bool, Option<String>) {
        if let Some(c) = self.color {
            (c, None)
        } else {
            let var = String::from("NO_COLOR");
            match env::var(&var) {
                Ok(_) => (false, Some(var)),
                _ => (COLOR == "true", None),
            }
        }
    }
    pub fn config_file(directory: Option<&str>) -> PathBuf {
        let mut path = base_dir(directory);
        path.push("config.ini");
        path
    }
    fn parse_workdays(serialized: &str) -> u8 {
        let mut workdays: u8 = 0;
        for c in serialized.chars() {
            if let Some(i) = "SMTWHFA".chars().position(|c2| c2 == c) {
                workdays = workdays | (1 << i);
            }
        }
        workdays
    }
    fn serialize_workdays(&self) -> String {
        let mut s = String::new();
        for (i, c) in "SMTWHFA".chars().enumerate() {
            if (1 << i) & self.workdays > 0 {
                s.push(c);
            }
        }
        s
    }
    pub fn is_workday(&self, date: &NaiveDate) -> bool {
        let i = (date.weekday().number_from_sunday() - 1) as u8;
        self.workdays & (1 << i) > 0
    }
    pub fn two_timer_config(&self) -> Option<Config> {
        Some(
            Config::new()
                .monday_starts_week(!self.sunday_begins_week)
                .pay_period_start(self.start_pay_period)
                .pay_period_length(self.length_pay_period),
        )
    }
    pub fn set_precision(&mut self, identifier: &str) {
        self.precision = Precision::from_s(identifier);
    }
    pub fn set_truncation(&mut self, identifier: &str) {
        self.truncation = Truncation::from_s(identifier);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_quarter() {
        let trunctation = Truncation::Round;
        let precision = Precision::Quarter;
        assert_eq!(0.0, trunctation.prepare(0.0, &precision));
        assert_eq!(0.0, trunctation.prepare(0.11, &precision));
        assert_eq!(0.25, trunctation.prepare(0.125, &precision));
        assert_eq!(0.25, trunctation.prepare(0.26, &precision));
    }

    #[test]
    fn floor_quarter() {
        let trunctation = Truncation::Floor;
        let precision = Precision::Quarter;
        assert_eq!(0.0, trunctation.prepare(0.0, &precision));
        assert_eq!(0.0, trunctation.prepare(0.11, &precision));
        assert_eq!(0.0, trunctation.prepare(0.125, &precision));
        assert_eq!(0.25, trunctation.prepare(0.25, &precision));
        assert_eq!(0.25, trunctation.prepare(0.26, &precision));
    }

    #[test]
    fn ceiling_quarter() {
        let trunctation = Truncation::Ceiling;
        let precision = Precision::Quarter;
        assert_eq!(0.0, trunctation.prepare(0.0, &precision));
        assert_eq!(0.25, trunctation.prepare(0.11, &precision));
        assert_eq!(0.25, trunctation.prepare(0.125, &precision));
        assert_eq!(0.25, trunctation.prepare(0.25, &precision));
        assert_eq!(0.5, trunctation.prepare(0.26, &precision));
    }

    #[test]
    fn floor_p0() {
        let trunctation = Truncation::Floor;
        let precision = Precision::P0;
        assert_eq!(0.0, trunctation.prepare(0.0, &precision));
        assert_eq!(0.0, trunctation.prepare(0.9, &precision));
        assert_eq!(1.0, trunctation.prepare(1.0, &precision));
        assert_eq!(1.0, trunctation.prepare(1.9, &precision));
    }

    #[test]
    fn ceiling_p0() {
        let trunctation = Truncation::Ceiling;
        let precision = Precision::P0;
        assert_eq!(0.0, trunctation.prepare(0.0, &precision));
        assert_eq!(1.0, trunctation.prepare(0.11, &precision));
        assert_eq!(1.0, trunctation.prepare(1.0, &precision));
        assert_eq!(2.0, trunctation.prepare(1.1, &precision));
    }

    #[test]
    fn floor_p1() {
        let trunctation = Truncation::Floor;
        let precision = Precision::P1;
        assert_eq!(0.0, trunctation.prepare(0.0, &precision));
        assert_eq!(0.0, trunctation.prepare(0.09, &precision));
        assert_eq!(0.1, trunctation.prepare(0.1, &precision));
        assert_eq!(0.1, trunctation.prepare(0.19, &precision));
    }

    #[test]
    fn ceiling_p1() {
        let trunctation = Truncation::Ceiling;
        let precision = Precision::P1;
        assert_eq!(0.0, trunctation.prepare(0.0, &precision));
        assert_eq!(0.1, trunctation.prepare(0.011, &precision));
        assert_eq!(0.1, trunctation.prepare(0.1, &precision));
        assert_eq!(0.2, trunctation.prepare(0.11, &precision));
    }

    #[test]
    fn floor_p2() {
        let trunctation = Truncation::Floor;
        let precision = Precision::P2;
        assert_eq!(0.0, trunctation.prepare(0.0, &precision));
        assert_eq!(0.0, trunctation.prepare(0.009, &precision));
        assert_eq!(0.01, trunctation.prepare(0.01, &precision));
        assert_eq!(0.01, trunctation.prepare(0.019, &precision));
    }

    #[test]
    fn ceiling_p2() {
        let trunctation = Truncation::Ceiling;
        let precision = Precision::P2;
        assert_eq!(0.0, trunctation.prepare(0.0, &precision));
        assert_eq!(0.01, trunctation.prepare(0.0011, &precision));
        assert_eq!(0.01, trunctation.prepare(0.01, &precision));
        assert_eq!(0.02, trunctation.prepare(0.011, &precision));
    }

    #[test]
    fn floor_p3() {
        let trunctation = Truncation::Floor;
        let precision = Precision::P3;
        assert_eq!(0.0, trunctation.prepare(0.0, &precision));
        assert_eq!(0.0, trunctation.prepare(0.0009, &precision));
        assert_eq!(0.001, trunctation.prepare(0.001, &precision));
        assert_eq!(0.001, trunctation.prepare(0.0019, &precision));
    }

    #[test]
    fn ceiling_p3() {
        let trunctation = Truncation::Ceiling;
        let precision = Precision::P3;
        assert_eq!(0.0, trunctation.prepare(0.0, &precision));
        assert_eq!(0.001, trunctation.prepare(0.00011, &precision));
        assert_eq!(0.001, trunctation.prepare(0.001, &precision));
        assert_eq!(0.002, trunctation.prepare(0.0011, &precision));
    }
}
