use crate::util::{base_dir, fatal, is_valid_style, success, valid_time_expression, warn, Style};
use chrono::{Datelike, Duration, NaiveDate};
use colonnade::{Alignment, Colonnade};
use ini::Ini;
use regex::Regex;
use std::collections::BTreeMap;
use std::env;
use std::fs::File;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::LazyLock;
use two_timer::{parse, Config};

pub const PRECISION: &str = "2";
pub const SUNDAY_BEGINS_WEEK: &str = "true";
pub const LENGTH_PAY_PERIOD: &str = "14";
pub const DAY_LENGTH: &str = "8";
pub const BEGINNING_WORK_DAY: (usize, usize) = (9, 0);
pub const WORKDAYS: &str = "MTWHF";
pub const COLOR: &str = "true";
pub const TRUNCATION: &str = "round";
pub const CLOCK: &str = "12";
pub const STYLES: &[[&str; 4]] = &[
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

pub const AFTER_HELP: &str = "\
    Set or display configuration parameters that control date interpretation, log summarization, etc. \
    Some configuration may be taken from environment variables -- VISUAL, EDITOR, NO_COLOR. \
If this is occurring, this will be explained when you list the configuration.

The nu-ansi-term crate (a maintained fork of ansi_term) is used to provide the optional styling. One can find a list of the fixed color \
    values at https://en.wikipedia.org/wiki/ANSI_escape_code#8-bit. Style specifications are parsed \
according to the following grammar:

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

fn outro() -> &'static str {
    "All prefixes of 'configure' are aliases of the subcommand.\n"
}

fn describe_styles() -> String {
    let mut data = vec![["IDENTIFIER", "DEFAULT STYLE", "DESCRIPTION", "EXAMPLE"]
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>()];
    for row in STYLES {
        data.push(row.iter().map(|s| s.to_string()).collect());
    }
    let max_width = terminal_size::terminal_size()
        .map(|(w, _)| w.0 as usize)
        .unwrap_or(100);
    let width = if max_width > 100 { 100 } else { max_width };
    let mut colonnade = Colonnade::new(4, width).expect("could not tabulate styles");
    colonnade
        .spaces_between_rows(1)
        .padding_left(2)
        .expect("insufficient space to tabulate styles");
    colonnade.columns[0].set_priority(0);
    colonnade.columns[1].set_priority(0);
    colonnade.columns[2].set_priority(1);
    colonnade.columns[3].set_priority(1);
    colonnade
        .tabulate(data)
        .expect("could not tabulate data")
        .join("\n")
        + "\n"
}

fn after_help() -> String {
    let mut s = String::from(AFTER_HELP);
    s.push_str(&describe_styles());
    s.push('\n');
    s.push_str(outro());
    s
}

fn valid_length_pay_period(v: &str) -> Result<String, String> {
    match v.parse::<u32>() {
        Ok(n) if n > 0 => Ok(v.to_owned()),
        Ok(_) => Err(String::from("a pay period must have some positive length")),
        Err(_) => Err(String::from("some (small) whole number of days expected")),
    }
}

fn valid_day_length(v: &str) -> Result<String, String> {
    match v.parse::<f32>() {
        Ok(n) if n > 0.0 && n <= 24.0 => Ok(v.to_owned()),
        Ok(n) if n > 24.0 => Err(String::from("one cannot work more than 24 hours in a day")),
        Ok(_) => Err(String::from("a positive number of hours expected")),
        Err(_) => Err(String::from("some (small) number of hours expected")),
    }
}

fn valid_max_width(v: &str) -> Result<String, String> {
    match v.parse::<usize>() {
        Ok(n) if n >= 40 => Ok(v.to_owned()),
        Ok(_) => Err(String::from(
            "summaries in fewer than 40 columns will be unreadable",
        )),
        Err(_) => Err(String::from("some whole number of columns expected")),
    }
}

fn valid_beginning_work_day(v: &str) -> Result<String, String> {
    static RX: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\A([1-9]\d?)(?::([0-6]\d))?\z").unwrap());
    if let Some(captures) = RX.captures(v) {
        let hour = captures[1].parse::<usize>().unwrap();
        if hour < 24 {
            if let Some(m) = captures.get(2) {
                let minute = m.as_str().parse::<usize>().unwrap();
                if minute < 60 {
                    Ok(v.to_owned())
                } else {
                    Err(format!(
                        "minute in beginning work day expression '{}' must be less than 60",
                        v
                    ))
                }
            } else {
                Ok(v.to_owned())
            }
        } else {
            Err(format!(
                "hour in beginning work day expression '{}' must be less than 24",
                v
            ))
        }
    } else {
        Err(String::new())
    }
}

fn valid_workdays(v: &str) -> Result<String, String> {
    static RX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A[SMTWHFA]+\z").unwrap());
    if RX.is_match(v) {
        Ok(v.to_owned())
    } else {
        Err(String::from(
            "must contain only the letters SMTWHFA, where S means Sunday and A, Saturday, etc.",
        ))
    }
}

#[derive(Debug, clap::Args)]
#[command(
    aliases = ["c", "co", "con", "conf", "confi", "config", "configu", "configur"],
    about = "Sets or displays configuration parameters",
    after_help = after_help(),
    after_long_help = after_help()
)]
pub struct Args {
    /// Sets decimal places of precision in display of time; default value: 2
    ///
    /// The number of decimal places of precision used in the display of lengths of periods in numbers of hours. \
    /// If the number is 0, probably not what you want, all periods will be rounded to a whole number of hours. \
    /// The default value is 2. If the precision is a fraction like 'quarter' times will be rounded to the closest fraction that size of the hour for display.
    // NOTE keep in sync with the option in summary
    #[arg(
        long,
        value_name = "precision",
        value_parser = ["0", "1", "2", "3", "half", "third", "quarter", "sixth", "twelfth", "sixtieth"]
    )]
    pub precision: Option<String>,

    /// Sets how fractional parts of a duration too small to display for the given precision are handled; default value: round
    ///
    /// When an events duration is displayed, there is generally some amount of information not \
    /// displayed given the precision. By default this portion is rounded, so if the precision is a quarter \
    /// hour and the duration is 7.5 minutes, this will be displayed as 0.25 hours. Alternatively, one could \
    /// use the floor, in which case this would be 0.00 hours, or the ceiling, in which case even a single \
    /// second task would be shown as taking 0.25 hours.
    // NOTE keep in sync with the option in summary
    #[arg(long, value_name = "function", value_parser = ["round", "floor", "ceiling"])]
    pub truncation: Option<String>,

    /// Sets the first day of some pay period
    ///
    /// A day relative to which all pay periods will be calculated. See --length-pay-period.
    #[arg(long, value_name = "date", value_parser = valid_time_expression)]
    pub start_pay_period: Option<String>,

    /// Sets whether Sunday should be considered the first day of the week; default value; true
    #[arg(long, value_name = "bool", value_parser = ["true", "false"])]
    pub sunday_begins_week: Option<String>,

    /// Sets times should be displayed with a 12-hour or a 24-hour clock; default value; 12
    #[arg(long, value_name = "type", value_parser = ["12", "24"])]
    pub clock: Option<String>,

    /// Sets the number of days in a pay period; default value: 14
    #[arg(long, value_name = "int", value_parser = valid_length_pay_period)]
    pub length_pay_period: Option<String>,

    /// Sets expected number of hours in a workday; default value: 8
    #[arg(long, value_name = "num", value_parser = valid_day_length)]
    pub day_length: Option<String>,

    /// Sets when a work day typically begins; default value: 9:00
    #[arg(long, value_name = "hours[:minutes]", value_parser = valid_beginning_work_day)]
    pub beginning_work_day: Option<String>,

    /// Sets which days you are expected to work; default value: MTWHF
    ///
    /// Workdays during the week represented as a subset of SMTWHFA, where S is Sunday and A is Saturday, etc. Default value: MTWHF.
    #[arg(long, value_name = "days", value_parser = valid_workdays)]
    pub workdays: Option<String>,

    /// Sets text editor to use when manually editing the log
    ///
    /// A text editor that the edit command will invoke. E.g., /usr/bin/vim. \
    /// If no editor is set, job falls back to the environment variables VISUAL and EDITOR in that order. \
    /// If there is still no editor, you cannot use the edit command to edit the log. \
    /// Note, whatever editor you use must be invocable from the shell as <editor> <file>. \
    /// If you need to pass additional arguments to the executable, provide them delimited by spaces \
    /// in the same argument. E.g., --editor='/usr/bin/open -W -n -t'
    #[arg(long, value_name = "path")]
    pub editor: Option<String>,

    /// Sets maximum number of columns when summarizing data
    #[arg(long, value_name = "num", value_parser = valid_max_width)]
    pub max_width: Option<String>,

    /// Sets whether to use colors; default value: true
    ///
    /// Color variation helps one parse information quickly, but if you don't want it, \
    /// or the ANSI color codes that produce it cause you trouble, you can turn it off. \
    /// If you haven't set this parameter and you don't have the NO_COLOR environment variable, Job Log will use color.
    #[arg(long, value_name = "bool", value_parser = ["true", "false"])]
    pub color: Option<String>,

    /// Sets the style for a particular style identifier
    ///
    /// Sets the style for a particular style identifier. E.g., --style header 'bold italic purple'
    #[arg(long, value_name = "id spec", num_args = 2)]
    pub style: Vec<String>,

    /// Sets the time budget for a particular tag
    ///
    /// Sets the time budget within the pay period for a particular tag. See the \"when\" command. \
    /// E.g., --budget foo 12.5
    #[arg(short = 'b', long, value_name = "tag hours", num_args = 2)]
    pub budget: Vec<String>,

    /// Returns a configurable parameter to its default; to unset styles you need to provide both \
    /// 'style' and the parameter you wish to unset; e.g., --unset 'style even'. \
    /// Likewise for time budgets you need to provide both 'budget' and a tag identifying a particular \
    /// budget; e.g., --unset 'budget foo'
    #[arg(short = 'u', long, value_name = "param")]
    pub unset: Vec<String>,

    /// Lists all configuration parameters
    ///
    /// List all configuration parameters and their values.
    #[arg(short = 'l', long)]
    pub list: bool,
}

pub fn run(directory: Option<&str>, args: &Args) {
    let mut did_something = false;
    let mut write = false;
    let mut conf = Configuration::read(None, directory);
    if let Some(v) = &args.start_pay_period {
        let tt_conf = Config::new()
            .monday_starts_week(!conf.sunday_begins_week)
            .pay_period_length(conf.length_pay_period)
            .pay_period_start(conf.start_pay_period);
        if let Ok(range) = parse(v, Some(tt_conf)) {
            did_something = true;
            let start_date_time = range.start;
            let year = start_date_time.year();
            let month = start_date_time.month();
            let day = start_date_time.day();
            let start_date = NaiveDate::from_ymd_opt(year, month, day).unwrap();
            if conf.start_pay_period.is_some()
                && &start_date == conf.start_pay_period.as_ref().unwrap()
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
        } else {
            warn(
                format!("could not parse start pay period date: {}", v),
                &conf,
            );
        }
    }
    if let Some(v) = &args.sunday_begins_week {
        did_something = true;
        let v: bool = v.parse().unwrap();
        if v == conf.sunday_begins_week {
            warn(format!("sunday-begins-week is already {}!", v), &conf);
        } else {
            success(format!("setting sunday-begins-week to {}!", v), &conf);
            conf.sunday_begins_week = v;
            write = true;
        }
    }
    if let Some(v) = &args.clock {
        did_something = true;
        if (v == CLOCK) == conf.h12 {
            warn(format!("clock is already {}!", v), &conf);
        } else {
            success(format!("setting clock to {}!", v), &conf);
            conf.h12 = v == CLOCK;
            write = true;
        }
    }
    if let Some(v) = &args.color {
        did_something = true;
        let v: bool = v.parse().unwrap();
        conf.color = Some(v);
        // demonstrate that we've set the color
        success(format!("set color to {}!", v), &conf);
        write = true;
    }
    if let Some(v) = &args.length_pay_period {
        did_something = true;
        let v: u32 = v.parse().unwrap();
        if v == conf.length_pay_period {
            warn(format!("length-pay-period is already {}!", v), &conf);
        } else {
            success(format!("setting length-pay-period to {}!", v), &conf);
            conf.length_pay_period = v;
            write = true;
        }
    }
    if let Some(v) = &args.beginning_work_day {
        did_something = true;
        static RX: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"\A(\d+)(?::0*(\d+))?\z").unwrap());
        let captures = RX.captures(v).unwrap();
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
    if let Some(v) = &args.day_length {
        did_something = true;
        let v: f32 = v.parse().unwrap();
        if v == conf.day_length {
            warn(format!("day-length is already {}!", v), &conf);
        } else {
            success(format!("setting day-length to {}!", v), &conf);
            conf.day_length = v;
            write = true;
        }
    }
    if let Some(v) = &args.precision {
        did_something = true;
        let v = Precision::from_str(v).unwrap();
        if v == conf.precision {
            warn(format!("precision is already {}!", v), &conf);
        } else {
            success(format!("setting precision to {}!", v), &conf);
            conf.precision = v;
            write = true;
        }
    }
    if let Some(v) = &args.truncation {
        did_something = true;
        let v = Truncation::from_str(v).unwrap();
        if v == conf.truncation {
            warn(format!("truncation is already {}!", v), &conf);
        } else {
            success(format!("setting truncation to {}!", v), &conf);
            conf.truncation = v;
            write = true;
        }
    }
    if let Some(v) = &args.workdays {
        did_something = true;
        if v == &conf.serialize_workdays() {
            warn(format!("workdays is already {}!", v), &conf);
        } else {
            success(format!("setting workdays to {}!", v), &conf);
            conf.workdays(v);
            write = true;
        }
    }
    if let Some(v) = &args.editor {
        did_something = true;
        if let Some(editor) = &conf.editor {
            if *v == editor.join(" ") {
                warn(format!("editor is already {}!", v), &conf);
            } else {
                success(format!("setting editor to {}!", v), &conf);
                conf.editor(v);
                write = true;
            }
        } else {
            success(format!("setting editor to {}!", v), &conf);
            conf.editor(v);
            write = true;
        }
    }
    if let Some(v) = &args.max_width {
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
    if !args.style.is_empty() {
        for v in args.style.windows(2) {
            let identifier = v[0].clone();
            let style = v[1].clone();
            if !is_valid_style(&style) {
                fatal(
                    format!("cannot parse \"{}\" as a style specification", style),
                    &conf,
                );
            }
            if !conf.style_map.contains_key(&identifier) {
                fatal(
                    format!("there is no configurable style named '{}'", identifier),
                    &conf,
                );
            }
            conf.style_map.insert(identifier, style);
            success(format!("set {} to {}", v[0], v[1]), &conf);
            did_something = true;
            write = true;
        }
    }
    if !args.budget.is_empty() {
        if let Some(total_hours) = conf.hours_in_pay_period() {
            if total_hours == 0.0 {
                fatal(
                    "cannot set time budgets if there are no expected work hours in pay period"
                        .to_owned(),
                    &conf,
                );
            } else {
                let mut budgets: Vec<(String, f32)> = conf
                    .budgets
                    .clone()
                    .unwrap_or_default()
                    .iter()
                    .map(|(tag, hours)| (tag.clone(), *hours))
                    .collect();
                for v in args.budget.windows(2) {
                    let tag = v[0].clone();
                    let hours = v[1].clone();
                    if let Ok(h) = hours.parse::<f32>() {
                        if let Some(pair) = budgets.iter_mut().find(|p| p.0 == tag) {
                            pair.1 = h;
                        } else {
                            budgets.push((tag, h))
                        }
                        success(
                            format!("set time budget for \"{}\" to {} hours", v[0], v[1]),
                            &conf,
                        );
                        did_something = true;
                        write = true;
                    } else {
                        fatal(
                            format!("cannot parse \"{}\" as a number of hours", hours),
                            &conf,
                        );
                    }
                }
                let budgeted_hours: f32 = budgets.iter().map(|p| p.1).sum();
                conf.budgets = Some(budgets);
                if budgeted_hours > total_hours {
                    warn(
                        format!(
                            "hours budgeted: {}; hours in pay period: {}",
                            budgeted_hours, total_hours
                        ),
                        &conf,
                    )
                }
            }
        } else {
            fatal(
                "cannot set time budgets without an established pay period".to_owned(),
                &conf,
            )
        }
    }
    for v in &args.unset {
        did_something = true;
        let mut set = true;
        let mut warning = None;
        match v.as_str() {
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
                conf.precision = Precision::from_str(PRECISION).unwrap();
                write = true;
            }
            "truncation" => {
                conf.truncation = Truncation::from_str(TRUNCATION).unwrap();
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
                    if conf.style_map.contains_key(parts[1]) {
                        write = true;
                        set = true;
                        conf.style_map
                            .insert(parts[1].to_owned(), default_style(parts[1]).to_owned());
                    } else {
                        warning = Some(format!("unknown style: \"{}\"", parts[1]));
                        set = false;
                    }
                } else if parts.len() > 1 && parts[0] == "budget" {
                    let tag = parts[1..parts.len()].join(" ");
                    let mut budgets: Vec<(String, f32)> = conf
                        .budgets
                        .clone()
                        .unwrap_or_default()
                        .iter()
                        .map(|(tag, hours)| (tag.clone(), *hours))
                        .collect();
                    if let Some(i) = budgets
                        .iter()
                        .position(|p| p.0.split_whitespace().collect::<Vec<_>>().join(" ") == tag)
                    {
                        write = true;
                        set = true;
                        if budgets.len() == 1 {
                            conf.budgets = None
                        } else {
                            budgets.remove(i);
                            conf.budgets = Some(budgets)
                        }
                    } else {
                        warning = Some(format!("unknown budget: \"{}\"", tag));
                        set = false
                    }
                } else {
                    set = false
                }
            }
        }
        if set {
            success(format!("unset {}", v), &conf);
        } else {
            warn(
                warning.unwrap_or_else(|| format!("unknown configuration parameter: {}", v)),
                &conf,
            );
        }
    }
    if write {
        conf.write()
    }
    if args.list {
        let mut footnotes: Vec<String> = Vec::new();
        if did_something {
            println!();
        } else {
            did_something = true;
        }
        let mut attributes = vec![
            vec![String::from("precision"), format!("{}", conf.precision)],
            vec![String::from("truncation"), format!("{}", conf.truncation)],
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
                                editor.push('*');
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
                        color.push('*');
                    }
                    footnotes.push(source);
                }
                color
            }],
        ];
        for style in &conf.style_map {
            attributes.push(vec![style.0.clone(), style.1.clone()]);
        }
        if let Some(budgets) = &conf.budgets {
            attributes.push(vec!["time budgets".to_owned(), "".to_owned()]);
            for budget in budgets.iter() {
                attributes.push(vec![
                    format!("\u{00A0}\u{00A0}{}", budget.0),
                    format!("{}", budget.1),
                ])
            }
        }
        let mut table = Colonnade::new(2, conf.width()).unwrap();
        table.columns[1]
            .set_alignment(Alignment::Right)
            .set_left_margin(2);
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
                    let asterisks = "*".repeat(i + 1);
                    vec![asterisks, s]
                })
                .collect();
            table = Colonnade::new(2, conf.width()).unwrap();
            table.columns[0]
                .set_alignment(Alignment::Right)
                .set_left_margin(2);
            for line in table.tabulate(data).expect("data too wide") {
                println!("{}", line);
            }
        }
    }
    if !did_something {
        let cmd = clap::Command::new("configure");
        let mut cmd = <Args as clap::Args>::augment_args(cmd);
        println!("{}", cmd.render_usage());
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Truncation {
    Round,
    Floor,
    Ceiling,
}

impl Truncation {
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

impl std::fmt::Display for Truncation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Truncation::Round => "round",
            Truncation::Floor => "floor",
            Truncation::Ceiling => "ceiling",
        };
        write!(f, "{}", s)
    }
}

impl FromStr for Truncation {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "round" => Ok(Truncation::Round),
            "ceiling" => Ok(Truncation::Ceiling),
            "floor" => Ok(Truncation::Floor),
            _ => Err(format!("\"{}\" is not a valid truncation", s)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

impl std::fmt::Display for Precision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
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
        };
        write!(f, "{}", s)
    }
}

impl FromStr for Precision {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "0" => Ok(Precision::P0),
            "1" => Ok(Precision::P1),
            "2" => Ok(Precision::P2),
            "3" => Ok(Precision::P3),
            "half" => Ok(Precision::Half),
            "third" => Ok(Precision::Third),
            "quarter" => Ok(Precision::Quarter),
            "sixth" => Ok(Precision::Sixth),
            "twelfth" => Ok(Precision::Twelfth),
            "sixtieth" => Ok(Precision::Sixtieth),
            _ => Err(format!("\"{}\" is not a valid precision", s)),
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
    dir: String,
    pub h12: bool,
    pub style_map: BTreeMap<String, String>,
    pub budgets: Option<Vec<(String, f32)>>,
}

fn default_style(identifier: &str) -> &'static str {
    let row = STYLES
        .iter()
        .find(|r| r[0] == identifier)
        .unwrap_or_else(|| panic!("there is no {} style", identifier));
    row[1]
}

impl Configuration {
    fn max_term_size() -> usize {
        // if terminal_size fails us, use a default of 80
        terminal_size::terminal_size()
            .map(|(w, _)| w.0 as usize)
            .unwrap_or(80)
    }
    // the minimum of the current terminal width or the configured width, if any
    pub fn width(&self) -> usize {
        let t = Configuration::max_term_size();
        if let Some(n) = self.max_width {
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
        let path = path.unwrap_or_else(|| Configuration::config_file(directory));
        if !path.as_path().exists() {
            File::create(path.to_str().unwrap()).unwrap_or_else(|_| {
                panic!(
                    "could not create configuration file {}",
                    path.to_str().unwrap()
                )
            });
        }
        let directory = path
            .as_path()
            .canonicalize()
            .unwrap_or_else(|_| panic!("could not canonicalize the path {}", path.display()))
            .parent()
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        if let Ok(ini) = Ini::load_from_file(path.as_path()) {
            let editor = ini
                .get_from(Some("external"), "editor")
                .map(|s| s.split_whitespace().map(|s| s.to_owned()).collect());
            let color = ini.get_from(Some("color"), "color").map(|s| s == COLOR);
            let start_pay_period = ini.get_from(Some("time"), "start-pay-period").map(|s| {
                let parts = s.split(' ').collect::<Vec<&str>>();
                NaiveDate::from_ymd_opt(
                    parts[0].parse().unwrap(),
                    parts[1].parse().unwrap(),
                    parts[2].parse().unwrap(),
                )
                .unwrap()
            });
            let beginning_work_day = if let Some(s) =
                ini.get_from(Some("time"), "beginning-work-day")
            {
                let parts: Vec<usize> = s.split(':').map(|s| s.parse::<usize>().unwrap()).collect();
                (parts[0], parts[1])
            } else {
                BEGINNING_WORK_DAY
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
                editor,
                length_pay_period: ini
                    .get_from_or(Some("time"), "pay-period-length", LENGTH_PAY_PERIOD)
                    .parse()
                    .unwrap(),
                precision: Precision::from_str(ini.get_from_or(
                    Some("summary"),
                    "precision",
                    PRECISION,
                ))
                .unwrap(),
                truncation: Truncation::from_str(ini.get_from_or(
                    Some("summary"),
                    "truncation",
                    TRUNCATION,
                ))
                .unwrap(),
                start_pay_period,
                sunday_begins_week: ini.get_from_or(
                    Some("time"),
                    "sunday-begins-week",
                    SUNDAY_BEGINS_WEEK,
                ) == "true",
                h12: ini.get_from_or(Some("summary"), "clock", CLOCK) == "12",
                color,
                workdays: Configuration::parse_workdays(ini.get_from_or(
                    Some("time"),
                    "workdays",
                    WORKDAYS,
                )),
                max_width: ini
                    .get_from(Some("summary"), "max-width")
                    .and_then(|s| s.parse().ok()),
                dir: directory,
                style_map: map,
                budgets: ini.section(Some("budget")).map(|p| {
                    p.iter()
                        .map(|(key, value)| (String::from(key), value.parse::<f32>().unwrap()))
                        .collect()
                }),
            }
        } else {
            Configuration::defaults(directory)
        }
    }
    // factored out to facilitate testing
    fn defaults(directory: String) -> Configuration {
        let mut map = BTreeMap::new();
        for style in STYLES {
            map.insert(style[0].to_owned(), style[1].to_owned());
        }
        Configuration {
            day_length: DAY_LENGTH.parse().unwrap(),
            editor: None,
            length_pay_period: LENGTH_PAY_PERIOD.parse().unwrap(),
            beginning_work_day: BEGINNING_WORK_DAY,
            precision: Precision::from_str(PRECISION).unwrap(),
            truncation: Truncation::from_str(TRUNCATION).unwrap(),
            start_pay_period: None,
            color: None,
            sunday_begins_week: SUNDAY_BEGINS_WEEK == "true",
            workdays: Configuration::parse_workdays(WORKDAYS),
            max_width: None,
            dir: directory,
            h12: CLOCK == "12",
            style_map: map,
            budgets: None,
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
        if self.precision != Precision::from_str(PRECISION).unwrap() {
            ini.with_section(Some("summary"))
                .set("precision", format!("{}", self.precision));
        }
        if self.truncation != Truncation::from_str(TRUNCATION).unwrap() {
            ini.with_section(Some("summary"))
                .set("truncation", format!("{}", self.truncation));
        }
        if let Some(spp) = self.start_pay_period {
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
                .set("clock", (if self.h12 { "12" } else { "24" }).to_string());
        }
        if let Some(c) = self.color {
            ini.with_section(Some("color"))
                .set("color", format!("{}", c));
        }
        let s = self.serialize_workdays();
        if s != WORKDAYS {
            ini.with_section(Some("time")).set("workdays", s);
        }
        if let Some(max_width) = self.max_width {
            ini.with_section(Some("summary"))
                .set("max-width", format!("{}", max_width));
        }
        for style in &self.style_map {
            if style.1 != default_style(style.0) {
                ini.with_section(Some("style")).set(style.0, style.1);
            }
        }
        if let Some(budgets) = &self.budgets {
            for pair in budgets {
                ini.with_section(Some("budget"))
                    .set(pair.0.clone(), format!("{}", pair.1));
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
            let var = String::from("VISUAL");
            match env::var(&var) {
                Ok(s) => Some((
                    s.split_whitespace().map(|s| s.to_owned()).collect(),
                    Some(var),
                )),
                _ => {
                    let var = String::from("EDITOR");
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
                workdays |= 1 << i;
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
    // find the first pay period start date *after* the given date
    pub fn next_start_pay_period(&self, date: &NaiveDate) -> Option<NaiveDate> {
        if let Some(known_pay_period_start_date) = self.start_pay_period {
            let delta = date
                .signed_duration_since(known_pay_period_start_date)
                .num_days();
            let l = self.length_pay_period as i64;
            let remainder = delta % l;
            if remainder < 0 {
                Some(*date - Duration::days(remainder))
            } else {
                Some(*date + Duration::days(l - remainder))
            }
        } else {
            None
        }
    }
    pub fn hours_in_pay_period(&self) -> Option<f32> {
        if let Some(d) = self.start_pay_period {
            let mut acc: f32 = 0.0;
            let mut d = d;
            for _ in 0..self.length_pay_period {
                if self.is_workday(&d) {
                    acc += self.day_length;
                }
                d += Duration::days(1)
            }
            Some(acc)
        } else {
            None
        }
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
        self.precision = Precision::from_str(identifier).unwrap();
    }
    pub fn set_truncation(&mut self, identifier: &str) {
        self.truncation = Truncation::from_str(identifier).unwrap();
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

    #[test]
    fn next_start_pay_period_same() {
        let mut c = Configuration::defaults("foo".to_owned());
        let start_pp = NaiveDate::from_ymd_opt(2022, 5, 15).unwrap();
        let date = start_pp;
        c.start_pay_period = Some(start_pp);
        c.length_pay_period = 7;
        assert_eq!(
            date + Duration::days(7),
            c.next_start_pay_period(&date).unwrap()
        )
    }

    #[test]
    fn next_start_pay_period_after() {
        let mut c = Configuration::defaults("foo".to_owned());
        let start_pp = NaiveDate::from_ymd_opt(2022, 5, 15).unwrap();
        let date = start_pp + Duration::days(1);
        c.start_pay_period = Some(start_pp);
        c.length_pay_period = 7;
        assert_eq!(
            date + Duration::days(6),
            c.next_start_pay_period(&date).unwrap()
        )
    }

    #[test]
    fn next_start_pay_period_before() {
        let mut c = Configuration::defaults("foo".to_owned());
        let start_pp = NaiveDate::from_ymd_opt(2022, 5, 15).unwrap();
        let date = start_pp - Duration::days(1);
        c.start_pay_period = Some(start_pp);
        c.length_pay_period = 7;
        assert_eq!(
            date + Duration::days(1),
            c.next_start_pay_period(&date).unwrap()
        )
    }

    #[test]
    fn next_start_pay_period_long_after() {
        let mut c = Configuration::defaults("foo".to_owned());
        let start_pp = NaiveDate::from_ymd_opt(2022, 5, 15).unwrap();
        let date = start_pp + Duration::days(15);
        c.start_pay_period = Some(start_pp);
        c.length_pay_period = 7;
        assert_eq!(
            date + Duration::days(6),
            c.next_start_pay_period(&date).unwrap()
        )
    }

    #[test]
    fn next_start_pay_period_long_before() {
        let mut c = Configuration::defaults("foo".to_owned());
        let start_pp = NaiveDate::from_ymd_opt(2022, 5, 15).unwrap();
        let date = start_pp - Duration::days(15);
        c.start_pay_period = Some(start_pp);
        c.length_pay_period = 7;
        assert_eq!(
            date + Duration::days(1),
            c.next_start_pay_period(&date).unwrap()
        )
    }
}
