extern crate ansi_term;
extern crate chrono;
extern crate clap;
extern crate colonnade;
extern crate dirs;
extern crate regex;

use crate::configure::Configuration;
use crate::log::{Event, Item, LogController, Note};
use ansi_term::Colour::{Black, Blue, Cyan, Green, Purple, Red};
use chrono::{Datelike, Local, NaiveDate, NaiveDateTime};
use clap::{App, Arg, ArgMatches};
use colonnade::{Alignment, Colonnade};
use dirs::home_dir;
use regex::Regex;
use std::collections::BTreeMap;
use std::fs::{create_dir, File};
use std::io;
use std::io::Write;
use std::path::PathBuf;
use std::str::FromStr;

const ONGOING: &str = "ongoing";

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
            .help("Considers notes, not events")
            .long_help("Considers only notes, not events. If this is false, only events are considered, not notes.")
            .display_order(1)
            )
    } else {
        app
    }.arg(
        Arg::with_name("tag")
        .short("t")
        .long("tag")
        .visible_alias("tag-all")
        .validator(|v| if some_nws(&v) {Ok(())} else {Err(format!("tag {:?} needs some non-whitespace character", v))})
        .multiple(true)
        .number_of_values(1)
        .help(match for_events {
            Some(true) => "Skips events that lack this tag",
            Some(false) => "Skips notes that lack this tag",
            None => "Skips events/notes that lack this tag"
        })
        .long_help(match for_events {
            Some(true) => "Skips events that lack this tag.",
            Some(false) => "Skips notes that lack this tag.",
            None => "Skips events/notes that lack this tag."
        })
        .value_name("tag")
        .display_order(1)
    )
    .arg(
        Arg::with_name("tag-none")
        .short("T")
        .long("tag-none")
        .visible_alias("tn")
        .validator(|v| if some_nws(&v) {Ok(())} else {Err(format!("tag {:?} needs some non-whitespace character", v))})
        .multiple(true)
        .number_of_values(1)
        .help(match for_events {
            Some(true) => "Skips events that have this tag",
            Some(false) => "Skips notes that have this tag",
            None => "Skips events/notes that have this tag"
        })
        .long_help(match for_events {
            Some(true) => "Skips events that have this tag.",
            Some(false) => "Skips notes that have this tag.",
            None => "Skips events/notes that have this tag."
        })
        .value_name("tag")
        .display_order(2)
    )
    .arg(
        Arg::with_name("tag-some")
        .short("s")
        .long("tag-some")
        .validator(|v| if some_nws(&v) {Ok(())} else {Err(format!("tag {:?} needs some non-whitespace character", v))})
        .visible_alias("ts")
        .multiple(true)
        .number_of_values(1)
        .help(match for_events {
            Some(true) => "Skips events that lack any of these tags",
            Some(false) => "Skips notes that lack any of these tags",
            None => "Skips events/notes that lack any of these tags"
        })
        .long_help(match for_events {
            Some(true) => "Skips events that lack any of these tags. This is identical to --tag if only one value is provided. It is useful when you are looking for the last event that is tagged with some subset of a particular set of tags.",
            Some(false) => "Skips notes that lack any of these tags. This is identical to --tag if only one value is provided. It is useful when you are looking for the last note that is tagged with some subset of a particular set of tags.",
            None => "Skips events or notes that lack any of these tags. This is identical to --tag if only one value is provided. It is useful when you are looking for the last event/note that is tagged with some subset of a particular set of tags."
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
            Some(true) => "Finds events whose description matches this pattern",
            Some(false) => "Finds notes whose text matches this pattern",
            None => "Finds events/notes whose description/text matches this pattern"
        })
        .long_help(match for_events {
            Some(true) => "Finds events whose descriptions match this regular expression.",
            Some(false) => "Finds notes whose text matches this regular expression.",
            None => "Finds events or notes whose descriptions or text match this regular expression."
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
            Some(true) => "Skips events whose descriptions match this pattern",
            Some(false) => "Skips notes whose text matches this pattern",
            None => "Skips events/notes whose descriptions/text match this pattern"
        })
        .long_help(match for_events {
            Some(true) => "Finds events whose descriptions do not match this regular expression.",
            Some(false) => "Finds notes whose text does not match this regular expression.",
            None => "Finds events or notes whose descriptions or text do not match this regular expression."
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

pub fn base_dir(directory: Option<&str>) -> std::path::PathBuf {
    if let Some(dir) = directory {
        PathBuf::from_str(dir).expect(&format!("could not treat {} as a file path", dir))
    } else {
        let mut dir = home_dir().unwrap();
        dir.push(".joblog");
        dir
    }
}

pub fn log_path(directory: Option<&str>) -> std::path::PathBuf {
    let mut dir = base_dir(directory);
    dir.push("log");
    dir
}

fn time_string(this_time: &Option<NaiveDateTime>, conf: &Configuration) -> String {
    if let Some(this_time) = this_time {
        let format = if conf.h12 { "%l:%M" } else { "%k:%M" };
        // replace a space with non-breaking whitespace that won't be stripped or split by colonnade
        format!("{}", this_time.format(format))
            .as_str()
            .replace(" ", "\u{00A0}")
    } else {
        String::from(ONGOING)
    }
}

pub fn duration_string(duration: f32, conf: &Configuration) -> String {
    format!(
        "{0:.1$}",
        conf.truncation
            .prepare(duration / (60.0 * 60.0), &conf.precision),
        conf.precision.precision()
    )
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
    conf: &Configuration,
) {
    let style = Style::new(conf);
    let same_year = start.year() == end.year();
    let mut last_date: Option<NaiveDate> = None;
    let data: Vec<Vec<String>> = notes
        .iter()
        .map(|n| {
            let mut parts = Vec::with_capacity(3);
            parts.push(time_string(&Some(n.time), conf));
            parts.push(n.tags.join(", "));
            parts.push(n.description.clone());
            parts
        })
        .collect();
    let mut note_table = Colonnade::new(3, conf.width()).unwrap();
    note_table.priority(0).left_margin(2).unwrap();
    note_table.columns[0].alignment(Alignment::Right);
    note_table.columns[1].priority(1);
    note_table.columns[2].priority(2);

    for (offset, row) in note_table.macerate(data).unwrap().iter().enumerate() {
        let date = notes[offset].time.date();
        if last_date.is_none() || last_date.unwrap() != date {
            println!("{}", style.blue(date_string(&date, same_year)));
        }
        last_date = Some(date);
        for line in row {
            for (cell_num, (margin, cell)) in line.iter().enumerate() {
                let cell = match cell_num {
                    1 => style.green(cell),
                    _ => cell.to_owned(),
                };
                print!("{}{}", margin, cell);
            }
            println!();
        }
    }
}

pub fn display_events(
    events: Vec<Event>,
    start: &NaiveDateTime,
    end: &NaiveDateTime,
    conf: &Configuration,
) {
    let style = Style::new(conf);
    let mut last_date: Option<NaiveDate> = None;
    let mut durations: BTreeMap<String, f32> = BTreeMap::new();
    let mut total_duration = 0.0;
    let mut untagged_duration = 0.0;
    let mut vacation_duration = 0.0;
    let now = Local::now().naive_local();
    let same_year = start.year() == end.year();
    let data: Vec<Vec<String>> = events
        .iter()
        .map(|e| {
            if let Some(&date) = last_date.as_ref() {
                if date != e.start.date() {
                    last_date = Some(e.start.date());
                }
            } else {
                last_date = Some(e.start.date());
            }
            let mut parts = Vec::with_capacity(6);
            parts.push(time_string(&Some(e.start), conf));
            parts.push(String::from("-"));
            parts.push(time_string(&e.end, conf));
            let duration = e.duration(&now);
            parts.push(duration_string(duration, conf));
            parts.push(e.tags.join(", "));
            for tag in e.tags.iter() {
                *durations.entry(tag.clone()).or_insert(0.0) += duration;
            }
            if e.tags.is_empty() {
                untagged_duration += duration;
            }
            if e.vacation {
                vacation_duration += duration;
            }
            total_duration += duration;
            parts.push(e.description.clone());
            parts
        })
        .collect();
    let mut event_table =
        Colonnade::new(6, conf.width()).expect("insufficient space for events table");
    event_table
        .priority(0)
        .left_margin(2)
        .expect("insufficient space for events table -- setting margin");
    event_table.columns[0].alignment(Alignment::Right);
    event_table.columns[1].left_margin(1);
    event_table.columns[2].left_margin(1);
    //.alignment(Alignment::Right);
    event_table.columns[4].priority(1);
    event_table.columns[5].priority(2);

    last_date = None;
    for (offset, row) in event_table
        .macerate(data)
        .expect("failed to macerate data")
        .iter()
        .enumerate()
    {
        let date = events[offset].start.date();
        if last_date.is_none() || last_date.unwrap() != date {
            println!("{}", style.blue(date_string(&date, same_year)));
        }
        last_date = Some(date);
        let ongoing = ONGOING.to_owned();
        for line in row {
            for (cell_num, (margin, cell)) in line.iter().enumerate() {
                let cell = match cell_num {
                    2 => {
                        if cell == &ongoing {
                            style.purple(cell)
                        } else {
                            cell.to_owned()
                        }
                    }
                    3 => {
                        if events[offset].vacation {
                            style.purple(cell)
                        } else {
                            style.green(cell)
                        }
                    }
                    4 => style.blue(cell),
                    _ => cell.to_owned(),
                };
                print!("{}{}", margin, cell);
            }
            println!();
        }
    }
    println!();

    let mut tags_table =
        Colonnade::new(2, conf.width()).expect("insufficient space for tags table");
    tags_table.columns[1].alignment(Alignment::Right);
    let mut data = vec![vec![
        String::from("TOTAL HOURS"),
        duration_string(total_duration, conf),
    ]];
    let mut header_count = 1;
    if untagged_duration > 0.0 {
        header_count += 1;
        data.push(vec![
            String::from("UNTAGGED"),
            duration_string(untagged_duration, conf),
        ])
    }
    if vacation_duration > 0.0 {
        header_count += 1;
        data.push(vec![
            String::from("VACATION"),
            duration_string(vacation_duration, conf),
        ])
    }
    for (tag, duration) in durations.iter() {
        data.push(vec![tag.clone(), duration_string(*duration, conf)]);
    }
    for (offset, row) in tags_table
        .macerate(data)
        .expect("could not macerate tag data")
        .iter()
        .enumerate()
    {
        for line in row {
            for (cell_num, (margin, cell)) in line.iter().enumerate() {
                // header values are red
                let cell = if cell_num == 0 && offset < header_count {
                    style.red(cell)
                } else {
                    cell.to_owned()
                };
                print!("{}{}", margin, cell);
            }
            println!();
        }
    }
}

pub fn success<T: ToString>(msg: T, conf: &Configuration) {
    let style = Style::new(&conf);
    eprintln!("{} {}", style.bold(style.green("ok:")), msg.to_string());
}

pub fn warn<T: ToString>(msg: T, conf: &Configuration) {
    let style = Style::new(&conf);
    eprintln!(
        "{} {}",
        style.bold(style.purple("warning:")),
        msg.to_string()
    );
}

pub fn fatal<T: ToString>(msg: T, conf: &Configuration) {
    let style = Style::new(&conf);
    eprintln!("{} {}", style.bold(style.red("error:")), msg.to_string());
    std::process::exit(1);
}

pub fn describe(action: &str, extra: Option<&str>, item: Item, conf: &Configuration) {
    let style = Style::new(conf);
    let mut s = style.bold(style.green(action));
    s += " ";
    if let Some(extra) = extra {
        s += extra;
        s += " ";
    }
    match item {
        Item::Event(
            Event {
                description, tags, ..
            },
            _,
        ) => {
            s += &description;
            s += " (";
            if tags.is_empty() {
                s += &style.purple("no tags");
            } else {
                s += &style.blue(tags.join(", "));
            }
            s += ")"
        }
        Item::Note(
            Note {
                description, tags, ..
            },
            _,
        ) => {
            s += &description;
            s += " (";
            if tags.is_empty() {
                s += &style.purple("no tags");
            } else {
                s += "tags: ";
                s += &style.blue(tags.join(", "));
            }
            s += ")"
        }
        Item::Done(d, _) => s += &style.blue(format!("{}", d.0.format("at %l:%M %P"))),
        _ => (),
    }
    println!("{}", s)
}

// this is really a check for ongoing *multi-day* events
pub fn check_for_ongoing_event(reader: &mut LogController, conf: &Configuration) {
    if reader.forgot_to_end_last_event() {
        warn(
            "it appears an event begun on a previous day is ongoing",
            conf,
        );
        println!();
        let last_event = reader.last_event().unwrap();
        let start = &last_event.start.clone();
        let now = Local::now().naive_local();
        let event = Event::gather_by_day(vec![last_event], &now);
        display_events(event, start, &now, conf);
        println!();
    }
}

// make sure base directory and its files are present
pub fn init(directory: Option<&str>) {
    if !base_dir(directory).as_path().exists() {
        create_dir(base_dir(directory).to_str().unwrap()).expect(&format!(
            "could not create base directory {}",
            base_dir(directory).to_str().unwrap()
        ));
        let hidden = base_dir(directory)
            .as_path()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with(".");
        println!(
            "initialized {}directory {} for Job Log",
            if hidden { "hidden " } else { "" },
            base_dir(directory).to_str().unwrap()
        );
    }
    if !log_path(directory).as_path().exists() {
        let mut log =
            File::create(log_path(directory).to_str().unwrap()).expect("could not create log file");
        log.write_all(b"# job log\n")
            .expect("could not write comment to log file");
    }
    let mut readme_path = base_dir(directory);
    readme_path.push("README");
    if !readme_path.as_path().exists() {
        let mut readme =
            File::create(readme_path.to_str().unwrap()).expect("could not create README file");
        readme.write_all(b"\nJob Log\n\nThis directory holds files used by Job Log to maintain\na work log. For more details type\n\n   job --help\n\non the command line.\n\n       
").expect("could not write README");
    }
}

// putting this into a common struct so we can easily turn color off
pub struct Style<'a> {
    noop: bool,
    #[allow(dead_code)] // saving this for when the color of individual elements is configurable
    conf: &'a Configuration,
}

impl<'a> Style<'a> {
    pub fn new(conf: &'a Configuration) -> Style<'a> {
        Style {
            conf,
            noop: !conf.effective_color().0,
        }
    }
    pub fn bold<T: ToString>(&self, text: T) -> String {
        if self.noop {
            return text.to_string();
        }
        format!("{}", ansi_term::Style::new().bold().paint(text.to_string()))
    }
    pub fn italic<T: ToString>(&self, text: T) -> String {
        if self.noop {
            return text.to_string();
        }
        format!(
            "{}",
            ansi_term::Style::new().italic().paint(text.to_string())
        )
    }
    // the odd line in a striped table
    pub fn odd_line<T: ToString>(&self, text: T) -> String {
        if self.noop {
            return text.to_string();
        }
        format!(
            "{}",
            ansi_term::Style::new()
                .on(Cyan)
                .fg(Black)
                .paint(text.to_string())
        )
    }
    pub fn cyan<T: ToString>(&self, text: T) -> String {
        if self.noop {
            return text.to_string();
        }
        format!("{}", Cyan.paint(text.to_string()))
    }
    pub fn green<T: ToString>(&self, text: T) -> String {
        if self.noop {
            return text.to_string();
        }
        format!("{}", Green.paint(text.to_string()))
    }
    pub fn blue<T: ToString>(&self, text: T) -> String {
        if self.noop {
            return text.to_string();
        }
        format!("{}", Blue.paint(text.to_string()))
    }
    pub fn red<T: ToString>(&self, text: T) -> String {
        if self.noop {
            return text.to_string();
        }
        format!("{}", Red.paint(text.to_string()))
    }
    pub fn purple<T: ToString>(&self, text: T) -> String {
        if self.noop {
            return text.to_string();
        }
        format!("{}", Purple.paint(text.to_string()))
    }
}

// for use in validating tags
pub fn some_nws(s: &str) -> bool {
    for c in s.chars() {
        if !c.is_whitespace() {
            return true;
        }
    }
    return false;
}

// ask a yes or no question and await an answer
pub fn yes_or_no<T: ToString>(msg: T) -> bool {
    loop {
        print!("{} [Yn] ", msg.to_string());
        io::stdout().flush().expect("could not flush stdout");
        let mut buffer = String::new();
        io::stdin()
            .read_line(&mut buffer)
            .expect("failed to read response");
        let buffer = buffer.as_str().trim().to_owned();
        if buffer.len() == 0 {
            return true;
        }
        match buffer.as_str() {
            "y" | "Y" => return true,
            "n" | "N" => return false,
            _ => {
                println!("please answer y or n");
            }
        }
    }
}
