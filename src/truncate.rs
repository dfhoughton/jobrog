use crate::configure::Configuration;
use crate::log::LogController;
use crate::util::{base_dir, fatal, log_path, success, warn, yes_or_no};

use flate2::write::GzEncoder;
use flate2::Compression;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use two_timer::{parsable, parse};

const BUFFER_SIZE: usize = 16 * 1024;

pub const AFTER_HELP: &str = "\
Over time your log will fill with cruft: work no one is interested in any longer, \
tags whose meaning you've forgotten. What you want to do at this point is chop off \
all the old stuff, stash it somewhere you can find it if need be, and \
retain in your active log only the more recent events. This is what truncate is for. \
You give it a starting date and it splits your log into two with the active portion \
containing all moments on that date or after. The older portion is \
retained in the hidden directory.

All prefixes of 'truncate' excepting 't' are aliases of the subcommand. The 't' alias belongs \
to the tag subcommand.";

#[derive(Debug, clap::Args)]
#[command(
    aliases = ["tr", "tru", "trun", "trunc", "trunca", "truncat"],
    about = "Truncates the log so it only contains recent events",
    after_help = AFTER_HELP,
    after_long_help = AFTER_HELP,
    trailing_var_arg = true
)]
pub struct Args {
    /// Compresses truncated head of log with gzip
    ///
    /// To conserve space, compress the truncated head of the log with Gzip.
    #[arg(short = 'g', long)]
    pub gzip: bool,

    /// earliest time to preserve in log
    ///
    /// All the <date> arguments are concatenated to produce the cutoff date. Events earlier
    /// than this moment will be preserved in the truncated head of the log. Events on or after
    /// this date will remain in the active log.
    #[arg(value_name = "date", required = true, num_args = 1..)]
    pub date: Vec<String>,
}

pub fn run(directory: Option<&str>, args: &Args) {
    let time_expression = args.date.join(" ");
    let conf = Configuration::read(None, directory);
    if parsable(&time_expression) {
        let range = parse(&time_expression, conf.two_timer_config()).unwrap();
        let t = range.start;
        let mut log = LogController::new(None, &conf).expect("could not read the log file");
        if let Some(item) = log.find_line(&t) {
            let filename = format!("log.head-to-{}", t).replace(' ', "_");
            let filename = if args.gzip {
                format!("{}.gz", filename)
            } else {
                filename
            };
            let mut path = base_dir(conf.directory());
            path.push(&filename);
            if path.as_path().exists() {
                let overwrite = yes_or_no(format!(
                    "file {} already exists; overwrite?",
                    path.to_str().unwrap()
                ));
                if !overwrite {
                    fatal("could not truncate log", &conf);
                }
            }
            if temp_log_path(conf.directory()).as_path().exists() {
                let overwrite = yes_or_no(format!(
                    "the temporary log file {} already exists; overwrite?",
                    temp_log_path(conf.directory()).to_str().unwrap()
                ));
                if !overwrite {
                    fatal("could not truncate log", &conf);
                }
            }
            let offset = log.larry.offset(item.offset()).unwrap() as usize;
            let mut bytes_read = 0;
            let original_file =
                File::open(log_path(conf.directory())).expect("cannot open log file for reading");
            let mut reader = BufReader::new(original_file);
            let head_file = File::create(path)
                .unwrap_or_else(|_| panic!("could not open {} for writing", filename));
            let mut head_writer = BufWriter::new(head_file);
            let mut buffer = vec![0u8; BUFFER_SIZE];
            if args.gzip {
                let mut encoder = GzEncoder::new(head_writer, Compression::best());
                while bytes_read < offset {
                    let delta = (offset - bytes_read).min(BUFFER_SIZE);
                    reader
                        .read_exact(&mut buffer[..delta])
                        .expect("failed to read data from log");
                    encoder
                        .write_all(&buffer[..delta])
                        .expect("failed to write data to head file");
                    bytes_read += delta;
                }
                encoder
                    .finish()
                    .expect("failed to complete compression of head file");
            } else {
                while bytes_read < offset {
                    let delta = (offset - bytes_read).min(BUFFER_SIZE);
                    reader
                        .read_exact(&mut buffer[..delta])
                        .expect("failed to read data from log");
                    head_writer
                        .write_all(&buffer[..delta])
                        .expect("failed to write data to head file");
                    bytes_read += delta;
                }
                head_writer.flush().expect("failed to close head file");
            }
            let tail_file = File::create(temp_log_path(conf.directory()))
                .expect("could not open log.tmp for writing");
            let mut tail_writer = BufWriter::new(tail_file);
            loop {
                let bytes_read = reader.read(&mut buffer).expect("failed to read from log");
                if bytes_read == 0 {
                    tail_writer.flush().expect("failed to close log.tmp");
                    break;
                }
                tail_writer
                    .write_all(&buffer[..bytes_read])
                    .expect("failed to write to log.tmp");
            }
            std::fs::rename(temp_log_path(conf.directory()), log_path(conf.directory()))
                .expect("failed to copy new log file into place");
            success(
                format!("saved truncated portion of log to {}", filename),
                &conf,
            );
        } else {
            warn(
                format!(
                    "could not find anything in log on or after '{}'; not truncating",
                    time_expression
                ),
                &conf,
            );
        }
    } else {
        fatal(
            format!("cannot parse '{}' as a time expression", time_expression),
            &conf,
        );
    }
}

fn temp_log_path(directory: Option<&str>) -> std::path::PathBuf {
    let mut path = base_dir(directory);
    path.push("log.tmp");
    path
}
