use crate::configure::Configuration;
use crate::util::{duration_string, log_path, FilterArgs};
use chrono::{Datelike, Duration, Local, NaiveDate, NaiveDateTime, Timelike};
use larry::Larry;
use regex::{Regex, RegexSet};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Lines, Write};
use std::path::PathBuf;
use std::sync::LazyLock;

// a log line begins with a timestamp: a four-digit year followed by five one- or
// two-digit numbers, all separated by whitespace
static TIMESTAMP_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\A\s*[1-9]\d{3}(?:\s+\d{1,2}){5}\s*").unwrap());

// parses a log line into an appropriate data structure preserving the line offset
pub fn parse_line(line: &str, offset: usize) -> Item {
    // the line grammar tolerates trailing whitespace (including line terminators) at the
    // end of the line; anything else after an embedded newline makes the line unparseable
    let content = match line.find('\n') {
        Some(i) => {
            if line[i..].chars().all(|c| c.is_whitespace()) {
                &line[..i]
            } else {
                return Item::Error(String::from("unexpected line format"), offset);
            }
        }
        None => line,
    };
    if let Some(m) = TIMESTAMP_RE.find(content) {
        let rest = &content[m.end()..];
        // a DONE line is a timestamp followed by ":DONE" and nothing but whitespace
        if let Some(after) = rest.strip_prefix(":DONE") {
            if after.chars().all(|c| c.is_whitespace()) {
                return match parse_timestamp(m.as_str()) {
                    Err(s) => Item::Error(s, offset),
                    Ok(timestamp) => Item::Done(Done(timestamp), offset),
                };
            }
        }
        // otherwise the timestamp is followed by ":" (an event) or "<NOTE>" (a note),
        // then tags, then ":", then the description
        let (is_note, sep_len) = if rest.starts_with("<NOTE>") {
            (true, "<NOTE>".len())
        } else if rest.starts_with(':') {
            (false, 1)
        } else {
            return Item::Error(String::from("unexpected line format"), offset);
        };
        let tags_and_description = &rest[sep_len..];
        // scan the tags for the terminating unescaped ':'; tags may not contain an
        // unescaped ':' or '<', and a trailing lone backslash is an error
        let mut chars = tags_and_description.char_indices();
        let colon = loop {
            match chars.next() {
                None => return Item::Error(String::from("unexpected line format"), offset),
                Some((_, '\\')) => {
                    if chars.next().is_none() {
                        return Item::Error(String::from("unexpected line format"), offset);
                    }
                }
                Some((i, ':')) => break i,
                Some((_, '<')) => {
                    return Item::Error(String::from("unexpected line format"), offset)
                }
                Some(_) => (),
            }
        };
        let tags = parse_tags(&tags_and_description[..colon]);
        // leading whitespace between the ':' and the description is not part of the description
        let description = tags_and_description[colon + 1..].trim_start();
        match parse_timestamp(m.as_str()) {
            Err(s) => Item::Error(s, offset),
            Ok(timestamp) => {
                if is_note {
                    Item::Note(
                        Note {
                            time: timestamp,
                            description: description.to_owned(),
                            tags,
                        },
                        offset,
                    )
                } else {
                    Item::Event(
                        Event {
                            start: timestamp,
                            start_overlap: false,
                            end: None,
                            end_overlap: false,
                            description: description.to_owned(),
                            tags,
                            vacation: false,
                            vacation_type: None,
                        },
                        offset,
                    )
                }
            }
        }
    } else if content.chars().all(|c| c.is_whitespace()) {
        Item::Blank(offset)
    } else if content.trim_start().starts_with('#') {
        Item::Comment(offset)
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
        Larry::new(log.as_path())
            .map(|larry| LogController {
                larry,
                path: path.expect("log path is not valid UTF-8").to_owned(),
            })
            .map_err(std::io::Error::other)
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
        item.map(|i| *i.time().unwrap().0)
    }
    pub fn last_timestamp(&mut self) -> Option<NaiveDateTime> {
        let item = ItemsBefore::new(self.larry.len(), self).find(|i| i.has_time());
        item.map(|i| *i.time().unwrap().0)
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
        } else if t2 < time {
            // the sought time is beyond every line in this bracket, so the best line
            // is the first line bearing the bracket's latest timestamp
            return self.advance_to_first(end);
        } else if t1 > time {
            // the sought time is before every line in this bracket
            return start;
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
            } else if let Some((t, _)) = next.time() {
                match t.cmp(time) {
                    std::cmp::Ordering::Equal => return next,
                    std::cmp::Ordering::Less => {
                        // recurse only if the lower bound's timestamp would advance;
                        // otherwise every line up to o3 shares start's timestamp and the
                        // recursive call would repeat this one forever
                        if self.advance_to_first(next.clone()) == start {
                            o1 = o3;
                            o3 = self.estimate(time, t1, o1, t2, o2);
                            if o3 == o1 {
                                return start;
                            }
                        } else {
                            return self.narrow_in(time, next, end);
                        }
                    }
                    std::cmp::Ordering::Greater => return self.narrow_in(time, start, next),
                }
            } else {
                unreachable!()
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
                let time_delta = t2.and_utc().timestamp() - t1.and_utc().timestamp();
                let lines_per_second = (line_delta as f64) / (time_delta as f64);
                let seconds = (time.and_utc().timestamp() - t1.and_utc().timestamp()) as f64;
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
            let item = parse_line(&self.larry.get(i).unwrap(), i);
            let t = item.time();
            if t.is_some() {
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
            let item = parse_line(&self.larry.get(i).unwrap(), i);
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
            let next = parse_line(&self.larry.get(i).unwrap(), i);
            let next_time = next.time();
            if let Some((next_time, _)) = next_time {
                match time.cmp(next_time) {
                    std::cmp::Ordering::Equal => ptr = next,
                    std::cmp::Ordering::Greater => return ptr,
                    std::cmp::Ordering::Less => (),
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
        if !self.larry.is_empty() {
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
        let mut log = OpenOptions::new().append(true).open(&self.path).unwrap();
        if self.needs_newline() {
            writeln!(log).expect("could not append to log file");
        }
        let now = Local::now().naive_local();
        if let Some(ts) = self.last_timestamp() {
            if ts.date() != now.date() {
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

impl ItemsBefore<'_> {
    fn new(offset: usize, reader: &mut LogController) -> ItemsBefore {
        ItemsBefore {
            offset: if offset == 0 { None } else { Some(offset) },
            larry: &mut reader.larry,
        }
    }
}

impl Iterator for ItemsBefore<'_> {
    type Item = Item;
    fn next(&mut self) -> Option<Item> {
        if let Some(o) = self.offset {
            let o2 = o - 1;
            let line = self.larry.get(o2).unwrap();
            let item = parse_line(&line, o);
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

impl NotesBefore<'_> {
    fn new(offset: usize, reader: &mut LogController) -> NotesBefore {
        NotesBefore {
            item_iterator: ItemsBefore::new(offset, reader),
        }
    }
}

impl Iterator for NotesBefore<'_> {
    type Item = Note;
    fn next(&mut self) -> Option<Note> {
        loop {
            let item = self.item_iterator.next();
            if let Some(item) = item {
                if let Item::Note(n, _) = item {
                    return Some(n);
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
                if let Item::Note(n, _) = item {
                    return Some(n);
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

impl EventsBefore<'_> {
    fn new(offset: usize, reader: &mut LogController) -> EventsBefore {
        // the last event may be underway at the offset, so find out when it ends
        let items_after = ItemsAfter::new(offset, &reader.path);
        let timed_item = items_after
            .filter(|i| matches!(i, Item::Event(_, _) | Item::Done(_, _)))
            .find(|i| i.time().is_some());
        let last_time = timed_item.map(|i| i.time().unwrap().0.to_owned());
        EventsBefore {
            last_time,
            item_iterator: ItemsBefore::new(offset, reader),
        }
    }
}

impl Iterator for EventsBefore<'_> {
    type Item = Event;
    fn next(&mut self) -> Option<Event> {
        let mut last_time = self.last_time;
        let mut event: Option<Event> = None;
        for i in self.item_iterator.by_ref() {
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
        }
        self.last_time = event.as_ref().map_or(last_time, |e| Some(e.start));
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
                        let time = e.start;
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
                if let Item::Event(e, _) = i {
                    return Some(e.bounded_time(self.get_end_time()));
                }
            } else {
                return None;
            }
        }
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
                    e.start
                } else {
                    *time
                }
            }
            Item::Note(n, _) => {
                if time < &n.time {
                    n.time
                } else {
                    *time
                }
            }
            Item::Done(d, _) => {
                if time < &d.0 {
                    d.0
                } else {
                    *time
                }
            }
            _ => *time,
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
        matches!(
            self,
            Item::Event(_, _) | Item::Note(_, _) | Item::Done(_, _)
        )
    }
    // the line offset of the item
    pub fn offset(&self) -> usize {
        match self {
            Item::Event(_, i)
            | Item::Note(_, i)
            | Item::Done(_, i)
            | Item::Blank(i)
            | Item::Comment(i)
            | Item::Error(_, i) => *i,
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
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d+").unwrap());
    let numbers: Vec<&str> = RE.find_iter(timestamp).map(|m| m.as_str()).collect();
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
        Some(date) => Ok(date.and_hms_opt(hour, minute, second).unwrap()),
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
                if !current.is_empty() && !parsed.contains(&current) {
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
    if !current.is_empty() && !parsed.contains(&current) {
        parsed.push(current);
    }
    parsed
}

// convert tags back into a part of a log string
pub fn tags(tags: &[String]) -> String {
    let mut v = tags.to_vec();
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
            description,
            tags,
            vacation: false,
            vacation_type: None,
        }
    }
    fn bounded_time(self, end: Option<NaiveDateTime>) -> Self {
        Event { end, ..self }
    }
    pub fn ongoing(&self) -> bool {
        self.end.is_none()
    }
    // the duration of the task in seconds
    // the second parameter is necessary for ongoing tasks
    pub fn duration(&self, now: &NaiveDateTime) -> f32 {
        let end = self.end.as_ref().unwrap_or(now);
        (end.and_utc().timestamp() - self.start.and_utc().timestamp()) as f32
    }
    // split an event into two at a time boundary
    fn split(self, time: NaiveDateTime) -> (Self, Self) {
        assert!(time > self.start);
        assert!(self.end.is_none() || self.end.unwrap() > time);
        let mut start = self;
        let mut end = start.clone();
        start.end_overlap = true;
        start.end = Some(time);
        end.start = time;
        end.end_overlap = true;
        (start, end)
    }
    // take a vector of events and convert them into sets not overlapping by day
    pub fn gather_by_day(events: Vec<Event>, end_date: &NaiveDateTime) -> Vec<Event> {
        let mut ret = vec![];
        let mut end_date = end_date;
        let now = Local::now().naive_local(); // we assume there are no future events in the log
        if &now < end_date {
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
                        let split_date =
                            e.start.date().and_hms_opt(0, 0, 0).unwrap() + Duration::days(1);
                        let (e1, e2) = e.split(split_date);
                        e = e2;
                        ret.push(e1);
                    }
                    None => {
                        if e.start.date() == end_date.date() {
                            ret.push(e);
                            break;
                        } else {
                            let split_date =
                                e.start.date().and_hms_opt(0, 0, 0).unwrap() + Duration::days(1);
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
                format!(
                    "\"vacation\":\"{}\",",
                    if t.is_empty() { "ordinary" } else { t }
                )
            } else {
                String::new()
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
            description,
            tags,
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
    pub fn new(args: &'a FilterArgs) -> Filter<'a> {
        let all_tags = if args.tag.is_empty() {
            None
        } else {
            Some(args.tag.iter().map(String::as_str).collect())
        };
        let no_tags = if args.tag_none.is_empty() {
            None
        } else {
            Some(args.tag_none.iter().map(String::as_str).collect())
        };
        let some_tags = if args.tag_some.is_empty() {
            None
        } else {
            Some(args.tag_some.iter().map(String::as_str).collect())
        };
        let some_patterns = if args.rx.is_empty() {
            None
        } else {
            Some(RegexSet::new(&args.rx).unwrap())
        };
        let no_patterns = if args.rx_not.is_empty() {
            None
        } else {
            Some(RegexSet::new(&args.rx_not).unwrap())
        };
        let empty = args.no_tags;
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use rand::seq::SliceRandom;
    use rand::{rng, Rng};
    use std::fs::File;
    use std::io::LineWriter;
    use std::ops::AddAssign;

    enum Need {
        E,
        N,
        B,
        C,
        Error,
    }

    fn random_tag() -> String {
        let choices = ["foo", "bar", "baz", "plugh", "work", "play", "tedium"];
        choices[rng().random_range(0..choices.len())].to_owned()
    }

    fn random_words(min: usize, max: usize) -> Vec<String> {
        (0..rng().random_range(min..max + 1))
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
        let n = rng().random_range(0..100);
        let need = if let Some(need) = need {
            need
        } else if n < 4 {
            Need::B
        } else if n < 10 {
            Need::C
        } else if n < 11 {
            Need::Error
        } else if n < 20 {
            Need::N
        } else {
            Need::E
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
                time.add_assign(Duration::seconds(rng().random_range(1..1000)));
                Item::Note(
                    Note {
                        time: *time,
                        description: random_text(),
                        tags: random_tags(),
                    },
                    offset,
                )
            }
            Need::E => {
                time.add_assign(Duration::seconds(rng().random_range(1..1000)));
                if open_event && n < 30 {
                    Item::Done(Done(*time), offset)
                } else {
                    Item::Event(
                        Event {
                            start: *time,
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
        let mut initial_time = NaiveDate::from_ymd_opt(2019, 12, 22)
            .unwrap()
            .and_hms_opt(9, 39, 30)
            .unwrap();
        let mut items: Vec<Item> = Vec::with_capacity(length);
        let mut open_event = false;
        // tests are run in parallel, so we need to prevent collisions, but it's nice to
        // have the files handy to look at in case of failure
        // this technique seems to suffice
        let path = format!(
            "{}-{}-{}.log",
            disambiguator,
            length,
            Local::now().naive_local().and_utc().timestamp_millis()
        );
        let file = File::create(path.clone()).unwrap();
        let mut file = LineWriter::new(file);
        let mut need: Vec<(usize, Need)> = if need.is_empty() {
            vec![]
        } else {
            // randomly assign needs to lines
            let mut indices: Vec<usize> = (0..length).collect();
            indices.shuffle(&mut rng());
            let mut need = need;
            need.shuffle(&mut rng());
            let mut need = need
                .into_iter()
                .map(|n| (indices.remove(0), n))
                .collect::<Vec<_>>();
            need.sort_unstable_by(|(a, _), (b, _)| a.cmp(b));
            need
        };
        for offset in 0..length {
            let t = if let Some((i, _)) = need.first() {
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
                Item::Done(Done(t), _) => last_time = Some(*t),
                Item::Event(e, _) => {
                    let mut e = e.clone();
                    if last_time.is_some() {
                        e.end = last_time;
                    }
                    last_time = Some(e.start);
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
            if let Item::Note(n, _) = i {
                ret.push(n.clone());
            }
        }
        ret
    }

    fn test_configuration(path: &str) -> (String, Configuration) {
        let conf_path = format!("{}_conf", path);
        File::create(PathBuf::from(&conf_path))
            .unwrap_or_else(|_| panic!("could not create file {}", conf_path));
        let conf = Configuration::read(Some(PathBuf::from(&conf_path)), None);
        (conf_path, conf)
    }

    fn cleanup(paths: &[&str]) {
        for p in paths {
            let pb = PathBuf::from(p);
            if pb.as_path().exists() {
                std::fs::remove_file(p).unwrap_or_else(|_| panic!("failed to remove {}", p));
            }
        }
    }

    #[test]
    fn test_notes_in_range() {
        let (items, path) = random_log(100, vec![Need::N, Need::N], "test_notes_in_range");
        let notes = notes(items);
        assert!(notes.len() > 1, "found more than one note");
        let (conf_path, conf) = test_configuration("test_notes_in_range");
        let mut log_reader = LogController::new(Some(PathBuf::from(&path)), &conf).unwrap();
        for i in 0..notes.len() - 1 {
            for j in i..notes.len() {
                let found_notes = log_reader.notes_in_range(&notes[i].time, &notes[j].time);
                assert!(
                    j - i == found_notes.len(),
                    "found as many events as expected"
                );
                for (offset, found) in found_notes.iter().enumerate() {
                    let k = i + offset;
                    assert_eq!(notes[k].time, found.time, "same time");
                    assert_eq!(notes[k].tags, found.tags, "same tags");
                    assert_eq!(notes[k].description, found.description, "same description");
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
        let mut log_reader = LogController::new(Some(PathBuf::from(&path)), &conf).unwrap();
        for i in 0..events.len() - 1 {
            for j in i..events.len() {
                let found_events = log_reader.events_in_range(&events[i].start, &events[j].start);
                assert!(
                    j - i <= found_events.len(),
                    "found at least as many events as expected"
                );
                for (offset, found) in found_events.iter().enumerate() {
                    let k = i + offset;
                    assert_eq!(events[k].start, found.start, "same start");
                    assert_eq!(events[k].end, found.end, "same end");
                    assert_eq!(events[k].tags, found.tags, "same tags");
                    assert_eq!(events[k].description, found.description, "same description");
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
        let mut log_reader = LogController::new(Some(PathBuf::from(&path)), &conf).unwrap();
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
        let log_reader = LogController::new(Some(PathBuf::from(&path)), &conf).unwrap();
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
        let mut log_reader = LogController::new(Some(PathBuf::from(&path)), &conf).unwrap();
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
        let log_reader = LogController::new(Some(PathBuf::from(&path)), &conf).unwrap();
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
            let mut log_reader = LogController::new(Some(PathBuf::from(&path)), &conf).unwrap();
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
                                panic!("failed to revert to found time when looking for missing intermediate time {}", intermediate_time);
                            }
                        }
                    }
                    last_timed_item = Some(found_item);
                } else {
                    panic!("could not find item at offset {}", offset);
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
            _ => panic!("failed to parse an event line"),
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
                for t in ["foo", "bar"] {
                    assert!(tags.contains(&t.to_owned()));
                }
                assert_eq!(
                    "an event with some tags", &description,
                    "got correct description"
                )
            }
            _ => panic!("failed to parse an event line"),
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
                {
                    let t = "foo bar";
                    assert!(tags.contains(&t.to_owned()));
                }
                assert_eq!(
                    "an event with some tags", &description,
                    "got correct description"
                )
            }
            _ => panic!("failed to parse an event line"),
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
                {
                    let t = "foo";
                    assert!(tags.contains(&t.to_owned()));
                }
                assert_eq!(
                    "plugh baz baz foo play play work baz tedium foo tedium foo work bar", &description,
                    "got correct description"
                )
            }
            _ => panic!("failed to parse an event line"),
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
            _ => panic!("failed to parse a NOTE line"),
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
                for t in ["foo", "bar"] {
                    assert!(tags.contains(&t.to_owned()));
                }
                assert_eq!("a short note", &description, "got correct description")
            }
            _ => panic!("failed to parse a NOTE line"),
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
                assert_eq!(tags.len(), 4, "there are four tags");
                for t in ["f:oo", "b<ar", "b az", r"pl\ugh"] {
                    assert!(tags.contains(&t.to_owned()), "escaping worked");
                }
                assert_eq!("a short note", &description, "got correct description")
            }
            _ => panic!("failed to parse a NOTE line"),
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
                for t in ["foo", "bar"] {
                    assert!(tags.contains(&t.to_owned()));
                }
                assert_eq!("a short note", &description, "got correct description")
            }
            _ => panic!("failed to parse a NOTE line"),
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
                for t in ["foo", "bar"] {
                    assert!(tags.contains(&t.to_owned()));
                }
                assert_eq!("a short note", &description, "got correct description")
            }
            _ => panic!("failed to parse a NOTE line"),
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
                assert_eq!(tags.len(), 4, "there are four tags");
                for t in ["foo", "play", "tedium", "work"] {
                    assert!(tags.contains(&t.to_owned()));
                }
                assert_eq!("baz tedium baz tedium foo plugh bar foo bar play plugh foo baz play baz tedium work work play play bar", &description, "got correct description")
            }
            _ => panic!("failed to parse a NOTE line"),
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
                {
                    let t = "bar";
                    assert!(tags.contains(&t.to_owned()));
                }
                assert_eq!(
                    "tedium plugh baz play tedium baz play work", &description,
                    "got correct description"
                )
            }
            _ => panic!("failed to parse a NOTE line"),
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
            _ => panic!("failed to parse a DONE line"),
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
            _ => panic!("failed to parse a DONE line"),
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
            _ => panic!("failed to parse line as an event"),
        }
    }

    #[test]
    fn test_zero_padding() {
        match parse_line("2019 12 01 16 03 30:DONE", 0) {
            Item::Done(Done(time), _) => {
                assert_eq!(1, time.day());
                assert_eq!(3, time.minute());
            }
            _ => panic!("failed to parse a DONE line"),
        };
    }

    #[test]
    fn test_comment() {
        let success = matches!(parse_line("#foo", 0), Item::Comment(_));
        assert!(success, "recognized '#foo' as a comment line");
        let success = matches!(parse_line("   #foo", 0), Item::Comment(_));
        assert!(success, "comments can have leading space");
    }

    #[test]
    fn test_error() {
        let success = matches!(parse_line("foo", 0), Item::Error(_, _));
        assert!(success, "recognized 'foo' as a malformed log line");
    }

    #[test]
    fn test_blank() {
        let success = matches!(parse_line("", 0), Item::Blank(_));
        assert!(success, "recognized an empty line as a blank");
        let success = matches!(parse_line("     ", 0), Item::Blank(_));
        assert!(success, "recognized a whitespace line as a blank");
    }

    #[test]
    fn test_malformed_lines() {
        // a timestamp with nothing after it is an error
        assert!(matches!(
            parse_line("2019 12 1 16 3 30", 0),
            Item::Error(_, _)
        ));
        // an unescaped '<' in the tags is an error
        assert!(matches!(
            parse_line("2019 12 1 16 3 30:foo<bar:desc", 0),
            Item::Error(_, _)
        ));
        // tags with no terminating colon are an error
        assert!(matches!(
            parse_line("2019 12 1 16 3 30:foo bar", 0),
            Item::Error(_, _)
        ));
        // ":DONE" followed by non-whitespace is an error
        assert!(matches!(
            parse_line("2019 12 1 16 3 30:DONE x", 0),
            Item::Error(_, _)
        ));
        // a trailing lone backslash in the tags is an error
        assert!(matches!(
            parse_line("2019 12 1 16 3 30:foo\\", 0),
            Item::Error(_, _)
        ));
        // impossible timestamps are errors
        assert!(matches!(
            parse_line("2019 13 1 16 3 30::desc", 0),
            Item::Error(_, _)
        ));
        assert!(matches!(
            parse_line("2019 12 1 25 3 30::desc", 0),
            Item::Error(_, _)
        ));
        // ...but ":DONE" with trailing whitespace is fine
        assert!(matches!(
            parse_line("2019 12 1 16 3 30:DONE  ", 0),
            Item::Done(_, _)
        ));
        // a description may contain colons and '<'
        match parse_line("2019 12 1 16 3 30::a:b <c>", 0) {
            Item::Event(
                Event {
                    description, tags, ..
                },
                _,
            ) => {
                assert!(tags.is_empty());
                assert_eq!("a:b <c>", description);
            }
            _ => panic!("failed to parse an event line"),
        }
    }

    #[test]
    fn stack_overflow_regression() {
        let (items, path) = random_log(23, vec![Need::E, Need::E], "stack_overflow_regression");
        let events = closed_events(items);
        assert!(events.len() > 1, "found more than one event");
        let (conf_path, conf) = test_configuration("stack_overflow_regression");
        let mut log_reader = LogController::new(Some(PathBuf::from(&path)), &conf).unwrap();
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

    #[test]
    fn test_find_line_out_of_bounds() {
        // regression test: find_line must terminate (and return the sensible bound)
        // when the sought time is before the first line or after the last line,
        // even when the bracket is large enough to use interpolation in estimate
        let path = format!(
            "test_find_line_out_of_bounds-{}.log",
            Local::now().naive_local().and_utc().timestamp_millis()
        );
        let mut contents = String::from("# job log\n");
        // 40 events with distinct, increasing timestamps, all sharing a few timestamps
        // in groups to exercise advance_to_first as well
        for day in 1..=20 {
            for hour in [9, 17] {
                contents.push_str(&format!(
                    "2020 1 {} {} 0 0:work:task day {} hour {}\n",
                    day, hour, day, hour
                ));
            }
        }
        std::fs::write(&path, contents).unwrap();
        let (conf_path, conf) = test_configuration("test_find_line_out_of_bounds");
        let mut log = LogController::new(Some(PathBuf::from(&path)), &conf).unwrap();

        let before_all = NaiveDate::from_ymd_opt(2019, 1, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap();
        let item = log.find_line(&before_all).expect("should find a line");
        assert_eq!(
            item.offset(),
            1,
            "time before all lines finds the first line"
        );

        let after_all = NaiveDate::from_ymd_opt(2021, 1, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap();
        let item = log.find_line(&after_all).expect("should find a line");
        let (t, _) = item.time().unwrap();
        assert_eq!(
            NaiveDate::from_ymd_opt(2020, 1, 20)
                .unwrap()
                .and_hms_opt(17, 0, 0)
                .unwrap(),
            *t,
            "time after all lines finds the latest timestamp"
        );

        let exact = NaiveDate::from_ymd_opt(2020, 1, 10)
            .unwrap()
            .and_hms_opt(9, 0, 0)
            .unwrap();
        let item = log.find_line(&exact).expect("should find a line");
        let (t, _) = item.time().unwrap();
        assert_eq!(exact, *t, "exact timestamp finds its line");

        cleanup(&[&path, &conf_path]);
    }
}
