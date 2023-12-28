use std::{
    collections::HashMap,
    io::{BufRead, Write},
};

use askama::Template;
use chrono::{Datelike, Month, Weekday};
use color_eyre::eyre::Context;

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

const WEEKDAYS: [Weekday; 7] = [
    Weekday::Mon,
    Weekday::Tue,
    Weekday::Wed,
    Weekday::Thu,
    Weekday::Fri,
    Weekday::Sat,
    Weekday::Sun,
];

#[derive(Debug)]
struct Event {
    year: i32,
    month: Month,
    day: u32,
    title: String,
}

impl Event {
    fn new(date: chrono::NaiveDate, title: String) -> color_eyre::eyre::Result<Self> {
        let year = date.year_ce().1.try_into().wrap_err("Failed to get year")?;
        let month = Month::try_from((date.month0() + 1) as u8).wrap_err("Failed to get month")?;
        let day = date.day0() + 1;

        Ok(Self {
            year,
            month,
            day,
            title,
        })
    }

    fn day_is_within_month(&self) -> bool {
        (1..=days_in_month(self.year, self.month)).contains(&self.day)
    }

    fn parse(input: &str, year: i32) -> color_eyre::eyre::Result<Vec<Self>> {
        let Some((day, month_or_weekday, title)) = Some(input).and_then(|input| {
            let space_or_tab = |c: char| c == ' ' || c == '\t';

            let (day, input) = input.trim().split_once(space_or_tab)?;
            let (month_or_weekday, title) = input.trim().split_once(space_or_tab)?;

            Some((day.trim(), month_or_weekday.trim(), title.trim()))
        }) else {
            color_eyre::eyre::bail!("Invalid event: {input}")
        };

        let day = day.parse().wrap_err_with(|| format!("Invalid day {day}"))?;

        for month in MONTHS {
            let month_name = month.name();

            if month_name.eq_ignore_ascii_case(month_or_weekday)
                || month_name
                    .get(0..3)
                    .is_some_and(|month_name| month_name.eq_ignore_ascii_case(month_or_weekday))
            {
                return Ok([Self {
                    year,
                    month,
                    day,
                    title: title.into(),
                }]
                .into_iter()
                .filter(Event::day_is_within_month)
                .collect());
            }
        }

        for weekday in WEEKDAYS {
            let weekday_name = match weekday {
                Weekday::Mon => "Monday",
                Weekday::Tue => "Tuesday",
                Weekday::Wed => "Wednesday",
                Weekday::Thu => "Thursday",
                Weekday::Fri => "Friday",
                Weekday::Sat => "Saturday",
                Weekday::Sun => "Sunday",
            };

            if weekday_name.eq_ignore_ascii_case(month_or_weekday)
                || weekday_name
                    .get(0..3)
                    .is_some_and(|month_name| month_name.eq_ignore_ascii_case(month_or_weekday))
            {
                return Ok(MONTHS
                    .iter()
                    .filter_map(|&month| {
                        let day = chrono::NaiveDate::from_weekday_of_month_opt(
                            year,
                            month as u32,
                            weekday,
                            day as u8,
                        )?
                        .day0()
                            + 1;

                        Some(Self {
                            year,
                            month,
                            day,
                            title: title.into(),
                        })
                    })
                    .filter(Event::day_is_within_month)
                    .collect());
            }
        }

        color_eyre::eyre::bail!("Invalid event: {input}")
    }
}

#[derive(Debug)]
struct EventGroup {
    title: String,
    events: Vec<Event>,
}

#[derive(Clone)]
enum CalendarCell {
    Empty,
    Day { day: u32, events: Vec<String> },
    MonthAndYear { month: Month, year: i32 },
}

#[derive(Template)]
#[template(path = "calendar.http", escape = "html")]
struct Calendar {
    events: Vec<Vec<CalendarCell>>,
}

#[derive(PartialEq, Eq, Hash)]
struct MonthAndDay {
    month: Month,
    day: u32,
}

#[derive(clap::Parser)]
struct Args {
    #[clap(short, long)]
    calendar_file: Option<std::path::PathBuf>,
    #[clap(short, long)]
    year: Option<i32>,
    #[clap(short = 'a', long)]
    include_all_events: bool,
    #[clap(short = 'e', long)]
    include_events: Option<Vec<String>>,
    #[clap(long)]
    list_event_groups: bool,
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

fn main() -> color_eyre::eyre::Result<()> {
    let Args {
        calendar_file,
        year,
        include_all_events,
        include_events,
        list_event_groups,
    } = clap::Parser::parse();

    color_eyre::install()?;

    let mut stdin = std::io::stdin().lines();
    let mut stdout = std::io::stdout();

    let calendar_file = match calendar_file {
        Some(calendar_file) => calendar_file,
        None => {
            print!("Enter Calendar File: ");
            stdout.flush().wrap_err("Failed to flush stdout")?;

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
            stdout.flush().wrap_err("Failed to flush stdout")?;

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
            let Some(title) = line.strip_suffix(']') else {
                color_eyre::eyre::bail!(
                    "Error on line {line_num}: Event Group titles must end with a ']'"
                );
            };

            event_groups.push(EventGroup {
                title: title.trim().into(),
                events: Vec::new(),
            });
        } else {
            let Some(current_group) = event_groups.last_mut() else {
                color_eyre::eyre::bail!("Calendar must start with an event group");
            };

            current_group.events.extend(Event::parse(line, year)?)
        }
    }

    let easter = computus::gregorian(year)
        .map_err(|err| color_eyre::eyre::eyre!("Failed to calculate easter: {err}"))
        .map(|computus::Date { year, month, day }| {
            chrono::NaiveDate::from_ymd_opt(year, month, day).unwrap()
        })?;

    event_groups.push(EventGroup {
        title: "Easter Days".into(),
        events: [
            ("Ash Wednesday", -46),
            ("Palm Sunday", -7),
            ("Good Friday", -2),
            ("Easter", 0),
            ("Easter Monday", 1),
            ("Ascension (HO)", 39),
            ("Pentecost", 49),
            ("Pentecost Monday", 50),
            ("Fête-Dieu", 60),
            ("Corpus Christi", 63),
        ]
        .into_iter()
        .map(|(title, easter_offset)| {
            Event::new(easter + chrono::Duration::days(easter_offset), title.into())
        })
        .collect::<Result<_, _>>()?,
    });

    event_groups.push(EventGroup {
        title: "British Summer Time".into(),
        events: vec![
            Event::new(
                find_date(year, Month::March, 31, Weekday::Sun, -1),
                "BST Begins".into(),
            )?,
            Event::new(
                find_date(year, Month::October, 31, Weekday::Sun, -1),
                "BST Ends".into(),
            )?,
        ],
    });

    if list_event_groups {
        for EventGroup { title, .. } in event_groups {
            println!("{title}");
        }

        return Ok(());
    }

    let mut calendar_events = HashMap::new();

    for EventGroup { title, events } in event_groups {
        let include_group = include_all_events
            || match &include_events {
                Some(include_events) => include_events
                    .iter()
                    .any(|include_events| include_events.eq_ignore_ascii_case(&title)),
                None => loop {
                    print!("Include {title:?} (y/n)?: ");
                    stdout.flush().wrap_err("Failed to flush stdout")?;

                    break match stdin
                        .next()
                        .transpose()
                        .wrap_err("Failed to read from stdin")?
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

        for Event {
            year: _,
            month,
            day,
            title,
        } in events
        {
            calendar_events
                .entry(MonthAndDay { month, day })
                .or_insert_with(Vec::new)
                .push(title);
        }
    }

    let calendar = Calendar {
        events: MONTHS
            .iter()
            .map(|&month| {
                let days_before_start =
                    chrono::NaiveDate::from_ymd_opt(year, month.number_from_month(), 1)
                        .unwrap()
                        .weekday()
                        .num_days_from_monday() as usize;

                std::iter::repeat(CalendarCell::Empty)
                    .take(days_before_start)
                    .chain((1..=days_in_month(year, month)).map(|day| {
                        CalendarCell::Day {
                            day,
                            events: calendar_events
                                .get(&MonthAndDay { month, day })
                                .map(Vec::as_slice)
                                .unwrap_or_default()
                                .into(),
                        }
                    }))
                    .chain(std::iter::repeat(CalendarCell::Empty))
                    .take(33)
                    .chain(std::iter::once(CalendarCell::MonthAndYear { month, year }))
                    .collect()
            })
            .collect(),
    };

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

    calendar
        .write_into(&mut socket)
        .wrap_err("Failed to write HTTP response")?;

    Ok(())
}
