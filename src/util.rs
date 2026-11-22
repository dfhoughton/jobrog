use crate::configure::Configuration;
use crate::log::{Event, Item, LogController, Note};
use chrono::{Datelike, Local, NaiveDate, NaiveDateTime};
use clap::Args as ClapArgs;
use colonnade::{Alignment, Colonnade};
use dirs::home_dir;
use nu_ansi_term::{Color, Style as AnsiStyle};
use regex::Regex;
use std::collections::BTreeMap;
use std::fs::{create_dir, File};
use std::io;
use std::io::Write;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::LazyLock;

const ONGOING: &str = "ongoing";

/// Arguments used in many subcommands concerned with searching for or filtering events
#[derive(Debug, ClapArgs)]
pub struct FilterArgs {
    /// Considers only notes, not events
    ///
    /// Considers only notes, not events. If this is false, only events are considered, not notes.
    #[arg(short = 'n', long)]
    pub notes: bool,

    /// Skips events/notes that lack this tag
    ///
    /// Skips events/notes that lack this tag.
    #[arg(
        short = 't',
        long,
        value_name = "tag",
        visible_alias = "tag-all",
        conflicts_with = "no_tags",
        value_parser = valid_tag
    )]
    pub tag: Vec<String>,

    /// Skips events/notes that have this tag
    ///
    /// Skips events/notes that have this tag.
    #[arg(
        short = 'T',
        long,
        value_name = "tag",
        visible_alias = "tn",
        value_parser = valid_tag
    )]
    pub tag_none: Vec<String>,

    /// Skips events/notes that lack any of these tags
    ///
    /// Skips events or notes that lack any of these tags. This is identical to --tag if only
    /// one value is provided. It is useful when you are looking for the last event/note that
    /// is tagged with some subset of a particular set of tags.
    #[arg(
        short = 's',
        long,
        value_name = "tag",
        visible_alias = "ts",
        conflicts_with = "no_tags",
        value_parser = valid_tag
    )]
    pub tag_some: Vec<String>,

    /// Selects events/notes that lack tags
    #[arg(
        short = 'e',
        long = "empty",
        visible_alias = "no-tags",
        conflicts_with_all = ["tag_some", "tag"]
    )]
    pub no_tags: bool,

    /// Finds events/notes whose description/text matches this pattern
    ///
    /// Finds events or notes whose descriptions or text match this regular expression.
    #[arg(long, value_name = "pattern", value_parser = valid_rx)]
    pub rx: Vec<String>,

    /// Skips events/notes whose descriptions/text match this pattern
    ///
    /// Finds events or notes whose descriptions or text do not match this regular expression.
    #[arg(long, value_name = "pattern", value_parser = valid_rx)]
    pub rx_not: Vec<String>,
}

/// A tag must have some non-whitespace character
pub fn valid_tag(v: &str) -> Result<String, String> {
    if some_nws(v) {
        Ok(v.to_owned())
    } else {
        Err(format!("tag {:?} needs some non-whitespace character", v))
    }
}

/// A time expression must be parsable by two_timer
pub fn valid_time_expression(v: &str) -> Result<String, String> {
    if two_timer::parsable(v) {
        Ok(v.to_owned())
    } else {
        Err(format!("cannot parse '{}' as a time expression", v))
    }
}

/// A pattern must be a valid regular expression
pub fn valid_rx(v: &str) -> Result<String, String> {
    match Regex::new(v) {
        Ok(_) => Ok(v.to_owned()),
        Err(_) => Err(format!("'{}' cannot be parsed as a regular expression", v)),
    }
}

pub fn base_dir(directory: Option<&str>) -> std::path::PathBuf {
    if let Some(dir) = directory {
        PathBuf::from_str(dir).expect("could not treat the argument as a file path")
    } else {
        let mut dir = home_dir().expect("could not find home directory");
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
        format!("{}", this_time.format(format)).replace(' ', "\u{00A0}")
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
            vec![
                time_string(&Some(n.time), conf),
                n.tags.join(", "),
                n.description.clone(),
            ]
        })
        .collect();
    let mut note_table = Colonnade::new(3, conf.width()).unwrap();
    note_table.priority(0).left_margin(2).unwrap();
    note_table.columns[0].set_alignment(Alignment::Right);
    note_table.columns[1].set_priority(1);
    note_table.columns[2].set_priority(2);

    for (offset, row) in note_table.macerate(data).unwrap().iter().enumerate() {
        let date = notes[offset].time.date();
        if last_date.is_none_or(|d| d != date) {
            println!("{}", style.paint("header", date_string(&date, same_year)));
        }
        last_date = Some(date);
        for line in row {
            for (cell_num, (margin, cell)) in line.iter().enumerate() {
                let cell = match cell_num {
                    1 => style.paint("tags", cell),
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
    static ANY_CONTENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\S").unwrap());
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
    event_table.columns[0].set_alignment(Alignment::Right);
    event_table.columns[1].set_left_margin(1);
    event_table.columns[2].set_left_margin(1);
    event_table.columns[4].set_priority(1);
    event_table.columns[5].set_priority(2);

    last_date = None;
    for (offset, row) in event_table
        .macerate(data)
        .expect("failed to macerate data")
        .iter()
        .enumerate()
    {
        let e = events.get(offset).unwrap();
        let date = e.start.date();
        if date < start.date() {
            continue;
        }
        if last_date.is_none_or(|d| d != date) {
            println!("{}", style.paint("header", date_string(&date, same_year)));
        }
        last_date = Some(date);
        for line in row {
            for (cell_num, (margin, cell)) in line.iter().enumerate() {
                let cell = match cell_num {
                    0 => {
                        if e.overlaps_start() && ANY_CONTENT.is_match(cell) {
                            style.paint("alert", cell)
                        } else {
                            cell.to_owned()
                        }
                    }
                    2 => {
                        if (e.overlaps_end() && ANY_CONTENT.is_match(cell)) || cell == ONGOING {
                            style.paint("alert", cell)
                        } else {
                            cell.to_owned()
                        }
                    }
                    3 => {
                        if events[offset].vacation {
                            style.paint("alert", cell)
                        } else {
                            style.paint("duration", cell)
                        }
                    }
                    4 => style.paint("tags", cell),
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
    tags_table.columns[1].set_alignment(Alignment::Right);
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
                let cell = if cell_num == 0 {
                    if offset < header_count {
                        style.paint("important", cell)
                    } else {
                        style.paint("tags", cell)
                    }
                } else {
                    style.paint("duration", cell)
                };
                print!("{}{}", margin, cell);
            }
            println!();
        }
    }
}

pub fn success<T: ToString>(msg: T, conf: &Configuration) {
    let style = Style::new(conf);
    eprintln!("{} {}", style.paint("success", "ok:"), msg.to_string());
}

pub fn warn<T: ToString>(msg: T, conf: &Configuration) {
    let style = Style::new(conf);
    eprintln!("{} {}", style.paint("warning", "warning:"), msg.to_string());
}

pub fn fatal<T: ToString>(msg: T, conf: &Configuration) -> ! {
    let style = Style::new(conf);
    eprintln!("{} {}", style.paint("error", "error:"), msg.to_string());
    std::process::exit(1);
}

pub fn describe(action: &str, extra: Option<&str>, item: Item, conf: &Configuration) {
    let style = Style::new(conf);
    let mut s = style.paint("success", action);
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
                s += &style.paint("alert", "no tags");
            } else {
                s += &style.paint("tags", tags.join(", "));
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
                s += &style.paint("alert", "no tags");
            } else {
                s += "tags: ";
                s += &style.paint("tags", tags.join(", "));
            }
            s += ")"
        }
        Item::Done(d, _) => {
            s += &style.paint("important", format!("{}", d.0.format("at %l:%M %P")))
        }
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
    }
}

// make sure base directory and its files are present
pub fn init(directory: Option<&str>) {
    if !base_dir(directory).as_path().exists() {
        create_dir(base_dir(directory).to_str().unwrap()).unwrap_or_else(|_| {
            panic!(
                "could not create base directory {}",
                base_dir(directory).to_str().unwrap()
            )
        });
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
        readme.write_all(b"\nJob Log\n\nThis directory holds files used by Job Log to maintain\na work log. For more details type\n\n   job --help\n\non the command line.\n\n       \n").expect("could not write README");
    }
}

const NON_COLORS: [&str; 7] = [
    "bold",
    "italic",
    "underline",
    "dimmed",
    "blink",
    "reverse",
    "hidden",
];

const NAMED_COLORS: [&str; 8] = [
    "black", "red", "green", "yellow", "blue", "purple", "cyan", "white",
];

/// One component of a style specification, e.g. `bold`, `red`, or `bg 1`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StyleSpec {
    NonColor(&'static str),
    Foreground(Color),
    Background(Color),
}

fn parse_color(token: &str) -> Option<Color> {
    const COLORS: [Color; 8] = [
        Color::Black,
        Color::Red,
        Color::Green,
        Color::Yellow,
        Color::Blue,
        Color::Purple,
        Color::Cyan,
        Color::White,
    ];
    if let Some(i) = NAMED_COLORS.iter().position(|name| *name == token) {
        return Some(COLORS[i]);
    }
    {
        // fixed colors are the decimal numbers 0-255 without leading zeroes
        if !token.is_empty()
            && token.bytes().all(|b| b.is_ascii_digit())
            && (token.len() == 1 || !token.starts_with('0'))
        {
            token.parse::<u8>().ok().map(Color::Fixed)
        } else {
            None
        }
    }
}

/// Parses a style specification such as "bold red bg 1" into its component specs.
/// Returns `None` if the string is not a valid style specification.
pub fn parse_style_spec(s: &str) -> Option<Vec<StyleSpec>> {
    let mut specs = Vec::new();
    let mut tokens = s.split_whitespace();
    while let Some(token) = tokens.next() {
        if NON_COLORS.contains(&token) {
            specs.push(StyleSpec::NonColor(match token {
                "bold" => "bold",
                "italic" => "italic",
                "underline" => "underline",
                "dimmed" => "dimmed",
                "blink" => "blink",
                "reverse" => "reverse",
                "hidden" => "hidden",
                _ => unreachable!(),
            }));
        } else if token == "fg" || token == "foreground" {
            let color = parse_color(tokens.next()?)?;
            specs.push(StyleSpec::Foreground(color));
        } else if token == "bg" || token == "background" {
            let color = parse_color(tokens.next()?)?;
            specs.push(StyleSpec::Background(color));
        } else if let Some(color) = parse_color(token) {
            specs.push(StyleSpec::Foreground(color));
        } else {
            return None;
        }
    }
    Some(specs)
}

/// Whether the string is a valid style specification
pub fn is_valid_style(s: &str) -> bool {
    parse_style_spec(s).is_some()
}

fn build_ansi_style(specs: &[StyleSpec]) -> AnsiStyle {
    // the last foreground color, if any, forms the base style
    let mut style = specs
        .iter()
        .rev()
        .find_map(|spec| match spec {
            StyleSpec::Foreground(color) => Some(color.normal()),
            _ => None,
        })
        .unwrap_or_default();
    for spec in specs {
        match spec {
            StyleSpec::NonColor(name) => {
                style = match *name {
                    "bold" => style.bold(),
                    "italic" => style.italic(),
                    "underline" => style.underline(),
                    "dimmed" => style.dimmed(),
                    "blink" => style.blink(),
                    "reverse" => style.reverse(),
                    "hidden" => style.hidden(),
                    _ => unreachable!(),
                }
            }
            // the foreground color is already incorporated as the base style
            StyleSpec::Foreground(_) => (),
            StyleSpec::Background(color) => style = style.on(*color),
        }
    }
    style
}

// putting this into a common struct so we can easily turn color off
pub struct Style {
    noop: bool,
    style_map: BTreeMap<String, AnsiStyle>,
}

impl Style {
    pub fn new(conf: &Configuration) -> Style {
        let mut style_map = BTreeMap::new();
        for pair in &conf.style_map {
            let style = parse_style_spec(pair.1)
                .map(|specs| build_ansi_style(&specs))
                .unwrap_or_default();
            style_map.insert(pair.0.clone(), style);
        }
        Style {
            noop: !conf.effective_color().0,
            style_map,
        }
    }
    pub fn paint<T: ToString>(&self, style: &str, text: T) -> String {
        if self.noop {
            text.to_string()
        } else {
            format!(
                "{}",
                self.style_map.get(style).unwrap().paint(text.to_string())
            )
        }
    }
}

// for use in validating tags
pub fn some_nws(s: &str) -> bool {
    s.chars().any(|c| !c.is_whitespace())
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
        let buffer = buffer.trim();
        if buffer.is_empty() {
            return true;
        }
        match buffer {
            "y" | "Y" => return true,
            "n" | "N" => return false,
            _ => {
                println!("please answer y or n");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn styles_that_match() {
        for style in &[
            "",
            "bold",
            " bold ",
            "bold underline",
            "bold bold",
            " bold  bold ",
            "red bold",
            "bold red",
            "fg red",
            "foreground red",
            "foreground 0",
            "foreground 255",
            "0",
            "255",
            "bg black",
            "background black",
        ] {
            assert!(is_valid_style(style), "{:?} should be a valid style", style);
        }
    }

    #[test]
    fn styles_that_dont_match() {
        for style in &[
            "boldbold",
            "boldunderline",
            "foreground 256",
            "256",
            "fg",
            "01",
            "+1",
            "redx",
        ] {
            assert!(
                !is_valid_style(style),
                "{:?} should not be a valid style",
                style
            );
        }
    }

    #[test]
    fn parsing_styles() {
        let specs = parse_style_spec("red bold bg 1").unwrap();
        assert_eq!(
            specs,
            vec![
                StyleSpec::Foreground(Color::Red),
                StyleSpec::NonColor("bold"),
                StyleSpec::Background(Color::Fixed(1)),
            ]
        );
        let specs = parse_style_spec("bold dimmed bg cyan").unwrap();
        assert_eq!(
            specs,
            vec![
                StyleSpec::NonColor("bold"),
                StyleSpec::NonColor("dimmed"),
                StyleSpec::Background(Color::Cyan),
            ]
        );
    }
}
