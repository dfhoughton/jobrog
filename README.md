<!-- omit in toc -->
# jobrog

A rewrite of [JobLog](https://metacpan.org/pod/App::JobLog) in Rust.

JobLog, referred to herein variously as "JobLog", "Job Log", "jobrog", "job log", and "job", is a command line utility
that helps one keep track of what one does in a day. With a little anonymization, here is an example of an actual
report of use in the wild:

    > job summary last friday
    Friday, 10 January
      8:55 -  9:35  0.75  e, o        email
      9:35 -  9:41  0.00  f, o        filing time
      9:41 - 10:50  1.25  30, mr, fo  Create booster view which lets you add people to booster
     10:50 - 12:15  1.50  10, mr, fo  get multi-tenant S3 attachments to work
     12:15 - 12:16  0.00  c, o        feeding the cat
     12:16 -  3:46  3.50  10, mr, fo  get multi-tenant S3 attachments to work
      3:46 -  3:50  0.00  29, mr, fo  Make it so the Plugh API is only called for gargamel stuff
      3:50 -  3:50  0.00  mtg, fo     FO/UPI Monthly Check-In
      3:50 -  4:01  0.25  29, mr, fo  Make it so the Plugh API is only called for gargamel stuff
      4:01 -  4:30  0.50  mtg, fo     FO/UPI Monthly Check-In
      4:30 -  5:01  0.50  29, mr, fo  Make it so the Plugh API is only called for gargamel stuff

    TOTAL HOURS 8.00
    10          5.00
    29          0.75
    30          1.25
    c           0.00
    e           0.75
    f           0.00
    mr          6.75
    mtg         0.50
    o           0.75
    fo          7.25

In this case the user (me) has typed something like

    job add --tag email --tag o email

or, more likely,

    job a -t e -t o email

or still more likely,

    job r -t e

and added a line to `~/.joblog/log` which looks like

    2020  1 10  8 55 27:e o:email

Job log lets one manage a log of one's activities as a log file. A log line consists of a timestamp, some metadata, and a description of
the current event.

<!-- omit in toc -->
## Table of Contents

- [Screencasts](#screencasts)
- [Why](#why)
- [How](#how)
- [NOTE](#note)
- [Suggestions](#suggestions)
  - [Pattern of Usage](#pattern-of-usage)
  - [Keeping a TODO List](#keeping-a-todo-list)
- [Installation](#installation)
- [Changes from App::JobLog](#changes-from-appjoblog)
- [Why Rewrite App::JobLog?](#why-rewrite-appjoblog)
- [Acknowledgements](#acknowledgements)

## Screencasts

Watch Job Log in action!

* [tour](https://asciinema.org/a/PsNtfEjmZUIHr6UBbOaqGWeyl)
* [configuration](https://asciinema.org/a/8n8H9MZ9GzgwrvdNyscAdTUYI)
* [when am I done for the day?](https://asciinema.org/a/4jwyN4IIfzAjkXqmxDrcnWxPU)
* [vacation time](https://asciinema.org/a/K1pXQ4DcIobSaiT2XZRhZoZRl)
* [report time by the quarter hour](https://asciinema.org/a/ITDGBCFnoPFyATYE8Wpb9qdCT)
* [what was the last thing I logged?](https://asciinema.org/a/EPYUW38VzW1hNxQyRwzUJAz4r)
* [taking notes](https://asciinema.org/a/TvozRcprzy3joEEP0inuJs7CP)

## Why

There are many alternatives to JobLog. One can use [Harvest](https://www.getharvest.com/), for instance. The advantages of JobLog
over web apps are

* your data is on your own machine; it is your own file; you can keep it across changes of employer
* if you live on the command line, or typically have one handy, the mental context switch and manual dexterity required is less when one changes tasks; one simply tabs to the command line and types `job a new task`
* it doesn't need any internet connection
* job log is blazingly fast
* job log keeps random notes for you as well as events; this sometimes is helpful

Some other command-line time trackers I've come across

* [work_tock](https://crates.io/crates/work_tock)
* [Timewarrior](https://timewarrior.net/)

I'm partial to JobLog, of course, because I wrote it and so it does exactly what I need.

JobLog can produce JSON summaries, so it should be possible to export JobRog events to other time trackers.

## How

The typical things one does with job log are

* register a change of task
* take a note
* register going off the clock
* summarize a period to enter it into some other time tracking system

Here is the complete list (`job help`):

```
testing 0.3.0
dfhoughton <dfhoughton@gmail.com>
command line job clock

USAGE:
    job [OPTIONS] [SUBCOMMAND]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -d, --directory <dir>    Looks in this directory for the log rather than ~/.joblog

SUBCOMMANDS:
    add           Adds a new task
    summary       Says when you will have worked all the hours expected within the given period
    done          Ends a currently open task
    resume        Resumes a stopped task
    last          Shows the last task recorded
    first         Shows the first task recorded
    note          Adds a new note
    when          Says when you will have worked all the hours expected within the given period
    edit          Opens the job log in a text editor
    configure     Sets or displays configuration parameters
    vacation      Records vacation time
    parse-time    Shows the start and end timestamps you get from a particular time expression
    truncate      Truncates the log so it only contains recent events
    statistics    Shows overall statistics of the log
    help          Prints this message or the help of the given subcommand(s)

The 'job' executable allows one to maintain and view a log of daily activity.
```

## NOTE

The examples shown here and throughout the job log documentation are generally the most verbose possible
for the sake of clarity. They all have short forms, however, to save keystrokes. Instead of

```
job add --tag overhead --tag email Reading the morning email.
```

you can type

```
job a -t overhead -t email Reading the morning email.
```

You will probably find that long tags like this are irksome and reduce them as well:

```
job a -t o -t e Reading the morning email.
```

But if there is something you do frequently, the easiest thing to do is to give it a distinctive tag and just resume it:

```
job resume -t e
```

or

```
job r -t e
```

## Suggestions

### Pattern of Usage

If you have to keep a log of activity for billing purposes you often need to keep distinct bins for
different accounts, overhead versus work for a particular client, etc. In addition you may need to keep
track of different projects or subcategories within a particular account. I find it useful, therefore, to
use a major category tag and one or more minor category tags with every task. Typically a non-overhead task
consists of a major category, such as `sb`, a minor billing category, such as `cs`, and a github issue
number. Then when I need to add items to my time sheet I type

    job s -d yesterday -T o -T sb

first to confirm that I remembered to put everything in some major category bin. If this tells me there
are no items, I have succeeded. Then I subdivide the tasks by major category.

    job s -d yesterday -t o

I find this clears away the clutter so the task goes more smoothly.

In a particular major category I find it useful to eliminate things I've already entered.

    job s -d yesterday -t sb -T 123 -T 124 -T 125

This makes it progressively easier to focus on the next thing I need to enter.

### Keeping a TODO List

You can use the `note` subcommand to maintain a todo list.

Add the following or some variant thereof to a shell profile file, `~/.zshrc` in my case:

```bash
# add an item to the TODO list
alias todo="job n -t todo"
# show TODOs yet to do
alias todos="job s -n -t todo -T done"
# mark at TODO as completed
function did {
        local rx=$1; shift
        job tag -fnt todo -T done --rx $rx -a done $*
}
# show completed TODOs
alias triumphs="job s -n -t todo -t done"
```

Now (in a new terminal or after you type `source <shell profile file>` ) to create a todo item you type `todo <what should be done>`.

To list today's todo items you type `todos`.

To list all todo items ever you type `todos ever`; for all this week, `todos this week`; for yesterday's, `todos yesterday`; etc.

To cross a particular item off the list you type `did <some word unique to today's item>`. The thing after `did` is interpreted
as a regular expression. Only the *first* todo item in the given period whose description matches the regular expression will be
marked as done. If you need to mark something as done which you didn't add today you need to provide the appropriate period. E.g.,

    did something yesterday

Here is a [screencast](https://asciinema.org/a/6W7Ap6l5597eFzXEAVQZ3miMe) of some todo list manipulation.

## Installation

To be ensured the latest version, one needs to use [`cargo`](https://doc.rust-lang.org/cargo/):

    cargo install jobrog

There is also a [homebrew](https://brew.sh/) tap:

    brew install dfhoughton/tap/jobrog

## Changes from App::JobLog

For the most part the features of jobrog are a superset of those of [App::JobLog](https://metacpan.org/pod/App::JobLog).
There are some differences, though:

* You can mark when repeating vacation intervals go into effect or become inactive. If you use this feature however, or if
you add a new repeating vacation interval, your vacation file will no longer be readable by App::JobLog. This feature adds two
colon-delimited timestamps to the end of the relevant line. This is the only breaking change I know of.
* There is optional color!
* There is a `first` subcommand parallel to `last`.
* The `today` subcommand has been subsumed into `summary`, which now has "today" as its default period.
* There are fewer compression options for the `truncate` subcommand in the interest of simplicity.
* The filtering options for summaries behave somewhat differently and are, for me, more useful.
* You can round up, round down, or "round center" the durations for lawyer-billing, saint-billing, and ordinary-shmoe-billing modes.
* You can configure jobrog to use fractional hour precision, like quarter and half.
* You can obtain summaries as line-delimited JSON as well as tabulated text.
* The merging and display of summary information is considerably less configurable.
* There is a statistics subcommand if you want a quick overview of a time period.
* There is no modify subcommand.
* The tags subcommand adds or removes tags instead of listing them.

## Why Rewrite App::JobLog?

* Everyone's doing it!
* Rust is fun!
* The Rust version is perceptibly, and in some cases usefully, faster.
* My Perl skills were in little demand and thus becoming rusty.

## Acknowledgements

I would like to thank

* my wife Paula, who has been the only consistent user of Job Log other than myself over the past ten years or so
* my son Jude, who helps me debug stuff and prodded me to get back on task when I was letting the JobLog rewrite lie fallow
* my co-workers, who humor me when I talk about JobLog and then go back to using other mechanisms to keep track of their time
