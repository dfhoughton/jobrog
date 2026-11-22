# Change Log

## 2.0.0 *2026-11-21*
* moved to Rust edition 2021; minimum supported Rust version is now 1.85 (`rust-version` in `Cargo.toml`)
* replaced clap 2 with clap 4 (derive API); the command-line interface is unchanged, including aliases and help text
* replaced the pidgin PEG parser with hand-rolled parsers for log and vacation lines, removing the pidgin dependency
* replaced `ansi_term` with `nu-ansi-term` and `term_size` with `terminal_size`
* replaced `lazy_static` with `std::sync::LazyLock`
* upgraded all dependencies to their latest versions (chrono 0.4, regex 1, larry 0.4, two_timer 3, colonnade 2, rust-ini 0.21, dirs 6, flate2 1, serde_json 1)
* fixed a stack overflow in `find_line` when the sought time lay outside the range of timestamps in the log (e.g. `job truncate tomorrow` on a short log)
* optimized release builds (`lto = "fat"`, `codegen-units = 1`, `panic = "abort"`, stripped symbols)
* added integration tests for the CLI (`tests/cli.rs`) and a CI workflow (fmt, clippy `-D warnings`, tests)
* clippy-clean: no warnings with `cargo clippy --all-targets -- -D warnings`; code formatted with `cargo fmt`

## 1.1.2
* upgraded some dependencies -- notably, an upgrade to two-timer allows the parsing of 12 as December in appropriate contexts
## 1.1.1 *2023-1-29*
* bumped dependencies to satisfy cargo-audit
## 1.1.0 *2022-8-6*
* bumped dirs and clapp versions
* added time budget to when command
* fixed listing of fixed vacation times
* improved feedback when unsetting unfamiliar styles and budgets
* general documentation cleanup and extension
## 1.0.2 *2021-7-3*
* escape whitespace in tags
* squish whitespace in tags
## 1.0.1 *2020-11-24*
* change the name displayed by --version to "job"
## 1.0.0 *2020-11-21*
There are no particular changes. I just feel jobrog is sufficiently stable that I'll promote the current
code to 1.0.0.
## 0.3.4 *2020-10-19*
* fix the 0.3.3 bug properly
## 0.3.3 *2020-10-19*
* fix bug where the when subcommand added in vacation time after the present moment
## 0.3.2 *2020-5-17*
* Pull in the latest two-timer.
## 0.3.1 *2020-5-9*
* Fixed validation bug -- was getting a panic on lines that contained impossible timestamps
## 0.3.0 *2020-4-5*
* added tag subcommand
* removed t as an alias for the truncate subcommand so it could be assigned to tag
* added --empty to common filtering options
* prevented merging of events across day boundaries
* improved the display of events that overlap day boundaries
* fixed bug where the event or note immediately preceding the period summarized was being included
* changed the day boundary warning so it doesn't print any events, just the warning
* shortened some long help text
## 0.2.1 *2020-3-7*
* some changes to make styling more consistent
* bumped two-timer version number to get better time parsing
## 0.2.0 *2020-2-22*
* fixed bug in statistics where the first reported timestamp was that immediately before that sought -- stats were off by one log line
* made styles configurable
* simplified styles
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
