# Change Log

## 0.2.0
* fixed bug in statistics where the first reported timestamp was that immediately before that sought -- stats were off by one log line
## 0.1.9 *2020-2-8*
* better time string formatting
* changes to other formatting
* allow zero-padding of times in timestamps
## 0.1.8 *2020-2-4*
* fixed when-on-fresh-day bug
## 0.1.7 *2020-2-2*
* fixed unsetting workdays bug
* improved screencasts
## 0.1.6 *2020-2-2*
* make it so you can unset the clock configuration parameter
* various documentation fixes
* fix editor configuration parameter so you can pass arguments
## 0.1.5 *2020-2-1*
* changing text emitted by resume subcommand
* adding vacation information to JSON output of summary subcommand
* adding precision and truncation to summary subcommand
* bumped colonnade version to get better grapheme handling
* adding hours clocked to statistics and allowing one to collect statistics by time period
* made 12 vs 24 hour clock configurable and removed am/pm
* fixed time display so colons line up
## 0.1.4 *2020-1-26*
* fix use of max_termsize so we can test things outside of tty
* added 'today' alias to the summary subcommand for Paula's sake
## 0.1.3 *2020-1-25*
* added success message type
* fix editing-to-add-DONE bug
* fixed backup deletion bug
* added --error-comments flag to edit subcommand
* added --directory option to facilitate testing in homebrew
## 0.1.2 *2020-1-23*
* adding more fractional precision options: half hour, third, etc.
## 0.1.1 *2020-1-20*
* adding JSON output as summary option
