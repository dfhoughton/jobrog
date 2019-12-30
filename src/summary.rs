extern crate clap;
extern crate two_timer;

use crate::configure::Configuration;
use crate::log_items::{Event, Filter, LogReader, Note};
use crate::util::{common_search_or_filter_arguments, display_events, display_notes, fatal, warn};
use clap::{App, Arg, ArgMatches, SubCommand};
use two_timer::{parsable, parse};

pub fn cli(mast: App<'static, 'static>) -> App<'static, 'static> {
    mast.subcommand(common_search_or_filter_arguments(
        SubCommand::with_name("summary")
            .aliases(&["s", "su", "sum", "summ", "summa", "summar"])
            .about("says when you will have worked all the hours expected within the given period")
            .after_help(".")
            .arg(
                Arg::with_name("period")
                    .help("description of time period summarized")
                    .long_help(
                        "Words describing the period summarized. E.g., 'last week' or '2016-10-2'.",
                    )
                    .value_name("word")
                    .default_value("today")
                    .multiple(true)
            )
            .display_order(3),
            None,
    ).arg(
        Arg::with_name("date")
        .long("date")
        .short("d")
        .help("the time expression as an option rather than an argument")
        .long_help("If you are frequently reviewing the tasks done in a particular pay period, filtering them by tag, say, it may be convenient for the date not to be at the end of the command line -- better to add filters here. In this case you can use the --date option.")
        .validator(|v| if parsable(&v) {Ok(())} else {Err(format!("cannot parse '{}' as a time expression", v))} )
        .value_name("phrase")
    ).arg(
        Arg::with_name("no-merge")
        .long("no-merge")
        .help("don't merge contiguous events with the same tags")
        .long_help("By default contiguous events with the same tags are displayed as a single event with the sub-events' descriptions joined with '; '. --no-merge prevents this.")
    ))
}

pub fn run(matches: &ArgMatches) {
    let phrase = matches
        .values_of("period")
        .unwrap()
        .collect::<Vec<&str>>()
        .join(" ");
    let date = matches.value_of("date").unwrap_or(&phrase);
    if phrase.len() > 0 && matches.is_present("date") {
        warn(format!(
            "--date option '{}' is overriding '{}' as a time expression",
            date, phrase
        ));
    }
    if let Ok((start, end, _)) = parse(&phrase, None) {
        let filter = Filter::new(matches);
        let mut reader = LogReader::new(None).expect("could not read log");
        let configuration = Configuration::read();
        if matches.is_present("notes") {
            let note: Vec<Note> = reader
                .notes_in_range(&start, &end)
                .into_iter()
                .filter(|n| filter.matches(n))
                .collect();
            if note.is_empty() {
                warn("no note found")
            } else {
                display_notes(note, &start, &end, &configuration);
            }
        } else {
            let events = reader
                .events_in_range(&start, &end)
                .into_iter()
                .filter(|n| filter.matches(n))
                .collect();
            let events = if matches.is_present("no-merge") {
                Event::gather_by_day(events, &end)
            } else {
                Event::gather_by_day_and_merge(events, &end)
            };
            if events.is_empty() {
                warn("no event found")
            } else {
                display_events(events, &start, &end, &configuration);
            }
        }
    } else {
        fatal(format!("could not parse '{}' as a time expression", phrase))
    }
}
