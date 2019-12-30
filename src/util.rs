extern crate ansi_term;
extern crate chrono;
extern crate clap;
extern crate colonnade;
extern crate dirs;
extern crate regex;

use crate::configure::Configuration;
use crate::log::{Event, Item, Log, Note};
use ansi_term::Colour::{Blue, Cyan, Green, Purple, Red};
use ansi_term::Style;
use chrono::{Datelike, Local, NaiveDate, NaiveDateTime, Timelike};
use clap::{App, Arg, ArgMatches};
use colonnade::{Alignment, Colonnade};
use dirs::home_dir;
use regex::Regex;
use std::collections::BTreeMap;
use std::fs::{create_dir, File};
use std::io::Write;

// a collection of arguments used in many subcommands concerned with searching for or filtering events
pub fn common_search_or_filter_arguments(
    app: App<'static, 'static>,
    for_events: Option<bool>,
) -> App<'static, 'static> {
    if for_events.is_none() {
        app.arg(
            Arg::with_name("notes")
            .short("n")
            .long("notes")
            .help("consider notes, not events")
            .long_help("Consider only notes, not events. If this is false, only events are considered, not notes.")
            .display_order(1)
            )
    } else {
        app
    }.arg(
        Arg::with_name("tag")
        .short("t")
        .long("tag")
        .visible_alias("tag-all")
        .multiple(true)
        .number_of_values(1)
        .help(match for_events {
            Some(true) => "skip events that lack this tag",
            Some(false) => "skip notes that lack this tag",
            None => "skip events/notes that lack this tag"
        })
        .long_help(match for_events {
            Some(true) => "Skip events that lack this tag.",
            Some(false) => "Skip notes that lack this tag.",
            None => "Skip events/notes that lack this tag."
        })
        .value_name("tag")
        .display_order(1)
    )
    .arg(
        Arg::with_name("tag-none")
        .short("T")
        .long("tag-none")
        .visible_alias("tn")
        .multiple(true)
        .number_of_values(1)
        .help(match for_events {
            Some(true) => "skip events that have this tag",
            Some(false) => "skip notes that have this tag",
            None => "skip events/notes that have this tag"
        })
        .long_help(match for_events {
            Some(true) => "Skip events that have this tag.",
            Some(false) => "Skip notes that have this tag.",
            None => "Skip events/notes that have this tag."
        })
        .value_name("tag")
        .display_order(2)
    )
    .arg(
        Arg::with_name("tag-some")
        .short("s")
        .long("tag-some")
        .visible_alias("ts")
        .multiple(true)
        .number_of_values(1)
        .help(match for_events {
            Some(true) => "skip events that lack any of these tags",
            Some(false) => "skip notes that lack any of these tags",
            None => "skip events/notes that lack any of these tags"
        })
        .long_help(match for_events {
            Some(true) => "Skip events that lack any of these tags. This is identical to --tag if only one value is provided. It is useful when you are looking for the last event that is tagged with some subset of a particular set of tags.",
            Some(false) => "Skip notes that lack any of these tags. This is identical to --tag if only one value is provided. It is useful when you are looking for the last note that is tagged with some subset of a particular set of tags.",
            None => "Skip events or notes that lack any of these tags. This is identical to --tag if only one value is provided. It is useful when you are looking for the last event/note that is tagged with some subset of a particular set of tags."
        })
        .value_name("tag")
        .display_order(3)
    )
    .arg(
        Arg::with_name("rx")
        .long("rx")
        .multiple(true)
        .number_of_values(1)
        .help(match for_events {
            Some(true) => "find events whose description matches this pattern",
            Some(false) => "find notes whose text matches this pattern",
            None => "find events/notes whose description/text matches this pattern"
        })
        .long_help(match for_events {
            Some(true) => "Find events whose description matches this regular expression.",
            Some(false) => "Find notes whose text matches this regular expression.",
            None => "Find events or notes whose description or text matches this regular expression."
        })
        .value_name("pattern")
        .validator(|arg| if Regex::new(&arg).is_ok() {Ok(())} else {Err(format!("'{}' cannot be parsed as a regular expression", &arg))})
        .display_order(4)
    )
    .arg(
        Arg::with_name("rx-not")
        .long("rx-not")
        .multiple(true)
        .number_of_values(1)
        .help(match for_events {
            Some(true) => "skip events whose description matches this pattern",
            Some(false) => "skip notes whose text matches this pattern",
            None => "skip events/notes whose description/text matches this pattern"
        })
        .long_help(match for_events {
            Some(true) => "Find events whose description does not match this regular expression.",
            Some(false) => "Find notes whose text does not match this regular expression.",
            None => "Find events or notes whose description or text does not match this regular expression."
        })
        .value_name("pattern")
        .validator(|arg| if Regex::new(&arg).is_ok() {Ok(())} else {Err(format!("'{}' cannot be parsed as a regular expression", &arg))})
        .display_order(5)
    )
}

// concatenate the trailing arguments -- we need to do this often enough it seems worth DRYing up
pub fn remainder(argname: &str, matches: &ArgMatches) -> std::string::String {
    matches
        .values_of(argname)
        .unwrap()
        .collect::<Vec<&str>>()
        .join(" ")
}

pub fn base_dir() -> std::path::PathBuf {
    let mut dir = home_dir().unwrap();
    dir.push(".joblog");
    dir
}

pub fn log_path() -> std::path::PathBuf {
    let mut dir = base_dir();
    dir.push("log");
    dir
}

fn time_string(this_time: &Option<NaiveDateTime>, last_time: &Option<NaiveDateTime>) -> String {
    if let Some(this_time) = this_time {
        let format =
            if last_time.is_none() || last_time.unwrap().hour() < 13 && this_time.hour() >= 13 {
                "%l:%M %P"
            } else {
                "%l:%M"
            };
        format!("{}", this_time.format(format))
    } else {
        String::from("ongoing")
    }
}

fn duration_string(duration: f32, precision: u8) -> String {
    format!("{0:.1$}", duration / 60.0, (precision as usize))
}

fn date_string(date: &NaiveDate, same_year: bool) -> String {
    if same_year {
        format!("{}", date.format("%A, %e %B"))
    } else {
        format!("{}", date.format("%A, %e %B %Y"))
    }
}

pub fn display_notes(
    notes: Vec<Note>,
    start: &NaiveDateTime,
    end: &NaiveDateTime,
    configuration: &Configuration,
) {
    let same_year = start.year() == end.year();
    let mut last_time: Option<NaiveDateTime> = None;
    let mut last_date: Option<NaiveDate> = None;
    let data: Vec<Vec<String>> = notes
        .iter()
        .map(|n| {
            let mut parts = Vec::with_capacity(3);
            parts.push(time_string(&Some(n.time), &last_time));
            last_time = Some(n.time);
            parts.push(n.tags.join(", "));
            parts.push(n.description.clone());
            parts
        })
        .collect();
    let mut note_table = Colonnade::new(3, configuration.width()).unwrap();
    note_table.priority(0).left_margin(2).unwrap();
    note_table.columns[1].priority(1);
    note_table.columns[2].priority(2);

    for (offset, row) in note_table.macerate(data).unwrap().iter().enumerate() {
        let date = notes[offset].time.date();
        if last_date.is_none() || last_date.unwrap() != date {
            let style = Style::new().fg(Blue);
            println!("{}", style.paint(date_string(&date, same_year)));
        }
        last_date = Some(date);
        for line in row {
            for (cell_num, (margin, cell)) in line.iter().enumerate() {
                let mut style = Style::new();
                match cell_num {
                    1 => {
                        style = style.fg(Green);
                        ()
                    }
                    _ => (),
                }
                print!("{}{}", margin, style.paint(cell));
            }
            println!();
        }
    }
}

pub fn display_events(
    events: Vec<Event>,
    start: &NaiveDateTime,
    end: &NaiveDateTime,
    configuration: &Configuration,
) {
    let mut last_time: Option<NaiveDateTime> = None;
    let mut last_date: Option<NaiveDate> = None;
    let mut durations: BTreeMap<String, f32> = BTreeMap::new();
    let mut total_duration = 0.0;
    let mut untagged_duration = 0.0;
    let now = Local::now().naive_local();
    let same_year = start.year() == end.year();
    let data: Vec<Vec<String>> = events
        .iter()
        .map(|e| {
            if let Some(&date) = last_date.as_ref() {
                if date != e.start.date() {
                    last_time = None;
                    last_date = Some(e.start.date());
                }
            } else {
                last_date = Some(e.start.date());
            }
            let mut parts = Vec::with_capacity(6);
            parts.push(time_string(&Some(e.start), &last_time));
            parts.push(String::from("-"));
            last_time = Some(e.start);
            parts.push(time_string(&e.end, &last_time));
            last_time = e.end;
            let duration = e.duration(&now);
            parts.push(duration_string(duration, configuration.precision));
            parts.push(e.tags.join(", "));
            for tag in e.tags.iter() {
                *durations.entry(tag.clone()).or_insert(0.0) += duration;
            }
            if e.tags.is_empty() {
                untagged_duration += e.duration(&now);
            }
            total_duration += duration;
            parts.push(e.description.clone());
            parts
        })
        .collect();
    let mut event_table = Colonnade::new(6, configuration.width()).unwrap();
    event_table.priority(0).left_margin(2).unwrap();
    event_table.columns[0].alignment(Alignment::Right);
    event_table.columns[1].left_margin(1);
    event_table.columns[2].left_margin(1);
    event_table.columns[4].priority(1);
    event_table.columns[5].priority(2);

    last_date = None;
    for (offset, row) in event_table.macerate(data).unwrap().iter().enumerate() {
        let date = events[offset].start.date();
        if last_date.is_none() || last_date.unwrap() != date {
            let style = Style::new().fg(Blue);
            println!("{}", style.paint(date_string(&date, same_year)));
        }
        last_date = Some(date);
        for line in row {
            for (cell_num, (margin, cell)) in line.iter().enumerate() {
                let mut style = Style::new();
                match cell_num {
                    3 => {
                        style = style.fg(Cyan);
                        ()
                    }
                    4 => {
                        style = style.fg(Green);
                        ()
                    }
                    _ => (),
                }
                print!("{}{}", margin, style.paint(cell));
            }
            println!();
        }
    }
    println!();

    let mut tags_table = Colonnade::new(2, configuration.width()).unwrap();
    tags_table.columns[1].alignment(Alignment::Right);
    let mut data = vec![vec![
        String::from("TOTAL HOURS"),
        duration_string(total_duration, configuration.precision),
    ]];
    if untagged_duration > 0.0 {
        data.push(vec![
            String::from("UNTAGGED"),
            duration_string(untagged_duration, configuration.precision),
        ])
    }
    for (tag, duration) in durations.iter() {
        data.push(vec![
            tag.clone(),
            duration_string(*duration, configuration.precision),
        ]);
    }
    for (offset, row) in tags_table.macerate(data).unwrap().iter().enumerate() {
        let mut style = Style::new();
        match offset {
            0 => {
                style = style.fg(Red);
                ()
            }
            1 => {
                if untagged_duration > 0.0 {
                    style = style.fg(Red);
                }
            }
            _ => (),
        }
        for line in row {
            for (cell_num, (margin, cell)) in line.iter().enumerate() {
                if cell_num == 0 {
                    print!("{}{}", margin, style.paint(cell));
                } else {
                    print!("{}{}", margin, cell);
                }
            }
            println!();
        }
    }
}

pub fn warn<T: ToString>(msg: T) {
    eprintln!("{}", Purple.paint(msg.to_string()));
}

pub fn fatal<T: ToString>(msg: T) {
    eprintln!("ERROR: {}", Red.paint(msg.to_string()));
    std::process::exit(1);
}

pub fn describe(action: &str, item: Item) {
    let mut s = action.to_owned();
    s += " ";
    match item {
        Item::Event(
            Event {
                description, tags, ..
            },
            _,
        ) => {
            s += &description;
            s += " ";
            if tags.is_empty() {
                s += &Blue.paint("no tags").to_string();
            } else {
                s += "(";
                s += &Blue.paint(tags.join(", ")).to_string();
                s += ")"
            }
        }
        Item::Note(
            Note {
                description, tags, ..
            },
            _,
        ) => {
            s += &description;
            s += " ";
            if tags.is_empty() {
                s += &Blue.paint("no tags").to_string();
            } else {
                s += "(";
                s += &Blue.paint(tags.join(", ")).to_string();
                s += ")"
            }
        }
        Item::Done(d, _) => s += &format!("{}", d.0.format("at %l:%M %P")),
        _ => (),
    }
    println!("{}", s)
}

// this is really a check for ongoing *multi-day* events
pub fn check_for_ongoing_event(reader: &mut Log) {
    if reader.forgot_to_end_last_event() {
        warn("it appears an event begun on a previous day is ongoing");
        println!();
        let last_event = reader.last_event().unwrap();
        let start = &last_event.start.clone();
        let now = Local::now().naive_local();
        let event = Event::gather_by_day(vec![last_event], &now);
        let configuration = Configuration::read();
        display_events(event, start, &now, &configuration);
        println!();
    }
}

// make sure base directory and its files are present
pub fn init() {
    if !base_dir().as_path().exists() {
        create_dir(base_dir().to_str().unwrap()).expect("could not create base directory");
    }
    if !log_path().as_path().exists() {
        let mut log =
            File::create(log_path().to_str().unwrap()).expect("could not create log file");
        log.write_all(b"# job log\n")
            .expect("could not write comment to log file");
    }
    let mut readme_path = base_dir();
    readme_path.push("README");
    if !readme_path.as_path().exists() {
        let mut readme =
            File::create(readme_path.to_str().unwrap()).expect("could not create README file");
        readme.write_all(b"\nJob Log\n\nThis directory holds files used by Job Log to maintain\na work log. For more details type\n\n   job --help\n\non the command line.\n\n       
").expect("could not write README");
    }
}
