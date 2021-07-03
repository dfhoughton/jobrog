// stuff for
extern crate chrono;
extern crate clap;
extern crate larry;
extern crate pidgin;
extern crate regex;
extern crate serde_json;
use crate::configure::Configuration;
use crate::util::{duration_string, log_path};
use chrono::{Datelike, Duration, Local, NaiveDate, NaiveDateTime, Timelike};
use clap::ArgMatches;
use larry::Larry;
use pidgin::{Grammar, Matcher};
use regex::{Regex, RegexSet};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Lines, Write};
use std::path::PathBuf;

lazy_static! {
    // making this public is useful for testing, but best to keep it hidden to
    // limit complexity and commitment
    #[doc(hidden)]
    pub static ref LOG_LINES: Grammar = grammar!{

        TOP -> r(r"\A") <log_item> r(r"\z")

        log_item         -> <timestamped_item> | <blank> | <comment>
        blank            -> r(r"\s*")
        comment          -> r(r"\s*#.*")
        timestamped_item -> <timestamp> <ti_continuation>
        timestamp        -> r(r"\s*[1-9]\d{3}(?:\s+\d{1,2}){5}\s*")
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
            match parse_timestamp(timestamp.as_str()) {
                Err(s) => Item::Error(s, offset),
                Ok(timestamp) => {
                    if ast.has("done") {
                        Item::Done(Done(timestamp), offset)
                    } else {
                        let tags = parse_tags(ast.name("tags").unwrap().as_str());
                        let description = ast.name("description").unwrap().as_str();
                        if ast.has("event") {
                            Item::Event(
                                Event {
                                    start: timestamp,
                                    start_overlap: false,
                                    end: None,
                                    end_overlap: false,
                                    description: description.to_owned(),
                                    tags: tags,
                                    vacation: false,
                                    vacation_type: None,
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

pub struct LogController {
    pub larry: Larry,
    pub path: String,
}

impl LogController {
    pub fn new(
        log: Option<PathBuf>,
        conf: &Configuration,
    ) -> Result<LogController, std::io::Error> {
        let log = log.unwrap_or(log_path(conf.directory()));
        let path = log.as_path().to_str();
        Larry::new(log.as_path()).and_then(|log| {
            Ok(LogController {
                larry: log,
                path: path.unwrap().to_owned(),
            })
        })
    }
    // find best line offset for a timestamp in a log file
    // best is the earliest instance of the line with the timestamp or, barring that, the earliest
    // timestamped line immediately before the timestamp
    pub fn find_line(&mut self, time: &NaiveDateTime) -> Option<Item> {
        if let Some(start) = self.get_after(0) {
            let end = self.get_before(self.larry.len() - 1);
            let time = start.advance(time);
            Some(self.narrow_in(&time, start, end))
        } else {
            None
        }
    }
    pub fn first_timestamp(&self) -> Option<NaiveDateTime> {
        let item = ItemsAfter::new(0, &self.path).find(|i| i.has_time());
        item.and_then(|i| Some(i.time().unwrap().0.clone()))
    }
    pub fn last_timestamp(&mut self) -> Option<NaiveDateTime> {
        let item = ItemsBefore::new(self.larry.len(), self).find(|i| i.has_time());
        item.and_then(|i| Some(i.time().unwrap().0.clone()))
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
    fn get_after(&mut self, i: usize) -> Option<Item> {
        for i in i..self.larry.len() {
            let item = parse_line(self.larry.get(i).unwrap(), i);
            let t = item.time();
            if let Some((_, _)) = t {
                return Some(item);
            }
        }
        None
    }
    // just returns iterator from a given offset forward -- needed for validation
    pub fn items_before(&mut self, offset: usize) -> ItemsBefore {
        ItemsBefore::new(offset, self)
    }
    // get the first index-item pair at
    // this moves in reverse from later lines to earlier
    fn get_before(&mut self, i: usize) -> Item {
        let mut i = i;
        if i >= self.larry.len() {
            i = self.larry.len() - 1;
        }
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
    pub fn events_from_the_end(&mut self) -> EventsBefore {
        EventsBefore::new(self.larry.len(), self)
    }
    pub fn notes_from_the_end(&mut self) -> NotesBefore {
        NotesBefore::new(self.larry.len(), self)
    }
    pub fn events_from_the_beginning(self) -> EventsAfter {
        EventsAfter::new(0, &self)
    }
    pub fn notes_from_the_beginning(self) -> NotesAfter {
        NotesAfter::new(0, &self)
    }
    pub fn events_in_range(&mut self, start: &NaiveDateTime, end: &NaiveDateTime) -> Vec<Event> {
        let mut ret = vec![];
        if let Some(item) = self.find_line(start) {
            for e in EventsAfter::new(item.offset(), self) {
                if &e.start < end {
                    ret.push(e);
                } else {
                    break;
                }
            }
        }
        ret
    }
    pub fn tagable_items_in_range(
        &mut self,
        start: &NaiveDateTime,
        end: &NaiveDateTime,
    ) -> Vec<Item> {
        let mut ret = vec![];
        if let Some(item) = self.find_line(start) {
            for i in ItemsAfter::new(item.offset(), &self.path) {
                match &i {
                    Item::Note(n, _) => {
                        if &n.time > end {
                            break;
                        } else {
                            ret.push(i);
                        }
                    }
                    Item::Event(e, _) => {
                        if &e.start > end {
                            break;
                        } else {
                            ret.push(i);
                        }
                    }
                    _ => (),
                }
            }
        }
        ret
    }
    pub fn notes_in_range(&mut self, start: &NaiveDateTime, end: &NaiveDateTime) -> Vec<Note> {
        let mut ret = vec![];
        if let Some(item) = self.find_line(start) {
            let mut at_first = true;
            for n in NotesAfter::new(item.offset(), self) {
                if at_first && &n.time < start {
                    at_first = false;
                    continue;
                } else {
                    at_first = false
                }
                if &n.time < end {
                    ret.push(n);
                } else {
                    break;
                }
            }
        }
        ret
    }
    pub fn last_event(&mut self) -> Option<Event> {
        // because Larry caches the line, re-acquiring the last event is cheap
        self.events_from_the_end().find(|_| true)
    }
    pub fn forgot_to_end_last_event(&mut self) -> bool {
        if let Some(event) = self.last_event() {
            if event.ongoing() {
                let now = Local::now().naive_local();
                event.start.date() != now.date()
            } else {
                false
            }
        } else {
            false
        }
    }
    fn needs_newline(&mut self) -> bool {
        if self.larry.len() > 0 {
            let last_line = self
                .larry
                .get(self.larry.len() - 1)
                .expect("could not obtain last line of log");
            let last_char = last_line.bytes().last().unwrap();
            !(last_char == 0x0D || last_char == 0x0A)
        } else {
            false
        }
    }
    // this method devours the reader because it invalidates the information cached in larry
    pub fn append_event(&mut self, description: String, tags: Vec<String>) -> (Event, usize) {
        let event = Event::coin(description, tags);
        self.append_to_log(event, "could not append event to log")
    }
    // this method devours the reader because it invalidates the information cached in larry
    pub fn append_note(&mut self, description: String, tags: Vec<String>) -> (Note, usize) {
        let note = Note::coin(description, tags);
        self.append_to_log(note, "could not append note to log")
    }
    pub fn close_event(&mut self) -> (Done, usize) {
        let done = Done(Local::now().naive_local());
        self.append_to_log(done, "could not append DONE line to log")
    }
    pub fn append_to_log<T: LogLine>(&mut self, item: T, error_message: &str) -> (T, usize) {
        let mut log = OpenOptions::new()
            .write(true)
            .append(true)
            .open(&self.path)
            .unwrap();
        if self.needs_newline() {
            writeln!(log, "").expect("could not append to log file");
        }
        let now = Local::today().naive_local();
        if let Some(ts) = self.last_timestamp() {
            if ts.date() != now {
                writeln!(log, "# {}/{}/{}", now.year(), now.month(), now.day())
                    .expect("could not append date comment to log");
            }
        } else {
            writeln!(log, "# {}/{}/{}", now.year(), now.month(), now.day())
                .expect("could not append date comment to log");
        }
        writeln!(log, "{}", &item.to_line()).expect(error_message);
        (item, self.larry.len())
    }
    // iterator over all items, first to last
    pub fn items(&self) -> ItemsAfter {
        ItemsAfter::new(0, &self.path)
    }
}

pub struct ItemsBefore<'a> {
    offset: Option<usize>,
    larry: &'a mut Larry,
}

impl<'a> ItemsBefore<'a> {
    fn new(offset: usize, reader: &mut LogController) -> ItemsBefore {
        ItemsBefore {
            offset: if offset == 0 { None } else { Some(offset) },
            larry: &mut reader.larry,
        }
    }
}

impl<'a> Iterator for ItemsBefore<'a> {
    type Item = Item;
    fn next(&mut self) -> Option<Item> {
        if let Some(o) = self.offset {
            let o2 = o - 1;
            let line = self.larry.get(o2).unwrap();
            let item = parse_line(line, o);
            self.offset = if o2 > 0 { Some(o2) } else { None };
            Some(item)
        } else {
            None
        }
    }
}

pub struct ItemsAfter {
    offset: usize,
    bufreader: Lines<BufReader<File>>,
}

impl ItemsAfter {
    pub fn new(offset: usize, path: &str) -> ItemsAfter {
        let mut bufreader =
            BufReader::new(File::open(path).expect("could not open log file")).lines();
        for _ in 0..offset {
            bufreader.next();
        }
        ItemsAfter { offset, bufreader }
    }
}

impl Iterator for ItemsAfter {
    type Item = Item;
    fn next(&mut self) -> Option<Item> {
        if let Some(res) = self.bufreader.next() {
            let line = res.expect("could not read log line");
            let item = parse_line(&line, self.offset);
            self.offset += 1;
            Some(item)
        } else {
            None
        }
    }
}

pub struct NotesBefore<'a> {
    item_iterator: ItemsBefore<'a>,
}

impl<'a> NotesBefore<'a> {
    fn new(offset: usize, reader: &mut LogController) -> NotesBefore {
        NotesBefore {
            item_iterator: ItemsBefore::new(offset, reader),
        }
    }
}

impl<'a> Iterator for NotesBefore<'a> {
    type Item = Note;
    fn next(&mut self) -> Option<Note> {
        loop {
            let item = self.item_iterator.next();
            if let Some(item) = item {
                match item {
                    Item::Note(n, _) => return Some(n),
                    _ => (),
                }
            } else {
                return None;
            }
        }
    }
}

pub struct NotesAfter {
    item_iterator: ItemsAfter,
}

impl NotesAfter {
    fn new(offset: usize, reader: &LogController) -> NotesAfter {
        NotesAfter {
            item_iterator: ItemsAfter::new(offset, &reader.path),
        }
    }
}

impl Iterator for NotesAfter {
    type Item = Note;
    fn next(&mut self) -> Option<Note> {
        loop {
            let item = self.item_iterator.next();
            if let Some(item) = item {
                match item {
                    Item::Note(n, _) => return Some(n),
                    _ => (),
                }
            } else {
                return None;
            }
        }
    }
}

pub struct EventsBefore<'a> {
    last_time: Option<NaiveDateTime>,
    item_iterator: ItemsBefore<'a>,
}

impl<'a> EventsBefore<'a> {
    fn new(offset: usize, reader: &mut LogController) -> EventsBefore {
        // the last event may be underway at the offset, so find out when it ends
        let items_after = ItemsAfter::new(offset, &reader.path);
        let timed_item = items_after
            .filter(|i| match i {
                Item::Event(_, _) | Item::Done(_, _) => true,
                _ => false,
            })
            .find(|i| i.time().is_some());
        let last_time = if let Some(i) = timed_item {
            Some(i.time().unwrap().0.to_owned())
        } else {
            None
        };
        EventsBefore {
            last_time,
            item_iterator: ItemsBefore::new(offset, reader),
        }
    }
}

impl<'a> Iterator for EventsBefore<'a> {
    type Item = Event;
    fn next(&mut self) -> Option<Event> {
        let mut last_time = self.last_time;
        let mut event: Option<Event> = None;
        loop {
            if let Some(i) = self.item_iterator.next() {
                match i {
                    Item::Event(e, _) => {
                        event = Some(e.bounded_time(last_time));
                        break;
                    }
                    Item::Done(d, _) => {
                        last_time = Some(d.0);
                    }
                    _ => (),
                }
            } else {
                break;
            }
        }
        self.last_time = if event.is_some() {
            Some(event.as_ref().unwrap().start.clone())
        } else {
            last_time
        };
        event
    }
}

pub struct EventsAfter {
    next_item: Option<Event>,
    item_iterator: ItemsAfter,
}

impl EventsAfter {
    fn new(offset: usize, reader: &LogController) -> EventsAfter {
        EventsAfter {
            next_item: None,
            item_iterator: ItemsAfter::new(offset, &reader.path),
        }
    }
    fn get_end_time(&mut self) -> Option<NaiveDateTime> {
        self.next_item = None;
        loop {
            if let Some(i) = self.item_iterator.next() {
                match i {
                    Item::Event(e, _) => {
                        let time = e.start.clone();
                        self.next_item = Some(e);
                        return Some(time);
                    }
                    Item::Done(d, _) => return Some(d.0),
                    _ => (),
                }
            } else {
                return None;
            }
        }
    }
}

impl Iterator for EventsAfter {
    type Item = Event;
    fn next(&mut self) -> Option<Event> {
        if let Some(event) = &self.next_item {
            return Some(event.clone().bounded_time(self.get_end_time()));
        }
        loop {
            if let Some(i) = self.item_iterator.next() {
                match i {
                    Item::Event(e, _) => return Some(e.bounded_time(self.get_end_time())),
                    _ => (),
                }
            } else {
                return None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use rand::seq::SliceRandom;
    use rand::{thread_rng, Rng};
    use std::fs::File;
    use std::io::LineWriter;
    use std::ops::AddAssign;
    use std::str::FromStr;

    enum Need {
        E,
        N,
        B,
        C,
        Error,
    }

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
        tags.sort_unstable();
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

    fn random_line(
        time: &mut NaiveDateTime,
        open_event: bool,
        offset: usize,
        need: Option<Need>,
    ) -> Item {
        let n = rand::thread_rng().gen_range(0, 100);
        let need = if let Some(need) = need {
            need
        } else {
            if n < 4 {
                Need::B
            } else if n < 10 {
                Need::C
            } else if n < 11 {
                Need::Error
            } else if n < 20 {
                Need::N
            } else {
                Need::E
            }
        };
        match need {
            Need::B => Item::Blank(offset),
            Need::C => {
                let mut comment = String::from("# ");
                comment.push_str(&random_text());
                Item::Comment(offset)
            }
            Need::Error => Item::Error(random_text(), offset),
            Need::N => {
                time.add_assign(Duration::seconds(rand::thread_rng().gen_range(1, 1000)));
                Item::Note(
                    Note {
                        time: time.clone(),
                        description: random_text(),
                        tags: random_tags(),
                    },
                    offset,
                )
            }
            Need::E => {
                time.add_assign(Duration::seconds(rand::thread_rng().gen_range(1, 1000)));
                if open_event && n < 30 {
                    Item::Done(Done(time.clone()), offset)
                } else {
                    Item::Event(
                        Event {
                            start: time.clone(),
                            start_overlap: false,
                            end: None,
                            end_overlap: false,
                            tags: random_tags(),
                            description: random_text(),
                            vacation: false,
                            vacation_type: None,
                        },
                        offset,
                    )
                }
            }
        }
    }

    // the need is a set of things you need at least one of in the log
    fn random_log(length: usize, need: Vec<Need>, disambiguator: &str) -> (Vec<Item>, String) {
        let mut initial_time = NaiveDate::from_ymd(2019, 12, 22).and_hms(9, 39, 30);
        let mut items: Vec<Item> = Vec::with_capacity(length);
        let mut open_event = false;
        // tests are run in parallel, so we need to prevent collisions, but it's nice to
        // have the files handy to look at in case of failure
        // this technique seems to suffice
        let path = format!(
            "{}-{}-{}.log",
            disambiguator,
            length,
            Local::now().naive_local().timestamp_millis()
        );
        let file = File::create(path.clone()).unwrap();
        let mut file = LineWriter::new(file);
        let mut need: Vec<(usize, Need)> = if need.is_empty() {
            vec![]
        } else {
            // randomly assign needs to lines
            let mut indices: Vec<usize> = (0..length).collect();
            indices.shuffle(&mut thread_rng());
            let mut need = need;
            need.shuffle(&mut thread_rng());
            let mut need = need
                .into_iter()
                .map(|n| (indices.remove(0), n))
                .collect::<Vec<_>>();
            need.sort_unstable_by(|(a, _), (b, _)| a.cmp(b));
            need
        };
        for offset in 0..length {
            let t = if let Some((i, _)) = need.get(0) {
                if i == &offset {
                    let t = need.remove(0).1;
                    Some(t)
                } else {
                    None
                }
            } else {
                None
            };
            let item = random_line(&mut initial_time, open_event, offset, t);
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

    fn closed_events(mut items: Vec<Item>) -> Vec<Event> {
        items.reverse();
        let mut ret = Vec::with_capacity(items.len());
        let mut last_time: Option<NaiveDateTime> = None;
        for i in items.iter() {
            match i {
                Item::Done(Done(t), _) => last_time = Some(t.clone()),
                Item::Event(e, _) => {
                    let mut e = e.clone();
                    if last_time.is_some() {
                        e.end = last_time;
                    }
                    last_time = Some(e.start.clone());
                    ret.push(e);
                }
                _ => (),
            }
        }
        ret.reverse();
        ret
    }

    fn notes(items: Vec<Item>) -> Vec<Note> {
        let mut ret = Vec::with_capacity(items.len());
        for i in items.iter() {
            match i {
                Item::Note(n, _) => {
                    ret.push(n.clone());
                }
                _ => (),
            }
        }
        ret
    }

    fn test_configuration(path: &str) -> (String, Configuration) {
        let conf_path = format!("{}_conf", path);
        File::create(
            PathBuf::from_str(&conf_path)
                .expect(&format!("could not create path {}", conf_path))
                .as_path(),
        )
        .expect(&format!("could not create file {}", conf_path));
        let pb = PathBuf::from_str(&conf_path)
            .expect(&format!("could not form path from {}", conf_path));
        let conf = Configuration::read(Some(pb), None);
        (conf_path, conf)
    }

    fn cleanup(paths: &[&str]) {
        for p in paths {
            let pb = PathBuf::from_str(p).expect(&format!("cannot form a path from {}", p));
            if pb.as_path().exists() {
                std::fs::remove_file(p).expect(&format!("failed to remove {}", p))
            }
        }
    }

    #[test]
    fn test_notes_in_range() {
        let (items, path) = random_log(100, vec![Need::N, Need::N], "test_notes_in_range");
        let notes = notes(items);
        assert!(notes.len() > 1, "found more than one note");
        let (conf_path, conf) = test_configuration("test_notes_in_range");
        let mut log_reader =
            LogController::new(Some(PathBuf::from_str(&path).unwrap()), &conf).unwrap();
        for i in 0..notes.len() - 1 {
            for j in i..notes.len() {
                let found_notes = log_reader.notes_in_range(&notes[i].time, &notes[j].time);
                assert!(
                    j - i == found_notes.len(),
                    "found as many events as expected"
                );
                for offset in 0..found_notes.len() {
                    let k = i + offset;
                    assert_eq!(notes[k].time, found_notes[offset].time, "same time");
                    assert_eq!(notes[k].tags, found_notes[offset].tags, "same tags");
                    assert_eq!(
                        notes[k].description, found_notes[offset].description,
                        "same description"
                    );
                }
            }
        }
        cleanup(&[&path, &conf_path]);
    }

    #[test]
    fn test_events_in_range() {
        let (items, path) = random_log(20, vec![Need::E, Need::E], "test_events_in_range");
        let events = closed_events(items);
        assert!(events.len() > 1, "found more than one event");
        let (conf_path, conf) = test_configuration("test_events_in_range");
        let mut log_reader =
            LogController::new(Some(PathBuf::from_str(&path).unwrap()), &conf).unwrap();
        for i in 0..events.len() - 1 {
            for j in i..events.len() {
                let found_events = log_reader.events_in_range(&events[i].start, &events[j].start);
                assert!(
                    j - i <= found_events.len(),
                    "found at least as many events as expected"
                );
                for offset in 0..found_events.len() {
                    let k = i + offset;
                    assert_eq!(events[k].start, found_events[offset].start, "same start");
                    assert_eq!(events[k].end, found_events[offset].end, "same end");
                    assert_eq!(events[k].tags, found_events[offset].tags, "same tags");
                    assert_eq!(
                        events[k].description, found_events[offset].description,
                        "same description"
                    );
                }
            }
        }
        cleanup(&[&path, &conf_path]);
    }

    #[test]
    fn test_notes_from_end() {
        let (items, path) = random_log(100, vec![Need::N], "test_notes_from_end");
        let mut notes = notes(items);
        notes.reverse();
        let (conf_path, conf) = test_configuration("test_notes_from_end");
        let mut log_reader =
            LogController::new(Some(PathBuf::from_str(&path).unwrap()), &conf).unwrap();
        let found_notes = log_reader.notes_from_the_end().collect::<Vec<_>>();
        assert_eq!(
            notes.len(),
            found_notes.len(),
            "found the right number of notes"
        );
        for (i, e) in notes.iter().enumerate() {
            assert_eq!(e.time, found_notes[i].time, "they occur at the same time");
            assert_eq!(e.tags, found_notes[i].tags, "they have the same tags");
            assert_eq!(
                e.description, found_notes[i].description,
                "they have the same text"
            );
        }
        cleanup(&[&path, &conf_path]);
    }

    #[test]
    fn test_notes_from_beginning() {
        let (items, path) = random_log(103, vec![Need::N], "test_notes_from_beginning");
        let notes = notes(items);
        let (conf_path, conf) = test_configuration("test_notes_from_beginning");
        let log_reader =
            LogController::new(Some(PathBuf::from_str(&path).unwrap()), &conf).unwrap();
        let found_notes = log_reader.notes_from_the_beginning().collect::<Vec<_>>();
        assert_eq!(
            notes.len(),
            found_notes.len(),
            "found the right number of notes"
        );
        for (i, n) in notes.iter().enumerate() {
            assert_eq!(n.time, found_notes[i].time, "they occur at the same time");
            assert_eq!(n.tags, found_notes[i].tags, "they have the same tags");
            assert_eq!(
                n.description, found_notes[i].description,
                "they have the same text"
            );
        }
        cleanup(&[&path, &conf_path]);
    }

    #[test]
    fn test_events_from_end() {
        let (items, path) = random_log(107, vec![Need::E], "test_events_from_end");
        let mut events = closed_events(items);
        events.reverse();
        let (conf_path, conf) = test_configuration("test_events_from_end");
        let mut log_reader =
            LogController::new(Some(PathBuf::from_str(&path).unwrap()), &conf).unwrap();
        let found_events = log_reader.events_from_the_end().collect::<Vec<_>>();
        assert_eq!(
            events.len(),
            found_events.len(),
            "found the right number of events"
        );
        for (i, e) in events.iter().enumerate() {
            assert_eq!(
                e.start, found_events[i].start,
                "they start at the same time"
            );
            assert_eq!(e.end, found_events[i].end, "they end at the same time");
            assert_eq!(e.tags, found_events[i].tags, "they have the same tags");
            assert_eq!(
                e.description, found_events[i].description,
                "they have the same description"
            );
        }
        cleanup(&[&path, &conf_path]);
    }

    #[test]
    fn test_events_from_beginning() {
        let (items, path) = random_log(100, vec![Need::E], "test_events_from_beginning");
        let events = closed_events(items);
        let (conf_path, conf) = test_configuration("test_events_from_beginning");
        let log_reader =
            LogController::new(Some(PathBuf::from_str(&path).unwrap()), &conf).unwrap();
        let found_events = log_reader.events_from_the_beginning().collect::<Vec<_>>();
        assert_eq!(
            events.len(),
            found_events.len(),
            "found the right number of events"
        );
        for (i, e) in events.iter().enumerate() {
            assert_eq!(
                e.start, found_events[i].start,
                "they start at the same time"
            );
            assert_eq!(e.end, found_events[i].end, "they end at the same time");
            assert_eq!(e.tags, found_events[i].tags, "they have the same tags");
            assert_eq!(
                e.description, found_events[i].description,
                "they have the same description"
            );
        }
        cleanup(&[&path, &conf_path]);
    }

    fn test_log(length: usize, disambiguator: &str) {
        let (items, path) = random_log(length, vec![], disambiguator);
        if items.is_empty() {
            println!("empty file; skipping...");
        } else {
            let (conf_path, conf) = test_configuration(&path);
            let mut log_reader =
                LogController::new(Some(PathBuf::from_str(&path).unwrap()), &conf).unwrap();
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
                                assert!(false, "failed to revert to found time when looking for missing intermediate time {}", intermediate_time);
                            }
                        }
                    }
                    last_timed_item = Some(found_item);
                } else {
                    assert!(false, "could not find item at offset {}", offset);
                }
                cleanup(&[&conf_path]);
            }
        }
        cleanup(&[&path]);
    }

    #[test]
    fn test_empty_file() {
        test_log(0, "test_empty_file");
    }

    #[test]
    fn test_100_tiny_files() {
        for i in 0..100 {
            test_log(5, &format!("test_100_tiny_files_{}", i));
        }
    }

    #[test]
    fn test_10_small_files() {
        for i in 0..10 {
            test_log(100, &format!("test_10_small_files_{}", i));
        }
    }

    #[test]
    fn test_large_file() {
        test_log(10000, "test_large_file");
    }

    #[test]
    fn test_event() {
        match parse_line("2019 12 1 16 3 30::an event with no tags", 0) {
            Item::Event(
                Event {
                    start,
                    tags,
                    description,
                    ..
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
                    tags,
                    description,
                    ..
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
        // can parse tags with spaces
        match parse_line("2019 12 1 16 3 30:foo\\ bar:an event with some tags", 0) {
            Item::Event(
                Event {
                    start,
                    tags,
                    description,
                    ..
                },
                _,
            ) => {
                assert_eq!(2019, start.year());
                assert_eq!(12, start.month());
                assert_eq!(1, start.day());
                assert_eq!(16, start.hour());
                assert_eq!(3, start.minute());
                assert_eq!(30, start.second());
                assert_eq!(1, tags.len(), "there are some tags");
                for t in vec!["foo bar"] {
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
                    tags,
                    description,
                    ..
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
    fn test_tag_whitespace_handling() {
        let e = Event::coin(
            String::from("foo"),
            vec![String::from("foo bar"), String::from("baz   plugh")],
        );
        match parse_line(e.to_line().as_str(), 0) {
            Item::Event(Event { tags, .. }, _) => {
                assert_eq!(2, tags.len());
                assert!(tags.contains(&String::from("foo bar")));
                assert!(tags.contains(&String::from("baz plugh")));
            }
            _ => assert!(false, "failed to parse line as an event"),
        }
    }

    #[test]
    fn test_zero_padding() {
        match parse_line("2019 12 01 16 03 30:DONE", 0) {
            Item::Done(Done(time), _) => {
                assert_eq!(1, time.day());
                assert_eq!(3, time.minute());
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

    #[test]
    fn stack_overflow_regression() {
        let (items, path) = random_log(23, vec![Need::E, Need::E], "stack_overflow_regression");
        let events = closed_events(items);
        assert!(events.len() > 1, "found more than one event");
        let (conf_path, conf) = test_configuration("stack_overflow_regression");
        let mut log_reader =
            LogController::new(Some(PathBuf::from_str(&path).unwrap()), &conf).unwrap();
        let e = events.first().unwrap();
        let false_start = e.start - Duration::days(1);
        let found_events = log_reader.events_in_range(&false_start, e.end.as_ref().unwrap());
        assert_eq!(1, found_events.len(), "found one event");
        assert_eq!(e.start, found_events[0].start, "same start");
        assert_eq!(e.end, found_events[0].end, "same end");
        assert_eq!(e.tags, found_events[0].tags, "same tags");
        assert_eq!(
            e.description, found_events[0].description,
            "same description"
        );
        cleanup(&[&path, &conf_path]);
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
    fn advance(&self, time: &NaiveDateTime) -> NaiveDateTime {
        match self {
            Item::Event(e, _) => {
                if time < &e.start {
                    e.start.clone()
                } else {
                    time.clone()
                }
            }
            Item::Note(n, _) => {
                if time < &n.time {
                    n.time.clone()
                } else {
                    time.clone()
                }
            }
            Item::Done(d, _) => {
                if time < &d.0 {
                    d.0.clone()
                } else {
                    time.clone()
                }
            }
            _ => time.clone(),
        }
    }
    pub fn time(&self) -> Option<(&NaiveDateTime, usize)> {
        match self {
            Item::Event(e, offset) => Some((&e.start, *offset)),
            Item::Note(n, offset) => Some((&n.time, *offset)),
            Item::Done(d, offset) => Some((&d.0, *offset)),
            _ => None,
        }
    }
    pub fn has_time(&self) -> bool {
        match self {
            Item::Event(_, _) | Item::Note(_, _) | Item::Done(_, _) => true,
            _ => false,
        }
    }
    // the line offset of the item
    pub fn offset(&self) -> usize {
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

pub fn parse_timestamp(timestamp: &str) -> Result<NaiveDateTime, String> {
    lazy_static! {
        static ref RE: Regex = Regex::new(r"\d+").unwrap();
    }
    let numbers: Vec<_> = RE.find_iter(timestamp).map(|m| m.as_str()).collect();
    // at this point the log lines grammar ensures all the parsing will be fine
    let year = numbers[0].parse::<i32>().unwrap();
    let month = numbers[1].parse::<u32>().unwrap();
    if month == 0 || month > 12 {
        return Err(format!("bad month: {}; must be in the range 1-12", month));
    }
    let day = numbers[2].parse::<u32>().unwrap();
    if day == 0 || day > 31 {
        return Err(format!("bad day: {}; day must be in the range 1-31", day));
    }
    let hour = numbers[3].parse::<u32>().unwrap();
    if hour > 23 {
        return Err(format!("bad hour: {}; hour must be less than 24", hour));
    }
    let minute = numbers[4].parse::<u32>().unwrap();
    if minute > 59 {
        return Err(format!(
            "bad minute: {}; minute must be less than 60",
            minute
        ));
    }
    let second = numbers[5].parse::<u32>().unwrap();
    if second > 59 {
        return Err(format!(
            "bad second: {}; second must be less than 60",
            second
        ));
    }
    match NaiveDate::from_ymd_opt(year, month, day) {
        Some(date) => Ok(date.and_hms(hour, minute, second)),
        _ => Err(String::from("impossible date")),
    }
}

pub fn timestamp(ts: &NaiveDateTime) -> String {
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
pub fn parse_tags(tags: &str) -> Vec<String> {
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
pub fn tags(tags: &Vec<String>) -> String {
    let mut v = tags.clone();
    v.sort_unstable();
    v.dedup(); // there may still be duplicates after we normalize whitespace below; oh, well
    let mut s = String::new();
    for (i, tag) in v.iter().enumerate() {
        if i > 0 {
            s.push(' ');
        }
        let mut ws = false;
        for c in tag.chars() {
            match c {
                ':' | '\\' | '<' => s.push('\\'),
                _ => (),
            }
            if c.is_whitespace() {
                if !ws {
                    ws = true;
                    s.push('\\');
                    s.push(' '); // normalize whitespace
                }
            } else {
                ws = false;
                s.push(c);
            }
        }
    }
    s
}

#[derive(Debug, Clone)]
pub struct Event {
    pub start: NaiveDateTime,
    pub start_overlap: bool,
    pub end: Option<NaiveDateTime>,
    pub end_overlap: bool,
    pub description: String,
    pub tags: Vec<String>,
    pub vacation: bool,
    pub vacation_type: Option<String>,
}

impl Event {
    pub fn coin(description: String, mut tags: Vec<String>) -> Event {
        tags.sort_unstable();
        tags.dedup();
        Event {
            start: Local::now().naive_local(),
            start_overlap: false,
            end: None,
            end_overlap: false,
            description: description,
            tags: tags,
            vacation: false,
            vacation_type: None,
        }
    }
    fn bounded_time(self, end: Option<NaiveDateTime>) -> Self {
        Event {
            start: self.start,
            start_overlap: self.start_overlap,
            end: end,
            end_overlap: self.end_overlap,
            description: self.description,
            tags: self.tags,
            vacation: self.vacation,
            vacation_type: self.vacation_type,
        }
    }
    pub fn ongoing(&self) -> bool {
        self.end.is_none()
    }
    // the duration of the task in seconds
    // the second parameter is necessary for ongoing tasks
    pub fn duration(&self, now: &NaiveDateTime) -> f32 {
        let end = self.end.as_ref().unwrap_or(now);
        (end.timestamp() - self.start.timestamp()) as f32
    }
    // split an event into two at a time boundary
    fn split(self, time: NaiveDateTime) -> (Self, Self) {
        assert!(time > self.start);
        assert!(self.end.is_none() || self.end.unwrap() > time);
        let mut start = self;
        let mut end = start.clone();
        start.end_overlap = true;
        start.end = Some(time.clone());
        end.start = time;
        end.end_overlap = true;
        (start, end)
    }
    // take a vector of events and convert them into sets not overlapping by day
    pub fn gather_by_day(events: Vec<Event>, end_date: &NaiveDateTime) -> Vec<Event> {
        let mut ret = vec![];
        let mut end_date = end_date;
        let now = Local::now().naive_local(); // we assume there are no future events in the log
        if &now < &end_date {
            end_date = &now;
        }
        for mut e in events {
            if &e.start >= end_date {
                break;
            }
            loop {
                match e.end.as_ref() {
                    Some(&time) => {
                        if time.date() == e.start.date() {
                            ret.push(e);
                            break;
                        }
                        let split_date = e.start.date().and_hms(0, 0, 0) + Duration::days(1);
                        let (e1, e2) = e.split(split_date);
                        e = e2;
                        ret.push(e1);
                    }
                    None => {
                        if e.start.date() == end_date.date() {
                            ret.push(e);
                            break;
                        } else {
                            let split_date = e.start.date().and_hms(0, 0, 0) + Duration::days(1);
                            let (e1, e2) = e.split(split_date);
                            e = e2;
                            ret.push(e1);
                        }
                    }
                }
            }
        }
        ret
    }
    fn mergeable(&self, other: &Self) -> bool {
        if self.end_overlap {
            // keep overlapped events separate to facilitate display
            return false;
        }
        if let Some(t) = self.end {
            t.day() == self.start.day() && // other isn't in a different day -- don't merge across day boundaries
            t == other.start  && self.tags == other.tags
        } else {
            false
        }
    }
    // this event was split off a larger one that overlapped a day boundary
    // it is the second part
    pub fn overlaps_start(&self) -> bool {
        self.end_overlap && self.start.hour() == 0
    }
    // this event was split off a larger one that overlapped a day boundary
    // it is the first part
    pub fn overlaps_end(&self) -> bool {
        if !self.end_overlap {
            return false;
        }
        if let Some(t) = self.end {
            t.day() != self.start.day()
        } else {
            false
        }
    }
    fn merge(&mut self, other: Self) {
        self.description = self.description.clone() + "; " + &other.description;
        self.end = other.end;
        self.end_overlap = other.end_overlap;
    }
    // like gather_by_day, but it also merges similar events -- similar events must have the same date and tags
    pub fn gather_by_day_and_merge(events: Vec<Event>, end_date: &NaiveDateTime) -> Vec<Event> {
        let mut events = Self::gather_by_day(events, end_date);
        if events.is_empty() {
            return events;
        }
        let mut ret = vec![];
        ret.push(events.remove(0));
        for e in events {
            let i = ret.len() - 1;
            if ret[i].mergeable(&e) {
                ret[i].merge(e);
            } else {
                ret.push(e);
            }
        }
        ret
    }
    pub fn to_json(&self, now: &NaiveDateTime, conf: &Configuration) -> String {
        let end = if let Some(time) = self.end {
            serde_json::to_string(&format!("{}", time)).unwrap()
        } else {
            "null".to_owned()
        };
        format!(
            r#"{{"type":"Event","start":{},"end":{},"duration":{},{}"tags":{},"description":{}}}"#,
            serde_json::to_string(&format!("{}", self.start)).unwrap(),
            end,
            duration_string(self.duration(now), conf),
            if let Some(t) = &self.vacation_type {
                format!("\"vacation\":\"{}\",", if t == "" { "ordinary" } else { t })
            } else {
                "".to_owned()
            },
            serde_json::to_string(&self.tags).unwrap(),
            serde_json::to_string(&self.description).unwrap()
        )
    }
}

impl Searchable for Event {
    fn text(&self) -> &str {
        &self.description
    }
    fn tags(&self) -> Vec<&str> {
        self.tags.iter().map(|s| s.as_str()).collect()
    }
}

#[derive(Debug, Clone)]
pub struct Note {
    pub time: NaiveDateTime,
    pub description: String,
    pub tags: Vec<String>,
}

impl Note {
    pub fn coin(description: String, mut tags: Vec<String>) -> Note {
        tags.sort_unstable();
        tags.dedup();
        Note {
            time: Local::now().naive_local(),
            description: description,
            tags: tags,
        }
    }
    pub fn to_json(&self, _now: &NaiveDateTime, _conf: &Configuration) -> String {
        format!(
            r#"{{"type":"Note","time":{},"tags":{},"description":{}}}"#,
            serde_json::to_string(&format!("{}", self.time)).unwrap(),
            serde_json::to_string(&self.tags).unwrap(),
            serde_json::to_string(&self.description).unwrap()
        )
    }
}

impl Searchable for Note {
    fn text(&self) -> &str {
        &self.description
    }
    fn tags(&self) -> Vec<&str> {
        self.tags.iter().map(|s| s.as_str()).collect()
    }
}

#[derive(Debug, Clone)]
pub struct Done(pub NaiveDateTime);

impl Done {
    pub fn coin() -> Done {
        Done(Local::now().naive_local())
    }
}

pub enum Direction {
    Forward,
    Back,
}

pub trait LogLine {
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

pub trait Searchable {
    fn tags(&self) -> Vec<&str>;
    fn text(&self) -> &str;
}

pub struct Filter<'a> {
    all_tags: Option<Vec<&'a str>>,
    no_tags: Option<Vec<&'a str>>,
    some_tags: Option<Vec<&'a str>>,
    some_patterns: Option<RegexSet>,
    no_patterns: Option<RegexSet>,
    empty: bool,
}

impl<'a> Filter<'a> {
    pub fn dummy() -> Filter<'a> {
        Filter {
            all_tags: None,
            no_tags: None,
            some_tags: None,
            some_patterns: None,
            no_patterns: None,
            empty: false,
        }
    }
    pub fn new(matches: &'a ArgMatches) -> Filter<'a> {
        let all_tags = matches
            .values_of("tag")
            .and_then(|values| Some(values.collect()));
        let no_tags = matches
            .values_of("tag-none")
            .and_then(|values| Some(values.collect()));
        let some_tags = matches
            .values_of("tag-some")
            .and_then(|values| Some(values.collect()));
        let some_patterns = matches
            .values_of("rx")
            .and_then(|values| Some(RegexSet::new(values).unwrap()));
        let no_patterns = matches
            .values_of("rx-not")
            .and_then(|values| Some(RegexSet::new(values).unwrap()));
        let empty = matches.is_present("no-tags");
        Filter {
            all_tags,
            no_tags,
            some_tags,
            some_patterns,
            no_patterns,
            empty,
        }
    }
    pub fn matches<T: Searchable>(&self, filterable: &T) -> bool {
        let tags = filterable.tags();
        let text = filterable.text();
        if tags.is_empty() {
            if self.empty {
                if let Some(rx_set) = self.some_patterns.as_ref() {
                    if !rx_set.is_match(text) {
                        return false;
                    }
                }
                if let Some(rx_set) = self.no_patterns.as_ref() {
                    if rx_set.is_match(text) {
                        return false;
                    }
                }
                return true;
            } else if !(self.all_tags.is_none() && self.some_tags.is_none()) {
                return false;
            }
        } else if self.empty {
            return false;
        } else {
            if self.some_tags.is_some()
                && !self
                    .some_tags
                    .as_ref()
                    .unwrap()
                    .iter()
                    .any(|t| tags.contains(t))
            {
                return false;
            }
            if self.all_tags.is_some()
                && self
                    .all_tags
                    .as_ref()
                    .unwrap()
                    .iter()
                    .any(|t| !tags.contains(t))
            {
                return false;
            }
            if self.no_tags.is_some()
                && self
                    .no_tags
                    .as_ref()
                    .unwrap()
                    .iter()
                    .any(|t| tags.contains(t))
            {
                return false;
            }
        }
        if let Some(rx_set) = self.some_patterns.as_ref() {
            if !rx_set.is_match(text) {
                return false;
            }
        }
        if let Some(rx_set) = self.no_patterns.as_ref() {
            if rx_set.is_match(text) {
                return false;
            }
        }
        true
    }
}
