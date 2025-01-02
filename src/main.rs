use std::{
    collections::HashMap,
    fmt,
    io::{BufRead, Write},
    str::FromStr,
};

use anyhow::Context;
use askama::Template;
use chrono::{Datelike, Month, Weekday};
use druid::{
    im::Vector,
    text::ArcStr,
    widget::{prelude::*, Button, Checkbox, Flex, Label, List, RadioGroup, Stepper},
    Data, Lens, LensExt, Widget, WidgetExt,
};
use itertools::Itertools;

const HTTP_RESPONSE_HEADER: &str = include_str!("response.http");

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
    #[allow(clippy::wrong_self_convention)]
    fn is_weekend(self) -> bool;
}

impl WeekdayExt for Weekday {
    fn is_weekend(self) -> bool {
        matches!(self, Self::Sat | Self::Sun)
    }
}

fn weekdays(starting: Weekday) -> impl Iterator<Item = Weekday> {
    std::iter::successors(Some(starting), |weekday| Some(weekday.succ()))
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Data)]
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

struct EventWithGroupId {
    title: ArcStr,
    group_id: GroupId,
}

impl Default for EventWithGroupId {
    fn default() -> Self {
        Self {
            title: "".into(),
            group_id: GroupId::default(),
        }
    }
}

struct EventDay {
    day: u32,
}

impl fmt::Display for EventDay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { day } = self;

        write!(f, "{day:02}")
    }
}

enum CalendarCell {
    Empty,
    Day {
        day: EventDay,
        events: Vec<EventWithGroupId>,
    },
    MonthAndYear {
        month: Month,
        year: i32,
    },
}

struct CalendarEventStyles(Vec<(GroupId, ArcStr)>);

impl fmt::Display for CalendarEventStyles {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<style>")?;

        for (id, style) in &self.0 {
            write!(f, ".{id} {{ {style} }}")?;
        }

        write!(f, "</style>")
    }
}

#[derive(Template)]
#[template(path = "monthly_calendar.html")]
struct MonthlyCalendar {
    calendar_event_styles: CalendarEventStyles,
    events: Vec<Vec<CalendarCell>>,
}

enum YearlyCalendarDay<W> {
    Empty {
        weekday: W,
    },
    Day {
        weekday: W,
        day: EventDay,
        events: Vec<EventWithGroupId>,
    },
}

impl YearlyCalendarDay<()> {
    fn with_weekday(self, weekday: Weekday) -> YearlyCalendarDay<Weekday> {
        match self {
            Self::Empty { weekday: () } => YearlyCalendarDay::Empty { weekday },
            Self::Day {
                weekday: (),
                day,
                events,
            } => YearlyCalendarDay::Day {
                weekday,
                day,
                events,
            },
        }
    }
}

impl YearlyCalendarDay<Weekday> {
    fn background_class(&self) -> &'static str {
        let is_weekend = match self {
            YearlyCalendarDay::Empty { weekday } | YearlyCalendarDay::Day { weekday, .. } => {
                weekday.is_weekend()
            }
        };

        if is_weekend {
            "shadedBackground"
        } else {
            ""
        }
    }
}

struct YearlyCalendarMonth {
    month: Month,
    days: Vec<YearlyCalendarDay<Weekday>>,
}

struct YearlyCalendarPage {
    months: Vec<YearlyCalendarMonth>,
}

#[derive(Template)]
#[template(path = "yearly_calendar.html")]
struct YearlyCalendar {
    title: &'static str,
    calendar_event_styles: CalendarEventStyles,
    year: i32,
    weekday_titles: Vec<Weekday>,
    pages: Vec<YearlyCalendarPage>,
}

impl YearlyCalendar {
    const ROWS_COUNT: usize = 37;

    fn body_class(&self) -> &'static str {
        if self.pages.len() == 1 {
            "fullyear"
        } else {
            "halfyear"
        }
    }
}

enum DiaryCell {
    Empty,
    Day {
        weekday: Weekday,
        day: EventDay,
        events: Vec<EventWithGroupId>,
    },
}

struct DiaryPage {
    month: Month,
    cells: Vec<DiaryCell>,
}

#[derive(Template)]
#[template(path = "diary.html")]
struct Diary {
    calendar_event_styles: CalendarEventStyles,
    pages: Vec<Vec<DiaryPage>>,
}

#[derive(PartialEq, Eq, Hash)]
struct MonthAndDay {
    month: Month,
    day: u32,
}

#[derive(Clone, Data, PartialEq, Eq)]
enum Output {
    MonthlyCalendar,
    YearlyCalendar { split_in_two: bool },
    Diary,
}

#[derive(Clone)]
enum EventDescriptionData {
    FixedDate {
        month: Month,
        day: u32,
    },
    NthWeekdayOfMonth {
        n: i16,
        weekday: Weekday,
        // None Means Every Month
        month: Option<Month>,
    },
    DaysAfterEaster {
        day_offset: i16,
    },
    FuzzySunday(Box<EventDescriptionData>),
}

impl EventDescriptionData {
    fn dates(&self, year: i32) -> anyhow::Result<Vec<chrono::NaiveDate>> {
        match *self {
            EventDescriptionData::FixedDate { month, day } => {
                Ok(vec![chrono::NaiveDate::from_ymd_opt(
                    year,
                    month.number_from_month(),
                    day,
                )
                .with_context(|| {
                    format!("Invalid date {year}/{}/{day}", month.name())
                })?])
            }
            EventDescriptionData::NthWeekdayOfMonth { n, weekday, month } => month
                .as_ref()
                .map_or(&MONTHS[..], std::slice::from_ref)
                .iter()
                .filter_map(|&month| {
                    fn nth_weekday(
                        year: i32,
                        month: Month,
                        weekday: Weekday,
                        n: i16,
                    ) -> anyhow::Result<Option<chrono::NaiveDate>> {
                        Ok(match n.cmp(&0) {
                            std::cmp::Ordering::Equal => {
                                anyhow::bail!("nth weekday cannot be 0");
                            }
                            std::cmp::Ordering::Greater => {
                                let first = find_date(year, month, 1, weekday, 1);

                                let event_day = first + chrono::Duration::weeks((n - 1).into());

                                (event_day.with_day0(first.day0()) == Some(first))
                                    .then_some(event_day)
                            }
                            std::cmp::Ordering::Less => {
                                let last =
                                    find_date(year, month, days_in_month(year, month), weekday, -1);

                                let event_day = last + chrono::Duration::weeks((n + 1).into());

                                (event_day.with_day0(last.day0()) == Some(last))
                                    .then_some(event_day)
                            }
                        })
                    }

                    nth_weekday(year, month, weekday, n).transpose()
                })
                .collect(),
            EventDescriptionData::DaysAfterEaster { day_offset } => {
                let easter = computus::gregorian(year)
                    .map_err(|err| anyhow::anyhow!("Failed to calculate Easter: {err}"))
                    .map(|computus::Date { year, month, day }| {
                        chrono::NaiveDate::from_ymd_opt(year, month, day).unwrap()
                    })?;

                Ok(vec![easter + chrono::Duration::days(day_offset.into())])
            }
            EventDescriptionData::FuzzySunday(ref event_description_data) => event_description_data
                .dates(year)?
                .into_iter()
                .map(|date| match date.weekday() {
                    Weekday::Mon => date
                        .checked_sub_days(chrono::Days::new(1))
                        .with_context(|| format!("No date before {date}")),
                    Weekday::Sat => date
                        .checked_add_days(chrono::Days::new(1))
                        .with_context(|| format!("No date after {date}")),
                    _ => Ok(date),
                })
                .collect(),
        }
    }
}

trait StrExt {
    fn case_insensitive_strip_prefix<'a>(&'a self, prefix: &str) -> Option<&'a Self>;
}

impl StrExt for str {
    fn case_insensitive_strip_prefix<'a>(&'a self, prefix: &str) -> Option<&'a Self> {
        let mut chars = self.chars();

        prefix
            .chars()
            .zip(&mut chars)
            .all(|(a, b)| a.to_ascii_lowercase() == b.to_ascii_lowercase())
            .then_some(chars.as_str())
    }
}

#[derive(Clone)]
struct EventDescription {
    title: ArcStr,
    data: EventDescriptionData,
    group_id: GroupId,
}

impl EventDescription {
    fn parse(input: &str, group_id: GroupId) -> anyhow::Result<Self> {
        if let Some(input) = input.case_insensitive_strip_prefix("ho repl ") {
            let Self {
                title,
                data,
                group_id,
            } = Self::parse(input, group_id)?;

            return Ok(Self {
                title,
                data: EventDescriptionData::FuzzySunday(Box::new(data)),
                group_id,
            });
        }

        let Some((index, category, title)) = Some(input).and_then(|input| {
            let space_or_tab = |c: char| c == ' ' || c == '\t';

            let (index, input) = input.trim().split_once(space_or_tab)?;
            let (month_or_weekday, title) = input.trim().split_once(space_or_tab)?;

            Some((index.trim(), month_or_weekday.trim(), title.trim()))
        }) else {
            anyhow::bail!("Invalid event: {input}")
        };

        let index = index
            .parse::<i16>()
            .with_context(|| format!("Invalid index {index}"))?;

        Ok(Self {
            title: title.into(),
            group_id,
            data: if category.eq_ignore_ascii_case("easter") {
                EventDescriptionData::DaysAfterEaster { day_offset: index }
            } else if let Some((weekday, month)) =
                category.split_once('/').and_then(|(weekday, month)| {
                    Some((
                        Weekday::from_str(weekday).ok()?,
                        Month::from_str(month).ok()?,
                    ))
                })
            {
                EventDescriptionData::NthWeekdayOfMonth {
                    n: index,
                    weekday,
                    month: Some(month),
                }
            } else if let Ok(month) = Month::from_str(category) {
                EventDescriptionData::FixedDate {
                    month,
                    day: index
                        .try_into()
                        .map_err(|_| anyhow::anyhow!("Invalid date {}/{index}", month.name()))?,
                }
            } else if let Ok(weekday) = Weekday::from_str(category) {
                EventDescriptionData::NthWeekdayOfMonth {
                    n: index,
                    weekday,
                    month: None,
                }
            } else {
                anyhow::bail!("Invalid event: {input}")
            },
        })
    }
}

#[derive(Clone, Data, Lens)]
struct EventGroupDescription {
    #[data(ignore)]
    id: GroupId,
    #[data(ignore)]
    title: ArcStr,
    #[data(ignore)]
    style: Option<ArcStr>,
    #[data(ignore)]
    events: Vector<EventDescription>,
    is_selected: bool,
}

#[derive(Clone)]
struct ErrorMessage(ArcStr);

impl ErrorMessage {
    fn new(err: anyhow::Error) -> Self {
        Self(format!("{err:?}").into())
    }
}

impl Data for ErrorMessage {
    fn same(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.0, &other.0)
    }
}

impl druid::piet::TextStorage for ErrorMessage {
    fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl druid::text::TextStorage for ErrorMessage {}

#[derive(Clone, Data, Lens)]
struct AppState {
    error_message: Option<ErrorMessage>,
    year: i32,
    output: Output,
    event_group_descriptions: Vector<EventGroupDescription>,
}

impl AppState {
    fn show_calendar(&self, events: druid::ExtEventSink) -> anyhow::Result<()> {
        let year = self.year;

        let mut calendar_events = HashMap::new();

        for EventGroupDescription {
            events,
            is_selected,
            ..
        } in &self.event_group_descriptions
        {
            if !is_selected {
                continue;
            }

            for &EventDescription {
                ref title,
                ref data,
                group_id,
            } in events
            {
                for date in data.dates(year)? {
                    let month =
                        Month::try_from((date.month()) as u8).context("Failed to get month")?;
                    let day = date.day();

                    calendar_events
                        .entry(MonthAndDay { month, day })
                        .or_insert_with(Vec::new)
                        .push(EventWithGroupId {
                            title: title.clone(),
                            group_id,
                        });
                }
            }
        }

        let calendar_event_styles = CalendarEventStyles(
            self.event_group_descriptions
                .iter()
                .filter_map(|event_group_description| {
                    event_group_description
                        .is_selected
                        .then(|| {
                            Some((
                                event_group_description.id,
                                event_group_description.style.clone()?,
                            ))
                        })
                        .flatten()
                })
                .collect(),
        );

        let output = match self.output {
            Output::MonthlyCalendar => MonthlyCalendar {
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
                                    day: EventDay { day },
                                    events: calendar_events
                                        .remove(&MonthAndDay { month, day })
                                        .unwrap_or_default(),
                                }
                            }))
                            .chain(std::iter::repeat_with(|| CalendarCell::Empty))
                            .take(40)
                            .chain(std::iter::once(CalendarCell::MonthAndYear { month, year }))
                            .collect()
                    })
                    .collect(),
            }
            .render(),
            Output::YearlyCalendar { split_in_two } => {
                let mut months = MONTHS
                    .iter()
                    .map(|&month| {
                        let days_before_start =
                            chrono::NaiveDate::from_ymd_opt(year, month.number_from_month(), 1)
                                .unwrap()
                                .weekday()
                                .num_days_from_monday() as usize;

                        YearlyCalendarMonth {
                            month,
                            days: std::iter::repeat_with(|| YearlyCalendarDay::Empty {
                                weekday: (),
                            })
                            .take(days_before_start)
                            .chain((1..=days_in_month(year, month)).map(|day| {
                                YearlyCalendarDay::Day {
                                    weekday: (),
                                    day: EventDay { day },
                                    events: calendar_events
                                        .remove(&MonthAndDay { month, day })
                                        .unwrap_or_default(),
                                }
                            }))
                            .chain(std::iter::repeat_with(|| YearlyCalendarDay::Empty {
                                weekday: (),
                            }))
                            .take(YearlyCalendar::ROWS_COUNT)
                            .zip(weekdays(Weekday::Mon))
                            .map(|(day, weekday)| day.with_weekday(weekday))
                            .collect(),
                        }
                    })
                    .collect_vec();

                YearlyCalendar {
                    title: if split_in_two { "Half-Year" } else { "Year" },
                    calendar_event_styles,
                    year,
                    weekday_titles: weekdays(Weekday::Mon)
                        .take(YearlyCalendar::ROWS_COUNT)
                        .collect(),
                    pages: if split_in_two {
                        let latter_months = months.split_off(6);
                        vec![
                            YearlyCalendarPage { months },
                            YearlyCalendarPage {
                                months: latter_months,
                            },
                        ]
                    } else {
                        vec![YearlyCalendarPage { months }]
                    },
                }
                .render()
            }
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
                                day: EventDay { day },
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
        .context("Failed to render calendar")?;

        fn worker(output: String) -> anyhow::Result<()> {
            let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
                .context("Failed to listen over HTTP")?;

            let http_port = listener
                .local_addr()
                .context("Failed to get http address")?
                .port();

            webbrowser::open(&format!("http://127.0.0.1:{http_port}/"))
                .context("Failed to open web browser")?;

            let (mut socket, _remote_address) = listener
                .accept()
                .context("Failed to accept http connection")?;

            for line in std::io::BufReader::new(&mut socket).lines() {
                if line
                    .context("Failed to read HTTP request")?
                    .trim()
                    .is_empty()
                {
                    break;
                }
            }

            socket
                .write_all(output.as_bytes())
                .context("Failed to write HTTP response")
        }

        std::thread::spawn(move || {
            if let Err(err) = worker(output) {
                if let Err(err) =
                    events.submit_command(SET_ERROR, ErrorMessage::new(err), druid::Target::Global)
                {
                    eprintln!("{err}");
                }
            }
        });

        Ok(())
    }
}

macro_rules! create_keys {
    ($($k:ident : $t:ty),* $(,)?) => {
        $(
            const $k: ::druid::Key::<$t> = ::druid::Key::new(concat!(env!("CARGO_PKG_NAME"), ".key.", stringify!($k)));
        )*
    };
}

create_keys!(
    WIDGET_PADDING_INSETS: druid::Insets,
    MARKDOWN_LIST_PADDING: f64,
    EVENT_GROUP_TITLE: ArcStr,
);

macro_rules! create_selectors {
    ($($k:ident : $t:ty),* $(,)?) => {
        $(
            const $k: ::druid::Selector::<$t> = ::druid::Selector::new(concat!(env!("CARGO_PKG_NAME"), ".selector.", stringify!($k)));
        )*
    };
}

create_selectors!(
    SET_ERROR: ErrorMessage,
    SHOW_HELP: (),
    OPEN_LINK: String,
);

struct AppController;

impl AppController {
    fn cache_path() -> Option<std::path::PathBuf> {
        let project_directories = directories::ProjectDirs::from("", "", "calendargenerator")?;
        let config_directory = project_directories.config_local_dir();

        std::fs::create_dir_all(config_directory)
            .context("Failed to create config directory")
            .map_err(|err| eprintln!("{err:?}"))
            .ok();

        Some(
            [config_directory, "calendar_file_path.txt".as_ref()]
                .into_iter()
                .collect(),
        )
    }

    fn open_calendar_dialog() -> druid::Command {
        druid::commands::SHOW_OPEN_PANEL.with(
            druid::FileDialogOptions::new()
                .allowed_types(vec![druid::FileSpec::new("Calendar", &["txt"])])
                .title("Open a calendar"),
        )
    }

    fn help() -> impl Widget<AppState> {
        Self::help_blocks(markdown::tokenize(include_str!("../README.md"))).scroll()
    }

    fn help_blocks(blocks: Vec<markdown::Block>) -> impl Widget<AppState> {
        blocks.into_iter().fold(
            Flex::column().cross_axis_alignment(druid::widget::CrossAxisAlignment::Start),
            |column, block| match block {
                markdown::Block::Header(spans, level) => column
                    .with_spacer(match level {
                        1 => 0.0,
                        2 => 19.92,
                        3 => 18.72,
                        4 => 21.28,
                        5 => 22.1776,
                        6 => 24.9776,
                        _ => 0.0,
                    })
                    .with_child(Self::help_spans_with_modify_text(spans, move |text| {
                        text.size(match level {
                            1 => 32.0,
                            2 => 24.0,
                            3 => 18.72,
                            4 => 16.0,
                            5 => 13.28,
                            6 => 10.72,
                            _ => 16.0,
                        });

                        text.text_color(druid::Color::rgb8(0x56, 0x9c, 0xd6));

                        text.underline(true);
                    }))
                    .with_spacer(match level {
                        1 => 21.44,
                        2 => 19.92,
                        3 => 18.72,
                        4 => 21.28,
                        5 => 22.1776,
                        6 => 24.9776,
                        _ => 0.0,
                    }),
                markdown::Block::Paragraph(spans) => column.with_child(Self::help_spans(spans)),
                markdown::Block::Blockquote(_) => unimplemented!(),
                markdown::Block::CodeBlock(_, _) => unimplemented!(),
                markdown::Block::OrderedList(_, _) => unimplemented!(),
                markdown::Block::UnorderedList(list) => column.with_child(
                    list.into_iter()
                        .map(|item| {
                            let row = Flex::row()
                                .cross_axis_alignment(druid::widget::CrossAxisAlignment::Start)
                                .with_spacer(MARKDOWN_LIST_PADDING)
                                .with_child(Label::new("•"));

                            match item {
                                markdown::ListItem::Simple(spans) => {
                                    row.with_child(Self::help_spans(spans))
                                }
                                markdown::ListItem::Paragraph(blocks) => {
                                    row.with_child(Self::help_blocks(blocks))
                                }
                            }
                        })
                        .fold(
                            Flex::column()
                                .cross_axis_alignment(druid::widget::CrossAxisAlignment::Start),
                            Flex::with_child,
                        ),
                ),
                markdown::Block::Raw(_) => unimplemented!(),
                markdown::Block::Hr => unimplemented!(),
            },
        )
    }

    fn apply_help_spans(
        rows: &mut Vec<druid::text::RichTextBuilder>,
        modify_text: &dyn Fn(&mut druid::text::AttributesAdder),
        spans: Vec<markdown::Span>,
    ) {
        for span in spans {
            let mut text_attributes = match span {
                markdown::Span::Break => {
                    rows.push(druid::text::RichTextBuilder::new());
                    continue;
                }
                markdown::Span::Text(text) => rows.last_mut().unwrap().push(&text),
                markdown::Span::Code(text) => {
                    let mut text_attributes = rows.last_mut().unwrap().push(&text);

                    text_attributes
                        .font_family(druid::FontFamily::MONOSPACE)
                        .text_color(druid::Color::rgb8(0xce, 0x91, 0x78));

                    text_attributes
                }
                markdown::Span::Link(text, url, _title) => {
                    let mut text_attributes = rows.last_mut().unwrap().push(&text);

                    text_attributes.link(OPEN_LINK.with(url)).underline(true);

                    text_attributes
                }
                markdown::Span::Image(_, _, _) => unimplemented!(),
                markdown::Span::Emphasis(spans) => {
                    Self::apply_help_spans(
                        rows,
                        &|text| {
                            modify_text(text);
                            text.style(druid::FontStyle::Italic);
                        },
                        spans,
                    );

                    continue;
                }
                markdown::Span::Strong(spans) => {
                    Self::apply_help_spans(
                        rows,
                        &|text| {
                            modify_text(text);
                            text.weight(druid::FontWeight::BOLD);
                        },
                        spans,
                    );

                    continue;
                }
            };

            modify_text(&mut text_attributes);
        }
    }

    fn help_spans(spans: Vec<markdown::Span>) -> impl Widget<AppState> {
        Self::help_spans_with_modify_text(spans, |_| {})
    }

    fn help_spans_with_modify_text(
        spans: Vec<markdown::Span>,
        modify_text: impl Fn(&mut druid::text::AttributesAdder),
    ) -> impl Widget<AppState> {
        let mut rows = vec![druid::text::RichTextBuilder::new()];

        Self::apply_help_spans(&mut rows, &modify_text, spans);

        rows.into_iter()
            .map(|text| Label::raw().lens(druid::lens::Constant(text.build())))
            .fold(Flex::column(), Flex::with_child)
    }

    fn parse_calendar(
        calendar_file: &std::path::Path,
    ) -> anyhow::Result<Vector<EventGroupDescription>> {
        let calendar_text = std::fs::read_to_string(calendar_file)
            .with_context(|| format!("Failed to read {}", calendar_file.display()))?;

        let mut event_group_descriptions = Vec::<EventGroupDescription>::new();

        for (line_num, line) in calendar_text.lines().enumerate() {
            let line_num = line_num + 1;

            let line = line.trim();

            if line.is_empty() {
                continue;
            }

            if let Some(line) = line.strip_prefix('[') {
                let Some(title_and_style) = line.strip_suffix(']') else {
                    anyhow::bail!(
                        "Error on line {line_num}: Event Group titles must end with a ']'"
                    );
                };

                let (title, style) = title_and_style
                    .split_once(':')
                    .map(|(title, style)| (title.trim(), Some(style.into())))
                    .unwrap_or((title_and_style, None));
                let id = GroupId::Group(event_group_descriptions.len());

                event_group_descriptions.push(EventGroupDescription {
                    id,
                    title: title.trim().into(),
                    style,
                    events: Vector::new(),
                    is_selected: false,
                });
            } else {
                let Some(current_group) = event_group_descriptions.last_mut() else {
                    anyhow::bail!("Calendar must start with an event group");
                };

                current_group
                    .events
                    .push_back(EventDescription::parse(line, current_group.id)?)
            }
        }

        Ok(event_group_descriptions.into())
    }
}

impl<W: Widget<AppState>> druid::widget::Controller<AppState, W> for AppController {
    fn event(
        &mut self,
        child: &mut W,
        ctx: &mut EventCtx,
        event: &druid::Event,
        data: &mut AppState,
        env: &Env,
    ) {
        if let druid::Event::WindowConnected = event {
            if let Some(events) = Self::cache_path().and_then(|cache_path| {
                let path = std::fs::read_to_string(cache_path).ok()?;

                AppController::parse_calendar(path.trim().as_ref()).ok()
            }) {
                data.event_group_descriptions = events;
            } else {
                ctx.submit_command(Self::open_calendar_dialog());
            }
        }

        child.event(ctx, event, data, env)
    }
}

impl druid::AppDelegate<AppState> for AppController {
    fn command(
        &mut self,
        ctx: &mut druid::DelegateCtx,
        _target: druid::Target,
        command: &druid::Command,
        data: &mut AppState,
        _env: &Env,
    ) -> druid::Handled {
        if let Some(error) = command.get(SET_ERROR) {
            data.error_message = Some(error.clone());

            druid::Handled::Yes
        } else if let Some(()) = command.get(SHOW_HELP) {
            ctx.new_window::<AppState>(
                druid::WindowDesc::new(AppController::help().controller(AppController))
                    .title("Help"),
            );

            druid::Handled::Yes
        } else if let Some(url) = command.get(OPEN_LINK) {
            if let Err(err) = webbrowser::open(url).context("Failed to open browser") {
                data.error_message = Some(ErrorMessage::new(err));
            }

            druid::Handled::Yes
        } else if let Some(calendar_file) = command.get(druid::commands::OPEN_FILE) {
            if let Err(err) =
                Self::parse_calendar(calendar_file.path()).and_then(|event_group_descriptions| {
                    data.event_group_descriptions = event_group_descriptions;

                    if let Some(cache_path) = Self::cache_path() {
                        std::fs::write(
                            cache_path,
                            calendar_file.path().as_os_str().as_encoded_bytes(),
                        )
                        .context("Failed to write cached calendar path")?;
                    }

                    Ok(())
                })
            {
                data.error_message = Some(ErrorMessage::new(err));
            }

            druid::Handled::Yes
        } else {
            druid::Handled::No
        }
    }
}

fn app_view() -> impl Widget<AppState> {
    Flex::column()
        .with_child(
            druid::widget::Maybe::or_empty(|| {
                Flex::column()
                    .with_child(
                        Flex::column()
                            .with_child(Label::new("Error!"))
                            .with_default_spacer()
                            .with_child(Label::raw())
                            .border(
                                druid::theme::BORDER_DARK,
                                druid::theme::TEXTBOX_BORDER_WIDTH,
                            )
                            .expand_width(),
                    )
                    .with_default_spacer()
                    .expand_width()
            })
            .lens(AppState::error_message),
        )
        .with_child(
            Flex::column()
                .with_child(Label::new("Year"))
                .with_default_spacer()
                .with_child(
                    Flex::row()
                        .with_child(Label::dynamic(|data, _env| format!("{data}")))
                        .with_child(Stepper::new()),
                )
                .with_default_spacer()
                .border(
                    druid::theme::BORDER_DARK,
                    druid::theme::TEXTBOX_BORDER_WIDTH,
                )
                .expand_width()
                .lens(AppState::year.then(druid::lens::Map::new(
                    |&year: &i32| year.into(),
                    |current_year, new_year: f64| *current_year = new_year as i32,
                ))),
        )
        .with_default_spacer()
        .with_child(
            Flex::column()
                .with_child(Label::new("Calendar Type"))
                .with_default_spacer()
                .with_child(RadioGroup::column([
                    ("Month", Output::MonthlyCalendar),
                    (
                        "Year",
                        Output::YearlyCalendar {
                            split_in_two: false,
                        },
                    ),
                    ("Half-Year", Output::YearlyCalendar { split_in_two: true }),
                    ("Diary", Output::Diary),
                ]))
                .with_default_spacer()
                .border(
                    druid::theme::BORDER_DARK,
                    druid::theme::TEXTBOX_BORDER_WIDTH,
                )
                .expand_width()
                .lens(AppState::output),
        )
        .with_default_spacer()
        .with_flex_child(
            Flex::column()
                .with_child(Label::new("Include Event Groups"))
                .with_default_spacer()
                .with_flex_child(
                    List::new(|| {
                        Checkbox::new(|_data: &bool, env: &Env| env.get(EVENT_GROUP_TITLE))
                            .lens(EventGroupDescription::is_selected)
                            .env_scope(|env: &mut Env, data: &EventGroupDescription| {
                                env.set(EVENT_GROUP_TITLE, data.title.clone())
                            })
                    })
                    .align_horizontal(druid::UnitPoint::CENTER)
                    .scroll()
                    .vertical()
                    .lens(AppState::event_group_descriptions),
                    1.0,
                )
                .border(
                    druid::theme::BORDER_DARK,
                    druid::theme::TEXTBOX_BORDER_WIDTH,
                )
                .expand(),
            1.0,
        )
        .with_default_spacer()
        .with_child(
            Button::new("Create").on_click(|ctx, data: &mut AppState, _| {
                if let Err(err) = data.show_calendar(ctx.get_external_handle()) {
                    data.error_message = Some(ErrorMessage::new(err));
                }
            }),
        )
        .padding(WIDGET_PADDING_INSETS)
}

fn main() -> anyhow::Result<()> {
    let app_name = "Create Calendar";

    druid::AppLauncher::with_window(
        druid::WindowDesc::new(app_view().controller(AppController))
            .title(app_name)
            .menu(move |_, _, _| {
                druid::Menu::new(app_name)
                    .entry(
                        druid::MenuItem::new("Open Calendar")
                            .command(AppController::open_calendar_dialog()),
                    )
                    .separator()
                    .entry(druid::MenuItem::new("Help").command(SHOW_HELP))
            })
            .window_size(Size::new(800.0, 600.0)),
    )
    .delegate(AppController)
    .configure_env(|env, _| {
        let padding_horizontal = env.get(druid::theme::WIDGET_PADDING_HORIZONTAL);
        let padding_vertical = env.get(druid::theme::WIDGET_PADDING_VERTICAL);

        env.set(
            WIDGET_PADDING_INSETS,
            druid::Insets::uniform_xy(padding_horizontal, padding_vertical),
        );

        env.set(MARKDOWN_LIST_PADDING, 2.0 * padding_horizontal)
    })
    .launch(AppState {
        error_message: None,
        year: chrono::Local::now().year(),
        output: Output::MonthlyCalendar,
        event_group_descriptions: Vector::new(),
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn create_help() {
        super::AppController::help();
    }
}
