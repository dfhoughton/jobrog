use crate::configure::Configuration;
use crate::log::{parse_tags, parse_timestamp, tags, timestamp, Event, Filter};
use crate::util::{base_dir, fatal, success, valid_tag, valid_time_expression, warn, Style};
use chrono::{Datelike, Duration, Local, NaiveDate, NaiveDateTime, Timelike};

use colonnade::{Alignment, Colonnade};
use regex::Regex;
use std::cmp::Ordering;
use std::fs::{copy, File};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::LazyLock;
use two_timer::parse;

pub const AFTER_HELP: &str = "\
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
had it been a regular workday. To set the start and end times of a fixed vacation you need to provide explicit \
start and end times to the --when option; e.g.,

  > job vacation --type=flex --when=\"from 10 to 11\" eye appointment

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
";

// a vacation file timestamp: a four-digit year, two one- or two-digit fields with no
// leading zero, and three fields that may be zero, all separated by whitespace
static VACATION_TIMESTAMP_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\A\s*[1-9]\d{3}(?:\s+[1-9]\d?){2}(?:\s+(?:0|[1-9]\d?)){3}\s*\z").unwrap()
});

// tags may contain escaped colons but no unescaped colons or '<'
static VACATION_TAGS_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\A(?:\\.|[^:<\\])*\z").unwrap());

// descriptions may contain escaped colons but no unescaped colons
static VACATION_DESCRIPTION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\A(?:\\.|[^:\\])*\z").unwrap());

// used in three places, so it's factored out
fn over_as_of_rx() -> &'static LazyLock<Regex> {
    static RX: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\A(\d+)(?:\s+(\S.*?)\s*)?\z").unwrap());
    &RX
}

fn number_date_validator(v: &str) -> Result<String, String> {
    if let Some(captures) = over_as_of_rx().captures(v) {
        let index = &captures[1];
        if index.parse::<usize>().is_ok() {
            if let Some(s) = captures.get(2) {
                let date = s.as_str();
                if two_timer::parsable(date) {
                    Ok(v.to_owned())
                } else {
                    Err(format!(
                        "data expression in '{}', '{}', cannot be parsed",
                        v, date
                    ))
                }
            } else {
                Ok(v.to_owned())
            }
        } else {
            Err(String::from("bad format for number"))
        }
    } else {
        Err(String::from("bad format"))
    }
}

fn valid_index(v: &str) -> Result<String, String> {
    match v.parse::<usize>() {
        Ok(_) => Ok(v.to_owned()),
        Err(_) => Err(format!("could not parse {} as a vacation record index", v)),
    }
}

#[derive(Debug, clap::Args)]
#[command(
    aliases = ["v", "va", "vac", "vaca", "vacat", "vacati", "vacatio"],
    about = "Records vacation time",
    after_help = AFTER_HELP,
    after_long_help = AFTER_HELP,
    trailing_var_arg = true
)]
pub struct Args {
    /// Adds a vacation record (default action)
    #[arg(short = 'a', long, conflicts_with_all = ["delete", "over_as_of", "list", "clear"])]
    pub add: bool,

    /// Lists known vacation periods
    ///
    /// Just provide an enumerated list of the known vacation periods and do nothing further. This is a useful,
    /// probably necessary, precursor to deleting a vacation period.
    #[arg(short = 'l', long, conflicts_with_all = ["delete", "over_as_of", "tag", "add", "clear"])]
    pub list: bool,

    /// Sets vacation period
    ///
    /// The time period of the vacation. Unless the vacation is of the fixed type, only the dates of the time
    /// expression will be considered. 'Today at 2 pm' will have the same effect as 'today' or 'now'.
    #[arg(
        short = 'w',
        long,
        value_name = "period",
        default_value = "today",
        value_parser = valid_time_expression
    )]
    pub when: String,

    /// Adds this tag to the event
    ///
    /// A tag is just a short description, like 'religious', or 'family'. Add a tag to a vacation to facilitate
    /// filtering during log summaries.
    #[arg(
        short = 't',
        long,
        value_name = "tag",
        value_parser = valid_tag,
        conflicts_with_all = ["list", "delete", "over_as_of", "clear"]
    )]
    pub tag: Vec<String>,

    /// Marks the vacation as flex or fixed
    ///
    /// Flex and fixed vacations cannot repeat. They constrain the vacation period to some subportion of a
    /// normal workday. See the full --help text for more details.
    #[arg(
        long = "type",
        value_name = "type",
        value_parser = ["ordinary", "fixed", "flex"],
        default_value = "ordinary"
    )]
    pub kind: String,

    /// Marks the vacation as repeating either annually or monthly
    ///
    /// If you have a vacation that repeats at intervals you may mark it as such. It will be assumed that the
    /// repetition can be inferred from either the day of the month (monthly), or the day of the month and the
    /// month of the year (annual). Repeating vacations cannot be marked as fixed or flex.
    #[arg(
        long,
        value_name = "period",
        value_parser = ["annual", "monthly", "never"],
        default_value = "never"
    )]
    pub repeats: String,

    /// Indicates the end of a repeating vacation
    ///
    /// If you come to lose a vacation that repeated at intervals -- if you change jobs, for example, and lose a
    /// holiday -- this allows you to indicate when the repetition stops. You must identify the affected vacation
    /// by its number in the enumerated list (see --list). The date is 'today' by default.
    #[arg(
        long,
        value_name = "number [date]",
        value_parser = number_date_validator,
        conflicts_with_all = ["delete", "list", "add", "tag", "clear"]
    )]
    pub over_as_of: Option<String>,

    /// Indicates when a repeating vacation begins repeating
    ///
    /// If you gain a vacation that repeats at intervals -- if you change jobs, for example, and gain a holiday --
    /// this allows you to indicate when the repetition begins. You must identify the affected vacation by its number
    /// in the enumerated list (see --list). The date is 'today' by default. If you add a new repeating vacation, it
    /// will by default become effective immediately. This option is chiefly useful when adding a repeating vacation
    /// retroactively.
    #[arg(
        long,
        value_name = "number [date]",
        value_parser = number_date_validator,
        conflicts_with_all = ["delete", "list", "add", "tag", "clear"]
    )]
    pub effective_as_of: Option<String>,

    /// Deletes a particular vacation record
    ///
    /// If you wish to delete a single vacation record altogether, use --delete. You must identify the affected
    /// vacation by its number in the enumerated list (see --list).
    #[arg(
        long,
        short = 'd',
        value_name = "number",
        value_parser = valid_index,
        conflicts_with_all = ["over_as_of", "list", "add", "tag", "clear"]
    )]
    pub delete: Vec<String>,

    /// Deletes all vacation records
    #[arg(long, conflicts_with_all = ["over_as_of", "list", "add", "tag", "delete"])]
    pub clear: bool,

    /// some phrase identifying the vacation
    ///
    /// A description of the vacation period. This is required if you are creating a new vacation record.
    #[arg(value_name = "description", num_args = 0..)]
    pub description: Vec<String>,
}

pub fn run(directory: Option<&str>, args: &Args) {
    let conf = Configuration::read(None, directory);
    let mut controller = VacationController::read(None, conf.directory());
    if args.list {
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
                let mut row = vec![(i + 1).to_string()];
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
            table.columns[0]
                .set_alignment(Alignment::Right)
                .set_left_margin(0);
            table.columns[1].set_priority(1);
            table.columns[2].set_priority(2);
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
                            print!("{}", style.paint("header", contents));
                        } else {
                            match cell_num {
                                0 => print!("{}", style.paint("header", contents)),
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
    } else if let Some(value) = &args.over_as_of {
        let captures = over_as_of_rx().captures(value).unwrap();
        let index = captures[1].parse::<usize>().unwrap();
        let date = captures.get(2).map(|m| m.as_str()).unwrap_or("today");
        let range = parse(date, conf.two_timer_config()).unwrap();
        let date = range.start;
        match controller.set_over_as_of(index, &date) {
            Ok(s) => success(s, &conf),
            Err(s) => fatal(s, &conf),
        }
    } else if let Some(value) = &args.effective_as_of {
        let captures = over_as_of_rx().captures(value).unwrap();
        let index = captures[1].parse::<usize>().unwrap();
        let date = captures.get(2).map(|m| m.as_str()).unwrap_or("today");
        let range = parse(date, conf.two_timer_config()).unwrap();
        let date = range.start;
        match controller.set_effective_as_of(index, &date) {
            Ok(s) => success(s, &conf),
            Err(s) => fatal(s, &conf),
        }
    } else if !args.delete.is_empty() || args.clear {
        let mut rows = if args.clear {
            (1..=controller.vacations.len()).collect()
        } else {
            let mut rows: Vec<usize> = args
                .delete
                .iter()
                .map(|s| s.parse::<usize>().unwrap())
                .collect();
            rows.sort_unstable();
            rows.dedup();
            let mut problems: Vec<usize> = rows
                .iter()
                .filter(|&v| *v == 0 || *v > controller.vacations.len())
                .copied()
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
    } else if !args.description.is_empty() {
        let description = args.description.join(" ");
        let tags: Vec<String> = args.tag.clone();
        let range = parse(&args.when, conf.two_timer_config()).unwrap();
        let (start, end) = (range.start, range.end);
        let (description, recorded) = controller.record(
            description,
            tags,
            start,
            end,
            Some(&args.kind),
            Some(&args.repeats),
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
        let path = path.unwrap_or_else(|| vacation_path(directory));
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
    // takes a collection of events grouped by day and adds vacation pseudo-events
    pub fn add_vacation_times(
        &self,
        start: &NaiveDateTime,
        end: &NaiveDateTime,
        events: Vec<Event>, // these events *must be grouped by day*
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
        let now = now.unwrap_or_else(|| Local::now().naive_local());
        let end_date = if now.date() < end_date {
            now.date() + Duration::days(1)
        } else {
            end_date
        };
        let sorted_records = self.sorted_vacation_records();
        let mut events = events.into_iter().peekable();
        while date < end_date {
            let mut seconds_worked = 0;
            while events.peek().is_some_and(|e| e.start.date() == date) {
                let e = events.next().unwrap();
                seconds_worked += e.duration(&now) as usize;
                new_events.push(e);
            }
            if conf.is_workday(&date) {
                // only check for vacation time on workdays
                let s = date.and_hms_opt(0, 0, 0).unwrap();
                let e = s + Duration::days(1);
                // make sure we don't fetch in vacation time beyond the end of the last moment
                let e = if &e > end { *end } else { e };
                let start_workday = start_workday(&s, conf);
                let end_workday = start_workday + Duration::hours(conf.day_length as i64);
                // and the end of the workday won't be past the last moment either
                let end_workday = if end_workday > e { e } else { end_workday };
                let delta = (end_workday.and_utc().timestamp()
                    - start_workday.and_utc().timestamp()) as usize;
                let mut unworked_seconds = delta.saturating_sub(seconds_worked);
                // look through the vacation records for anything that overlaps this workday
                for v in &sorted_records {
                    if let Some(event) = v.overlap(&s, &e, unworked_seconds, conf) {
                        let duration = event.duration(&now) as usize;
                        if duration == 0 {
                            // I don't recall why this is safe; events are sorted by length, longest to shortest?
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
                                // can't add any more vacation time
                                break;
                            }
                        }
                    }
                }
            }
            date += Duration::days(1);
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
        sorted.sort();
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
                writeln!(write, "{}", vacation.serialize()).unwrap_or_else(|_| {
                    panic!(
                        "failed to write vacation record to vacation file: {:?}",
                        vacation
                    )
                });
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
            vacation.kind = Type::from_str(k).unwrap();
        }
        if let Some(r) = repetition {
            vacation.repetition = Repetition::from_str(r).unwrap();
            if vacation.repetition != Repetition::Never {
                vacation.effective_as_of = Some(Local::now().naive_local());
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
            self.vacations[index].over_as_of = Some(*date);
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
            self.vacations[index].effective_as_of = Some(*date);
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Type {
    Ordinary,
    Fixed,
    Flex,
}

// to simplify ordering logic: ordinary sorts first, then fixed, then flex
impl Type {
    fn to_num(self) -> &'static str {
        match self {
            Type::Ordinary => "0",
            Type::Flex => "1",
            Type::Fixed => "2",
        }
    }
    fn to_s(self) -> &'static str {
        match self {
            Type::Flex => "flex",
            Type::Fixed => "fixed",
            Type::Ordinary => "",
        }
    }
}

impl FromStr for Type {
    type Err = String;
    fn from_str(t: &str) -> Result<Type, String> {
        match t {
            "flex" => Ok(Type::Flex),
            "fixed" => Ok(Type::Fixed),
            "ordinary" => Ok(Type::Ordinary),
            "0" => Ok(Type::Ordinary),
            "1" => Ok(Type::Flex),
            "2" => Ok(Type::Fixed),
            _ => Err(format!("\"{}\" is not a vacation type", t)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Repetition {
    Monthly,
    Annual,
    Never,
}

// to make it easier to implement ordering: monthly sorts first, then annual, then never
impl Repetition {
    fn to_num(self) -> &'static str {
        match self {
            Repetition::Annual => "1",
            Repetition::Monthly => "2",
            Repetition::Never => "0",
        }
    }
    fn to_s(self) -> &'static str {
        match self {
            Repetition::Annual => "annual",
            Repetition::Monthly => "monthly",
            Repetition::Never => "",
        }
    }
}

impl FromStr for Repetition {
    type Err = String;
    fn from_str(t: &str) -> Result<Repetition, String> {
        match t {
            "monthly" | "2" => Ok(Repetition::Monthly),
            "annual" | "1" => Ok(Repetition::Annual),
            "never" | "0" => Ok(Repetition::Never),
            _ => Err(format!("\"{}\" is not a repetition period", t)),
        }
    }
}

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
        Some(self.cmp(other))
    }
}

impl Ord for Vacation {
    fn cmp(&self, other: &Self) -> Ordering {
        (
            &self.kind,
            &self.repetition,
            &self.start,
            &self.end,
            &self.description,
            &self.tags,
        )
            .cmp(&(
                &other.kind,
                &other.repetition,
                &other.start,
                &other.end,
                &other.description,
                &other.tags,
            ))
    }
}

impl PartialEq for Vacation {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
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

// description in, escaped and whitespace-normalized, out
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

// split a vacation line into its colon-delimited fields, honoring backslash escapes
fn split_vacation_fields(line: &str) -> Option<Vec<&str>> {
    let mut fields = Vec::new();
    let mut start = 0;
    let mut chars = line.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' => {
                chars.next()?;
            }
            ':' => {
                fields.push(&line[start..i]);
                start = i + 1;
            }
            _ => (),
        }
    }
    fields.push(&line[start..]);
    Some(fields)
}

fn parse_vacation_timestamp(field: &str) -> NaiveDateTime {
    assert!(
        VACATION_TIMESTAMP_RE.is_match(field),
        "encountered unparsable line in vacation log"
    );
    parse_timestamp(field).unwrap()
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
            Type::Fixed => format!("{}", self.end.format("%I:%M %p")),
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
        self.repetition != Repetition::Never
    }

    fn deserialize(line: &str) -> Option<Vacation> {
        // blank lines and comments are allowed (perhaps unwisely)
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            return None;
        }
        let fields = split_vacation_fields(line)
            .unwrap_or_else(|| panic!("encountered unparsable line in vacation log"));
        // the fields are start, end, kind+repetition, tags, description, and optionally
        // effective-as-of and over-as-of timestamps
        if fields.len() != 5 && fields.len() != 7 {
            panic!("encountered unparsable line in vacation log");
        }
        let start = parse_vacation_timestamp(fields[0]);
        let end = parse_vacation_timestamp(fields[1]);
        let mut kind_repetition = fields[2].chars().filter(|c| !c.is_whitespace());
        let kind = kind_repetition
            .next()
            .and_then(|c| Type::from_str(&c.to_string()).ok());
        let repetition = kind_repetition
            .next()
            .and_then(|c| Repetition::from_str(&c.to_string()).ok());
        if kind_repetition.next().is_some() || kind.is_none() || repetition.is_none() {
            panic!("encountered unparsable line in vacation log");
        }
        if !VACATION_TAGS_RE.is_match(fields[3]) || !VACATION_DESCRIPTION_RE.is_match(fields[4]) {
            panic!("encountered unparsable line in vacation log");
        }
        let tags = parse_tags(fields[3]);
        let description = unescape_description(fields[4]);
        let (effective_as_of, over_as_of) = if fields.len() == 7 {
            let effective_as_of = if fields[5].trim().is_empty() {
                None
            } else {
                Some(parse_vacation_timestamp(fields[5]))
            };
            let over_as_of = if fields[6].trim().is_empty() {
                None
            } else {
                Some(parse_vacation_timestamp(fields[6]))
            };
            (effective_as_of, over_as_of)
        } else {
            (None, None)
        };
        Some(Vacation {
            start,
            end,
            tags,
            description,
            kind: kind.unwrap(),
            repetition: repetition.unwrap(),
            effective_as_of,
            over_as_of,
        })
    }

    fn serialize(&self) -> String {
        let mut line = timestamp(&self.start);
        line.push(':');
        line.push_str(&timestamp(&self.end));
        line.push(':');
        line.push_str(self.kind.to_num());
        line.push_str(self.repetition.to_num());
        line.push(':');
        line.push_str(&tags(&self.tags));
        line.push(':');
        line.push_str(&escape_description(&self.description));
        if self.effective_as_of.is_some() || self.over_as_of.is_some() {
            line.push(':');
            if let Some(t) = self.effective_as_of {
                line.push_str(&timestamp(&t));
            }
            line.push(':');
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
                    let mut end_times = [&e, &end_available, end];
                    end_times.sort_unstable();
                    let e = *end_times[0];
                    Some((s, e))
                } else {
                    None
                }
            }
            _ => {
                let maybe_range = match self.repetition {
                    Repetition::Never => Some((self.start, self.end)),
                    Repetition::Annual => {
                        if self.effective_as_of.unwrap_or(*start) > *end
                            || self.over_as_of.unwrap_or(*end) < *start
                        {
                            None
                        } else {
                            let d1 = NaiveDate::from_ymd_opt(
                                start.year(),
                                self.start.month(),
                                self.start.day(),
                            )
                            .unwrap()
                            .and_hms_opt(
                                self.start.hour(),
                                self.start.minute(),
                                self.start.second(),
                            )
                            .unwrap();
                            let d2 = d1 + self.duration();
                            Some((d1, d2))
                        }
                    }
                    Repetition::Monthly => {
                        if self.effective_as_of.unwrap_or(*start) > *end
                            || self.over_as_of.unwrap_or(*end) < *start
                        {
                            None
                        } else {
                            let d1 = NaiveDate::from_ymd_opt(
                                start.year(),
                                start.month(),
                                self.start.day(),
                            )
                            .unwrap()
                            .and_hms_opt(
                                self.start.hour(),
                                self.start.minute(),
                                self.start.second(),
                            )
                            .unwrap();
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
                        let e = if &e > end { *end } else { e };
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
                let duration =
                    (self.end.and_utc().timestamp() - self.start.and_utc().timestamp()) as u32;
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
            *interval_2.0
        } else {
            *interval_1.0
        }; // the greater of the two starts
        let e = if interval_1.1 < interval_2.1 {
            *interval_1.1
        } else {
            *interval_2.1
        }; // the lesser of the two ends
        Some((s, e))
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
    time.date()
        .and_hms_opt(
            conf.beginning_work_day.0 as u32,
            conf.beginning_work_day.1 as u32,
            0,
        )
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::{Done, Event, LogController};

    // if the test panics, this leaves the file in the development directory for examination
    fn test_vacation_path(disambiguator: &str) -> Option<PathBuf> {
        Some(PathBuf::from(format!("test_vacation_{}", disambiguator)))
    }

    // ditto
    fn test_log_path(disambiguator: &str) -> Option<PathBuf> {
        Some(PathBuf::from(format!("test_log_{}", disambiguator)))
    }

    // so we have a known configuraiton
    fn test_configuration_path(disambiguator: &str) -> Option<PathBuf> {
        Some(PathBuf::from(format!(
            "test_configuration_{}",
            disambiguator
        )))
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
        let range = parse(phrase, None)
            .unwrap_or_else(|_| panic!("could not make test time from '{}'", phrase));
        (range.start, range.end)
    }

    fn test_now() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2001, 1, 1)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap()
    }

    // remove test files
    fn cleanup(disambiguator: &str) {
        for path in [
            test_configuration_path(disambiguator).unwrap(),
            test_vacation_path(disambiguator).unwrap(),
            test_log_path(disambiguator).unwrap(),
        ] {
            if path.as_path().exists() {
                std::fs::remove_file(path).expect("failed to remove test file");
            }
        }
    }

    fn add_event(log: &mut LogController, time: &NaiveDateTime, description: &str) {
        let mut event = Event::coin(description.to_owned(), Vec::new());
        event.start = *time;
        log.append_to_log(event, "could not add event");
    }

    fn end_event(log: &mut LogController, time: &NaiveDateTime) {
        log.append_to_log(Done(*time), "could not end event");
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
            *start,
            *end,
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
            Some(now),
            &filter,
        );
        assert_eq!(1, events.len(), "log now has one event");
        assert_eq!(
            conf.day_length * (60.0 * 60.0),
            events[0].duration(&now),
            "vacation lasts one work day"
        );
        assert!(events[0].vacation, "event is marked as vacation");
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
            Some(now),
            &filter,
        );
        assert_eq!(1, events.len(), "log now has one event");
        assert_eq!(
            conf.day_length * (60.0 * 60.0),
            events[0].duration(&now),
            "vacation lasts one work day"
        );
        assert!(events[0].vacation, "event is marked as vacation");
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
            Some(now),
            &filter,
        );
        assert_eq!(1, events.len(), "log now has one event");
        assert_eq!(
            conf.day_length * (60.0 * 60.0),
            events[0].duration(&now),
            "vacation lasts one work day"
        );
        assert!(events[0].vacation, "event is marked as vacation");
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
            Some(now),
            &filter,
        );
        assert_eq!(1, events.len(), "log now has one event");
        assert_eq!(
            conf.day_length * (60.0 * 60.0),
            events[0].duration(&now),
            "vacation lasts one work day"
        );
        assert!(events[0].vacation, "event is marked as vacation");
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
            Some(now),
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
        assert!(events[0].vacation, "event is marked as vacation");
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
                Some(now),
                &filter,
            );
            assert_eq!(1, events.len(), "log now has one event");
            assert_eq!(
                conf.day_length * (60.0 * 60.0),
                events[0].duration(&now),
                "vacation lasts one work day"
            );
            assert!(events[0].vacation, "event is marked as vacation");
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
            Some(now),
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
        assert!(events[0].vacation, "event is marked as vacation");
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
            Some(now),
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
        assert!(events[0].vacation, "event is marked as vacation");
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
    fn serialize_deserialize_round_trip() {
        let start = NaiveDate::from_ymd_opt(2020, 12, 25)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap();
        let end = start + Duration::days(1);
        let v = Vacation {
            description: "Christmas: a day off".to_owned(),
            tags: vec!["family".to_owned(), "religious".to_owned()],
            kind: Type::Ordinary,
            repetition: Repetition::Annual,
            start,
            end,
            effective_as_of: Some(start),
            over_as_of: None,
        };
        let line = v.serialize();
        let parsed = Vacation::deserialize(&line).expect("should deserialize");
        assert_eq!(parsed.start, v.start);
        assert_eq!(parsed.end, v.end);
        assert_eq!(parsed.description, v.description);
        assert_eq!(parsed.tags, v.tags);
        assert_eq!(parsed.kind, v.kind);
        assert_eq!(parsed.repetition, v.repetition);
        assert_eq!(parsed.effective_as_of, v.effective_as_of);
        assert_eq!(parsed.over_as_of, v.over_as_of);
        // blank lines and comments deserialize to None
        assert!(Vacation::deserialize("").is_none());
        assert!(Vacation::deserialize("   ").is_none());
        assert!(Vacation::deserialize("# a comment").is_none());
    }
}
