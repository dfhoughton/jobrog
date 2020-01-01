extern crate chrono;
extern crate clap;
extern crate colonnade;
extern crate pidgin;
extern crate two_timer;

use crate::configure::Configuration;
use crate::log::{parse_tags, parse_timestamp, tags, timestamp, Item, Log};
use crate::util::{base_dir, fatal, remainder, some_nws, Color};
use chrono::{Duration, Local, NaiveDateTime};
use clap::{App, AppSettings, Arg, ArgMatches, SubCommand};
use colonnade::{Alignment, Colonnade};
use pidgin::{Grammar, Matcher};
use std::fs::{copy, File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Lines, Read, Write};
use std::path::PathBuf;
use two_timer::{parsable, parse, Config};

pub fn cli(mast: App<'static, 'static>) -> App<'static, 'static> {
    mast.subcommand(
        SubCommand::with_name("vacation")
            .aliases(&["v", "va", "vac", "vaca", "vacat", "vacati", "vacatio"])
            .about("handle vacation time")
            .after_help(after_help_text())
            .arg(
                Arg::with_name("list")
                .short("l")
                .long("list")
                .help("list known vacation periods")
                .long_help("Just provide an enumerated list of the known vacation periods and do nothing further. This is a useful, probably necessary, precursor to deleting a vacation period.")
                .display_order(1)
            )
            .arg(
                Arg::with_name("when")
                .short("w")
                .long("when")
                .help("vacation period")
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
                .help("add this tag to the event")
                .long_help("A tag is just a short description, like 'religious', or 'family'. Add a tag to a vacation to facilitate filtering during log summaries.")
                .value_name("tag")
                .validator(|v| if some_nws(&v) {Ok(())} else {Err(format!("tag {:?} needs some non-whitespace character", v))})
                .display_order(3)
            )
            .arg(
                Arg::with_name("type")
                .long("type")
                .help("mark the vacation as flex or fixed")
                .long_help("Flex and fixed vacations cannot repeat. They constrain the vacation period to some subportion of a normal workday. See the full --help text for more details.")
                .value_name("type")
                .possible_values(&["fixed", "flex"])
                .display_order(4)
            )
            .arg(
                Arg::with_name("repeats")
                .long("repeats")
                .help("mark the vacation as repeating either annually or monthly")
                .long_help("If you have a vacation that repeats at intervals you may mark it as such. It will be assumed that the repetition can be inferred from either the day of the month (monthly), or the day of the month and the month of the year (annual). Repeating vacations cannot be marked as fixed or flex.")
                .value_name("period")
                .possible_values(&["annual", "monthly"])
                .display_order(5)
            )
            .arg(
                Arg::with_name("effective-as-of")
                .long("effective-as-of")
                .help("indicate the beginning of a repeating vacation")
                .long_help("If you come to have a vacation that repeats at intervals -- if you change jobs, for example, and gain a new holiday -- this allows you to indicate when the repetition begins.")
                .value_name("date")
                .default_value("today")
                .display_order(6)
            )
            .arg(
                Arg::with_name("over-as-of")
                .long("over-as-of")
                .help("indicate the end of a repeating vacation")
                .long_help("If you come to lose a vacation that repeated at intervals -- if you change jobs, for example, and lose a holiday -- this allows you to indicate when the repetition stops. You must identify the affected vacation by its number in the enumerated list (see --list). The date is 'today' by default.")
                .value_name("number [date]")
                .display_order(7)
            )
            .arg(
                Arg::with_name("delete")
                .long("delete")
                .short("d")
                .help("delete a particular vacation record")
                .long_help("If you wish to delete a vacation record altogether, use --delete. You must identify the affected vacation by its number in the enumerated list (see --list).")
                .value_name("number")
                .validator(|v| if v.parse::<usize>().is_ok() { Ok(())} else {Err(format!("could not parse {} as a vacation record index", v))})
                .display_order(8)
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
            .display_order(1)
    )
}

fn after_help_text() -> &'static str {
    "Vactation time is the dark matter of the log. It is not stored in the log and it can be simultaneous with
logged events inasmuch as it occurs on particular days when logged events also occur, but it generally doesn't
have specific start and end times.

Vacation times can be fixed -- with definite start and end times --, flex -- having a flexible extent that just
fills up unused workday hours in a particular day, or neither. The latter category is the default. The extent
of a vacation period on an ordinary vacation day is just as many hours as you would have been expected to work
had it been a regular workday.

In addition to these distinctions, a particular vacation may repeat annually or monthly. Repeated vacations are marked
as in force as of a particular data and, optionally, defunct as of another date. This way you can turn them on and
off and see correct log summaries of earlier periods.

Because the vacation format is so complex it should not be edited by hand but only through the vacation subcommand.
Generally this just means adding and subtracting vacation days. For the latter you will be presented with an
enumerated list of known vacations. You delete them by their number in the list.

If two vacation periods overlap non-repeating periods will be preferred to repeating, narrower periods to wider, and
ordinary > fixed > flex. In any case, a particular vacation moment will only be counted once.

Note, the Rust version of JobLog is adding some features to vacations: on and off times for repeating vacations.
Because of this you will not be able to use the vacation file with the Perl client after you add repeating vacations.
    "
}

pub fn run(matches: &ArgMatches) {
    let mut controller = VacationController::read();
    let conf = Configuration::read();
    if matches.is_present("list") {
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
            row.push(i.to_string());
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
        let color = Color::new(&conf);
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
                        print!("{}", color.heading(contents));
                    } else {
                        match cell_num {
                            2 => print!("{}", color.green(contents)),
                            3 => print!("{}", color.blue(contents)),
                            4 => print!("{}", color.green(contents)),
                            _ => print!("{}", contents),
                        }
                    }
                }
                println!();
            }
        }
        println!();
    } else if matches.is_present("delete") {
        let row = matches.value_of("delete").unwrap();
        let row: usize = row.parse().unwrap();
        match controller.destroy(row) {
            Ok(v) => println!(
                "deleted vacation record for {}, '{}'",
                v.start.format("%F"),
                v.description
            ),
            Err(e) => fatal(e, &conf),
        }
    } else {
        if matches.is_present("description") {
            let description = remainder("description", matches);
        } else {
            fatal(
                "You must provide some decription when creating a vacation record.",
                &conf,
            )
        }
    }
    controller.write();
}

fn vacation_path() -> PathBuf {
    let mut path = base_dir();
    path.push("vacation");
    path
}

fn vacation_path_bak() -> PathBuf {
    let mut path = base_dir();
    path.push("vacation.bak");
    path
}

// basically a namespace for vacation-related functions
struct VacationController {
    vacations: Vec<Vacation>,
    changed: bool,
}

impl VacationController {
    // fetch vacation information in from file
    fn read() -> VacationController {
        if vacation_path().as_path().exists() {
            let file = File::open(vacation_path()).expect("could not open vacation file");
            let reader = BufReader::new(file);
            let vacations = reader
                .lines()
                .map(|l| l.unwrap())
                .filter_map(|l| Vacation::deserialize(&l))
                .collect();
            VacationController {
                vacations,
                changed: false,
            }
        } else {
            VacationController {
                vacations: vec![],
                changed: false,
            }
        }
    }
    // serialize vacation records back to file
    fn write(&self) {
        if !self.changed {
            return;
        }
        if self.vacations.is_empty() {
            if vacation_path().as_path().exists() {
                std::fs::remove_file(vacation_path()).expect("failed to remove vacation file");
            }
        } else {
            // make a backup copy just in case
            copy(vacation_path(), vacation_path_bak())
                .expect("could not make backup of vacation file before saving changes");
            let mut write = BufWriter::new(
                File::create(vacation_path()).expect("could not open vacation file for writing"),
            );
            for vacation in &self.vacations {
                writeln!(write, "{}", vacation.serialize()).expect(&format!(
                    "failed to write vacation record to vacation file: {:?}",
                    vacation
                ));
            }
            std::fs::remove_file(vacation_path_bak())
                .expect("could not remove vacation backup file");
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
    // create a new vacation record
    fn record(
        &mut self,
        description: String,
        tags: Vec<String>,
        start: NaiveDateTime,
        end: NaiveDateTime,
        kind: Option<&str>,
        repetition: Option<&str>,
    ) {
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
        self.vacations.push(vacation);
        self.changed = true;
    }
    fn set_effective_as_of(&mut self, index: usize, date: NaiveDateTime) -> Result<(), String> {
        if index == 0 || self.vacations.len() >= index - 1 {
            return Err(format!("there is no record vacation number {}", index));
        }
        self.vacations[index - 1].effective_as_of = Some(date);
        self.changed = true;
        Ok(())
    }
    fn set_over_as_of(&mut self, index: usize, date: NaiveDateTime) -> Result<(), String> {
        if index == 0 || self.vacations.len() >= index - 1 {
            return Err(format!("there is no record vacation number {}", index));
        }
        self.vacations[index - 1].over_as_of = Some(date);
        self.changed = true;
        Ok(())
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
}

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
                s.push(' ');
                was_whitespace = Some(true);
            }
        } else {
            was_whitespace = Some(false);
            s.push(c);
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
}
