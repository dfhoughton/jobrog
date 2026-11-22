use crate::configure::Configuration;
use crate::util::{fatal, some_nws, Style};

use colonnade::Colonnade;
use two_timer::parse;

pub const AFTER_HELP: &str = "\
Natural language time expressions are easy to produce, but it isn't always obvious \
what fully-specified times they correspond to. Sometimes you may want to know this before \
you give one to Job Log. Perhaps you summarize the log for a particular period and the \
results don't look right. Perhaps you are about to make a change that involves a timestamp \
and you want to make sure you'll get the change you want. This is what parse-time is for. \
Give it a string and see what you get.

The parse-time subcommand returns the first moment inclusive of the time expression, the \
last moment exclusive and whether the expression explicitly name both the beginning and \
the end of the range.

Job log uses two grammars to parse time expressions: a simple grammar which it tries first \
and a more comprehensive grammar, which it falls back to. Here are some useful expressions \
which are available in the simple grammar:

  yesterday
  today
  now
  pay period
  pp
  Friday
  fri
  this week
  last week
  last month
  March
  March 20
  2016
  feb - mar

Both grammars are from https://github.com/dfhoughton/two-timer. You can find list of sample \
expressions at this URL and, in the source code, the actual grammars.

All prefixes of 'parse-time' are aliases of the subcommand.
";

#[derive(Debug, clap::Args)]
#[command(
    aliases = [
        "p", "pa", "par", "pars", "parse", "parse-", "parse-t", "parse-ti", "parse-tim"
    ],
    about = "Shows the start and end timestamps you get from a particular time expression",
    after_help = AFTER_HELP,
    after_long_help = AFTER_HELP
)]
pub struct Args {
    /// time expression
    ///
    /// A time expression. E.g., 'last week' or '2016-10-2'.
    #[arg(value_name = "word", num_args = 0..)]
    pub period: Vec<String>,
}

pub fn run(directory: Option<&str>, args: &Args) {
    let conf = Configuration::read(None, directory);
    if args.period.is_empty() {
        fatal("no time expression provided", &conf);
    }
    let phrase = args.period.join(" ");
    if some_nws(&phrase) {
        match parse(phrase.trim(), conf.two_timer_config()) {
            Ok(range) => {
                let style = Style::new(&conf);
                let data = [
                    [String::from("start"), format!("{}", range.start)],
                    [String::from("end"), format!("{}", range.end)],
                    [String::from("explicit end"), format!("{}", range.is_range)],
                ];
                let mut table = Colonnade::new(2, conf.width()).unwrap();
                println!();
                for row in table.macerate(&data).unwrap() {
                    for line in row {
                        for (cell_num, (margin, contents)) in line.iter().enumerate() {
                            if cell_num == 0 {
                                print!("{}{}", margin, style.paint("header", contents));
                            } else {
                                print!("{}{}", margin, contents);
                            }
                        }
                    }
                    println!();
                }
                println!();
            }
            Err(e) => fatal(e.msg(), &conf),
        }
    } else {
        fatal("no time expression provided", &conf);
    }
}
