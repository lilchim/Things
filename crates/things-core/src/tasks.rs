//! Tasks: things I should or can do, kept in `tasks.ron` on any thing (a
//! game's dailies, a chore, an errand). A task happens once, by an optional
//! date, or repeats: on the calendar (daily, weekly on a weekday, monthly on
//! a day) or every so many days after I last did it.
//!
//! Checking a task off records the day, so the file keeps a small history.
//! A task's day can start at a reset hour instead of midnight, so a game's
//! dailies flip when the game resets.
//!
//! [`Agenda`] sorts every task into what's overdue, what to do now, what's
//! coming up, what can wait, and what's done for now.

use chrono::{Datelike, Days, Months, NaiveDate, NaiveDateTime, TimeDelta};
use serde::{Deserialize, Serialize};

use crate::components::from_ron;
use crate::{Entity, EntityId};

/// The file every task list lives in.
pub const FILE: &str = "tasks.ron";

/// `tasks.ron`: a thing's tasks, in the order I listed them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Tasks {
    pub tasks: Vec<Task>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Task {
    pub name: String,
    pub repeat: Repeat,
    /// For a one-off: the day to do it by, `YYYY-MM-DD`. Empty for whenever.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub due: String,
    /// The hour (0–23) a new day starts for this task. Before it, it's
    /// still yesterday: a game that resets at 4:00 keeps last night's
    /// dailies open until 4:00.
    #[serde(skip_serializing_if = "is_zero")]
    pub resets_at: u8,
    /// The days I did it, `YYYY-MM-DD`, oldest first.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub done: Vec<String>,
}

fn is_zero(n: &u8) -> bool {
    *n == 0
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Repeat {
    /// One time only.
    #[default]
    Once,
    Daily,
    /// Every week, starting again on this day.
    Weekly(Weekday),
    /// Every month, starting again on this day of the month (1–31; a short
    /// month uses its last day).
    Monthly(u8),
    /// Due again this many days after I last did it.
    Every(u32),
}

impl Repeat {
    pub fn label(self) -> String {
        match self {
            Self::Once => "Once".into(),
            Self::Daily => "Daily".into(),
            Self::Weekly(day) => format!("Weekly from {}", day.label()),
            Self::Monthly(day) => format!("Monthly from day {day}"),
            Self::Every(1) => "Every day".into(),
            Self::Every(n) => format!("Every {n} days"),
        }
    }

    /// Whether it starts over on the calendar, done or not.
    pub fn is_calendar(self) -> bool {
        matches!(self, Self::Daily | Self::Weekly(_) | Self::Monthly(_))
    }

    /// What periods are called, for "done 9 of the last 10 days", and the
    /// one just gone, for "done yesterday".
    fn unit(self) -> (&'static str, &'static str) {
        match self {
            Self::Weekly(_) => ("weeks", "last week"),
            Self::Monthly(_) => ("months", "last month"),
            _ => ("days", "yesterday"),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Weekday {
    #[default]
    Mon,
    Tue,
    Wed,
    Thu,
    Fri,
    Sat,
    Sun,
}

impl Weekday {
    pub const ALL: [Weekday; 7] = [
        Self::Mon,
        Self::Tue,
        Self::Wed,
        Self::Thu,
        Self::Fri,
        Self::Sat,
        Self::Sun,
    ];

    pub fn label(self) -> &'static str {
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

    fn days_from_monday(self) -> u32 {
        self as u32
    }
}

/// Where a task stands right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum When {
    /// Its day has gone by and it isn't done.
    Overdue { since: NaiveDate },
    /// To do now, before `until`: the end of its day, week or month.
    Now { until: NaiveDateTime },
    /// Not due yet.
    Coming { on: NaiveDate },
    /// A one-off with no date.
    Anytime,
    /// Done for this day, week or month (or a one-off done today).
    Done,
    /// A one-off done before today. Off the agenda.
    Finished,
}

pub fn parse_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok()
}

fn format_date(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

/// Day `day` of a month, or its last day when the month is shorter.
fn month_day(year: i32, month: u32, day: u8) -> NaiveDate {
    (1..=u32::from(day.clamp(1, 31)))
        .rev()
        .find_map(|d| NaiveDate::from_ymd_opt(year, month, d))
        .expect("every month has a first day")
}

fn add_months(d: NaiveDate, months: i32, day: u8) -> NaiveDate {
    let first = d.with_day(1).expect("every month has a first day");
    let first = if months >= 0 {
        first + Months::new(months as u32)
    } else {
        first - Months::new(months.unsigned_abs())
    };
    month_day(first.year(), first.month(), day)
}

impl Task {
    pub fn named(name: &str) -> Self {
        Self {
            name: name.trim().to_owned(),
            ..Self::default()
        }
    }

    /// The day it is for this task: before the reset hour it's still
    /// yesterday.
    pub fn today(&self, now: NaiveDateTime) -> NaiveDate {
        (now - TimeDelta::hours(i64::from(self.resets_at.min(23)))).date()
    }

    /// When a day ends for this task: the next day at the reset hour.
    fn end_of(&self, day: NaiveDate) -> NaiveDateTime {
        (day + Days::new(1))
            .and_hms_opt(u32::from(self.resets_at.min(23)), 0, 0)
            .expect("hours under 24 are valid")
    }

    fn done_days(&self) -> impl Iterator<Item = NaiveDate> + '_ {
        self.done.iter().filter_map(|d| parse_date(d))
    }

    pub fn last_done(&self) -> Option<NaiveDate> {
        self.done_days().max()
    }

    /// The first day of the calendar period `day` falls in, for tasks that
    /// repeat on the calendar.
    fn period_start(&self, day: NaiveDate) -> Option<NaiveDate> {
        match self.repeat {
            Repeat::Daily => Some(day),
            Repeat::Weekly(start) => {
                let back =
                    (day.weekday().num_days_from_monday() + 7 - start.days_from_monday()) % 7;
                Some(day - Days::new(u64::from(back)))
            }
            Repeat::Monthly(start) => {
                let this = month_day(day.year(), day.month(), start);
                Some(if this <= day {
                    this
                } else {
                    add_months(day, -1, start)
                })
            }
            Repeat::Once | Repeat::Every(_) => None,
        }
    }

    /// The first day of the period after the one starting on `start`.
    fn next_period(&self, start: NaiveDate) -> NaiveDate {
        match self.repeat {
            Repeat::Weekly(_) => start + Days::new(7),
            Repeat::Monthly(day) => add_months(start, 1, day),
            _ => start + Days::new(1),
        }
    }

    fn previous_period(&self, start: NaiveDate) -> NaiveDate {
        match self.repeat {
            Repeat::Weekly(_) => start - Days::new(7),
            Repeat::Monthly(day) => add_months(start, -1, day),
            _ => start - Days::new(1),
        }
    }

    pub fn when(&self, now: NaiveDateTime) -> When {
        let today = self.today(now);
        let last = self.last_done();
        // A day it's due by: overdue, now, or coming.
        let by = |day: NaiveDate| {
            if day < today {
                When::Overdue { since: day }
            } else if day == today {
                When::Now {
                    until: self.end_of(day),
                }
            } else {
                When::Coming { on: day }
            }
        };
        match self.repeat {
            Repeat::Once => match last {
                Some(d) if d >= today => When::Done,
                Some(_) => When::Finished,
                None => parse_date(&self.due).map_or(When::Anytime, by),
            },
            Repeat::Every(n) => match last {
                Some(d) if d >= today => When::Done,
                Some(d) => by(d + Days::new(u64::from(n.max(1)))),
                None => by(today),
            },
            Repeat::Daily | Repeat::Weekly(_) | Repeat::Monthly(_) => {
                let start = self
                    .period_start(today)
                    .expect("calendar repeats have periods");
                if last.is_some_and(|d| d >= start) {
                    When::Done
                } else {
                    let end = self.next_period(start) - Days::new(1);
                    When::Now {
                        until: self.end_of(end),
                    }
                }
            }
        }
    }

    /// Done for now: this period, today, or for good.
    pub fn is_done(&self, now: NaiveDateTime) -> bool {
        matches!(self.when(now), When::Done | When::Finished)
    }

    /// Marks it done today. Returns false if today is already recorded.
    pub fn check(&mut self, now: NaiveDateTime) -> bool {
        let today = format_date(self.today(now));
        if self.done.contains(&today) {
            return false;
        }
        self.done.push(today);
        true
    }

    /// Takes back the latest time I did it.
    pub fn uncheck(&mut self) {
        if let Some(last) = self.last_done() {
            let last = format_date(last);
            self.done.retain(|d| d.trim() != last);
        }
    }

    /// How often it got done lately, for tasks that repeat on the calendar:
    /// "done 9 of the last 10 days". Counts the periods before this one,
    /// back to the first time I did it, at most `periods` of them.
    pub fn recent(&self, now: NaiveDateTime, periods: usize) -> Option<String> {
        let first = self.done_days().min()?;
        let mut start = self.period_start(self.today(now))?;
        let (mut hit, mut total) = (0, 0);
        while total < periods {
            let end = start;
            start = self.previous_period(start);
            if end <= first {
                break;
            }
            total += 1;
            if self.done_days().any(|d| start <= d && d < end) {
                hit += 1;
            }
        }
        let (unit, last) = self.repeat.unit();
        match (total, hit) {
            (0, _) => None,
            (1, 1) => Some(format!("done {last}")),
            (1, _) => Some(format!("missed {last}")),
            _ => Some(format!("done {hit} of the last {total} {unit}")),
        }
    }
}

/// One task on the agenda, with the thing it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: EntityId,
    /// Its place in the thing's `tasks.ron`.
    pub index: usize,
    pub task: Task,
    pub when: When,
}

/// Every task, sorted by what needs doing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Agenda {
    /// Oldest first.
    pub overdue: Vec<Entry>,
    /// Whatever ends soonest first.
    pub now: Vec<Entry>,
    /// Soonest first.
    pub coming: Vec<Entry>,
    pub anytime: Vec<Entry>,
    pub done: Vec<Entry>,
}

impl Agenda {
    pub fn new<'a>(
        lists: impl IntoIterator<Item = (&'a EntityId, &'a Tasks)>,
        now: NaiveDateTime,
    ) -> Self {
        let mut agenda = Self::default();
        for (id, tasks) in lists {
            for (index, task) in tasks.tasks.iter().enumerate() {
                let when = task.when(now);
                let list = match when {
                    When::Overdue { .. } => &mut agenda.overdue,
                    When::Now { .. } => &mut agenda.now,
                    When::Coming { .. } => &mut agenda.coming,
                    When::Anytime => &mut agenda.anytime,
                    When::Done => &mut agenda.done,
                    When::Finished => continue,
                };
                list.push(Entry {
                    id: id.clone(),
                    index,
                    task: task.clone(),
                    when,
                });
            }
        }
        let name = |e: &Entry| (e.id.title(), e.index);
        agenda.overdue.sort_by_cached_key(|e| match e.when {
            When::Overdue { since } => (since, name(e)),
            _ => unreachable!(),
        });
        agenda.now.sort_by_cached_key(|e| match e.when {
            When::Now { until } => (until, name(e)),
            _ => unreachable!(),
        });
        agenda.coming.sort_by_cached_key(|e| match e.when {
            When::Coming { on } => (on, name(e)),
            _ => unreachable!(),
        });
        agenda.anytime.sort_by_cached_key(name);
        agenda.done.sort_by_cached_key(name);
        agenda
    }

    pub fn is_empty(&self) -> bool {
        self.overdue.is_empty()
            && self.now.is_empty()
            && self.coming.is_empty()
            && self.anytime.is_empty()
            && self.done.is_empty()
    }
}

/// An entity's tasks, if it has a `tasks.ron` that parses. A file that
/// doesn't counts as missing; its page shows the error.
pub fn of(entity: &Entity) -> Option<Tasks> {
    entity
        .components
        .iter()
        .find(|c| c.path == FILE)
        .and_then(|c| from_ron(&c.text).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::to_ron;

    fn at(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap()
    }

    fn date(s: &str) -> NaiveDate {
        parse_date(s).unwrap()
    }

    fn task(repeat: Repeat, done: &[&str]) -> Task {
        Task {
            name: "t".into(),
            repeat,
            done: done.iter().map(|d| (*d).into()).collect(),
            ..Task::default()
        }
    }

    // 2026-10-13 is a Tuesday.
    const NOW: &str = "2026-10-13 20:00";

    #[test]
    fn reads_and_writes_tidy_files() {
        let text = r#"Tasks(tasks: [
            (name: "Dailies", repeat: Daily, resets_at: 4),
            (name: "Tower", repeat: Weekly(Mon)),
            (name: "Passport", due: "2026-11-01"),
        ])"#;
        let tasks: Tasks = from_ron(text).unwrap();
        assert_eq!(tasks.tasks[1].repeat, Repeat::Weekly(Weekday::Mon));
        assert_eq!(tasks.tasks[2].repeat, Repeat::Once);
        let written = to_ron(&tasks).unwrap();
        assert!(!written.contains("done"), "{written}");
        assert!(!written.contains("resets_at: 0"), "{written}");
        assert_eq!(from_ron::<Tasks>(&written).unwrap(), tasks);
    }

    #[test]
    fn the_day_starts_at_the_reset_hour() {
        let mut t = task(Repeat::Daily, &["2026-10-13"]);
        t.resets_at = 4;
        assert_eq!(t.today(at("2026-10-14 03:59")), date("2026-10-13"));
        assert_eq!(t.when(at("2026-10-14 03:59")), When::Done);
        assert_eq!(
            t.when(at("2026-10-14 04:00")),
            When::Now {
                until: at("2026-10-15 04:00")
            }
        );
    }

    #[test]
    fn calendar_repeats_reset_on_their_day() {
        let now = at(NOW);
        let weekly = task(Repeat::Weekly(Weekday::Mon), &["2026-10-12"]);
        assert_eq!(weekly.when(now), When::Done);
        let weekly = task(Repeat::Weekly(Weekday::Mon), &["2026-10-11"]);
        assert_eq!(
            weekly.when(now),
            When::Now {
                until: at("2026-10-19 00:00")
            }
        );
        // A Wednesday reset: the week started last Wednesday.
        let wed = task(Repeat::Weekly(Weekday::Wed), &["2026-10-07"]);
        assert_eq!(wed.when(now), When::Done);

        let monthly = task(Repeat::Monthly(15), &["2026-09-20"]);
        assert_eq!(monthly.when(now), When::Done);
        let monthly = task(Repeat::Monthly(15), &["2026-09-14"]);
        assert_eq!(
            monthly.when(now),
            When::Now {
                until: at("2026-10-15 00:00")
            }
        );
        // Day 31 in a 30-day month is its last day: the month started on
        // October 31st and starts again on November 30th.
        let end = task(Repeat::Monthly(31), &[]);
        assert_eq!(
            end.when(at("2026-11-10 12:00")),
            When::Now {
                until: at("2026-11-30 00:00")
            }
        );
    }

    #[test]
    fn one_offs_and_intervals_fall_due() {
        let now = at(NOW);
        let mut once = task(Repeat::Once, &[]);
        assert_eq!(once.when(now), When::Anytime);
        once.due = "2026-11-01".into();
        assert_eq!(
            once.when(now),
            When::Coming {
                on: date("2026-11-01")
            }
        );
        once.due = "2026-10-01".into();
        assert_eq!(
            once.when(now),
            When::Overdue {
                since: date("2026-10-01")
            }
        );
        once.check(now);
        assert_eq!(once.when(now), When::Done);
        assert_eq!(once.when(at("2026-10-14 09:00")), When::Finished);

        let lawn = task(Repeat::Every(14), &["2026-10-10"]);
        assert_eq!(
            lawn.when(now),
            When::Coming {
                on: date("2026-10-24")
            }
        );
        let lawn = task(Repeat::Every(14), &["2026-09-01"]);
        assert_eq!(
            lawn.when(now),
            When::Overdue {
                since: date("2026-09-15")
            }
        );
        assert!(matches!(
            task(Repeat::Every(14), &[]).when(now),
            When::Now { .. }
        ));
    }

    #[test]
    fn check_and_uncheck_record_the_day() {
        let now = at(NOW);
        let mut t = task(Repeat::Daily, &["2026-10-12"]);
        assert!(!t.is_done(now));
        assert!(t.check(now));
        assert!(!t.check(now));
        assert!(t.is_done(now));
        assert_eq!(t.done, ["2026-10-12", "2026-10-13"]);
        t.uncheck();
        assert_eq!(t.done, ["2026-10-12"]);
    }

    #[test]
    fn recent_counts_periods_since_the_first_done() {
        let now = at(NOW);
        let t = task(Repeat::Daily, &["2026-10-10", "2026-10-12", "2026-10-13"]);
        // The 10th, 11th and 12th; today doesn't count yet.
        assert_eq!(t.recent(now, 10).unwrap(), "done 2 of the last 3 days");
        let t = task(Repeat::Weekly(Weekday::Mon), &["2026-10-06"]);
        assert_eq!(t.recent(now, 10).unwrap(), "done last week");
        assert_eq!(task(Repeat::Daily, &[]).recent(now, 10), None);
        assert_eq!(task(Repeat::Daily, &["2026-10-13"]).recent(now, 10), None);
    }

    #[test]
    fn agenda_sorts_by_what_needs_doing() {
        let now = at(NOW);
        let ww = EntityId::parse("wuthering-waves-a1b2").unwrap();
        let home = EntityId::parse("house-c3d4").unwrap();
        let mut weekly = task(Repeat::Weekly(Weekday::Mon), &[]);
        weekly.name = "Tower".into();
        let mut daily = task(Repeat::Daily, &[]);
        daily.name = "Dailies".into();
        let game = Tasks {
            tasks: vec![weekly, daily, task(Repeat::Daily, &["2026-10-13"])],
        };
        let mut passport = task(Repeat::Once, &[]);
        passport.due = "2026-11-01".into();
        let chores = Tasks {
            tasks: vec![
                passport,
                task(Repeat::Every(14), &["2026-09-01"]),
                task(Repeat::Once, &[]),
                task(Repeat::Once, &["2026-01-01"]),
            ],
        };
        let agenda = Agenda::new([(&ww, &game), (&home, &chores)], now);
        let names = |l: &[Entry]| l.iter().map(|e| e.task.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(&agenda.now), ["Dailies", "Tower"]);
        assert_eq!(agenda.overdue.len(), 1);
        assert_eq!(agenda.coming.len(), 1);
        assert_eq!(agenda.anytime.len(), 1);
        assert_eq!(agenda.done.len(), 1);
        assert_eq!(
            (agenda.done[0].id.slug(), agenda.done[0].index),
            ("wuthering-waves", 2)
        );
    }
}
