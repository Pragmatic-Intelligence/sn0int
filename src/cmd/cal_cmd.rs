use crate::errors::*;

use chrono::Utc;
use chrono::prelude::*;
use crate::cmd::Cmd;
use crate::models::*;
use crate::shell::Shell;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::str::FromStr;
use structopt::StructOpt;
use structopt::clap::AppSettings;


#[derive(Debug, StructOpt)]
#[structopt(global_settings = &[AppSettings::ColoredHelp])]
pub struct Args {
    /// Show additional months for context
    #[structopt(short="C", long)]
    context: Option<u32>,
    args: Vec<DateArg>,
}

fn days_in_month(year: i32, month: u32) -> i64 {
    let start = Utc.ymd(year, month, 1);
    let end = if month == 12 {
        Utc.ymd(year + 1, 1, 1)
    } else {
        Utc.ymd(year, month + 1, 1)
    };
    end.signed_duration_since(start).num_days()
}

#[derive(Debug)]
enum DateArg {
    Month(u32),
    Num(i32),
}

impl FromStr for DateArg {
    type Err = Error;

    fn from_str(s: &str) -> Result<DateArg> {
        let ds = match s.to_lowercase().as_str() {
            "jan" | "january"   => DateArg::Month(1),
            "feb" | "february"  => DateArg::Month(2),
            "mar" | "march"     => DateArg::Month(3),
            "apr" | "april"     => DateArg::Month(4),
            "may"               => DateArg::Month(5),
            "jun" | "june"      => DateArg::Month(6),
            "jul" | "july"      => DateArg::Month(7),
            "aug" | "august"    => DateArg::Month(8),
            "sep" | "september" => DateArg::Month(9),
            "oct" | "october"   => DateArg::Month(10),
            "nov" | "november"  => DateArg::Month(11),
            "dec" | "december"  => DateArg::Month(12),
            _ => {
                let num = s.parse::<i32>()
                    .context("Input is not a month and not a number")?;
                DateArg::Num(num)
            },
        };
        Ok(ds)
    }
}

enum DateSpec {
    Year(i32),
    YearMonth((i32, u32)),
    YearMonthContext((i32, u32, u32)),
}

impl DateSpec {
    fn from_args(args: &[DateArg], context: Option<u32>) -> Result<DateSpec> {
        if args.len() > 2 {
            bail!("Too many datespec args");
        }

        let today = Utc::today();
        let ds = match (args.get(0), args.get(1), context) {
            (None, _, None) => DateSpec::YearMonth((today.year(), today.month())),
            (None, _, Some(context)) => DateSpec::YearMonthContext((today.year(), today.month(), context)),

            (Some(DateArg::Month(month)), None, None) => DateSpec::YearMonth((today.year(), *month)),
            (Some(DateArg::Num(year)), None, None) => DateSpec::Year(*year),
            (Some(DateArg::Month(month)), Some(DateArg::Num(year)), None) => DateSpec::YearMonth((*year, *month)),
            (Some(DateArg::Num(year)), Some(DateArg::Month(month)), None) => DateSpec::YearMonth((*year, *month)),

            (Some(DateArg::Month(month)), None, Some(context)) => DateSpec::YearMonthContext((today.year(), *month, context)),
            (Some(DateArg::Month(month)), Some(DateArg::Num(year)), Some(context)) => DateSpec::YearMonthContext((*year, *month, context)),
            (Some(DateArg::Num(year)), Some(DateArg::Month(month)), Some(context)) => DateSpec::YearMonthContext((*year, *month, context)),
            _ => bail!("Combination of datespec args is invalid"),
        };
        Ok(ds)
    }
}

const MONTH_LINES: i32 = 7;

fn merge_months(ctx: &Context, months: &[DateSpec]) -> String {
    let mut months = months.iter()
        .map(|ds| {
            let month = ds.to_term_string(ctx);
            month.lines()
                .map(String::from)
                .collect::<VecDeque<_>>()
        })
        .collect::<Vec<_>>();

    let mut out = String::new();
    for i in 0..=MONTH_LINES {
        let mut first = true;
        for m in &mut months {
            if !first {
                out.push_str("   ");
            }
            if let Some(line) = m.pop_front() {
                out.push_str(&line);
            } else {
                out.push_str(&" ".repeat(21));
            }
            first = false;
        }
        if i < MONTH_LINES {
            out.push('\n');
        }
    }
    out
}

fn chunk_months(ctx: &Context, months: &[DateSpec]) -> String {
    months
        .chunks(3)
        .map(|m| merge_months(ctx, m))
        .fold(String::new(), |a, b| {
            if a.is_empty() {
                a + &b
            } else {
                a + "\n" + &b
            }
        })
}

#[derive(Debug, Clone, PartialEq)]
enum ActivityGrade {
    None,
    One,
    Two,
    Three,
    Four,
}

impl ActivityGrade {
    fn as_term_str(&self) -> &'static str {
        match self {
            ActivityGrade::None => "\x1b[97m\x1b[48;5;238m",
            ActivityGrade::One => "\x1b[30m\x1b[48;5;148m",
            ActivityGrade::Two => "\x1b[30m\x1b[48;5;71m",
            ActivityGrade::Three => "\x1b[97m\x1b[48;5;34m",
            ActivityGrade::Four => "\x1b[97m\x1b[48;5;22m",
        }
    }
}

struct Context {
    events: HashMap<NaiveDate, u64>,
    max: u64,
    today: NaiveDate,
}

impl Context {
    #[inline]
    fn is_today(&self, date: &NaiveDate) -> bool {
        self.today == *date
    }

    #[inline]
    fn is_future(&self, date: &NaiveDate) -> bool {
        self.today < *date
    }

    fn activity_for_day(&self, date: &NaiveDate) -> ActivityGrade {
        if let Some(events) = self.events.get(date) {
            let max = self.max as f64;
            let events = *events as f64;
            let step = max / 4.0;

            let x = events / step;

            if x <= 1.0 {
                ActivityGrade::One
            } else if x <= 2.0 {
                ActivityGrade::Two
            } else if x <= 3.0 {
                ActivityGrade::Three
            } else {
                ActivityGrade::Four
            }
        } else {
            ActivityGrade::None
        }
    }
}

impl DateSpec {
    fn start(&self) -> NaiveDate {
        match self {
            DateSpec::Year(year) => NaiveDate::from_ymd(*year, 1, 1),
            DateSpec::YearMonth((year, month)) => NaiveDate::from_ymd(*year, *month, 1),
            DateSpec::YearMonthContext((year, month, context)) => {
                let mut year = *year - (*context / 12) as i32;
                let context = context % 12;
                let month = if context >= *month {
                    year -= 1;
                    12 - context + month
                } else {
                    month - context
                };
                NaiveDate::from_ymd(year, month, 1)
            },
        }
    }

    fn end(&self) -> NaiveDate {
        match self {
            DateSpec::Year(year) => NaiveDate::from_ymd(year + 1, 1, 1),
            DateSpec::YearMonth((year, month)) => {
                let (year, month) = if *month == 12 {
                    (*year + 1, 1)
                } else {
                    (*year, *month + 1)
                };
                NaiveDate::from_ymd(year, month, 1)
            },
            DateSpec::YearMonthContext((year, month, _context)) => {
                let (year, month) = if *month == 12 {
                    (*year + 1, 1)
                } else {
                    (*year, *month + 1)
                };
                NaiveDate::from_ymd(year, month, 1)
            },
        }
    }

    fn to_term_string(&self, ctx: &Context) -> String {
        match self {
            DateSpec::Year(year) => {
                let months = (1..=12)
                    .map(|month| DateSpec::YearMonth((*year, month)))
                    .collect::<Vec<_>>();
                chunk_months(ctx, &months)
            },
            DateSpec::YearMonth((year, month)) => {
                let mut w = String::new();

                let start = Utc.ymd(*year, *month, 1);
                let days = days_in_month(*year, *month) as u32;

                w.push_str(&format!("{:^21}\n", start.format("%B %Y")));
                w.push_str(" Su Mo Tu We Th Fr Sa\n");

                let mut cur_week_day = start.weekday();
                let week_progress = cur_week_day.num_days_from_sunday() as usize;
                w.push_str(&"   ".repeat(week_progress));

                let mut week_written = week_progress * 3;
                for cur_day in 1..=days {
                    let date = NaiveDate::from_ymd(*year, *month, cur_day);

                    if !ctx.is_future(&date) {
                        let activity = ctx.activity_for_day(&date);
                        w.push_str(activity.as_term_str());
                    }

                    if ctx.is_today(&date) {
                        w.push_str("\x1b[1m#");
                    } else {
                        w.push(' ');
                    }
                    w.push_str(&format!("{:2}", cur_day));
                    week_written += 3;
                    w.push_str("\x1b[0m");

                    // detect end of the week
                    if cur_week_day == Weekday::Sat {
                        if cur_day != days {
                            w.push('\n');
                        }
                        week_written = 0;
                    }

                    cur_week_day = cur_week_day.succ();
                }
                if week_written != 0 {
                    w.push_str(&" ".repeat(21 - week_written));
                }

                w
            }
            DateSpec::YearMonthContext((_year, _month, context)) => {
                let start = self.start();
                let mut year = start.year();
                let mut month = start.month();

                let mut months = Vec::new();

                for _ in 0..=*context {
                    months.push(DateSpec::YearMonth((year, month)));

                    if month == 12 {
                        year += 1;
                        month = 1;
                    } else {
                        month += 1;
                    }
                }

                chunk_months(ctx, &months)
            }
        }
    }
}

fn setup_graph_map(events: &[Activity]) -> (HashMap<NaiveDate, u64>, u64) {
    debug!("Found {} events in selected range", events.len());

    let mut cur = None;
    let mut ctr = 0;
    let mut max = 0;

    let mut map = HashMap::new();
    for event in events {
        let date = event.time.date();
        if let Some(cur) = cur.as_mut() {
            if date == *cur {
                ctr += 1;
            } else {
                if ctr > max {
                    max = ctr;
                }
                map.insert(cur.clone(), ctr);
                *cur = date;
                ctr = 1;
            }
        } else {
            cur = Some(date);
            ctr = 1;
        }
    }

    if ctr > 0 {
        if let Some(cur) = cur.take() {
            if ctr > max {
                max = ctr;
            }
            map.insert(cur, ctr);
        }
    }

    debug!("Maximum events per day is {}", max);

    (map, max)
}

impl Cmd for Args {
    #[inline]
    fn run(self, rl: &mut Shell) -> Result<()> {
        let ds = DateSpec::from_args(&self.args, self.context)
            .context("Failed to parse date spec")?;
        let filter = ActivityFilter {
            topic: None,
            since: Some(ds.start().and_hms(0, 0, 0)),
            until: Some(ds.end().and_hms(0, 0, 0)),
            location: false,
        };
        let events = Activity::query(rl.db(), &filter)?;
        let (events, max) = setup_graph_map(&events);
        let ctx = Context {
            events,
            max,
            today: Utc::today().naive_utc(),
        };
        println!("{}", ds.to_term_string(&ctx));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> Context {
        Context {
            events: HashMap::new(),
            max: 0,
            today: NaiveDate::from_ymd(2020, 05, 30),
        }
    }

    #[test]
    fn test_days_in_month_2020_05() {
        let days = days_in_month(2020, 05);
        assert_eq!(days, 31);
    }

    #[test]
    fn test_days_in_month_2020_04() {
        let days = days_in_month(2020, 04);
        assert_eq!(days, 30);
    }

    #[test]
    fn test_days_in_month_2020_02() {
        let days = days_in_month(2020, 02);
        assert_eq!(days, 29);
    }

    fn calc_activity_grade(cur: u64, max: u64) -> ActivityGrade {
        let mut events = HashMap::new();
        events.insert(NaiveDate::from_ymd(2020, 06, 06), cur);
        let ctx = Context {
            events,
            max,
            today: NaiveDate::from_ymd(2020, 06, 06),
        };
        ctx.activity_for_day(&NaiveDate::from_ymd(2020, 06, 06))
    }

    #[test]
    fn small_max_activity_0() {
        let events = HashMap::new();
        let ctx = Context {
            events,
            max: 0,
            today: NaiveDate::from_ymd(2020, 06, 06),
        };
        let grade = ctx.activity_for_day(&NaiveDate::from_ymd(2020, 06, 06));
        assert_eq!(grade, ActivityGrade::None);
    }

    #[test]
    fn small_max_activity_1() {
        let grade = calc_activity_grade(1, 1);
        assert_eq!(grade, ActivityGrade::Four);
    }

    #[test]
    fn small_max_activity_2() {
        let grade = calc_activity_grade(2, 2);
        assert_eq!(grade, ActivityGrade::Four);
    }

    #[test]
    fn small_max_activity_3() {
        let grade = calc_activity_grade(3, 3);
        assert_eq!(grade, ActivityGrade::Four);
    }

    #[test]
    fn small_max_activity_4() {
        let grade = calc_activity_grade(4, 4);
        assert_eq!(grade, ActivityGrade::Four);
    }

    #[test]
    fn small_max_activity_2_but_is_1() {
        let grade = calc_activity_grade(1, 2);
        assert_eq!(grade, ActivityGrade::Two);
    }

    #[test]
    fn small_max_activity_3_but_is_2() {
        let grade = calc_activity_grade(2, 3);
        assert_eq!(grade, ActivityGrade::Three);
    }

    #[test]
    fn small_max_activity_3_but_is_1() {
        let grade = calc_activity_grade(1, 3);
        assert_eq!(grade, ActivityGrade::Two);
    }

    #[test]
    fn small_max_activity_4_but_is_3() {
        let grade = calc_activity_grade(3, 4);
        assert_eq!(grade, ActivityGrade::Three);
    }

    #[test]
    fn small_max_activity_4_but_is_2() {
        let grade = calc_activity_grade(2, 4);
        assert_eq!(grade, ActivityGrade::Two);
    }

    #[test]
    fn small_max_activity_4_but_is_1() {
        let grade = calc_activity_grade(1, 4);
        assert_eq!(grade, ActivityGrade::One);
    }

    #[test]
    fn small_max_activity_5_but_is_4() {
        let grade = calc_activity_grade(4, 5);
        assert_eq!(grade, ActivityGrade::Four);
    }

    #[test]
    fn small_max_activity_5_but_is_3() {
        let grade = calc_activity_grade(3, 5);
        assert_eq!(grade, ActivityGrade::Three);
    }

    #[test]
    fn small_max_activity_5_but_is_2() {
        let grade = calc_activity_grade(2, 5);
        assert_eq!(grade, ActivityGrade::Two);
    }

    #[test]
    fn small_max_activity_5_but_is_1() {
        let grade = calc_activity_grade(1, 5);
        assert_eq!(grade, ActivityGrade::One);
    }

    #[test]
    fn test_datespec_year_month() {
        let ds = DateSpec::YearMonth((2020, 05));
        let out = ds.to_term_string(&context());
        assert_eq!(out, "      May 2020       
 Su Mo Tu We Th Fr Sa
               \u{1b}[97m\u{1b}[48;5;238m  1\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m  2\u{1b}[0m
\u{1b}[97m\u{1b}[48;5;238m  3\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m  4\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m  5\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m  6\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m  7\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m  8\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m  9\u{1b}[0m
\u{1b}[97m\u{1b}[48;5;238m 10\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 11\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 12\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 13\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 14\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 15\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 16\u{1b}[0m
\u{1b}[97m\u{1b}[48;5;238m 17\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 18\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 19\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 20\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 21\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 22\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 23\u{1b}[0m
\u{1b}[97m\u{1b}[48;5;238m 24\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 25\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 26\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 27\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 28\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 29\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m\u{1b}[1m#30\u{1b}[0m
 31\u{1b}[0m                  ");
    }

    #[test]
    fn test_datespec_year_month_ends_on_sat() {
        let ds = DateSpec::YearMonth((2020, 10));
        let out = ds.to_term_string(&context());
        assert_eq!(out, "    October 2020     
 Su Mo Tu We Th Fr Sa
              1\u{1b}[0m  2\u{1b}[0m  3\u{1b}[0m
  4\u{1b}[0m  5\u{1b}[0m  6\u{1b}[0m  7\u{1b}[0m  8\u{1b}[0m  9\u{1b}[0m 10\u{1b}[0m
 11\u{1b}[0m 12\u{1b}[0m 13\u{1b}[0m 14\u{1b}[0m 15\u{1b}[0m 16\u{1b}[0m 17\u{1b}[0m
 18\u{1b}[0m 19\u{1b}[0m 20\u{1b}[0m 21\u{1b}[0m 22\u{1b}[0m 23\u{1b}[0m 24\u{1b}[0m
 25\u{1b}[0m 26\u{1b}[0m 27\u{1b}[0m 28\u{1b}[0m 29\u{1b}[0m 30\u{1b}[0m 31\u{1b}[0m");
    }

    #[test]
    fn test_datespec_year() {
        let ds = DateSpec::Year(2020);
        let out = ds.to_term_string(&context());
        assert_eq!(out, "    January 2020            February 2020            March 2020      
 Su Mo Tu We Th Fr Sa    Su Mo Tu We Th Fr Sa    Su Mo Tu We Th Fr Sa
         \u{1b}[97m\u{1b}[48;5;238m  1\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m  2\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m  3\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m  4\u{1b}[0m                     \u{1b}[97m\u{1b}[48;5;238m  1\u{1b}[0m   \u{1b}[97m\u{1b}[48;5;238m  1\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m  2\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m  3\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m  4\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m  5\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m  6\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m  7\u{1b}[0m
\u{1b}[97m\u{1b}[48;5;238m  5\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m  6\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m  7\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m  8\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m  9\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 10\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 11\u{1b}[0m   \u{1b}[97m\u{1b}[48;5;238m  2\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m  3\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m  4\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m  5\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m  6\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m  7\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m  8\u{1b}[0m   \u{1b}[97m\u{1b}[48;5;238m  8\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m  9\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 10\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 11\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 12\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 13\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 14\u{1b}[0m
\u{1b}[97m\u{1b}[48;5;238m 12\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 13\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 14\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 15\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 16\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 17\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 18\u{1b}[0m   \u{1b}[97m\u{1b}[48;5;238m  9\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 10\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 11\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 12\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 13\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 14\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 15\u{1b}[0m   \u{1b}[97m\u{1b}[48;5;238m 15\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 16\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 17\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 18\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 19\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 20\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 21\u{1b}[0m
\u{1b}[97m\u{1b}[48;5;238m 19\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 20\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 21\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 22\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 23\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 24\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 25\u{1b}[0m   \u{1b}[97m\u{1b}[48;5;238m 16\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 17\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 18\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 19\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 20\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 21\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 22\u{1b}[0m   \u{1b}[97m\u{1b}[48;5;238m 22\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 23\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 24\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 25\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 26\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 27\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 28\u{1b}[0m
\u{1b}[97m\u{1b}[48;5;238m 26\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 27\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 28\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 29\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 30\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 31\u{1b}[0m      \u{1b}[97m\u{1b}[48;5;238m 23\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 24\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 25\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 26\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 27\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 28\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 29\u{1b}[0m   \u{1b}[97m\u{1b}[48;5;238m 29\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 30\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 31\u{1b}[0m            
                                                                     
     April 2020               May 2020                June 2020      
 Su Mo Tu We Th Fr Sa    Su Mo Tu We Th Fr Sa    Su Mo Tu We Th Fr Sa
         \u{1b}[97m\u{1b}[48;5;238m  1\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m  2\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m  3\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m  4\u{1b}[0m                  \u{1b}[97m\u{1b}[48;5;238m  1\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m  2\u{1b}[0m        1\u{1b}[0m  2\u{1b}[0m  3\u{1b}[0m  4\u{1b}[0m  5\u{1b}[0m  6\u{1b}[0m
\u{1b}[97m\u{1b}[48;5;238m  5\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m  6\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m  7\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m  8\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m  9\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 10\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 11\u{1b}[0m   \u{1b}[97m\u{1b}[48;5;238m  3\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m  4\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m  5\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m  6\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m  7\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m  8\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m  9\u{1b}[0m     7\u{1b}[0m  8\u{1b}[0m  9\u{1b}[0m 10\u{1b}[0m 11\u{1b}[0m 12\u{1b}[0m 13\u{1b}[0m
\u{1b}[97m\u{1b}[48;5;238m 12\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 13\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 14\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 15\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 16\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 17\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 18\u{1b}[0m   \u{1b}[97m\u{1b}[48;5;238m 10\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 11\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 12\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 13\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 14\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 15\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 16\u{1b}[0m    14\u{1b}[0m 15\u{1b}[0m 16\u{1b}[0m 17\u{1b}[0m 18\u{1b}[0m 19\u{1b}[0m 20\u{1b}[0m
\u{1b}[97m\u{1b}[48;5;238m 19\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 20\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 21\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 22\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 23\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 24\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 25\u{1b}[0m   \u{1b}[97m\u{1b}[48;5;238m 17\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 18\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 19\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 20\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 21\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 22\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 23\u{1b}[0m    21\u{1b}[0m 22\u{1b}[0m 23\u{1b}[0m 24\u{1b}[0m 25\u{1b}[0m 26\u{1b}[0m 27\u{1b}[0m
\u{1b}[97m\u{1b}[48;5;238m 26\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 27\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 28\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 29\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 30\u{1b}[0m         \u{1b}[97m\u{1b}[48;5;238m 24\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 25\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 26\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 27\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 28\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m 29\u{1b}[0m\u{1b}[97m\u{1b}[48;5;238m\u{1b}[1m#30\u{1b}[0m    28\u{1b}[0m 29\u{1b}[0m 30\u{1b}[0m            
                         31\u{1b}[0m                                          
      July 2020              August 2020           September 2020    
 Su Mo Tu We Th Fr Sa    Su Mo Tu We Th Fr Sa    Su Mo Tu We Th Fr Sa
           1\u{1b}[0m  2\u{1b}[0m  3\u{1b}[0m  4\u{1b}[0m                       1\u{1b}[0m           1\u{1b}[0m  2\u{1b}[0m  3\u{1b}[0m  4\u{1b}[0m  5\u{1b}[0m
  5\u{1b}[0m  6\u{1b}[0m  7\u{1b}[0m  8\u{1b}[0m  9\u{1b}[0m 10\u{1b}[0m 11\u{1b}[0m     2\u{1b}[0m  3\u{1b}[0m  4\u{1b}[0m  5\u{1b}[0m  6\u{1b}[0m  7\u{1b}[0m  8\u{1b}[0m     6\u{1b}[0m  7\u{1b}[0m  8\u{1b}[0m  9\u{1b}[0m 10\u{1b}[0m 11\u{1b}[0m 12\u{1b}[0m
 12\u{1b}[0m 13\u{1b}[0m 14\u{1b}[0m 15\u{1b}[0m 16\u{1b}[0m 17\u{1b}[0m 18\u{1b}[0m     9\u{1b}[0m 10\u{1b}[0m 11\u{1b}[0m 12\u{1b}[0m 13\u{1b}[0m 14\u{1b}[0m 15\u{1b}[0m    13\u{1b}[0m 14\u{1b}[0m 15\u{1b}[0m 16\u{1b}[0m 17\u{1b}[0m 18\u{1b}[0m 19\u{1b}[0m
 19\u{1b}[0m 20\u{1b}[0m 21\u{1b}[0m 22\u{1b}[0m 23\u{1b}[0m 24\u{1b}[0m 25\u{1b}[0m    16\u{1b}[0m 17\u{1b}[0m 18\u{1b}[0m 19\u{1b}[0m 20\u{1b}[0m 21\u{1b}[0m 22\u{1b}[0m    20\u{1b}[0m 21\u{1b}[0m 22\u{1b}[0m 23\u{1b}[0m 24\u{1b}[0m 25\u{1b}[0m 26\u{1b}[0m
 26\u{1b}[0m 27\u{1b}[0m 28\u{1b}[0m 29\u{1b}[0m 30\u{1b}[0m 31\u{1b}[0m       23\u{1b}[0m 24\u{1b}[0m 25\u{1b}[0m 26\u{1b}[0m 27\u{1b}[0m 28\u{1b}[0m 29\u{1b}[0m    27\u{1b}[0m 28\u{1b}[0m 29\u{1b}[0m 30\u{1b}[0m         
                         30\u{1b}[0m 31\u{1b}[0m                                       
    October 2020            November 2020           December 2020    
 Su Mo Tu We Th Fr Sa    Su Mo Tu We Th Fr Sa    Su Mo Tu We Th Fr Sa
              1\u{1b}[0m  2\u{1b}[0m  3\u{1b}[0m     1\u{1b}[0m  2\u{1b}[0m  3\u{1b}[0m  4\u{1b}[0m  5\u{1b}[0m  6\u{1b}[0m  7\u{1b}[0m           1\u{1b}[0m  2\u{1b}[0m  3\u{1b}[0m  4\u{1b}[0m  5\u{1b}[0m
  4\u{1b}[0m  5\u{1b}[0m  6\u{1b}[0m  7\u{1b}[0m  8\u{1b}[0m  9\u{1b}[0m 10\u{1b}[0m     8\u{1b}[0m  9\u{1b}[0m 10\u{1b}[0m 11\u{1b}[0m 12\u{1b}[0m 13\u{1b}[0m 14\u{1b}[0m     6\u{1b}[0m  7\u{1b}[0m  8\u{1b}[0m  9\u{1b}[0m 10\u{1b}[0m 11\u{1b}[0m 12\u{1b}[0m
 11\u{1b}[0m 12\u{1b}[0m 13\u{1b}[0m 14\u{1b}[0m 15\u{1b}[0m 16\u{1b}[0m 17\u{1b}[0m    15\u{1b}[0m 16\u{1b}[0m 17\u{1b}[0m 18\u{1b}[0m 19\u{1b}[0m 20\u{1b}[0m 21\u{1b}[0m    13\u{1b}[0m 14\u{1b}[0m 15\u{1b}[0m 16\u{1b}[0m 17\u{1b}[0m 18\u{1b}[0m 19\u{1b}[0m
 18\u{1b}[0m 19\u{1b}[0m 20\u{1b}[0m 21\u{1b}[0m 22\u{1b}[0m 23\u{1b}[0m 24\u{1b}[0m    22\u{1b}[0m 23\u{1b}[0m 24\u{1b}[0m 25\u{1b}[0m 26\u{1b}[0m 27\u{1b}[0m 28\u{1b}[0m    20\u{1b}[0m 21\u{1b}[0m 22\u{1b}[0m 23\u{1b}[0m 24\u{1b}[0m 25\u{1b}[0m 26\u{1b}[0m
 25\u{1b}[0m 26\u{1b}[0m 27\u{1b}[0m 28\u{1b}[0m 29\u{1b}[0m 30\u{1b}[0m 31\u{1b}[0m    29\u{1b}[0m 30\u{1b}[0m                   27\u{1b}[0m 28\u{1b}[0m 29\u{1b}[0m 30\u{1b}[0m 31\u{1b}[0m      
                                                                     ");
    }
}
