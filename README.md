# calendar-rs

A printable calendar generator.

## Event format

  + The start of an event group is a title surrounded by square brackets (`[` and `]`)
  + `day` `month` `title` - A single event on the specified day of the specified month
    + e.g. `7 April Event Name` is an event called "Event Name" on the 7th of April
  + `index` `weekday` `title` - An event on the `index`'th `weekday` of each month
    + e.g. `2 Wednesday Event Name` is an event called "Event Name" on the 2nd Wednesday of each month
  + `index` `weekday`/`month` `title` is an event called "Event Name" on the `index`'th `weekday` of `month`
    + e.g. `3 Friday/July Event Name` is an event called "Event Name" on the 3rd Friday of July
  + `offset` easter `title` - A single event `offset` days from Easter Sunday
    + e.g. `1 easter Easter Monday` is an event called "Easter Monday" on the day after Easter Sunday