use std::{
    collections::HashMap,
    fmt,
    io::{BufRead, Write},
    str::FromStr,
};

use askama::Template;
use chrono::{Datelike, Month, Weekday};
use color_eyre::eyre::Context;
use itertools::Itertools;

const MONTHS: [Month; 12] = [
    Month::January,
    Month::February,
    Month::March,
    Month::April,
    Month::May,
    Month::June,
    Month::July,
    Month::August,
    Month::September,
    Month::October,
    Month::November,
    Month::December,
];

fn days_in_month(year: i32, month: Month) -> u32 {
    match month {
        Month::April | Month::June | Month::September | Month::November => 30,

        Month::January
        | Month::March
        | Month::May
        | Month::July
        | Month::August
        | Month::October
        | Month::December => 31,

        Month::February => {
            if (year % 4) == 0 {
                29
            } else {
                28
            }
        }
    }
}

trait WeekdayExt {
    fn name(self) -> &'static str;

    fn is_weekend(self) -> bool;
}

impl WeekdayExt for Weekday {
    fn name(self) -> &'static str {
        match self {
            Self::Mon => "Monday",
            Self::Tue => "Tuesday",
            Self::Wed => "Wednesday",
            Self::Thu => "Thursday",
            Self::Fri => "Friday",
            Self::Sat => "Saturday",
            Self::Sun => "Sunday",
        }
    }

    fn is_weekend(self) -> bool {
        matches!(self, Self::Sat | Self::Sun)
    }
}

fn find_date(
    year: i32,
    month: Month,
    day: u32,
    target: Weekday,
    direction: i64,
) -> chrono::NaiveDate {
    let mut date = chrono::NaiveDate::from_ymd_opt(year, month.number_from_month(), day).unwrap();

    while date.weekday() != target {
        date += chrono::Duration::days(direction);
    }

    date
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
enum GroupId {
    #[default]
    NoGroup,
    Group(usize),
}

impl fmt::Display for GroupId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoGroup => Ok(()),
            Self::Group(id) => write!(f, "eventgroup{id}"),
        }
    }
}

#[derive(Debug)]
struct Event {
    month: Month,
    day: u32,
    title: String,
    group_id: GroupId,
}

impl Event {
    fn new(
        date: chrono::NaiveDate,
        title: String,
        group_id: GroupId,
    ) -> color_eyre::eyre::Result<Self> {
        let month = Month::try_from((date.month0() + 1) as u8).wrap_err("Failed to get month")?;
        let day = date.day0() + 1;

        Ok(Self {
            month,
            day,
            title,
            group_id,
        })
    }

    fn nth_weekday(
        year: i32,
        month: Month,
        weekday: Weekday,
        index: i16,
        title: &str,
        group_id: GroupId,
    ) -> Option<Self> {
        match index.cmp(&0) {
            std::cmp::Ordering::Equal => {
                println!("nth weekday cannot be 0");
                None
            }
            std::cmp::Ordering::Greater => {
                let first = find_date(year, month, 1, weekday, 1);

                let event_day = first + chrono::Duration::weeks((index - 1).into());

                (event_day.with_day0(first.day0()) == Some(first))
                    .then(|| Self::new(event_day, title.into(), group_id).ok())
                    .flatten()
            }
            std::cmp::Ordering::Less => {
                let last = find_date(year, month, days_in_month(year, month), weekday, -1);

                let event_day = last + chrono::Duration::weeks((index + 1).into());

                (event_day.with_day0(last.day0()) == Some(last))
                    .then(|| Self::new(event_day, title.into(), group_id).ok())
                    .flatten()
            }
        }
        .or_else(|| {
            println!("{index}'th {weekday} does not exist");

            None
        })
    }

    fn parse(input: &str, year: i32, group_id: GroupId) -> color_eyre::eyre::Result<Vec<Self>> {
        let Some((index, category, title)) = Some(input).and_then(|input| {
            let space_or_tab = |c: char| c == ' ' || c == '\t';

            let (index, input) = input.trim().split_once(space_or_tab)?;
            let (month_or_weekday, title) = input.trim().split_once(space_or_tab)?;

            Some((index.trim(), month_or_weekday.trim(), title.trim()))
        }) else {
            color_eyre::eyre::bail!("Invalid event: {input}")
        };

        let index = index
            .parse::<i16>()
            .wrap_err_with(|| format!("Invalid index {index}"))?;

        Ok(if category.eq_ignore_ascii_case("easter") {
            let easter = computus::gregorian(year)
                .map_err(|err| color_eyre::eyre::eyre!("Failed to calculate easter: {err}"))
                .map(|computus::Date { year, month, day }| {
                    chrono::NaiveDate::from_ymd_opt(year, month, day).unwrap()
                })?;

            vec![Self::new(
                easter + chrono::Duration::days(index.into()),
                title.into(),
                group_id,
            )?]
        } else if let Some((weekday, month)) =
            category.split_once('/').and_then(|(weekday, month)| {
                Some((
                    Weekday::from_str(weekday).ok()?,
                    Month::from_str(month).ok()?,
                ))
            })
        {
            Event::nth_weekday(year, month, weekday, index, title, group_id)
                .into_iter()
                .collect()
        } else if let Ok(month) = Month::from_str(category) {
            vec![Self::new(
                index
                    .try_into()
                    .ok()
                    .and_then(|day| {
                        chrono::NaiveDate::from_ymd_opt(year, month.number_from_month(), day)
                    })
                    .ok_or_else(|| {
                        color_eyre::eyre::eyre!("Invalid date {year}/{}/{index}", month.name())
                    })?,
                title.into(),
                group_id,
            )?]
        } else if let Ok(weekday) = Weekday::from_str(category) {
            MONTHS
                .iter()
                .filter_map(|&month| {
                    Self::nth_weekday(year, month, weekday, index, title, group_id)
                })
                .collect()
        } else {
            color_eyre::eyre::bail!("Invalid event: {input}")
        })
    }
}

#[derive(Debug)]
struct EventGroup {
    id: GroupId,
    title: String,
    style: Option<String>,
    events: Vec<Event>,
}

#[derive(Default)]
struct EventWithGroupId {
    title: String,
    group_id: GroupId,
}

enum CalendarCell {
    Empty,
    Day {
        day: u32,
        events: Vec<EventWithGroupId>,
    },
    MonthAndYear {
        month: Month,
        year: i32,
    },
}

#[derive(Template)]
#[template(path = "calendar.http", escape = "html")]
struct Calendar {
    calendar_event_styles: Vec<(GroupId, String)>,
    events: Vec<Vec<CalendarCell>>,
}

enum DiaryCell {
    Empty,
    Day {
        weekday: Weekday,
        day: u32,
        events: Vec<EventWithGroupId>,
    },
}

struct DiaryPage {
    month: Month,
    cells: Vec<DiaryCell>,
}

#[derive(Template)]
#[template(path = "diary.http", escape = "html")]
struct Diary {
    calendar_event_styles: Vec<(GroupId, String)>,
    pages: Vec<Vec<DiaryPage>>,
}

#[derive(PartialEq, Eq, Hash)]
struct MonthAndDay {
    month: Month,
    day: u32,
}

#[derive(clap::Subcommand)]
enum Command {
    /// List all event groups and exit
    ListEventGroups,
    /// Generate a Calendar
    Calendar,
    /// Generate a Diary
    Diary,
}

enum Output {
    Calendar,
    Diary,
}

#[derive(clap::Parser)]
struct Args {
    /// The calendar to read events from
    #[clap(short, long)]
    calendar_file: Option<std::path::PathBuf>,
    /// The year to create a calendar for
    #[clap(short, long)]
    year: Option<i32>,
    /// Include events from all event groups
    #[clap(short = 'a', long)]
    include_all_events: bool,
    /// Include events from the following event group. Can be included multiple times to included events from multiple event groups.
    #[clap(short = 'e', long, value_name = "EVENT GROUP NAME")]
    include_events: Option<Vec<String>>,
    /// List all event groups and exit
    #[clap(subcommand)]
    command: Option<Command>,
}

fn main() -> color_eyre::eyre::Result<()> {
    let Args {
        calendar_file,
        year,
        include_all_events,
        include_events,
        command,
    } = clap::Parser::parse();

    color_eyre::install()?;

    let mut stdin = std::io::stdin().lines();
    let mut stdout = std::io::stdout();

    let calendar_file = match calendar_file {
        Some(calendar_file) => calendar_file,
        None => {
            print!("Enter Calendar File: ");
            stdout.flush().unwrap();

            let Some(next_line) = stdin.next() else {
                return Ok(());
            };

            next_line
                .wrap_err("Failed to read calendar file path")?
                .into()
        }
    };

    let calendar_text = std::fs::read_to_string(&calendar_file)
        .wrap_err_with(|| format!("Failed to read {}", calendar_file.display()))?;

    let year = match year {
        Some(year) => year,
        None => loop {
            print!("Enter year: ");
            stdout.flush().unwrap();

            let Some(next_line) = stdin.next() else {
                return Ok(());
            };

            let year = next_line.wrap_err("Failed to read year")?;

            break match year.parse() {
                Ok(year) => year,
                Err(err) => {
                    println!("Invalid year ({year:?}): {err}");
                    continue;
                }
            };
        },
    };

    let mut event_groups = Vec::new();

    for (line_num, line) in calendar_text.lines().enumerate() {
        let line_num = line_num + 1;

        let line = line.trim();

        if line.is_empty() {
            continue;
        }

        if let Some(line) = line.strip_prefix('[') {
            let Some(title_and_style) = line.strip_suffix(']') else {
                color_eyre::eyre::bail!(
                    "Error on line {line_num}: Event Group titles must end with a ']'"
                );
            };

            let (title, style) = title_and_style
                .split_once(':')
                .map(|(title, style)| (title.trim(), Some(style.into())))
                .unwrap_or((title_and_style, None));
            let id = GroupId::Group(event_groups.len());

            event_groups.push(EventGroup {
                id,
                title: title.trim().into(),
                style,
                events: Vec::new(),
            });
        } else {
            let Some(current_group) = event_groups.last_mut() else {
                color_eyre::eyre::bail!("Calendar must start with an event group");
            };

            let group_id = current_group.id;

            current_group
                .events
                .extend(Event::parse(line, year, group_id)?)
        }
    }

    let output = match command {
        Some(Command::ListEventGroups) => {
            for EventGroup { title, .. } in event_groups {
                println!("{title}");
            }

            return Ok(());
        }
        Some(Command::Calendar) => Output::Calendar,
        Some(Command::Diary) => Output::Diary,
        None => loop {
            print!("What would you like to generate?");
            stdout.flush().unwrap();

            break match stdin
                .next()
                .transpose()
                .unwrap()
                .unwrap_or_default()
                .as_str()
            {
                "calendar" => Output::Calendar,
                "diary" => Output::Diary,
                _ => {
                    println!(r#"Please enter "calendar" or "diary""#);
                    continue;
                }
            };
        },
    };

    let mut calendar_events = HashMap::new();
    let mut calendar_event_styles = Vec::new();

    for EventGroup {
        id: group_id,
        title,
        style,
        events,
    } in event_groups
    {
        let include_group = include_all_events
            || match &include_events {
                Some(include_events) => include_events
                    .iter()
                    .any(|include_events| include_events.eq_ignore_ascii_case(&title)),
                None => loop {
                    print!("Include {title:?} (y/n)?: ");
                    stdout.flush().unwrap();

                    break match stdin
                        .next()
                        .transpose()
                        .unwrap()
                        .unwrap_or_default()
                        .trim()
                        .to_lowercase()
                        .as_str()
                    {
                        "" | "y" | "yes" => true,
                        "n" | "no" => false,
                        _ => {
                            println!(r#"Please enter "y" or "n""#);
                            continue;
                        }
                    };
                },
            };

        if !include_group {
            continue;
        }

        if let Some(style) = style {
            calendar_event_styles.push((group_id, style));
        }

        for Event {
            month,
            day,
            title,
            group_id,
        } in events
        {
            calendar_events
                .entry(MonthAndDay { month, day })
                .or_insert_with(Vec::new)
                .push(EventWithGroupId { title, group_id });
        }
    }

    let output = match output {
        Output::Calendar => Calendar {
            calendar_event_styles,
            events: MONTHS
                .iter()
                .map(|&month| {
                    let days_before_start =
                        chrono::NaiveDate::from_ymd_opt(year, month.number_from_month(), 1)
                            .unwrap()
                            .weekday()
                            .num_days_from_monday() as usize;

                    std::iter::repeat_with(|| CalendarCell::Empty)
                        .take(days_before_start)
                        .chain((1..=days_in_month(year, month)).map(|day| {
                            CalendarCell::Day {
                                day,
                                events: calendar_events
                                    .remove(&MonthAndDay { month, day })
                                    .unwrap_or_default(),
                            }
                        }))
                        .chain(std::iter::repeat_with(|| CalendarCell::Empty))
                        .take(33)
                        .chain(std::iter::once(CalendarCell::MonthAndYear { month, year }))
                        .collect()
                })
                .collect(),
        }
        .render(),
        Output::Diary => Diary {
            calendar_event_styles,
            pages: MONTHS
                .iter()
                .flat_map(|&month| {
                    (1..=days_in_month(year, month))
                        .map(|day| DiaryCell::Day {
                            weekday: chrono::NaiveDate::from_ymd_opt(
                                year,
                                month.number_from_month(),
                                day,
                            )
                            .unwrap()
                            .weekday(),
                            day,
                            events: calendar_events
                                .remove(&MonthAndDay { month, day })
                                .unwrap_or_default(),
                        })
                        .chain(std::iter::repeat_with(|| DiaryCell::Empty))
                        .chunks(16)
                        .into_iter()
                        .map(Vec::from_iter)
                        .take(2)
                        .map(|cells| DiaryPage { month, cells })
                        .collect_vec()
                })
                .chunks(8)
                .into_iter()
                .map(Vec::from_iter)
                .collect(),
        }
        .render(),
    }
    .unwrap();

    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .wrap_err("Failed to listen over HTTP")?;

    let http_port = listener
        .local_addr()
        .wrap_err("Failed to get http address")?
        .port();

    webbrowser::open(&format!("http://127.0.0.1:{http_port}/"))
        .wrap_err("Failed to open web browser")?;

    let (mut socket, _remote_address) = listener
        .accept()
        .wrap_err("Failed to accept http connection")?;

    for line in std::io::BufReader::new(&mut socket).lines() {
        if line
            .wrap_err("Failed to read HTTP request")?
            .trim()
            .is_empty()
        {
            break;
        }
    }

    socket
        .write_all(output.as_bytes())
        .wrap_err("Failed to write HTTP response")
}
