extern crate ansi_term;
extern crate chrono;
extern crate clap;
extern crate colonnade;
extern crate ini;
extern crate regex;
extern crate term_size;
extern crate two_timer;

use crate::util::base_dir;
use ansi_term::Colour::{Black, Cyan};
use ansi_term::Style;
use chrono::{Datelike, NaiveDate};
use clap::{App, Arg, ArgMatches, SubCommand};
use colonnade::{Alignment, Colonnade};
use ini::Ini;
use regex::Regex;
use std::path::PathBuf;
use two_timer::{parsable, parse, Config};

pub const PRECISION: &str = "2";
pub const SUNDAY_BEGINS_WEEK: &str = "true";
pub const LENGTH_PAY_PERIOD: &str = "14";
pub const DAY_LENGTH: &str = "8";
pub const WORKDAYS: &str = "MTWHF";

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

pub fn cli(mast: App<'static, 'static>) -> App<'static, 'static> {
    mast.subcommand(
        SubCommand::with_name("configure")
            .aliases(&["c", "co", "con", "conf", "confi", "config", "configu", "configur"])
            .about("set or display configuration parameters")
            .after_help("Set or display configuration parameters that control date interpretation, log summarization, etc.")
            .arg(
                Arg::with_name("precision")
                .long("precision")
                .help("decimal places of precision in display of time; default value: 2") // must match PRECISION
                .long_help("The number of decimal places of precision used in the display of lengths of periods in numbers of hours. If the number is 0, probably not what you want, all periods will be rounded to a whole number of hours. The default value is 2.")
                .possible_values(&["0", "1", "2", "3"])
                .value_name("int")
            )
            .arg(
                Arg::with_name("start-pay-period")
                .long("start-pay-period")
                .help("the first day of some pay period")
                .long_help("A day relative to which all pay periods will be calculated. See --length-pay-period.")
                .validator(|v| if parsable(&v) {Ok(())} else {Err(format!("cannot parse '{}' as a time expression", v))} )
                .value_name("date")
            )
            .arg(
                Arg::with_name("sunday-begins-week")
                .long("sunday-begins-week")
                .help("whether Sunday should be considered the first day of the week; default value: true") // must match SUNDAY_BEGINS_WEEK
                .possible_values(&["true", "false"])
                .value_name("bool")
            )
            .arg(
                Arg::with_name("length-pay-period")
                .long("length-pay-period")
                .help("the number of days in a pay period; default value: 14") // must match LENGTH_PAY_PERIOD
                .validator(valid_length_pay_period)
                .value_name("int")
            )
            .arg(
                Arg::with_name("day-length")
                .long("day-length")
                .help("expected number of hours in a workday; default value: 8") // must match DAY_LENGTH
                .validator(valid_day_length)
                .value_name("num")
            )
            .arg(
                Arg::with_name("workdays")
                .long("workdays")
                .help("which days you are expected to work; default value: MTWHF")
                .long_help("Workdays during the week represented as a subset of SMTWHFA, where S is Sunday and A is Saturday, etc. The default value is MTWHF.") // must match WORKDAYS
                .validator(|v| if Regex::new(r"\A[SMTWHFA]+\z").unwrap().is_match(&v) {Ok(())} else {Err(format!("must contain only the letters SMTWHFA, where S means Sunday and A, Saturday, etc."))})
                .value_name("days")
            )
            .arg(
                Arg::with_name("editor")
                .long("editor")
                .help("text editor to use when manually editing the log")
                .long_help("A text editor that the edit command will invoke. E.g., /usr/bin/vim.")
                .value_name("path")
            )
            .arg(
                Arg::with_name("max-width")
                .long("max-width")
                .help("maximum number of columns when summarizing data")
                .validator(valid_max_width)
                .value_name("num")
            )
            .arg(
                Arg::with_name("unset")
                .short("u")
                .long("unset")
                .help("return a configuration to its default")
                .value_name("param")
                .multiple(true)
                .number_of_values(1)
            )
            .arg(
                Arg::with_name("list")
                .short("l")
                .long("list")
                .help("list all configuration parameters")
                .long_help("List all configuration parameters and their values.")
            )
            .display_order(12)
    )
}

pub fn run(matches: &ArgMatches) {
    let mut did_something = false;
    let mut write = false;
    let mut conf = Configuration::read();
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
            println!("start-pay-period is already {} {} {}!", year, month, day);
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
                println!("sunday-begins-week is already {}!", v);
            } else {
                println!("setting sunday-begins-week to {}!", v);
                conf.sunday_begins_week = v;
                write = true;
            }
        }
    }
    if matches.is_present("length-pay-period") {
        did_something = true;
        if let Some(v) = matches.value_of("length-pay-period") {
            let v: u32 = v.parse().unwrap();
            if v == conf.length_pay_period {
                println!("length-pay-period is already {}!", v);
            } else {
                println!("setting length-pay-period to {}!", v);
                conf.length_pay_period = v;
                write = true;
            }
        }
    }
    if matches.is_present("day-length") {
        did_something = true;
        if let Some(v) = matches.value_of("day-length") {
            let v: f32 = v.parse().unwrap();
            if v == conf.day_length {
                println!("day-length is already {}!", v);
            } else {
                println!("setting day-length to {}!", v);
                conf.day_length = v;
                write = true;
            }
        }
    }
    if matches.is_present("workdays") {
        did_something = true;
        if let Some(v) = matches.value_of("workdays") {
            if v == &conf.serialize_workdays() {
                println!("workdays is already {}!", v);
            } else {
                println!("setting workdays to {}!", v);
                conf.workdays(v);
                write = true;
            }
        }
    }
    if let Some(v) = matches.value_of("editor") {
        did_something = true;
        if conf.editor.is_some() && v == conf.editor.as_ref().unwrap() {
            println!("editor is already {}!", v);
        } else {
            println!("setting editor to {}!", v);
            conf.editor(v);
            write = true;
        }
    }
    if let Some(v) = matches.value_of("max-width") {
        did_something = true;
        let v = v.parse::<usize>().unwrap();
        if conf.max_width.is_some() && v == conf.max_width.unwrap() {
            println!("max-width is already {}!", v);
        } else {
            println!("setting max-width to {}!", v);
            conf.max_width = Some(v);
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
                "length-pay-period" => {
                    conf.length_pay_period = LENGTH_PAY_PERIOD.parse().unwrap();
                    write = true;
                }
                "max-width" => {
                    conf.max_width = None;
                    write = true;
                }
                "precision" => {
                    conf.precision = PRECISION.parse().unwrap();
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
                    conf.workdays = WORKDAYS.parse().unwrap();
                    write = true;
                }
                &_ => set = false,
            }
            if set {
                println!("unset {}", v);
            } else {
                println!("unknown configuration parameter!: {}", v);
            }
        }
    }
    if write {
        conf.write()
    }
    if matches.is_present("list") {
        if did_something {
            println!("");
        } else {
            did_something = true;
        }
        let attributes = vec![
            vec![String::from("day-length"), format!("{}", conf.day_length)],
            vec![
                String::from("editor"),
                format!("{}", conf.editor.as_ref().unwrap_or(&String::from(""))),
            ],
            vec![
                String::from("length-pay-period"),
                format!("{}", conf.length_pay_period),
            ],
            vec![String::from("precision"), format!("{}", conf.precision)],
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
            vec![String::from("workdays"), conf.serialize_workdays()],
            vec![
                String::from("max-width"),
                if conf.max_width.is_some() {
                    format!("{}", conf.max_width.unwrap())
                } else {
                    String::from("")
                },
            ],
        ];
        let mut table = Colonnade::new(2, conf.width()).unwrap();
        println!("width: {}", conf.width());
        println!("{:?}", attributes);
        table.columns[1].alignment(Alignment::Right).left_margin(2);
        let odd_line = Style::new().on(Cyan).fg(Black);
        for (i, line) in table.tabulate(&attributes).unwrap().iter().enumerate() {
            if i % 2 == 1 {
                println!("{}", odd_line.paint(line))
            } else {
                println!("{}", line);
            }
        }
    }
    if !did_something {
        println!("{}", matches.usage());
    }
}

#[derive(Debug, Clone)]
pub struct Configuration {
    pub day_length: f32,
    pub editor: Option<String>,
    pub length_pay_period: u32,
    pub precision: u8,
    pub start_pay_period: Option<NaiveDate>,
    pub sunday_begins_week: bool,
    pub workdays: u8, // bit flags
    pub max_width: Option<usize>,
}

impl Configuration {
    fn max_term_size() -> usize {
        term_size::dimensions().unwrap().0
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
    pub fn read() -> Configuration {
        if let Ok(ini) = Ini::load_from_file(Configuration::config_file().as_path()) {
            let editor = if let Some(s) = ini.get_from(Some("external"), "editor") {
                Some(String::from(s))
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
            Configuration {
                day_length: ini
                    .get_from_or(Some("time"), "day-length", DAY_LENGTH)
                    .parse()
                    .unwrap(),
                editor: editor,
                length_pay_period: ini
                    .get_from_or(Some("time"), "pay-period-length", LENGTH_PAY_PERIOD)
                    .parse()
                    .unwrap(),
                precision: ini
                    .get_from_or(Some("summary"), "precision", PRECISION)
                    .parse()
                    .unwrap(),
                start_pay_period: start_pay_period,
                sunday_begins_week: ini.get_from_or(
                    Some("time"),
                    "sunday-begins-week",
                    SUNDAY_BEGINS_WEEK,
                ) == "true",
                workdays: Configuration::parse_workdays(ini.get_from_or(
                    Some("time"),
                    "workdays",
                    WORKDAYS,
                )),
                max_width: ini
                    .get_from(Some("summary"), "max-width")
                    .and_then(|s| Some(s.parse().unwrap())),
            }
        } else {
            Configuration {
                day_length: DAY_LENGTH.parse().unwrap(),
                editor: None,
                length_pay_period: LENGTH_PAY_PERIOD.parse().unwrap(),
                precision: PRECISION.parse().unwrap(),
                start_pay_period: None,
                sunday_begins_week: SUNDAY_BEGINS_WEEK == "true",
                workdays: Configuration::parse_workdays(WORKDAYS),
                max_width: None,
            }
        }
    }
    pub fn write(&self) {
        let mut ini = Ini::new();
        if self.day_length != DAY_LENGTH.parse::<f32>().unwrap() {
            ini.with_section(Some("time"))
                .set("day-length", format!("{}", self.day_length));
        }
        if let Some(s) = self.editor.as_ref() {
            ini.with_section(Some("external")).set("editor", s);
        }
        if self.length_pay_period != LENGTH_PAY_PERIOD.parse::<u32>().unwrap() {
            ini.with_section(Some("time"))
                .set("pay-period-length", format!("{}", self.length_pay_period));
        }
        if self.precision != PRECISION.parse::<u8>().unwrap() {
            ini.with_section(Some("summary"))
                .set("precision", format!("{}", self.precision));
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
        let s = self.serialize_workdays();
        if s != WORKDAYS {
            ini.with_section(Some("time")).set("workdays", s);
        }
        if self.max_width.is_some() {
            ini.with_section(Some("summary"))
                .set("max-width", format!("{}", self.max_width.unwrap()));
        }
        ini.write_to_file(Configuration::config_file()).unwrap();
    }
    fn workdays(&mut self, workdays: &str) {
        self.workdays = Configuration::parse_workdays(workdays);
    }
    fn editor(&mut self, editor: &str) {
        self.editor = Some(String::from(editor));
    }
    fn config_file() -> PathBuf {
        let mut path = base_dir();
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
    pub fn is_workday(&self, date: NaiveDate) -> bool {
        let i = (date.weekday().number_from_sunday() - 1) as u8;
        self.workdays & (1 << i) > 0
    }
}
