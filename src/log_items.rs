// stuff for
extern crate chrono;
extern crate larry;
extern crate pidgin;
extern crate regex;
use crate::util::log_path;
use chrono::{Datelike, Duration, Local, NaiveDate, NaiveDateTime, Timelike};
use larry::{Larry, Lerror};
use pidgin::{Grammar, Matcher};
use regex::Regex;
use std::fs::File;
use std::path::PathBuf;

lazy_static! {
    // making this public is useful for testing, but best to keep it hidden to
    // limit complexity and commitment
    #[doc(hidden)]
    // this is a stripped-down version of GRAMMAR that just containst the most commonly used expressions
    pub static ref LOG_LINES: Grammar = grammar!{

        TOP -> r(r"\A") <log_item> r(r"\z")

        log_item         -> <timestamped_item> | <blank> | <comment>
        blank            -> r(r"\s*")
        comment          -> r(r"\s*#.*")
        timestamped_item -> <timestamp> <ti_continuation>
        timestamp        -> r(r"\s*[1-9]\d{3}(?:\s+[1-9]\d?){2}(?:\s+(?:0|[1-9]\d?)){3}\s*")
        ti_continuation  -> <taggable> | <done>
        taggable         -> <tag_separator> <tags> (":") <description>
        tag_separator    -> <event> | <note>
        event            -> (":")
        note             -> ("<NOTE>")
        done             -> r(r":DONE\s*")
        tags             -> r(r"(?:\\.|[^:<\\])*") // colons, spaces, and < must be escaped, so the escape character \ must also be escaped
        description      -> r(r".*")
    };
    pub static ref MATCHER: Matcher = LOG_LINES.matcher().unwrap();
}

// parses a log line into an appropriate data structure preserving the line offset
pub fn parse_line(line: &str, offset: usize) -> Item {
    if let Some(ast) = MATCHER.parse(line) {
        if let Some(timestamp) = ast.name("timestamp") {
            let timestamp = parse_timestamp(timestamp.as_str());
            if ast.has("done") {
                Item::Done(Done(timestamp), offset)
            } else {
                let tags = parse_tags(ast.name("tags").unwrap().as_str());
                let description = ast.name("description").unwrap().as_str();
                if ast.has("event") {
                    Item::Event(
                        Event {
                            start: timestamp,
                            end: None,
                            description: description.to_owned(),
                            tags: tags,
                        },
                        offset,
                    )
                } else {
                    Item::Note(
                        Note {
                            time: timestamp,
                            description: description.to_owned(),
                            tags: tags,
                        },
                        offset,
                    )
                }
            }
        } else if ast.has("blank") {
            Item::Blank(offset)
        } else {
            Item::Comment(offset)
        }
    } else {
        Item::Error(String::from("unexpected line format"), offset)
    }
}

pub struct LogReader {
    larry: Larry,
}

impl LogReader {
    pub fn new(log: Option<PathBuf>) -> Result<LogReader, std::io::Error> {
        let log = log.unwrap_or(log_path());
        Larry::new(log.as_path()).and_then(|log| Ok(LogReader { larry: log }))
    }
    // find best line offset for a timestamp in a log file
    // best is the earliest instance of the line with the timestamp or, barring that, the earliest
    // timestamped line immediately before the timestamp
    pub fn find_line(&mut self, time: &NaiveDateTime) -> Option<Item> {
        if let Some(start) = self.get_after(0, time) {
            let end = self.get_before(self.larry.len() - 1);
            Some(self.narrow_in(time, start, end))
        } else {
            None
        }
    }
    fn narrow_in(&mut self, time: &NaiveDateTime, start: Item, end: Item) -> Item {
        let start = self.advance_to_first(start);
        let (t1, mut o1) = start.time().unwrap();
        if t1 == time {
            return start;
        }
        let (t2, o2) = end.time().unwrap();
        if t2 == time {
            return end;
        } else if t1 == t2 {
            return start;
        }
        // we want to find an intermediate index at this point but are concerned not to
        // get into an infinite loop where we estimate an intermediate index, loop for the timed
        // event at or before that index, and return to our start item
        let mut o3 = self.estimate(time, t1, o1, t2, o2);
        if o3 == o1 {
            return start;
        }
        loop {
            let next = self.get_before(o3);
            if next == start {
                // the time at o3 == the time at o1, so ...
                o1 = o3;
                o3 = self.estimate(time, t1, o1, t2, o2);
                if o3 == o1 {
                    return start;
                }
            } else {
                if let Some((t, _)) = next.time() {
                    if t == time {
                        return next;
                    } else if t < time {
                        return self.narrow_in(time, next, end);
                    } else {
                        return self.narrow_in(time, start, next);
                    }
                } else {
                    unreachable!()
                }
            }
        }
    }
    // given a time and two line and time offsets that bracket it, estimate the line
    // offset to find the time at
    fn estimate(
        &self,
        time: &NaiveDateTime,
        t1: &NaiveDateTime,
        o1: usize,
        t2: &NaiveDateTime,
        o2: usize,
    ) -> usize {
        let line_delta = o2 - o1;
        match line_delta {
            1 => o1,
            2 => o1 + 1,
            _ => {
                if line_delta <= 16 {
                    // this is an arbitrary threshold that could be optimized
                    // switch to binary search
                    return o1 + line_delta / 2;
                }
                let time_delta = t2.timestamp() - t1.timestamp();
                let lines_per_second = (line_delta as f64) / (time_delta as f64);
                let seconds = (time.timestamp() - t1.timestamp()) as f64;
                let additional_lines = (lines_per_second * seconds) as usize;
                // we've already looked at the end offsets, so make sure we don't hit those again
                let additional_lines = if additional_lines == 0 {
                    1
                } else if additional_lines == line_delta {
                    line_delta - 1
                } else {
                    additional_lines
                };
                o1 + additional_lines
            }
        }
    }
    // get an index-item pair at or before the given time starting at the given index
    // this moves forward from earlier lines to later
    fn get_after(&mut self, i: usize, time: &NaiveDateTime) -> Option<Item> {
        for i in i..self.larry.len() {
            let item = parse_line(self.larry.get(i).unwrap(), i);
            let t = item.time();
            if let Some((t, _)) = t {
                if t > time {
                    return None;
                } else {
                    return Some(item);
                }
            }
        }
        None
    }
    // get the first index-item pair at
    // this moves in reverse from later lines to earlier
    fn get_before(&mut self, i: usize) -> Item {
        let mut i = i;
        loop {
            let item = parse_line(self.larry.get(i).unwrap(), i);
            match item {
                Item::Done(_, _) | Item::Note(_, _) | Item::Event(_, _) => return item,
                _ => (),
            }
            if i == 0 {
                break;
            }
            i -= 1;
        }
        unreachable!()
    }
    // starting at the location of item, advance the pointer to the first item in the log with item's time
    // most often timestamps will be unique, but we do this just in case
    fn advance_to_first(&mut self, item: Item) -> Item {
        let (time, mut i) = item.time().unwrap();
        let mut ptr = item.clone();
        while i > 0 {
            i -= 1;
            let next = parse_line(self.larry.get(i).unwrap(), i);
            let next_time = next.time();
            if let Some((next_time, _)) = next_time {
                if time == next_time {
                    ptr = next;
                } else if time > next_time {
                    return ptr;
                }
            }
        }
        ptr
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::Rng;
    use std::io::prelude::*;
    use std::io::LineWriter;
    use std::ops::AddAssign;
    use std::str::FromStr;

    fn random_tag() -> String {
        let choices = ["foo", "bar", "baz", "plugh", "work", "play", "tedium"];
        choices[rand::thread_rng().gen_range(0, choices.len())].to_owned()
    }

    fn random_words(min: usize, max: usize) -> Vec<String> {
        (0..(rand::thread_rng().gen_range(min, max + 1)))
            .map(|_| random_tag())
            .collect()
    }

    fn random_tags() -> Vec<String> {
        let mut tags = random_words(0, 5);
        tags.dedup();
        tags
    }

    fn random_text() -> String {
        let mut words = random_words(5, 15);
        let mut word = words.remove(0);
        for w in words {
            word += " ";
            word.push_str(&w);
        }
        word
    }

    fn random_line(time: &mut NaiveDateTime, open_event: bool, offset: usize) -> Item {
        let n = rand::thread_rng().gen_range(0, 100);
        if n < 4 {
            // blank line
            Item::Blank(offset)
        } else if n < 10 {
            // comment
            let mut comment = String::from("# ");
            comment.push_str(&random_text());
            Item::Comment(offset)
        } else if n < 11 {
            // error
            Item::Error(random_text(), offset)
        } else if n < 20 {
            // note
            time.add_assign(Duration::seconds(rand::thread_rng().gen_range(1, 1000)));
            Item::Note(
                Note {
                    time: time.clone(),
                    description: random_text(),
                    tags: random_tags(),
                },
                offset,
            )
        } else {
            time.add_assign(Duration::seconds(rand::thread_rng().gen_range(1, 1000)));
            if open_event && n < 30 {
                Item::Done(Done(time.clone()), offset)
            } else {
                Item::Event(
                    Event {
                        start: time.clone(),
                        end: None,
                        tags: random_tags(),
                        description: random_text(),
                    },
                    offset,
                )
            }
        }
    }

    fn random_log(length: usize) -> (Vec<Item>, String) {
        let mut initial_time = NaiveDate::from_ymd(2019, 12, 22).and_hms(9, 39, 30);
        let mut items: Vec<Item> = Vec::with_capacity(length);
        let mut open_event = false;
        let path = String::from("test.log");
        let file = File::create(path.clone()).unwrap();
        let mut file = LineWriter::new(file);
        for offset in 0..length {
            let item = random_line(&mut initial_time, open_event, offset);
            open_event = match item {
                Item::Done(_, _) => false,
                Item::Event(_, _) => true,
                _ => open_event,
            };
            let line = match &item {
                Item::Event(e, _) => e.to_line(),
                Item::Note(n, _) => n.to_line(),
                Item::Done(d, _) => d.to_line(),
                Item::Blank(_) => String::new(),
                Item::Comment(_) => {
                    let mut s = String::from("# ");
                    s.push_str(&random_text());
                    s
                }
                Item::Error(s, _) => s.clone(),
            };
            file.write_all(line.as_ref()).unwrap();
            file.write_all("\n".as_ref()).unwrap();
            if item.has_time() {
                items.push(item);
            }
        }
        (items, path)
    }

    fn test_log(length: usize) {
        let (items, path) = random_log(length);
        if items.is_empty() {
            println!("empty file; skipping...");
        } else {
            let mut log_reader = LogReader::new(Some(PathBuf::from_str(&path).unwrap())).unwrap();
            let mut last_timed_item: Option<Item> = None;
            for item in items {
                let (time, offset) = item.time().unwrap();
                let found_item = log_reader.find_line(time);
                if let Some(found_item) = found_item {
                    assert_eq!(offset, found_item.offset());
                    if let Some(lti) = last_timed_item.clone() {
                        let (t1, _) = lti.time().unwrap();
                        let (t2, _) = found_item.time().unwrap();
                        let d = *t2 - *t1;
                        if d.num_seconds() > 1 {
                            let intermediate_time = t1
                                .checked_add_signed(Duration::seconds(d.num_seconds() / 2))
                                .unwrap();
                            let should_be_found = log_reader.find_line(&intermediate_time);
                            if let Some(should_be_found) = should_be_found {
                                assert_eq!(last_timed_item.unwrap(), should_be_found);
                            } else {
                                assert!(false, format!("failed to revert to found time when looking for missing intermediate time {}", intermediate_time));
                            }
                        }
                    }
                    last_timed_item = Some(found_item);
                } else {
                    assert!(false, format!("could not find item at offset {}", offset));
                }
            }
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn test_10_small_files() {
        for _ in 0..10 {
            test_log(100);
        }
    }

    #[test]
    fn test_large_file() {
        test_log(10000);
    }

    // #[test]
    // fn test_huge_file() {
    //     test_log(100000);
    // }

    #[test]
    fn test_event() {
        match parse_line("2019 12 1 16 3 30::an event with no tags", 0) {
            Item::Event(
                Event {
                    start,
                    end: _,
                    tags,
                    description,
                },
                _,
            ) => {
                assert_eq!(2019, start.year());
                assert_eq!(12, start.month());
                assert_eq!(1, start.day());
                assert_eq!(16, start.hour());
                assert_eq!(3, start.minute());
                assert_eq!(30, start.second());
                assert!(tags.is_empty(), "there are no tags");
                assert_eq!(
                    "an event with no tags", &description,
                    "got correct description"
                )
            }
            _ => assert!(false, "failed to parse an event line"),
        };
        match parse_line("2019 12 1 16 3 30:foo bar:an event with some tags", 0) {
            Item::Event(
                Event {
                    start,
                    end: _,
                    tags,
                    description,
                },
                _,
            ) => {
                assert_eq!(2019, start.year());
                assert_eq!(12, start.month());
                assert_eq!(1, start.day());
                assert_eq!(16, start.hour());
                assert_eq!(3, start.minute());
                assert_eq!(30, start.second());
                assert_eq!(2, tags.len(), "there are some tags");
                for t in vec!["foo", "bar"] {
                    assert!(tags.contains(&t.to_owned()));
                }
                assert_eq!(
                    "an event with some tags", &description,
                    "got correct description"
                )
            }
            _ => assert!(false, "failed to parse an event line"),
        };
        //regression?
        match parse_line("2019 12 22 12 49 24:foo:plugh baz baz foo play play work baz tedium foo tedium foo work bar", 0) {
            Item::Event(
                Event {
                    start,
                    end: _,
                    tags,
                    description,
                },
                _,
            ) => {
                assert_eq!(2019, start.year());
                assert_eq!(12, start.month());
                assert_eq!(22, start.day());
                assert_eq!(12, start.hour());
                assert_eq!(49, start.minute());
                assert_eq!(24, start.second());
                assert_eq!(1, tags.len(), "there are some tags");
                for t in vec!["foo"] {
                    assert!(tags.contains(&t.to_owned()));
                }
                assert_eq!(
                    "plugh baz baz foo play play work baz tedium foo tedium foo work bar", &description,
                    "got correct description"
                )
            }
            _ => assert!(false, "failed to parse an event line"),
        };
    }

    #[test]
    fn test_note() {
        match parse_line("2019 12 1 16 3 30<NOTE>:a note with no tags", 0) {
            Item::Note(
                Note {
                    time,
                    tags,
                    description,
                },
                _,
            ) => {
                assert_eq!(2019, time.year());
                assert_eq!(12, time.month());
                assert_eq!(1, time.day());
                assert_eq!(16, time.hour());
                assert_eq!(3, time.minute());
                assert_eq!(30, time.second());
                assert!(tags.is_empty(), "there are no tags");
                assert_eq!(
                    "a note with no tags", &description,
                    "got correct description"
                )
            }
            _ => assert!(false, "failed to parse a NOTE line"),
        };
        match parse_line("2019 12 1 16 3 30<NOTE>foo bar:a short note", 0) {
            Item::Note(
                Note {
                    time,
                    tags,
                    description,
                },
                _,
            ) => {
                assert_eq!(2019, time.year());
                assert_eq!(12, time.month());
                assert_eq!(1, time.day());
                assert_eq!(16, time.hour());
                assert_eq!(3, time.minute());
                assert_eq!(30, time.second());
                assert_eq!(tags.len(), 2, "there are two tags");
                for t in vec!["foo", "bar"] {
                    assert!(tags.contains(&t.to_owned()));
                }
                assert_eq!("a short note", &description, "got correct description")
            }
            _ => assert!(false, "failed to parse a NOTE line"),
        };
        match parse_line(
            r"2019 12 1 16 3 30<NOTE>f\:oo b\<ar b\ az pl\\ugh:a short note",
            0,
        ) {
            Item::Note(
                Note {
                    time,
                    tags,
                    description,
                },
                _,
            ) => {
                assert_eq!(2019, time.year());
                assert_eq!(12, time.month());
                assert_eq!(1, time.day());
                assert_eq!(16, time.hour());
                assert_eq!(3, time.minute());
                assert_eq!(30, time.second());
                assert_eq!(tags.len(), 4, "there are two tags");
                for t in vec!["f:oo", "b<ar", "b az", r"pl\ugh"] {
                    assert!(tags.contains(&t.to_owned()), "escaping worked");
                }
                assert_eq!("a short note", &description, "got correct description")
            }
            _ => assert!(false, "failed to parse a NOTE line"),
        };
        match parse_line("2019 12 1 16 3 30<NOTE>foo bar bar:a short note", 0) {
            Item::Note(
                Note {
                    time,
                    tags,
                    description,
                },
                _,
            ) => {
                assert_eq!(2019, time.year());
                assert_eq!(12, time.month());
                assert_eq!(1, time.day());
                assert_eq!(16, time.hour());
                assert_eq!(3, time.minute());
                assert_eq!(30, time.second());
                assert_eq!(tags.len(), 2, "there are two tags");
                for t in vec!["foo", "bar"] {
                    assert!(tags.contains(&t.to_owned()));
                }
                assert_eq!("a short note", &description, "got correct description")
            }
            _ => assert!(false, "failed to parse a NOTE line"),
        };
        match parse_line("2019 12 1 16 3 30<NOTE> foo  bar :a short note", 0) {
            Item::Note(
                Note {
                    time,
                    tags,
                    description,
                },
                _,
            ) => {
                assert_eq!(2019, time.year());
                assert_eq!(12, time.month());
                assert_eq!(1, time.day());
                assert_eq!(16, time.hour());
                assert_eq!(3, time.minute());
                assert_eq!(30, time.second());
                assert_eq!(tags.len(), 2, "there are two tags");
                for t in vec!["foo", "bar"] {
                    assert!(tags.contains(&t.to_owned()));
                }
                assert_eq!("a short note", &description, "got correct description")
            }
            _ => assert!(false, "failed to parse a NOTE line"),
        };
        //regression
        match parse_line("2019 12 22  9 59 34<NOTE>foo play tedium work:baz tedium baz tedium foo plugh bar foo bar play plugh foo baz play baz tedium work work play play bar", 0) {
            Item::Note(
                Note {
                    time,
                    tags,
                    description,
                },
                _,
            ) => {
                assert_eq!(2019, time.year());
                assert_eq!(12, time.month());
                assert_eq!(22, time.day());
                assert_eq!(9, time.hour());
                assert_eq!(59, time.minute());
                assert_eq!(34, time.second());
                assert_eq!(tags.len(), 4, "there are three tags");
                for t in vec!["foo", "play", "tedium", "work"] {
                    assert!(tags.contains(&t.to_owned()));
                }
                assert_eq!("baz tedium baz tedium foo plugh bar foo bar play plugh foo baz play baz tedium work work play play bar", &description, "got correct description")
            }
            _ => assert!(false, "failed to parse a NOTE line"),
        };
        //regression
        match parse_line(
            "2019 12 22 12  8  0<NOTE>bar:tedium plugh baz play tedium baz play work",
            0,
        ) {
            Item::Note(
                Note {
                    time,
                    tags,
                    description,
                },
                _,
            ) => {
                assert_eq!(2019, time.year());
                assert_eq!(12, time.month());
                assert_eq!(22, time.day());
                assert_eq!(12, time.hour());
                assert_eq!(8, time.minute());
                assert_eq!(0, time.second());
                assert_eq!(tags.len(), 1, "there is one tag");
                for t in vec!["bar"] {
                    assert!(tags.contains(&t.to_owned()));
                }
                assert_eq!(
                    "tedium plugh baz play tedium baz play work", &description,
                    "got correct description"
                )
            }
            _ => assert!(false, "failed to parse a NOTE line"),
        };
    }

    #[test]
    fn test_done() {
        match parse_line("2019 12 1 16 3 30:DONE", 0) {
            Item::Done(Done(time), _) => {
                assert_eq!(2019, time.year());
                assert_eq!(12, time.month());
                assert_eq!(1, time.day());
                assert_eq!(16, time.hour());
                assert_eq!(3, time.minute());
                assert_eq!(30, time.second());
            }
            _ => assert!(false, "failed to parse a DONE line"),
        };
        match parse_line(" 2019  12   1  16  3  30 :DONE", 0) {
            Item::Done(Done(time), _) => {
                assert_eq!(2019, time.year(), "space doesn't matter");
                assert_eq!(12, time.month(), "space doesn't matter");
                assert_eq!(1, time.day(), "space doesn't matter");
                assert_eq!(16, time.hour(), "space doesn't matter");
                assert_eq!(3, time.minute(), "space doesn't matter");
                assert_eq!(30, time.second(), "space doesn't matter");
            }
            _ => assert!(false, "failed to parse a DONE line"),
        };
    }

    #[test]
    fn test_comment() {
        let success = match parse_line("#foo", 0) {
            Item::Comment(_) => true,
            _ => false,
        };
        assert!(success, "recognized '#foo' as a comment line");
        let success = match parse_line("   #foo", 0) {
            Item::Comment(_) => true,
            _ => false,
        };
        assert!(success, "comments can have leading space");
    }

    #[test]
    fn test_error() {
        let success = match parse_line("foo", 0) {
            Item::Error(_, _) => true,
            _ => false,
        };
        assert!(success, "recognized 'foo' as a malformed log line");
    }

    #[test]
    fn test_blank() {
        let success = match parse_line("", 0) {
            Item::Blank(_) => true,
            _ => false,
        };
        assert!(success, "recognized an empty line as a blank");
        let success = match parse_line("     ", 0) {
            Item::Blank(_) => true,
            _ => false,
        };
        assert!(success, "recognized a whitespace line as a blank");
    }
}

// everything you could find in a stream of lines from a log
#[derive(Debug, Clone)]
pub enum Item {
    Event(Event, usize),
    Note(Note, usize),
    Done(Done, usize),
    Blank(usize),
    Comment(usize),
    Error(String, usize),
}

impl Item {
    fn time(&self) -> Option<(&NaiveDateTime, usize)> {
        match self {
            Item::Event(e, offset) => Some((&e.start, *offset)),
            Item::Note(n, offset) => Some((&n.time, *offset)),
            Item::Done(d, offset) => Some((&d.0, *offset)),
            _ => None,
        }
    }
    fn has_time(&self) -> bool {
        match self {
            Item::Event(_, _) | Item::Note(_, _) | Item::Done(_, _) => true,
            _ => false,
        }
    }
    fn offset(&self) -> usize {
        match self {
            Item::Event(_, i) => *i,
            Item::Note(_, i) => *i,
            Item::Done(_, i) => *i,
            Item::Blank(i) => *i,
            Item::Comment(i) => *i,
            Item::Error(_, i) => *i,
        }
    }
}

impl PartialEq for Item {
    fn eq(&self, other: &Item) -> bool {
        self.offset() == other.offset()
    }
}

impl PartialOrd for Item {
    fn partial_cmp(&self, other: &Item) -> Option<std::cmp::Ordering> {
        self.offset().partial_cmp(&other.offset())
    }
}

fn parse_timestamp(timestamp: &str) -> NaiveDateTime {
    lazy_static! {
        static ref RE: Regex = Regex::new(r"\d+").unwrap();
    }
    let numbers: Vec<_> = RE.find_iter(timestamp).map(|m| m.as_str()).collect();
    // at this point the log lines grammar ensures all the parsing will be fine
    let year = numbers[0].parse::<i32>().unwrap();
    let month = numbers[1].parse::<u32>().unwrap();
    let day = numbers[2].parse::<u32>().unwrap();
    let hour = numbers[3].parse::<u32>().unwrap();
    let minute = numbers[4].parse::<u32>().unwrap();
    let second = numbers[5].parse::<u32>().unwrap();
    NaiveDate::from_ymd(year, month, day).and_hms(hour, minute, second)
}

fn timestamp(ts: &NaiveDateTime) -> String {
    format!(
        "{} {:>2} {:>2} {:>2} {:>2} {:>2}",
        ts.year(),
        ts.month(),
        ts.day(),
        ts.hour(),
        ts.minute(),
        ts.second()
    )
}

// converts a tag string in the log into a deduped, unescaped set of tags
fn parse_tags(tags: &str) -> Vec<String> {
    let mut parsed = vec![];
    let mut escaped = false;
    let mut current = String::with_capacity(tags.len());
    for c in tags.chars() {
        if c == '\\' {
            if escaped {
                current.push(c);
            } else {
                escaped = true;
            }
        } else if c == ' ' {
            // we expect tags to be normalized at this point so all whitespaces is ' '
            if escaped {
                current.push(c);
            } else {
                if current.len() > 0 && !parsed.contains(&current) {
                    parsed.push(current.clone());
                }
                current.clear();
            }
            escaped = false;
        } else {
            current.push(c);
            escaped = false;
        }
    }
    if current.len() > 0 && !parsed.contains(&current) {
        parsed.push(current);
    }
    parsed
}

// convert tags back into a part of a log string
fn tags(tags: &Vec<String>) -> String {
    let mut v = tags.clone();
    v.sort_unstable();
    v.dedup(); // there may still be duplicates after we normalize whitespace below; oh, well
    let mut s = String::new();
    for (i, tag) in v.iter().enumerate() {
        if i > 0 {
            s.push(' ');
        }
        for c in tag.chars() {
            match c {
                ':' | '\\' | '<' => s.push('\\'),
                _ => (),
            }
            s.push(if c.is_whitespace() { ' ' } else { c }); // normalize whitespace
        }
    }
    s
}

#[derive(Debug, Clone)]
pub struct Event {
    pub start: NaiveDateTime,
    pub end: Option<NaiveDateTime>,
    pub description: String,
    pub tags: Vec<String>,
}

impl Event {
    pub fn coin(description: String, tags: Vec<String>) -> Event {
        Event {
            start: Local::now().naive_local(),
            end: None,
            description: description,
            tags: tags,
        }
    }
}

impl Searchable for Event {
    fn has_tag(&self, tag: &str) -> bool {
        self.tags.iter().any(|s| s == tag) // linear search is most likely fastest
    }
    fn search(&self, rx: &Regex) -> bool {
        rx.is_match(&self.description)
    }
}

#[derive(Debug, Clone)]
pub struct Note {
    pub time: NaiveDateTime,
    pub description: String,
    pub tags: Vec<String>,
}

impl Note {
    pub fn coin(description: String, tags: Vec<String>) -> Note {
        Note {
            time: Local::now().naive_local(),
            description: description,
            tags: tags,
        }
    }
}

impl Searchable for Note {
    fn has_tag(&self, tag: &str) -> bool {
        self.tags.iter().any(|s| s == tag) // linear search is most likely fastest
    }
    fn search(&self, rx: &Regex) -> bool {
        rx.is_match(&self.description)
    }
}

#[derive(Debug, Clone)]
pub struct Done(NaiveDateTime);

impl Done {
    pub fn coin() -> Done {
        Done(Local::now().naive_local())
    }
}

pub enum Direction {
    Forward,
    Back,
}

trait LogLine {
    fn to_line(&self) -> String;
}

impl LogLine for Done {
    fn to_line(&self) -> String {
        let mut ts = timestamp(&self.0);
        ts += ":DONE";
        ts
    }
}

impl LogLine for Note {
    fn to_line(&self) -> String {
        let mut ts = timestamp(&self.time);
        ts += "<NOTE>";
        let tags = tags(&self.tags);
        ts += &tags;
        ts.push(':');
        ts += &self.description;
        ts
    }
}

impl LogLine for Event {
    fn to_line(&self) -> String {
        let mut ts = timestamp(&self.start);
        ts.push(':');
        let tags = tags(&self.tags);
        ts += &tags;
        ts.push(':');
        ts += &self.description;
        ts
    }
}

trait Searchable {
    fn has_tag(&self, tag: &str) -> bool;
    fn search(&self, rx: &Regex) -> bool;
}
