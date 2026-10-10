//! Listening history, kept on this computer only.
//!
//! A play is noted once a song has run for thirty seconds, or half its
//! length when it is shorter, the way Spotify counts one. Each is a line
//! of JSON appended to a file per account, so a write cut short costs one
//! line and never the file. The Stats page sums them up by week, month,
//! year and all time. Nothing here leaves the machine.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use serde::{Deserialize, Serialize};

/// How far into a song a play counts, in milliseconds.
pub const COUNTS_AFTER_MS: u32 = 30_000;

const DAY: u64 = 86_400;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PlayRecord {
    /// Seconds since the Unix epoch when the play was counted.
    pub at: u64,
    pub uri: String,
    pub name: String,
    #[serde(default)]
    pub artists: Vec<String>,
    #[serde(default)]
    pub artist_ids: Vec<Option<String>>,
    #[serde(default)]
    pub album: String,
    #[serde(default)]
    pub album_id: Option<String>,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub duration_ms: u32,
}

/// Reads every whole line; a line cut short or from another version is
/// skipped rather than failing the file.
pub fn read(path: &Path) -> Vec<PlayRecord> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

pub fn append(path: &Path, record: &PlayRecord) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut line = serde_json::to_string(record).map_err(std::io::Error::other)?;
    line.push('\n');
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(line.as_bytes())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Period {
    Week,
    #[default]
    Month,
    Year,
    All,
}

impl Period {
    pub const ALL: [Period; 4] = [Self::Week, Self::Month, Self::Year, Self::All];

    pub fn label(self) -> &'static str {
        match self {
            Self::Week => "This week",
            Self::Month => "This month",
            Self::Year => "This year",
            Self::All => "All time",
        }
    }

    /// How many seconds back the period reaches from `now`.
    fn span(self) -> Option<u64> {
        match self {
            Self::Week => Some(7 * DAY),
            Self::Month => Some(30 * DAY),
            Self::Year => Some(365 * DAY),
            Self::All => None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TopSong {
    pub uri: String,
    pub name: String,
    pub artists: Vec<String>,
    pub image: Option<String>,
    pub plays: u32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TopArtist {
    pub id: Option<String>,
    pub name: String,
    pub plays: u32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TopAlbum {
    pub id: Option<String>,
    pub name: String,
    pub artist: String,
    pub image: Option<String>,
    pub plays: u32,
}

/// One bar of the listening chart: a day or a month, and its plays.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Bar {
    pub label: String,
    pub plays: u32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Summary {
    pub plays: u32,
    /// Whole songs counted, so a listen is its song's length.
    pub listened_ms: u64,
    pub distinct_songs: u32,
    pub distinct_artists: u32,
    pub top_songs: Vec<TopSong>,
    pub top_artists: Vec<TopArtist>,
    pub top_albums: Vec<TopAlbum>,
    /// By day for a week or a month, by month for a year or all time.
    pub bars: Vec<Bar>,
    /// Days in a row with a play, ending today or yesterday.
    pub streak_days: u32,
    /// The hour of the day most plays fall in, 0 to 23.
    pub peak_hour: Option<u8>,
}

const TOP: usize = 10;

/// Sums the records in `period`, as of `now` (seconds since the epoch).
pub fn summarize(records: &[PlayRecord], period: Period, now: u64) -> Summary {
    let since = period.span().map(|span| now.saturating_sub(span));
    let within: Vec<&PlayRecord> = records
        .iter()
        .filter(|record| since.is_none_or(|since| record.at >= since) && record.at <= now)
        .collect();
    let mut songs: HashMap<&str, TopSong> = HashMap::new();
    let mut artists: HashMap<String, TopArtist> = HashMap::new();
    let mut albums: HashMap<String, TopAlbum> = HashMap::new();
    let mut hours = [0u32; 24];
    let mut listened_ms = 0u64;
    for record in &within {
        listened_ms += u64::from(record.duration_ms);
        hours[((record.at % DAY) / 3_600) as usize] += 1;
        let song = songs.entry(&record.uri).or_insert_with(|| TopSong {
            uri: record.uri.clone(),
            name: record.name.clone(),
            artists: record.artists.clone(),
            image: record.image.clone(),
            plays: 0,
        });
        song.plays += 1;
        for (index, name) in record.artists.iter().enumerate() {
            let id = record.artist_ids.get(index).cloned().flatten();
            let key = id.clone().unwrap_or_else(|| name.to_lowercase());
            let artist = artists.entry(key).or_insert_with(|| TopArtist {
                id,
                name: name.clone(),
                plays: 0,
            });
            artist.plays += 1;
        }
        if !record.album.is_empty() {
            let key = record
                .album_id
                .clone()
                .unwrap_or_else(|| record.album.to_lowercase());
            let album = albums.entry(key).or_insert_with(|| TopAlbum {
                id: record.album_id.clone(),
                name: record.album.clone(),
                artist: record.artists.first().cloned().unwrap_or_default(),
                image: record.image.clone(),
                plays: 0,
            });
            album.plays += 1;
        }
    }
    let distinct_songs = songs.len() as u32;
    let distinct_artists = artists.len() as u32;
    let mut top_songs: Vec<TopSong> = songs.into_values().collect();
    top_songs.sort_by(|a, b| b.plays.cmp(&a.plays).then_with(|| a.name.cmp(&b.name)));
    top_songs.truncate(TOP);
    let mut top_artists: Vec<TopArtist> = artists.into_values().collect();
    top_artists.sort_by(|a, b| b.plays.cmp(&a.plays).then_with(|| a.name.cmp(&b.name)));
    top_artists.truncate(TOP);
    let mut top_albums: Vec<TopAlbum> = albums.into_values().collect();
    top_albums.sort_by(|a, b| b.plays.cmp(&a.plays).then_with(|| a.name.cmp(&b.name)));
    top_albums.truncate(TOP);
    let peak_hour = hours
        .iter()
        .enumerate()
        .filter(|(_, plays)| **plays > 0)
        .max_by_key(|(_, plays)| **plays)
        .map(|(hour, _)| hour as u8);
    Summary {
        plays: within.len() as u32,
        listened_ms,
        distinct_songs,
        distinct_artists,
        top_songs,
        top_artists,
        top_albums,
        bars: bars(&within, period, now),
        streak_days: streak(records, now),
        peak_hour,
    }
}

fn bars(within: &[&PlayRecord], period: Period, now: u64) -> Vec<Bar> {
    let today = now / DAY;
    match period {
        Period::Week | Period::Month => {
            let days = if period == Period::Week { 7 } else { 30 };
            let mut counts = vec![0u32; days];
            for record in within {
                let day = record.at / DAY;
                let back = today.saturating_sub(day) as usize;
                if back < days {
                    counts[days - 1 - back] += 1;
                }
            }
            counts
                .into_iter()
                .enumerate()
                .map(|(index, plays)| Bar {
                    label: day_label(today - (days - 1 - index) as u64),
                    plays,
                })
                .collect()
        }
        Period::Year | Period::All => {
            let (year, month) = year_month(today);
            let mut counts: BTreeMap<(i32, u32), u32> = BTreeMap::new();
            for back in 0..12u32 {
                let total = year as i64 * 12 + month as i64 - 1 - back as i64;
                let key = (
                    (total.div_euclid(12)) as i32,
                    (total.rem_euclid(12) + 1) as u32,
                );
                counts.insert(key, 0);
            }
            for record in within {
                let key = year_month(record.at / DAY);
                if let Some(count) = counts.get_mut(&key) {
                    *count += 1;
                }
            }
            counts
                .into_iter()
                .map(|((_, month), plays)| Bar {
                    label: MONTHS[(month - 1) as usize].into(),
                    plays,
                })
                .collect()
        }
    }
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const WEEKDAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];

/// The weekday of a day count since the epoch, which was a Thursday.
fn day_label(day: u64) -> String {
    WEEKDAYS[(day % 7) as usize].into()
}

/// The year and month of a day count since the epoch.
fn year_month(day: u64) -> (i32, u32) {
    // Howard Hinnant's civil-from-days.
    let z = day as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    (year as i32, m as u32)
}

/// Days in a row with at least one play, ending today or, if today has
/// none yet, yesterday.
fn streak(records: &[PlayRecord], now: u64) -> u32 {
    let today = now / DAY;
    let mut days: Vec<u64> = records.iter().map(|record| record.at / DAY).collect();
    days.sort_unstable();
    days.dedup();
    let Some(&last) = days.last() else {
        return 0;
    };
    if last + 1 < today {
        return 0;
    }
    let mut streak = 1;
    for pair in days.windows(2).rev() {
        if pair[1] == pair[0] + 1 {
            streak += 1;
        } else {
            break;
        }
    }
    streak
}

#[cfg(test)]
mod tests {
    use super::*;

    fn play(at: u64, uri: &str, artist: &str, album: &str) -> PlayRecord {
        PlayRecord {
            at,
            uri: uri.into(),
            name: uri.to_uppercase(),
            artists: vec![artist.into()],
            artist_ids: vec![Some(format!("id-{artist}"))],
            album: album.into(),
            album_id: Some(format!("alb-{album}")),
            image: None,
            duration_ms: 200_000,
        }
    }

    /// Only plays inside the period count, the most played lead each
    /// list, the chart has one bar per day of the week, and the streak
    /// counts the days in a row up to today.
    #[test]
    fn the_week_is_summed_from_its_own_plays() {
        let now = 20_000 * DAY + 10 * 3_600;
        let records = vec![
            play(now - 6 * DAY, "a", "Bonobo", "Fragments"),
            play(now - 6 * DAY + 60, "a", "Bonobo", "Fragments"),
            play(now - DAY, "b", "Khruangbin", "Mordechai"),
            play(now, "a", "Bonobo", "Fragments"),
            play(now - 20 * DAY, "c", "Old", "Old"),
        ];
        let week = summarize(&records, Period::Week, now);
        assert_eq!(week.plays, 4);
        assert_eq!(week.listened_ms, 800_000);
        assert_eq!(week.distinct_songs, 2);
        assert_eq!(week.distinct_artists, 2);
        assert_eq!(week.top_songs[0].uri, "a");
        assert_eq!(week.top_songs[0].plays, 3);
        assert_eq!(week.top_artists[0].name, "Bonobo");
        assert_eq!(week.top_albums[0].name, "Fragments");
        assert_eq!(week.bars.len(), 7);
        assert_eq!(
            week.bars.iter().map(|bar| bar.plays).collect::<Vec<_>>(),
            vec![2, 0, 0, 0, 0, 1, 1]
        );
        assert_eq!(week.streak_days, 2);
        assert_eq!(week.peak_hour, Some(10));
        let all = summarize(&records, Period::All, now);
        assert_eq!(all.plays, 5);
        assert_eq!(all.bars.len(), 12, "a year of months");
    }

    #[test]
    fn a_streak_ends_when_a_day_is_missed() {
        let now = 20_000 * DAY;
        let records = vec![
            play(now - 3 * DAY, "a", "x", "y"),
            play(now - 2 * DAY, "a", "x", "y"),
            play(now, "a", "x", "y"),
        ];
        assert_eq!(summarize(&records, Period::All, now).streak_days, 1);
        let gone = vec![play(now - 5 * DAY, "a", "x", "y")];
        assert_eq!(summarize(&gone, Period::All, now).streak_days, 0);
        assert_eq!(summarize(&[], Period::All, now).streak_days, 0);
    }

    #[test]
    fn civil_dates_come_out_right() {
        assert_eq!(year_month(0), (1970, 1));
        assert_eq!(year_month(20_000), (2024, 10));
        assert_eq!(day_label(0), "Thu");
        assert_eq!(day_label(4), "Mon");
    }

    /// A record survives the round trip through the file, and a line cut
    /// short is skipped rather than losing the rest.
    #[test]
    fn records_append_as_lines_and_a_torn_line_is_skipped() {
        let dir = std::env::temp_dir().join(format!(
            "oxidify-history-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = dir.join("history.jsonl");
        let first = play(1_000, "a", "x", "y");
        let second = play(2_000, "b", "x", "y");
        append(&path, &first).unwrap();
        append(&path, &second).unwrap();
        let mut text = std::fs::read_to_string(&path).unwrap();
        text.push_str("{\"at\":3000,\"uri\":\"c\"");
        std::fs::write(&path, text).unwrap();
        assert_eq!(read(&path), vec![first, second]);
        let _ = std::fs::remove_dir_all(dir);
    }
}
