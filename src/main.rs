use clap::{CommandFactory, Parser, Subcommand};
use jobrog::{
    add, configure, done, edit, first, last, note, parse, resume, statistics, summary, tag,
    truncate, util, vacation, when,
};

const AFTER_HELP: &str =
    "The 'job' executable allows one to maintain and view a log of daily activity.";

#[derive(Parser)]
#[command(
    name = "job",
    version,
    author,
    about = env!("CARGO_PKG_DESCRIPTION"),
    after_help = AFTER_HELP,
    after_long_help = AFTER_HELP
)]
struct Cli {
    /// Looks in this directory for the log rather than ~/.joblog
    ///
    /// If you need or want to use a directory other than .joblog in your home directory
    /// to store job log's log, vacation file, configuration file, and so forth, specify
    /// this alternative directory with --directory. As with .joblog, if it does not
    /// exist it will be created as needed.
    #[arg(short = 'd', long, value_name = "dir")]
    directory: Option<String>,

    #[command(subcommand)]
    command: Option<Commands>,
}

// the declaration order determines the order in which the subcommands are listed in the help
#[derive(Subcommand)]
enum Commands {
    Add(add::Args),
    Summary(summary::Args),
    Done(done::Args),
    Resume(resume::Args),
    Last(last::Args),
    First(first::Args),
    Note(note::Args),
    When(when::Args),
    Tag(tag::Args),
    Edit(edit::Args),
    Configure(configure::Args),
    Vacation(vacation::Args),
    ParseTime(parse::Args),
    Truncate(truncate::Args),
    Statistics(statistics::Args),
}

fn main() {
    let cli = Cli::parse();
    let directory = cli.directory.as_deref();
    util::init(directory);
    match cli.command {
        Some(Commands::Add(args)) => add::run(directory, &args),
        Some(Commands::Summary(args)) => summary::run(directory, &args),
        Some(Commands::Done(args)) => done::run(directory, &args),
        Some(Commands::Resume(args)) => resume::run(directory, &args),
        Some(Commands::Last(args)) => last::run(directory, &args),
        Some(Commands::First(args)) => first::run(directory, &args),
        Some(Commands::Note(args)) => note::run(directory, &args),
        Some(Commands::When(args)) => when::run(directory, &args),
        Some(Commands::Tag(args)) => tag::run(directory, &args),
        Some(Commands::Edit(args)) => edit::run(directory, &args),
        Some(Commands::Configure(args)) => configure::run(directory, &args),
        Some(Commands::Vacation(args)) => vacation::run(directory, &args),
        Some(Commands::ParseTime(args)) => parse::run(directory, &args),
        Some(Commands::Truncate(args)) => truncate::run(directory, &args),
        Some(Commands::Statistics(args)) => statistics::run(directory, &args),
        None => {
            let mut cmd = Cli::command();
            let _ = cmd.print_help();
            println!();
        }
    }
}
