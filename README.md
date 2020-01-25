# jobrog

A rewrite of [JobLog](https://metacpan.org/pod/App::JobLog) in Rust.

JobLog, referred to herein variously as "JobLog", "Job Log", "jobrog", "job log", and "job", is a command line utility
that helps one keep track of what one does in a day. With a little anonymization, here is an example of an actual
report of use in the wild:

    > job summary last friday
    Friday, 10 January
      8:55 am - 9:35     0.75  e, o        email                                                         
         9:35 - 9:41     0.00  f, o        filing time                                                   
         9:41 - 10:50    1.25  30, mr, fo  Create booster view which lets you add people to booster      
        10:50 - 12:15    1.50  10, mr, fo  get multi-tenant S3 attachments to work                       
        12:15 - 12:16    0.00  c, o        feeding the cat                                               
        12:16 - 3:46 pm  3.50  10, mr, fo  get multi-tenant S3 attachments to work                       
         3:46 - 3:50     0.00  29, mr, fo  Make it so the Plugh API is only called for gargamel stuff
         3:50 - 3:50     0.00  mtg, fo     FO/UPI Monthly Check-In                                       
         3:50 - 4:01     0.25  29, mr, fo  Make it so the Plugh API is only called for gargamel stuff
         4:01 - 4:30     0.50  mtg, fo     FO/UPI Monthly Check-In                                       
         4:30 - 5:01     0.50  29, mr, fo  Make it so the Plugh API is only called for gargamel stuff
    
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

There are many alternatives to JobLog. One can use [Harvest](https://www.getharvest.com/), for instance. The advantages of JobLog
over these alternatives are
* your data is on your own machine; it is your own file; you can keep it across changes of employer
* if live on the command line, or typically have one handy, the mental context switch, and manual dexterity, is less when one changes tasks; one simply tabs to the command line and types `job a new thing I'm doing`
* it doesn't need and internet connection
* job log is blazingly fast
* job log keeps random notes for you as well as events; this sometimes is helpful

The typical things one does with job log are
* register a change of task
* take a note
* register going off the clock
* summarize a period to enter it into some other time tracking system

Here is the complete list (`job help`):

    testing 0.1.0
    dfhoughton <dfhoughton@gmail.com>
    command line job clock
    
    USAGE:
        job [SUBCOMMAND]
    
    FLAGS:
        -h, --help       Prints help information
        -V, --version    Prints version information
    
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
    
      > job add creating demonstration events in the log
      starting creating demonstration events in the log (no tags)
      > job add events have a duration
      starting events have a duration (no tags)
      > sleep 60
      > job add --tag foo tags facilitate searching and aggregation
      starting tags facilitate searching and aggregation (tags: foo)
      > job note you can take notes as well
      noted you can take notes as well (no tags)
      > job note notes are events without a duration
      noted notes are events without a duration (no tags)
      > job add you can go off the clock
      starting you can go off the clock (no tags)
      > job done
      ending you can go off the clock at 11:13 am
      > job resume --tag foo
      starting tags facilitate searching and aggregation (tags: foo)
      > job note you can resume an earlier event
      noted you can resume an earlier event (no tags)
      > job note you can summarize the log
      noted you can summarize the log (no tags)
      > job summary today
      Sunday, 19 January
        11:11 am - 11:12    0.021       creating demonstration events in the log; events have a duration
           11:12 - 11:13    0.006  foo  tags facilitate searching and aggregation
           11:13 - 11:13    0.001       you can go off the clock
           11:13 - ongoing  0.007  foo  tags facilitate searching and aggregation
      
      TOTAL HOURS 0.036
      UNTAGGED    0.022
      foo         0.013
      > job summary --notes today
      Sunday, 19 January
        11:12 am    you can take notes as well
        11:12       notes are events without a duration
        11:13       you can resume an earlier event
        11:13       you can summarize the log
      > job note you can configure job
      noted you can configure job (no tags)
      > job configure --precision quarter
      setting precision to quarter!
      > job summary today
      Sunday, 19 January
        11:11 am - 11:12    0.00       creating demonstration events in the log; events have a duration
           11:12 - 11:13    0.00  foo  tags facilitate searching and aggregation
           11:13 - 11:13    0.00       you can go off the clock
           11:13 - ongoing  0.00  foo  tags facilitate searching and aggregation
      
      TOTAL HOURS 0.00
      UNTAGGED    0.00
      foo         0.00
    
    This version of job is a Rust implementation: https://github.com/dfhoughton/jobrog. The original implementation was in
    Perl: https://metacpan.org/pod/App::JobLog.

## Installation

At the moment the only mechanism is

    cargo install jobrog

I will add a homebrew tap shortly.

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

## Why Rewrite App::JobLog?

* Everyone's doing it!
* Rust is fun!
* The rust version is perceptibly faster and in some cases usefully faster.
* My Perl skills were in little demand and thus becoming rusty.
