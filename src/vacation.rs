extern crate chrono;
extern crate clap;
extern crate colonnade;
extern crate pidgin;
extern crate regex;
extern crate two_timer;

use crate::configure::Configuration;
use crate::log::{parse_tags, parse_timestamp, tags, timestamp, Event, Filter};
use crate::util::{base_dir, fatal, remainder, some_nws, success, warn, Style};
use chrono::{Datelike, Duration, Local, NaiveDate, NaiveDateTime, Timelike};
use clap::{App, AppSettings, Arg, ArgMatches, SubCommand};
use colonnade::{Alignment, Colonnade};
use pidgin::{Grammar, Matcher};
use regex::Regex;
use std::cmp::Ordering;
use std::fs::{copy, File};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::PathBuf;
use two_timer::{parsable, parse};

fn after_help() -> &'static str {
    "\
Vacation time is the dark matter of the log. It is not stored in the log and it can be simultaneous with \
logged events inasmuch as it occurs on particular days when logged events also occur, but it generally doesn't \
have specific start and end times.

  > job vacation --list

      description                 tags  start                end         type   repetition  started  ended
   1  New Year's                        2015-01-01
   2  New Year's Day                    2016-01-01                              annual
   3  Memorial Day                      2015-05-25
   4  Labor Day                         2015-09-07
   5  took day off to go on date        2015-10-23
   6  4 hours Christmas Eve             2015-12-24 12:00 AM  12:00 AM    fixed
   7  Christmas                         2015-12-25                              annual
   8  4 hours New Year's Eve            2015-12-31 12:00 AM  12:00 AM    fixed
   9  field trip with Moe               2016-05-31
  10  July 4th                          2016-07-04                              annual

Vacation times can be fixed -- with definite start and end times -- flex -- having a flexible extent that just \
fills up unused workday hours in a particular day, or neither. The latter category is the default. The extent \
of a vacation period on an ordinary vacation day is just as many hours as you would have been expected to work \
had it been a regular workday.

In addition to these distinctions a particular vacation may repeat annually or monthly. Repeated vacations are marked \
as in force as of a particular data and, optionally, defunct as of another date. This way you can turn them on and \
off and see correct log summaries of earlier periods.

Because the vacation format is so complex it should not be edited by hand but only through the vacation subcommand. \
Generally this just means adding and subtracting vacation days. For the latter you will be presented with an \
enumerated list of known vacations. You delete them by their number in the list.

If two vacation periods overlap repeating periods will be preferred to non-repeating, narrower periods to wider, and \
ordinary over fixed over flex. In any case, a particular vacation moment will only be counted once.

Note, the Rust version of JobLog is adding some features to vacations: on and off times for repeating vacations. \
Because of this you will not be able to use the vacation file with the Perl client after you add repeating vacations.

All prefixes of 'vacation' are aliases of the subcommand.
"
}

// used in three places, so it's factored out
fn over_as_of_rx() -> Regex {
    Regex::new(r"\A(\d+)(?:\s+(\S.*?)\s*)?\z").unwrap()
}

fn number_date_validator(v: String) -> Result<(), String> {
    if let Some(captures) = over_as_of_rx().captures(&v) {
        let index = captures[1].to_owned();
        if index.parse::<usize>().is_ok() {
            if let Some(s) = captures.get(2) {
                let date = s.as_str();
                if parsable(date) {
                    Ok(())
                } else {
                    Err(format!(
                        "data expression in '{}', '{}', cannot be parsed",
                        v, date
                    ))
                }
            } else {
                Ok(())
            }
        } else {
            Err(String::from("bad format for number"))
        }
    } else {
        Err(String::from("bad format"))
    }
}

pub fn cli(mast: App<'static, 'static>, display_order: usize) -> App<'static, 'static> {
    mast.subcommand(
        SubCommand::with_name("vacation")
            .aliases(&["v", "va", "vac", "vaca", "vacat", "vacati", "vacatio"])
            .about("Records vacation time")
            .after_help(after_help())
            .arg(
                Arg::with_name("add")
                .short("a")
                .long("add")
                .help("Adds a vacation record (default action)")
                .conflicts_with_all(&["delete", "over-as-of", "list", "clear"])
                .display_order(0)
            )
            .arg(
                Arg::with_name("list")
                .short("l")
                .long("list")
                .help("Lists known vacation periods")
                .long_help("Just provide an enumerated list of the known vacation periods and do nothing further. This is a useful, probably necessary, precursor to deleting a vacation period.")
                .conflicts_with_all(&["delete", "over-as-of", "tag", "add", "clear"])
                .display_order(1)
            )
            .arg(
                Arg::with_name("when")
                .short("w")
                .long("when")
                .help("Sets vacation period")
                .long_help("The time period of the vacation. Unless the vacation is of the fixed type, only the dates of the time expression will be considered. 'Today at 2 pm' will have the same effect as 'today' or 'now'.")
                .value_name("period")
                .validator(|v| if parsable(&v) {Ok(())} else {Err(format!("cannot parse '{}' as a time expression", v))} )
                .default_value("today")
                .display_order(2)
            )
            .arg(
                Arg::with_name("tag")
                .short("t")
                .long("tag")
                .multiple(true)
                .number_of_values(1)
                .help("Adds this tag to the event")
                .long_help("A tag is just a short description, like 'religious', or 'family'. Add a tag to a vacation to facilitate filtering during log summaries.")
                .value_name("tag")
                .validator(|v| if some_nws(&v) {Ok(())} else {Err(format!("tag {:?} needs some non-whitespace character", v))})
                .conflicts_with_all(&["list", "delete", "over-as-of", "clear"])
                .display_order(3)
            )
            .arg(
                Arg::with_name("type")
                .long("type")
                .help("Marks the vacation as flex or fixed")
                .long_help("Flex and fixed vacations cannot repeat. They constrain the vacation period to some subportion of a normal workday. See the full --help text for more details.")
                .value_name("type")
                .possible_values(&["ordinary", "fixed", "flex"])
                .default_value("ordinary")
                .display_order(4)
            )
            .arg(
                Arg::with_name("repeats")
                .long("repeats")
                .help("Marks the vacation as repeating either annually or monthly")
                .long_help("If you have a vacation that repeats at intervals you may mark it as such. It will be assumed that the repetition can be inferred from either the day of the month (monthly), or the day of the month and the month of the year (annual). Repeating vacations cannot be marked as fixed or flex.")
                .value_name("period")
                .possible_values(&["annual", "monthly", "never"])
                .default_value("never")
                .display_order(5)
            )
            .arg(
                Arg::with_name("over-as-of")
                .long("over-as-of")
                .help("Indicates the end of a repeating vacation")
                .long_help("If you come to lose a vacation that repeated at intervals -- if you change jobs, for example, and lose a holiday -- this allows you to indicate when the repetition stops. You must identify the affected vacation by its number in the enumerated list (see --list). The date is 'today' by default.")
                .value_name("number [date]")
                .validator(number_date_validator)
                .conflicts_with_all(&["delete", "list", "add", "tag", "clear"])
                .display_order(6)
            )
            .arg(
                Arg::with_name("effective-as-of")
                .long("effective-as-of")
                .help("Indicates when a repeating vacation begins repeating")
                .long_help("If you gain a vacation that repeats at intervals -- if you change jobs, for example, and gain a holiday -- this allows you to indicate when the repetition begins. You must identify the affected vacation by its number in the enumerated list (see --list). The date is 'today' by default. If you add a new repeating vacation, it will by default become effective immediately. This option is chiefly useful when adding a repeating vacation retroactively.")
                .value_name("number [date]")
                .validator(number_date_validator)
                .conflicts_with_all(&["delete", "list", "add", "tag", "clear"])
                .display_order(7)
            )
            .arg(
                Arg::with_name("delete")
                .long("delete")
                .short("d")
                .help("Deletes a particular vacation record")
                .long_help("If you wish to delete a single vacation record altogether, use --delete. You must identify the affected vacation by its number in the enumerated list (see --list).")
                .value_name("number")
                .validator(|v| if v.parse::<usize>().is_ok() { Ok(())} else {Err(format!("could not parse {} as a vacation record index", v))})
                .conflicts_with_all(&["over-as-of", "list", "add", "tag", "clear"])
                .multiple(true)
                .number_of_values(1)
                .display_order(8)
            )
            .arg(
                Arg::with_name("clear")
                .long("clear")
                .help("Deletes all vacation records")
                .conflicts_with_all(&["over-as-of", "list", "add", "tag", "delete"])
                .display_order(9)
            )
            .setting(AppSettings::TrailingVarArg)
            .arg(
                Arg::with_name("description")
                    .help("some phrase identifying the vacation")
                    .long_help(
                        "A description of the vacation period. This is required if you are creating a new vacation record.",
                    )
                    .value_name("description")
                    .multiple(true)
            )
            .display_order(display_order)
    )
}

pub fn run(directory: Option<&str>, matches: &ArgMatches) {
    let conf = Configuration::read(None, directory);
    let mut controller = VacationController::read(None, conf.directory());
    if matches.is_present("list") {
        if controller.vacations.is_empty() {
            warn("no vacation records", &conf);
        } else {
            let mut data = vec![vec![
                String::from(""),
                String::from("description"),
                String::from("tags"),
                String::from("start"),
                String::from("end"),
                String::from("type"),
                String::from("repetition"),
                String::from("started"),
                String::from("ended"),
            ]];
            for (i, v) in controller.vacations.iter().enumerate() {
                let mut row = Vec::with_capacity(9);
                row.push((i + 1).to_string());
                row.push(v.description.to_owned());
                row.push(v.tags.join(", "));
                row.push(v.start_description());
                row.push(v.end_description());
                row.push(v.kind.to_s().to_owned());
                row.push(v.repetition.to_s().to_owned());
                row.push(v.effective_as_of_description());
                row.push(v.over_as_of_description());
                data.push(row);
            }
            let style = Style::new(&conf);
            let mut table = Colonnade::new(9, conf.width())
                .expect("could not create table to display vacation records");
            table
                .priority(0)
                .left_margin(2)
                .expect("insufficient space for vacation table");
            table.columns[0].alignment(Alignment::Right).left_margin(0);
            table.columns[1].priority(1);
            table.columns[2].priority(2);
            println!();
            for (row_num, row) in table
                .macerate(data)
                .expect("could not lay out vacation records")
                .iter()
                .enumerate()
            {
                for line in row {
                    for (cell_num, (margin, contents)) in line.iter().enumerate() {
                        print!("{}", margin);
                        if row_num == 0 {
                            print!("{}", style.paint("vacation_header", contents));
                        } else {
                            match cell_num {
                                0 => print!("{}", style.paint("vacation_number", contents)),
                                2 => print!("{}", style.paint("tags", contents)),
                                _ => print!(
                                    "{}",
                                    if row_num % 2 == 0 {
                                        style.paint("even", contents)
                                    } else {
                                        style.paint("odd", contents)
                                    }
                                ),
                            }
                        }
                    }
                    println!();
                }
            }
            println!();
        }
    } else if matches.is_present("over-as-of") {
        let captures = over_as_of_rx()
            .captures(&matches.value_of("over-as-of").unwrap())
            .unwrap();
        let index = captures[1].parse::<usize>().unwrap();
        let date = captures
            .get(2)
            .and_then(|m| Some(m.as_str()))
            .unwrap_or("today");
        let (date, _, _) = parse(date, conf.two_timer_config()).unwrap();
        match controller.set_over_as_of(index, &date) {
            Ok(s) => success(s, &conf),
            Err(s) => fatal(s, &conf),
        }
    } else if matches.is_present("effective-as-of") {
        let captures = over_as_of_rx()
            .captures(&matches.value_of("effective-as-of").unwrap())
            .unwrap();
        let index = captures[1].parse::<usize>().unwrap();
        let date = captures
            .get(2)
            .and_then(|m| Some(m.as_str()))
            .unwrap_or("today");
        let (date, _, _) = parse(date, conf.two_timer_config()).unwrap();
        match controller.set_effective_as_of(index, &date) {
            Ok(s) => success(s, &conf),
            Err(s) => fatal(s, &conf),
        }
    } else if matches.is_present("delete") || matches.is_present("clear") {
        let mut rows = if matches.is_present("clear") {
            controller
                .vacations
                .iter()
                .enumerate()
                .map(|(i, _)| i + 1)
                .collect()
        } else {
            let mut rows: Vec<usize> = matches
                .values_of("delete")
                .unwrap()
                .map(|s| s.parse::<usize>().unwrap())
                .collect();
            rows.sort_unstable();
            rows.dedup();
            let mut problems: Vec<usize> = (&rows)
                .iter()
                .filter(|&v| v - 1 >= controller.vacations.len())
                .map(|v| v.to_owned())
                .collect();
            if !problems.is_empty() {
                if problems.len() > 1 {
                    problems.reverse();
                    fatal(
                        format!(
                            "the following indices correspond to no vacation records: {}",
                            problems
                                .iter()
                                .map(|v| v.to_string())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                        &conf,
                    );
                } else {
                    fatal(
                        format!("there is no vacation record {}", problems[0]),
                        &conf,
                    );
                }
            }
            rows
        };
        rows.reverse();
        for row in rows {
            match controller.destroy(row) {
                Ok(v) => success(format!("deleted {}", v.describe()), &conf),
                Err(e) => fatal(e, &conf),
            }
        }
    } else {
        if matches.is_present("description") {
            let description = remainder("description", matches);
            let tags: Vec<String> = if let Some(values) = matches.values_of("tag") {
                values.map(|s| s.to_string()).collect()
            } else {
                Vec::new()
            };
            let (start, end, _) =
                parse(matches.value_of("when").unwrap(), conf.two_timer_config()).unwrap();
            let (description, recorded) = controller.record(
                description,
                tags,
                start,
                end,
                matches.value_of("type"),
                matches.value_of("repeats"),
            );
            if recorded {
                success(format!("added {}", description), &conf);
            } else {
                fatal(description, &conf)
            }
        } else {
            fatal(
                "You must provide some decription when creating a vacation record.",
                &conf,
            )
        }
    }
    controller.write();
}

fn vacation_path(directory: Option<&str>) -> PathBuf {
    let mut path = base_dir(directory);
    path.push("vacation");
    path
}

// basically a namespace for vacation-related functions
pub struct VacationController {
    vacations: Vec<Vacation>,
    changed: bool,
    path: String,
}

impl VacationController {
    // fetch vacation information in from file
    // the option argument facilitates testing
    pub fn read(path: Option<PathBuf>, directory: Option<&str>) -> VacationController {
        let path = path.unwrap_or(vacation_path(directory));
        let path_str = path.to_str().expect("cannot stringify path").to_owned();
        if path.as_path().exists() {
            let file = File::open(path).expect("could not open vacation file");
            let reader = BufReader::new(file);
            let vacations = reader
                .lines()
                .map(|l| l.unwrap())
                .filter_map(|l| Vacation::deserialize(&l))
                .collect();
            VacationController {
                vacations,
                changed: false,
                path: path_str,
            }
        } else {
            VacationController {
                vacations: vec![],
                changed: false,
                path: path_str,
            }
        }
    }
    // vacation file path
    fn path_buf(&self) -> PathBuf {
        PathBuf::from(&self.path)
    }
    // backup file path
    fn path_buf_bak(&self) -> PathBuf {
        let pb = self.path_buf();
        let mut parts: Vec<String> = pb
            .iter()
            .map(|s| {
                s.to_str()
                    .expect("trouble converting vacation file path to backup vacation file path")
                    .to_owned()
            })
            .collect();
        parts
            .last_mut()
            .expect("couldn't get file name")
            .push_str(".bak");
        parts.iter().collect()
    }
    pub fn add_vacation_times(
        &self,
        start: &NaiveDateTime,
        end: &NaiveDateTime,
        mut events: Vec<Event>, // these events *must be grouped by day*
        conf: &Configuration,
        now: Option<NaiveDateTime>,
        filter: &Filter,
    ) -> Vec<Event> {
        if self.vacations.is_empty() {
            return events;
        }
        let mut new_events = Vec::new();
        let mut date = start.date();
        let end_date = end.date();
        let now = now.unwrap_or(Local::now().naive_local());
        let sorted_records = self.sorted_vacation_records();
        while date < end_date {
            let mut seconds_worked = 0;
            while events.len() > 0 && events[0].start.date() == date {
                seconds_worked += events[0].duration(&now) as usize;
                new_events.push(events.remove(0));
            }
            if conf.is_workday(&date) {
                // only check for vacation time on workdays
                let s = date.and_hms(0, 0, 0);
                let e = s + Duration::days(1);
                // make sure we don't fetch in vacation time beyond the end of the last moment
                let e = if &e > end { end } else { &e };
                let start_workday = start_workday(&s, conf);
                let end_workday = start_workday + Duration::hours(conf.day_length as i64);
                let end_workday = if &end_workday > e { e } else { &end_workday };
                let delta = (end_workday.timestamp() - start_workday.timestamp()) as usize;
                let mut unworked_seconds = if seconds_worked > delta {
                    0
                } else {
                    delta - seconds_worked
                };
                for v in &sorted_records {
                    if let Some(event) = v.overlap(&s, e, unworked_seconds, conf) {
                        let duration = event.duration(&now) as usize;
                        if duration == 0 {
                            break;
                        }
                        if filter.matches(&event) {
                            if duration > unworked_seconds {
                                unworked_seconds = 0;
                            } else {
                                unworked_seconds -= duration;
                            }
                            new_events.push(event);
                            if v.full_day(conf) {
                                break;
                            }
                        }
                    } else {
                    }
                }
            }
            date = date + Duration::days(1);
        }
        new_events.sort_by(|a, b| {
            if a.start == b.start {
                (a.duration(&now) as usize).cmp(&(b.duration(&now) as usize))
            } else {
                a.start.cmp(&b.start)
            }
        });
        new_events
    }
    fn sorted_vacation_records(&self) -> Vec<&Vacation> {
        let mut sorted = self.vacations.iter().collect::<Vec<&Vacation>>();
        sorted.sort_by(|a, b| a.cmp(b));
        sorted
    }
    // serialize vacation records back to file
    // returns whether there was any change to the file system
    fn write(&self) -> bool {
        if !self.changed {
            return false;
        }
        if self.vacations.is_empty() {
            if self.path_buf().as_path().exists() {
                std::fs::remove_file(self.path_buf()).expect("failed to remove vacation file");
                true
            } else {
                false
            }
        } else {
            let mut backed_up = false;
            if self.path_buf().exists() {
                // make a backup copy just in case
                copy(self.path_buf(), self.path_buf_bak())
                    .expect("could not make backup of vacation file before saving changes");
                backed_up = true;
            }
            let mut write = BufWriter::new(
                File::create(self.path_buf()).expect("could not open vacation file for writing"),
            );
            for vacation in &self.vacations {
                writeln!(write, "{}", vacation.serialize()).expect(&format!(
                    "failed to write vacation record to vacation file: {:?}",
                    vacation
                ));
            }
            if backed_up {
                std::fs::remove_file(self.path_buf_bak())
                    .expect("could not remove vacation backup file");
            }
            true
        }
    }
    // remove a particular vacation record
    fn destroy(&mut self, index: usize) -> Result<Vacation, String> {
        if index == 0 {
            return Err(String::from("there is no vacation record 0"));
        }
        if self.vacations.len() + 1 > index {
            let v = self.vacations.remove(index - 1);
            self.changed = true;
            Ok(v)
        } else {
            Err(format!("there is no vacation record {}", index))
        }
    }
    fn contains(&self, new: &Vacation) -> bool {
        self.vacations
            .iter()
            .any(|v| v.start == new.start && v.end == new.end)
    }
    // create a new vacation record
    // returns a description and whether any event was recorded
    fn record(
        &mut self,
        description: String,
        mut tags: Vec<String>,
        start: NaiveDateTime,
        end: NaiveDateTime,
        kind: Option<&str>,
        repetition: Option<&str>,
    ) -> (String, bool) {
        tags.sort_unstable();
        tags.dedup();
        let mut vacation = Vacation::new(description, tags, start, end);
        if let Some(k) = kind {
            vacation.kind = Type::from_str(k);
        }
        if let Some(r) = repetition {
            vacation.repetition = Repetition::from_str(r);
            match vacation.repetition {
                Repetition::Never => (),
                _ => vacation.effective_as_of = Some(Local::now().naive_local()),
            }
        }
        let description = vacation.describe();
        let period = vacation.period();
        match vacation.valid() {
            Ok(()) => {
                if self.contains(&vacation) {
                    (
                        format!("there is already a record for the {}", period),
                        false,
                    )
                } else {
                    self.vacations.push(vacation);
                    self.changed = true;
                    (description, true)
                }
            }
            Err(s) => (s, false),
        }
    }
    fn set_over_as_of(&mut self, index: usize, date: &NaiveDateTime) -> Result<String, String> {
        if index == 0 {
            return Err(format!("there is no vacation record number {}", index));
        }
        let index = index - 1;
        if self.vacations.len() <= index {
            return Err(format!(
                "the most recent vacation record is number {}",
                self.vacations.len()
            ));
        }
        if self.vacations[index].repeating() {
            self.vacations[index].over_as_of = Some(date.clone());
            self.changed = true;
            Ok(format!(
                "repetition over as of {}: {}",
                date.format("%F"),
                self.vacations[index].describe()
            ))
        } else {
            Err(format!(
                "does not repeat: {}",
                self.vacations[index].describe()
            ))
        }
    }
    fn set_effective_as_of(
        &mut self,
        index: usize,
        date: &NaiveDateTime,
    ) -> Result<String, String> {
        if index == 0 {
            return Err(format!("there is no vacation record number {}", index));
        }
        let index = index - 1;
        if self.vacations.len() <= index {
            return Err(format!(
                "the most recent vacation record is number {}",
                self.vacations.len()
            ));
        }
        if self.vacations[index].repeating() {
            self.vacations[index].effective_as_of = Some(date.clone());
            self.changed = true;
            Ok(format!(
                "repetition effective as of {}: {}",
                date.format("%F"),
                self.vacations[index].describe()
            ))
        } else {
            Err(format!(
                "does not repeat: {}",
                self.vacations[index].describe()
            ))
        }
    }
}

#[derive(Debug)]
enum Type {
    Flex,
    Fixed,
    Ordinary,
}

impl Type {
    fn from_str(t: &str) -> Type {
        match t {
            "flex" => Type::Flex,
            "fixed" => Type::Fixed,
            "ordinary" => Type::Ordinary,
            _ => unreachable!(),
        }
    }
    fn from_num(t: &str) -> Type {
        match t {
            "0" => Type::Ordinary,
            "1" => Type::Flex,
            "2" => Type::Fixed,
            _ => unreachable!(),
        }
    }
    fn to_num(&self) -> &str {
        match self {
            Type::Flex => "1",
            Type::Fixed => "2",
            Type::Ordinary => "0",
        }
    }
    fn to_s(&self) -> &str {
        match self {
            Type::Flex => "flex",
            Type::Fixed => "fixed",
            Type::Ordinary => "",
        }
    }
    // to simplify ordering logic
    fn to_u8(&self) -> u8 {
        match self {
            Type::Ordinary => 0,
            Type::Fixed => 1,
            Type::Flex => 2,
        }
    }
}

impl PartialOrd for Type {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.to_u8().cmp(&other.to_u8()))
    }
}

impl Ord for Type {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other).unwrap()
    }
}

impl PartialEq for Type {
    fn eq(&self, other: &Self) -> bool {
        match self.cmp(other) {
            Ordering::Equal => true,
            _ => false,
        }
    }
}

impl Eq for Type {}

#[derive(Debug)]
enum Repetition {
    Annual,
    Monthly,
    Never,
}

impl Repetition {
    fn from_str(t: &str) -> Repetition {
        match t {
            "monthly" => Repetition::Monthly,
            "annual" => Repetition::Annual,
            "never" => Repetition::Never,
            _ => unreachable!(),
        }
    }
    fn from_num(t: &str) -> Repetition {
        match t {
            "0" => Repetition::Never,
            "1" => Repetition::Annual,
            "2" => Repetition::Monthly,
            _ => unreachable!(),
        }
    }
    fn to_num(&self) -> &str {
        match self {
            Repetition::Annual => "1",
            Repetition::Monthly => "2",
            Repetition::Never => "0",
        }
    }
    fn to_s(&self) -> &str {
        match self {
            Repetition::Annual => "annual",
            Repetition::Monthly => "monthly",
            Repetition::Never => "",
        }
    }
    // to make it easier to implement ordering
    fn to_u8(&self) -> u8 {
        match self {
            Repetition::Monthly => 0,
            Repetition::Annual => 1,
            Repetition::Never => 2,
        }
    }
}

impl PartialOrd for Repetition {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.to_u8().partial_cmp(&other.to_u8())
    }
}

impl Ord for Repetition {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other).unwrap()
    }
}

impl PartialEq for Repetition {
    fn eq(&self, other: &Self) -> bool {
        self.to_u8() == other.to_u8()
    }
}

impl Eq for Repetition {}

#[derive(Debug)]
struct Vacation {
    description: String,
    tags: Vec<String>,
    kind: Type,
    repetition: Repetition,
    start: NaiveDateTime,
    end: NaiveDateTime,
    effective_as_of: Option<NaiveDateTime>,
    over_as_of: Option<NaiveDateTime>,
}

impl PartialOrd for Vacation {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        if self.kind == other.kind {
            if self.repetition == other.repetition {
                if self.start == other.start {
                    if self.end == other.end {
                        if self.description == other.description {
                            self.tags.partial_cmp(&other.tags)
                        } else {
                            self.description.partial_cmp(&other.description)
                        }
                    } else {
                        self.end.partial_cmp(&other.end)
                    }
                } else {
                    self.start.partial_cmp(&other.start)
                }
            } else {
                self.repetition.partial_cmp(&other.repetition)
            }
        } else {
            self.kind.partial_cmp(&other.kind)
        }
    }
}

impl Ord for Vacation {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other).unwrap()
    }
}

impl PartialEq for Vacation {
    fn eq(&self, other: &Self) -> bool {
        match self.cmp(other) {
            Ordering::Equal => true,
            _ => false,
        }
    }
}

impl Eq for Vacation {}

// remove escape sequences
fn unescape_description(description: &str) -> String {
    let mut escaped = false;
    let mut cleaned = String::with_capacity(description.len());
    for c in description.chars() {
        if c == '\\' {
            if escaped {
                cleaned.push(c);
            } else {
                escaped = true;
            }
        } else {
            cleaned.push(c);
            escaped = false;
        }
    }
    cleaned
}

// description inot
fn escape_description(description: &str) -> String {
    let mut s = String::new();
    let mut was_whitespace = None; // strip initial whitespace and condense internal and terminal whitespace, normalizing to ' '
    for c in description.chars() {
        match c {
            ':' | '\\' => s.push('\\'),
            _ => (),
        }
        if c.is_whitespace() {
            if let Some(false) = was_whitespace {
                was_whitespace = Some(true);
            } else {
                continue;
            }
        } else {
            was_whitespace = Some(false);
        }
        s.push(if c.is_whitespace() { ' ' } else { c }); // normalize whitespace
    }
    s.trim().to_owned()
}

impl Vacation {
    // create an ordinary vacation record
    fn new(
        description: String,
        tags: Vec<String>,
        start: NaiveDateTime,
        end: NaiveDateTime,
    ) -> Vacation {
        Vacation {
            description,
            tags,
            start,
            end,
            kind: Type::Ordinary,
            repetition: Repetition::Never,
            effective_as_of: None,
            over_as_of: None,
        }
    }

    fn valid(&self) -> Result<(), String> {
        match self.kind {
            Type::Fixed | Type::Flex => match self.repetition {
                Repetition::Never => Ok(()),
                _ => Err(String::from(
                    "fixed and flex vacation records cannot repeat",
                )),
            },
            _ => Ok(()),
        }
    }

    fn start_description(&self) -> String {
        match self.kind {
            Type::Fixed => format!("{}", self.start.format("%F %I:%M %p")),
            _ => format!("{}", self.start.format("%F")),
        }
    }

    fn end_description(&self) -> String {
        match self.kind {
            Type::Fixed => format!("{}", self.start.format("%I:%M %p")),
            _ => {
                let d = (self.end - Duration::seconds(1)).date();
                if self.start.date() == d {
                    String::from("")
                } else {
                    format!("{}", d.format("%F"))
                }
            }
        }
    }

    fn effective_as_of_description(&self) -> String {
        if let Some(t) = self.effective_as_of {
            format!("{}", t.format("%F"))
        } else {
            String::from("")
        }
    }

    fn over_as_of_description(&self) -> String {
        if let Some(t) = self.over_as_of {
            format!("{}", t.format("%F"))
        } else {
            String::from("")
        }
    }

    fn repeating(&self) -> bool {
        match self.repetition {
            Repetition::Never => false,
            _ => true,
        }
    }

    fn deserialize(line: &str) -> Option<Vacation> {
        lazy_static! {
            static ref VACATION: Grammar = grammar!{

                TOP -> r(r"\A") <vacation_line> r(r"\z")
                vacation_line   -> <vacation> | r(r"\s*(?:#.*)?") // allowing (perhaps unwisely) blank lines and comments
                vacation        -> <start> (":") <end> (":") <kind> <repetition> (":") <tags> (":") <description> <optional_bits>?
                start           -> <timestamp>
                end             -> <timestamp>
                kind            -> r("[012]")
                repetition      -> r("[012]")
                tags            -> r(r"(?:\\.|[^:<\\])*") // colons, spaces, and < must be escaped, so the escape character \ must also be escaped
                description     -> r(r"(?:\\.|[^:\\])*") //  colons escaped
                optional_bits   -> (":") <effective_as_of>? (":") <over_as_of>?
                effective_as_of -> <timestamp>
                over_as_of      -> <timestamp>
                timestamp       -> r(r"\s*[1-9]\d{3}(?:\s+[1-9]\d?){2}(?:\s+(?:0|[1-9]\d?)){3}\s*")
            };
            static ref MATCHER: Matcher = VACATION.matcher().unwrap();
        }
        if let Some(ast) = MATCHER.parse(line) {
            if let Some(vacation) = ast.name("vacation") {
                let start = parse_timestamp(vacation.name("start").unwrap().as_str());
                let end = parse_timestamp(vacation.name("end").unwrap().as_str());
                let tags = parse_tags(ast.name("tags").unwrap().as_str());
                let description = unescape_description(ast.name("description").unwrap().as_str());
                let kind = Type::from_num(vacation.name("kind").unwrap().as_str());
                let repetition =
                    Repetition::from_num(vacation.name("repetition").unwrap().as_str());
                let effective_as_of = vacation
                    .name("effective_as_of")
                    .and_then(|s| Some(parse_timestamp(s.as_str())));
                let over_as_of = vacation
                    .name("over_as_of")
                    .and_then(|s| Some(parse_timestamp(s.as_str())));
                Some(Vacation {
                    start,
                    end,
                    tags,
                    description,
                    kind,
                    repetition,
                    effective_as_of,
                    over_as_of,
                })
            } else {
                None
            }
        } else {
            panic!("encountered unparsable line in vacation log")
        }
    }

    fn serialize(&self) -> String {
        let mut line = timestamp(&self.start);
        line.push_str(":");
        line.push_str(&timestamp(&self.end));
        line.push_str(":");
        line.push_str(self.kind.to_num());
        line.push_str(self.repetition.to_num());
        line.push_str(":");
        line.push_str(&tags(&self.tags));
        line.push_str(":");
        line.push_str(&escape_description(&self.description));
        if self.effective_as_of.is_some() || self.over_as_of.is_some() {
            line.push_str(":");
            if let Some(t) = self.effective_as_of {
                line.push_str(&timestamp(&t));
            }
            line.push_str(":");
            if let Some(t) = self.over_as_of {
                line.push_str(&timestamp(&t));
            }
        }
        line
    }
    fn describe(&self) -> String {
        format!(
            "vacation record for {}: '{}'",
            self.period(),
            self.description
        )
    }
    fn period(&self) -> String {
        format!(
            "period {} - {}",
            self.start_description(),
            self.end_description()
        )
    }
    fn duration(&self) -> Duration {
        self.end - self.start
    }
    // return an "event" representing an overlap of a vacation record with this span of time
    fn overlap(
        &self,
        start: &NaiveDateTime, // the start of the interval which might overlap vacation
        end: &NaiveDateTime,   // the end of the interval
        available_seconds: usize, // for flex time, the number of expected seconds of work left in the given workday
        conf: &Configuration,
    ) -> Option<Event> {
        let range: Option<(NaiveDateTime, NaiveDateTime)> = match self.kind {
            Type::Fixed => available_overlap((&self.start, &self.end), (start, end)),
            Type::Flex => {
                if let Some((s, e)) = available_overlap((&self.start, &self.end), (start, end)) {
                    let (s, e) = fit_range_to_workday(&s, &e, conf);
                    let end_available = s + Duration::seconds(available_seconds as i64);
                    // we don't want the flex end time to be greater than the end parameter, though
                    let mut end_times = vec![&e, &end_available, end];
                    end_times.sort_unstable();
                    let e = end_times[0].clone();
                    Some((s, e))
                } else {
                    None
                }
            }
            _ => {
                let maybe_range = match self.repetition {
                    Repetition::Never => Some((self.start.clone(), self.end.clone())),
                    Repetition::Annual => {
                        if self.effective_as_of.as_ref().unwrap_or(start) > end
                            || self.over_as_of.as_ref().unwrap_or(end) < start
                        {
                            None
                        } else {
                            let d1 = NaiveDate::from_ymd(
                                start.year(),
                                self.start.month(),
                                self.start.day(),
                            )
                            .and_hms(
                                self.start.hour(),
                                self.start.minute(),
                                self.start.second(),
                            );
                            let d2 = d1 + self.duration();
                            Some((d1, d2))
                        }
                    }
                    Repetition::Monthly => {
                        if self.effective_as_of.as_ref().unwrap_or(start) > end
                            || self.over_as_of.as_ref().unwrap_or(end) < start
                        {
                            None
                        } else {
                            let d1 =
                                NaiveDate::from_ymd(start.year(), start.month(), self.start.day())
                                    .and_hms(
                                        self.start.hour(),
                                        self.start.minute(),
                                        self.start.second(),
                                    );
                            let d2 = d1 + self.duration();
                            Some((d1, d2))
                        }
                    }
                };
                if let Some((adjusted_start, adjusted_end)) = maybe_range {
                    if let Some((s, e)) =
                        available_overlap((&adjusted_start, &adjusted_end), (start, end))
                    {
                        let (s, e) = fit_range_to_workday(&s, &e, conf);
                        // if the end parameter is now, we cut it off
                        let e = if &e > end { end.clone() } else { e };
                        Some((s, e))
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
        };
        if let Some((s, e)) = range {
            Some(Event {
                description: self.description.clone(),
                tags: self.tags.clone(),
                vacation: true,
                start: s,
                end: Some(e),
                vacation_type: Some(self.kind.to_s().to_owned()),
                start_overlap: false,
                end_overlap: false,
            })
        } else {
            None
        }
    }
    // whether this vacation record necessarily covers a full day of work
    fn full_day(&self, conf: &Configuration) -> bool {
        match self.kind {
            Type::Ordinary | Type::Flex => true,
            _ => {
                let duration = (self.end.timestamp() - self.start.timestamp()) as u32;
                (conf.day_length as u32) * (60 * 60) <= duration
            }
        }
    }
}

fn any_overlap(
    interval_1: (&NaiveDateTime, &NaiveDateTime),
    interval_2: (&NaiveDateTime, &NaiveDateTime),
) -> bool {
    // order intervals so interval_1 is not after interval_2
    let (interval_1, interval_2) = if interval_1.0 < interval_2.0 {
        (interval_1, interval_2)
    } else {
        (interval_2, interval_1)
    };
    // now interval_2 must begin before interval_1 ends
    interval_2.0 < interval_1.1
}

fn available_overlap(
    interval_1: (&NaiveDateTime, &NaiveDateTime),
    interval_2: (&NaiveDateTime, &NaiveDateTime),
) -> Option<(NaiveDateTime, NaiveDateTime)> {
    if any_overlap(interval_1, interval_2) {
        let s = if interval_1.0 < interval_2.0 {
            interval_2.0
        } else {
            interval_1.0
        }; // the greater of the two starts
        let e = if interval_1.1 < interval_2.1 {
            interval_1.1
        } else {
            interval_2.1
        }; // the lesser of the two ends
        Some((s.clone(), e.clone()))
    } else {
        None
    }
}

fn fit_range_to_workday(
    start: &NaiveDateTime,
    end: &NaiveDateTime,
    conf: &Configuration,
) -> (NaiveDateTime, NaiveDateTime) {
    let wd_start = start_workday(start, conf);
    let wd_end = wd_start + Duration::hours(conf.day_length as i64);
    available_overlap((start, end), (&wd_start, &wd_end)).unwrap()
}

fn start_workday(time: &NaiveDateTime, conf: &Configuration) -> NaiveDateTime {
    time.date().and_hms(
        conf.beginning_work_day.0 as u32,
        conf.beginning_work_day.1 as u32,
        0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::{Done, Event, LogController};
    use std::str::FromStr;

    // if the test panics, this leaves the file in the development directory for examination
    fn test_vacation_path(disambiguator: &str) -> Option<PathBuf> {
        let path = PathBuf::from_str(&format!("test_vacation_{}", disambiguator))
            .expect("could not create test vacation path");
        Some(path)
    }

    // ditto
    fn test_log_path(disambiguator: &str) -> Option<PathBuf> {
        let path = PathBuf::from_str(&format!("test_log_{}", disambiguator))
            .expect("could not create test log path");
        Some(path)
    }

    // so we have a known configuraiton
    fn test_configuration_path(disambiguator: &str) -> Option<PathBuf> {
        let path = PathBuf::from_str(&format!("test_configuration_{}", disambiguator))
            .expect("could not create test configuration path");
        Some(path)
    }

    fn test_configuration(disambiguator: &str) -> Configuration {
        File::create(test_configuration_path(disambiguator).unwrap().as_path()).unwrap();
        Configuration::read(test_configuration_path(disambiguator), Some("."))
    }

    fn test_vacation_controller(fresh: bool, disambiguator: &str) -> VacationController {
        if fresh {
            File::create(test_vacation_path(disambiguator).unwrap().as_path()).unwrap();
        }
        VacationController::read(test_vacation_path(disambiguator), Some("."))
    }

    fn test_log_controller(
        fresh: bool,
        disambiguator: &str,
        conf: &Configuration,
    ) -> LogController {
        if fresh {
            File::create(test_log_path(disambiguator).unwrap().as_path()).unwrap();
        }
        LogController::new(test_log_path(disambiguator), conf).expect("could not open test log")
    }

    fn test_time(phrase: &str) -> (NaiveDateTime, NaiveDateTime) {
        let (start, end, _) =
            parse(phrase, None).expect(&format!("could not make test time from '{}'", phrase));
        (start, end)
    }

    fn test_now() -> NaiveDateTime {
        NaiveDate::from_ymd(2001, 1, 1).and_hms(12, 0, 0)
    }

    // remove test files
    fn cleanup(disambiguator: &str) {
        std::fs::remove_file(
            PathBuf::from_str(
                test_configuration_path(disambiguator)
                    .unwrap()
                    .to_str()
                    .expect("no configuration file"),
            )
            .expect("could not obtain path of configuration"),
        )
        .expect("failed to remove test configuration file");
        std::fs::remove_file(
            PathBuf::from_str(
                test_vacation_path(disambiguator)
                    .unwrap()
                    .to_str()
                    .expect("no vacation file"),
            )
            .expect("could not obtain path of vacation"),
        )
        .expect("failed to remove test vacation file");
        std::fs::remove_file(
            PathBuf::from_str(
                test_log_path(disambiguator)
                    .unwrap()
                    .to_str()
                    .expect("no log file"),
            )
            .expect("could not obtain path of log"),
        )
        .expect("failed to remove test log file");
    }

    fn add_event(log: &mut LogController, time: &NaiveDateTime, description: &str) {
        let mut event = Event::coin(description.to_owned(), Vec::new());
        event.start = time.clone();
        log.append_to_log(event, "could not add event");
    }

    fn end_event(log: &mut LogController, time: &NaiveDateTime) {
        log.append_to_log(Done(time.clone()), "could not end event");
    }

    fn add_vacation(
        vacation: &mut VacationController,
        description: &str,
        tags: Vec<&str>,
        start: &NaiveDateTime,
        end: &NaiveDateTime,
        kind: Option<&str>,
        repetition: Option<&str>,
    ) -> (String, bool) {
        vacation.record(
            description.to_owned(),
            tags.iter().map(|s| s.to_string()).collect(),
            start.clone(),
            end.clone(),
            kind,
            repetition,
        )
    }

    #[test]
    fn simple_test() {
        let disambiguator = "simple_test";
        let conf = test_configuration(disambiguator);
        let mut log = test_log_controller(true, disambiguator, &conf);
        let mut vacation = test_vacation_controller(true, disambiguator);
        let now = test_now();
        let filter = Filter::dummy();
        let (christmas_starts, christmas_ends) = test_time("Dec 25, 2000");
        add_vacation(
            &mut vacation,
            "Christmas",
            vec![],
            &christmas_starts,
            &christmas_ends,
            None,
            None,
        );
        let events = log.events_in_range(&christmas_starts, &christmas_ends);
        assert_eq!(0, events.len(), "nothing in log yet");
        let events = vacation.add_vacation_times(
            &christmas_starts,
            &christmas_ends,
            events,
            &conf,
            Some(now.clone()),
            &filter,
        );
        assert_eq!(1, events.len(), "log now has one event");
        assert_eq!(
            conf.day_length * (60.0 * 60.0),
            events[0].duration(&now),
            "vacation lasts one work day"
        );
        assert_eq!(true, events[0].vacation, "event is marked as vacation");
        assert_eq!(
            Some(String::from("")),
            events[0].vacation_type,
            "expected vacation type"
        );
        assert_eq!(
            String::from("Christmas"),
            events[0].description,
            "expected description"
        );
        assert_eq!(0, events[0].tags.len(), "no tags");
        cleanup(disambiguator);
    }

    #[test]
    fn tags() {
        let disambiguator = "tags";
        let conf = test_configuration(disambiguator);
        let mut log = test_log_controller(true, disambiguator, &conf);
        let mut vacation = test_vacation_controller(true, disambiguator);
        let now = test_now();
        let filter = Filter::dummy();
        let (christmas_starts, christmas_ends) = test_time("Dec 25, 2000");
        add_vacation(
            &mut vacation,
            "Christmas",
            vec!["foo", "bar"],
            &christmas_starts,
            &christmas_ends,
            None,
            None,
        );
        let events = log.events_in_range(&christmas_starts, &christmas_ends);
        assert_eq!(0, events.len(), "nothing in log yet");
        let events = vacation.add_vacation_times(
            &christmas_starts,
            &christmas_ends,
            events,
            &conf,
            Some(now.clone()),
            &filter,
        );
        assert_eq!(1, events.len(), "log now has one event");
        assert_eq!(
            conf.day_length * (60.0 * 60.0),
            events[0].duration(&now),
            "vacation lasts one work day"
        );
        assert_eq!(true, events[0].vacation, "event is marked as vacation");
        assert_eq!(
            Some(String::from("")),
            events[0].vacation_type,
            "expected vacation type"
        );
        assert_eq!(
            String::from("Christmas"),
            events[0].description,
            "expected description"
        );
        assert_eq!(
            vec!["bar", "foo"],
            events[0]
                .tags
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<&str>>(),
            "same tags"
        );
        cleanup(disambiguator);
    }

    #[test]
    fn no_workdays() {
        let disambiguator = "no_workdays";
        let mut conf = test_configuration(disambiguator);
        conf.workdays("");
        let mut log = test_log_controller(true, disambiguator, &conf);
        let mut vacation = test_vacation_controller(true, disambiguator);
        let filter = Filter::dummy();
        let (christmas_starts, christmas_ends) = test_time("Dec 25, 2000");
        add_vacation(
            &mut vacation,
            "Christmas",
            vec![],
            &christmas_starts,
            &christmas_ends,
            None,
            None,
        );
        let events = log.events_in_range(&christmas_starts, &christmas_ends);
        assert_eq!(0, events.len(), "nothing in log yet");
        let events = vacation.add_vacation_times(
            &christmas_starts,
            &christmas_ends,
            events,
            &conf,
            Some(test_now()),
            &filter,
        );
        assert_eq!(0, events.len(), "still nothing in log");
        cleanup(disambiguator);
    }

    #[test]
    fn repetition() {
        let disambiguator = "repetition";
        let mut conf = test_configuration(disambiguator);
        conf.workdays("SMTWHFA");
        let mut log = test_log_controller(true, disambiguator, &conf);
        let mut vacation = test_vacation_controller(true, disambiguator);
        let now = test_now();
        let filter = Filter::dummy();
        let (christmas_starts, christmas_ends) = test_time("Dec 25, 1999");
        add_vacation(
            &mut vacation,
            "Christmas",
            vec![],
            &christmas_starts,
            &christmas_ends,
            None,
            Some("annual"),
        );
        vacation
            .set_effective_as_of(1, &christmas_starts)
            .expect("could set effective date of repetition to time in past");
        let (christmas_starts, christmas_ends) = test_time("Dec 25, 2000");
        let events = log.events_in_range(&christmas_starts, &christmas_ends);
        assert_eq!(0, events.len(), "nothing in log yet");
        let events = vacation.add_vacation_times(
            &christmas_starts,
            &christmas_ends,
            events,
            &conf,
            Some(now.clone()),
            &filter,
        );
        assert_eq!(1, events.len(), "log now has one event");
        assert_eq!(
            conf.day_length * (60.0 * 60.0),
            events[0].duration(&now),
            "vacation lasts one work day"
        );
        assert_eq!(true, events[0].vacation, "event is marked as vacation");
        assert_eq!(
            Some(String::from("")),
            events[0].vacation_type,
            "expected vacation type"
        );
        assert_eq!(
            String::from("Christmas"),
            events[0].description,
            "expected description"
        );
        assert_eq!(0, events[0].tags.len(), "no tags");
        cleanup(disambiguator);
    }

    #[test]
    fn repetition_over() {
        let disambiguator = "repetition_over";
        let mut conf = test_configuration(disambiguator);
        conf.workdays("SMTWHFA");
        let mut log = test_log_controller(true, disambiguator, &conf);
        let mut vacation = test_vacation_controller(true, disambiguator);
        let filter = Filter::dummy();
        let (christmas_starts, christmas_ends) = test_time("Dec 25, 1999");
        add_vacation(
            &mut vacation,
            "Christmas",
            vec![],
            &christmas_starts,
            &christmas_ends,
            None,
            Some("annual"),
        );
        vacation
            .set_effective_as_of(1, &christmas_starts)
            .expect("could set effective date of repetition to time in past");
        let when_over = christmas_starts + Duration::days(30);
        vacation
            .set_over_as_of(1, &when_over)
            .expect("could set over date of repetition");
        let (christmas_starts, christmas_ends) = test_time("Dec 25, 2000");
        let events = log.events_in_range(&christmas_starts, &christmas_ends);
        assert_eq!(0, events.len(), "nothing in log yet");
        let events = vacation.add_vacation_times(
            &christmas_starts,
            &christmas_ends,
            events,
            &conf,
            Some(test_now()),
            &filter,
        );
        assert_eq!(0, events.len(), "still nothing in log");
        cleanup(disambiguator);
    }

    #[test]
    fn repetition_not_yet_begun() {
        let disambiguator = "repetition_not_yet_begun";
        let mut conf = test_configuration(disambiguator);
        conf.workdays("SMTWHFA");
        let mut log = test_log_controller(true, disambiguator, &conf);
        let mut vacation = test_vacation_controller(true, disambiguator);
        let filter = Filter::dummy();
        let (christmas_starts, christmas_ends) = test_time("Dec 25, 1999");
        add_vacation(
            &mut vacation,
            "Christmas",
            vec![],
            &christmas_starts,
            &christmas_ends,
            None,
            Some("annual"),
        );
        let (future_time, _) = test_time("Dec 25, 2001");
        vacation
            .set_effective_as_of(1, &future_time)
            .expect("could set effective date of repetition to time in future");
        let (christmas_starts, christmas_ends) = test_time("Dec 25, 2000");
        let events = log.events_in_range(&christmas_starts, &christmas_ends);
        assert_eq!(0, events.len(), "nothing in log yet");
        let events = vacation.add_vacation_times(
            &christmas_starts,
            &christmas_ends,
            events,
            &conf,
            Some(test_now()),
            &filter,
        );
        assert_eq!(0, events.len(), "still nothing in log");
        cleanup(disambiguator);
    }

    #[test]
    fn monthly_repetition() {
        let disambiguator = "monthly_repetition";
        let mut conf = test_configuration(disambiguator);
        conf.workdays("SMTWHFA");
        let mut log = test_log_controller(true, disambiguator, &conf);
        let mut vacation = test_vacation_controller(true, disambiguator);
        let now = test_now();
        let filter = Filter::dummy();
        let (ides_starts, ides_ends) = test_time("Dec 15, 1999");
        add_vacation(
            &mut vacation,
            "Ides",
            vec![],
            &ides_starts,
            &ides_ends,
            None,
            Some("monthly"),
        );
        vacation
            .set_effective_as_of(1, &ides_starts)
            .expect("could set effective date of repetition to time in past");
        let (ides_starts, ides_ends) = test_time("Jan 15, 2000");
        let events = log.events_in_range(&ides_starts, &ides_ends);
        assert_eq!(0, events.len(), "nothing in log yet");
        let events = vacation.add_vacation_times(
            &ides_starts,
            &ides_ends,
            events,
            &conf,
            Some(now.clone()),
            &filter,
        );
        assert_eq!(1, events.len(), "log now has one event");
        assert_eq!(
            conf.day_length * (60.0 * 60.0),
            events[0].duration(&now),
            "vacation lasts one work day"
        );
        assert_eq!(true, events[0].vacation, "event is marked as vacation");
        assert_eq!(
            Some(String::from("")),
            events[0].vacation_type,
            "expected vacation type"
        );
        assert_eq!(
            String::from("Ides"),
            events[0].description,
            "expected description"
        );
        assert_eq!(0, events[0].tags.len(), "no tags");
        cleanup(disambiguator);
    }

    #[test]
    fn one_before() {
        let disambiguator = "one_before";
        let mut conf = test_configuration(disambiguator);
        conf.workdays("SMTWHFA");
        let mut log = test_log_controller(true, disambiguator, &conf);
        let mut vacation = test_vacation_controller(true, disambiguator);
        let filter = Filter::dummy();
        let (christmas_starts, christmas_ends) = test_time("Dec 25, 1999");
        add_vacation(
            &mut vacation,
            "Christmas",
            vec![],
            &christmas_starts,
            &christmas_ends,
            None,
            Some("annual"),
        );
        vacation
            .set_effective_as_of(1, &christmas_starts)
            .expect("could set effective date of repetition to time in past");
        let (new_start, new_end) = test_time("Dec 24, 2000");
        let events = log.events_in_range(&new_start, &new_end);
        assert_eq!(0, events.len(), "nothing in log yet");
        let events = vacation.add_vacation_times(
            &new_start,
            &new_end,
            events,
            &conf,
            Some(test_now()),
            &filter,
        );
        assert_eq!(0, events.len(), "still nothing");
        cleanup(disambiguator);
    }

    #[test]
    fn one_after() {
        let disambiguator = "one_after";
        let mut conf = test_configuration(disambiguator);
        conf.workdays("SMTWHFA");
        let mut log = test_log_controller(true, disambiguator, &conf);
        let mut vacation = test_vacation_controller(true, disambiguator);
        let filter = Filter::dummy();
        let (christmas_starts, christmas_ends) = test_time("Dec 25, 1999");
        add_vacation(
            &mut vacation,
            "Christmas",
            vec![],
            &christmas_starts,
            &christmas_ends,
            None,
            Some("annual"),
        );
        vacation
            .set_effective_as_of(1, &christmas_starts)
            .expect("could set effective date of repetition to time in past");
        let (new_start, new_end) = test_time("Dec 26, 2000");
        let events = log.events_in_range(&new_start, &new_end);
        assert_eq!(0, events.len(), "nothing in log yet");
        let events = vacation.add_vacation_times(
            &new_start,
            &new_end,
            events,
            &conf,
            Some(test_now()),
            &filter,
        );
        assert_eq!(0, events.len(), "still nothing");
        cleanup(disambiguator);
    }

    #[test]
    fn simple_flex() {
        let disambiguator = "simple_flex";
        let mut conf = test_configuration(disambiguator);
        conf.workdays("SMTWHFA");
        let mut log = test_log_controller(true, disambiguator, &conf);
        let mut vacation = test_vacation_controller(true, disambiguator);
        let now = test_now();
        let filter = Filter::dummy();
        let (christmas_eve_starts, christmas_eve_ends) = test_time("Dec 24, 2000");
        add_vacation(
            &mut vacation,
            "Christmas Eve",
            vec![],
            &christmas_eve_starts,
            &christmas_eve_ends,
            Some("flex"),
            None,
        );
        let task_start = christmas_eve_starts + Duration::hours(8);
        add_event(&mut log, &task_start, "working a bit");
        let task_end = task_start + Duration::hours(4);
        end_event(&mut log, &task_end);
        let mut log = test_log_controller(false, disambiguator, &conf);
        let events = log.events_in_range(&christmas_eve_starts, &christmas_eve_ends);
        assert_eq!(1, events.len(), "the one event in log");
        let events = vacation.add_vacation_times(
            &christmas_eve_starts,
            &christmas_eve_ends,
            events,
            &conf,
            Some(now.clone()),
            &filter,
        );
        assert_eq!(2, events.len(), "task and vacation in log");
        let events = events
            .into_iter()
            .filter(|e| e.vacation)
            .collect::<Vec<Event>>();
        assert_eq!(1, events.len(), "only one vacation item added");
        assert_eq!(
            (conf.day_length - 4.0) * (60.0 * 60.0),
            events[0].duration(&now),
            "vacation lasts the remainder of the work day"
        );
        assert_eq!(true, events[0].vacation, "event is marked as vacation");
        assert_eq!(
            Some(String::from("flex")),
            events[0].vacation_type,
            "expected vacation type"
        );
        assert_eq!(
            String::from("Christmas Eve"),
            events[0].description,
            "expected description"
        );
        assert_eq!(0, events[0].tags.len(), "no tags");
        cleanup(disambiguator);
    }

    #[test]
    fn long_vacation() {
        let disambiguator = "long_vacation";
        let mut conf = test_configuration(disambiguator);
        conf.workdays("SMTWHFA");
        let mut log = test_log_controller(true, disambiguator, &conf);
        let mut vacation = test_vacation_controller(true, disambiguator);
        let now = test_now();
        let filter = Filter::dummy();
        let (vacation_starts, vacation_ends) = test_time("Dec 23, 2000 - Dec 31, 2000");
        add_vacation(
            &mut vacation,
            "Christmas",
            vec![],
            &vacation_starts,
            &vacation_ends,
            None,
            None,
        );
        for i in 23..32 {
            let (vacation_day_starts, vacation_day_ends) = test_time(&format!("Dec {}, 2000", i));
            let events = log.events_in_range(&vacation_day_starts, &vacation_day_ends);
            assert_eq!(0, events.len(), "nothing in log yet");
            let events = vacation.add_vacation_times(
                &vacation_day_starts,
                &vacation_day_ends,
                events,
                &conf,
                Some(now.clone()),
                &filter,
            );
            assert_eq!(1, events.len(), "log now has one event");
            assert_eq!(
                conf.day_length * (60.0 * 60.0),
                events[0].duration(&now),
                "vacation lasts one work day"
            );
            assert_eq!(true, events[0].vacation, "event is marked as vacation");
            assert_eq!(
                Some(String::from("")),
                events[0].vacation_type,
                "expected vacation type"
            );
            assert_eq!(
                String::from("Christmas"),
                events[0].description,
                "expected description"
            );
            assert_eq!(0, events[0].tags.len(), "no tags");
        }
        cleanup(disambiguator);
    }

    #[test]
    fn simple_fixed() {
        let disambiguator = "simple_fixed";
        let mut conf = test_configuration(disambiguator);
        conf.workdays("SMTWHFA");
        let mut log = test_log_controller(true, disambiguator, &conf);
        let mut vacation = test_vacation_controller(true, disambiguator);
        let now = test_now();
        let filter = Filter::dummy();
        let (random_day_starts, random_day_ends) = test_time("Dec 11, 2000 ");
        let vacation_starts = random_day_starts + Duration::hours(10);
        let vacation_ends = vacation_starts + Duration::hours(2);
        add_vacation(
            &mut vacation,
            "random time off",
            vec![],
            &vacation_starts,
            &vacation_ends,
            Some("fixed"),
            None,
        );
        let task_start = random_day_starts + Duration::hours(8);
        add_event(&mut log, &task_start, "working a bit");
        let task_end = task_start + Duration::hours(2);
        end_event(&mut log, &task_end);
        let mut log = test_log_controller(false, disambiguator, &conf);
        let events = log.events_in_range(&random_day_starts, &random_day_ends);
        assert_eq!(1, events.len(), "the one event in log");
        let events = vacation.add_vacation_times(
            &random_day_starts,
            &random_day_ends,
            events,
            &conf,
            Some(now.clone()),
            &filter,
        );
        assert_eq!(2, events.len(), "task and vacation in log");
        let events = events
            .into_iter()
            .filter(|e| e.vacation)
            .collect::<Vec<Event>>();
        assert_eq!(1, events.len(), "only one vacation item added");
        assert_eq!(
            (2.0 * 60.0 * 60.0),
            events[0].duration(&now),
            "vacation lasts two hours"
        );
        assert_eq!(
            vacation_starts, events[0].start,
            "vacation starts when expected"
        );
        assert_eq!(
            Some(vacation_ends),
            events[0].end,
            "vacation ends when expected"
        );
        assert_eq!(true, events[0].vacation, "event is marked as vacation");
        assert_eq!(
            Some(String::from("fixed")),
            events[0].vacation_type,
            "expected vacation type"
        );
        assert_eq!(
            String::from("random time off"),
            events[0].description,
            "expected description"
        );
        assert_eq!(0, events[0].tags.len(), "no tags");
        cleanup(disambiguator);
    }
    #[test]
    fn fixed_overlapping_task() {
        let disambiguator = "fixed_overlapping_task";
        let mut conf = test_configuration(disambiguator);
        conf.workdays("SMTWHFA");
        let mut log = test_log_controller(true, disambiguator, &conf);
        let mut vacation = test_vacation_controller(true, disambiguator);
        let now = test_now();
        let filter = Filter::dummy();
        let (random_day_starts, random_day_ends) = test_time("Dec 11, 2000 ");
        let vacation_starts = random_day_starts + Duration::hours(8);
        let vacation_ends = vacation_starts + Duration::hours(2);
        add_vacation(
            &mut vacation,
            "random time off",
            vec![],
            &vacation_starts,
            &vacation_ends,
            Some("fixed"),
            None,
        );
        let task_start = random_day_starts + Duration::hours(8);
        add_event(&mut log, &task_start, "working a bit");
        let task_end = task_start + Duration::hours(2);
        end_event(&mut log, &task_end);
        let mut log = test_log_controller(false, disambiguator, &conf);
        let events = log.events_in_range(&random_day_starts, &random_day_ends);
        assert_eq!(1, events.len(), "the one event in log");
        let events = vacation.add_vacation_times(
            &random_day_starts,
            &random_day_ends,
            events,
            &conf,
            Some(now.clone()),
            &filter,
        );
        assert_eq!(2, events.len(), "task and vacation in log");
        let events = events
            .into_iter()
            .filter(|e| e.vacation)
            .collect::<Vec<Event>>();
        assert_eq!(1, events.len(), "only one vacation item added");
        assert_eq!(
            (2.0 * 60.0 * 60.0),
            events[0].duration(&now),
            "vacation lasts two hours"
        );
        assert_eq!(
            vacation_starts, events[0].start,
            "vacation starts when expected"
        );
        assert_eq!(
            Some(vacation_ends),
            events[0].end,
            "vacation ends when expected"
        );
        assert_eq!(true, events[0].vacation, "event is marked as vacation");
        assert_eq!(
            Some(String::from("fixed")),
            events[0].vacation_type,
            "expected vacation type"
        );
        assert_eq!(
            String::from("random time off"),
            events[0].description,
            "expected description"
        );
        assert_eq!(0, events[0].tags.len(), "no tags");
        cleanup(disambiguator);
    }
}
