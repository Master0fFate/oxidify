//! Formatting helpers shared by every view.

/// `3:45` for track lengths, `1:02:03` past an hour.
pub fn format_duration_ms(ms: u32) -> String {
    let total = ms / 1000;
    let hours = total / 3600;
    let minutes = (total / 60) % 60;
    let seconds = total % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// `2 hr 13 min` for playlist totals, `45 min 12 sec` under an hour.
pub fn format_total_ms(ms: u64) -> String {
    let total = ms / 1000;
    let hours = total / 3600;
    let minutes = (total / 60) % 60;
    let seconds = total % 60;
    if hours > 0 {
        format!("{hours} hr {minutes} min")
    } else if minutes > 0 {
        format!("{minutes} min {seconds} sec")
    } else {
        format!("{seconds} sec")
    }
}

/// Episode lengths read as `1 hr 12 min` or `38 min`.
pub fn format_episode_ms(ms: u32) -> String {
    let minutes = ms / 60_000;
    let hours = minutes / 60;
    if hours > 0 {
        format!("{hours} hr {} min", minutes % 60)
    } else {
        format!("{} min", minutes.max(1))
    }
}

pub fn format_count(count: u64) -> String {
    let digits = count.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, character) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(character);
    }
    out
}

/// `Jan 5, 2024` from an ISO-8601 timestamp or a bare date.
pub fn format_date(iso: &str) -> String {
    let date = iso.get(..10).unwrap_or(iso);
    let mut parts = date.split('-');
    let (Some(year), Some(month)) = (parts.next(), parts.next()) else {
        return iso.to_string();
    };
    let day = parts.next();
    let month_name = match month {
        "01" => "Jan",
        "02" => "Feb",
        "03" => "Mar",
        "04" => "Apr",
        "05" => "May",
        "06" => "Jun",
        "07" => "Jul",
        "08" => "Aug",
        "09" => "Sep",
        "10" => "Oct",
        "11" => "Nov",
        "12" => "Dec",
        _ => return iso.to_string(),
    };
    match day.and_then(|day| day.trim_start_matches('0').parse::<u8>().ok()) {
        Some(day) => format!("{month_name} {day}, {year}"),
        None => format!("{month_name} {year}"),
    }
}

/// `Today`, `Yesterday`, or `N days ago` for the last 29 days, then a date.
pub fn format_added_at(iso: &str) -> String {
    format_added_at_from(iso, utc_today())
}

fn utc_today() -> (i32, u32, u32) {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    ymd_from_unix_days((secs / 86_400) as i32)
}

fn parse_ymd(iso: &str) -> Option<(i32, u32, u32)> {
    let date = iso.get(..10)?;
    let mut parts = date.split('-');
    let year = parts.next()?.parse().ok()?;
    let month = parts.next()?.parse().ok()?;
    let day = parts.next()?.parse().ok()?;
    ((1..=12).contains(&month) && (1..=31).contains(&day)).then_some((year, month, day))
}

fn days_from_civil(year: i32, month: u32, day: u32) -> i32 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400) as u32;
    let month_prime = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * month_prime + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era as i32 - 719_468
}

fn ymd_from_unix_days(days: i32) -> (i32, u32, u32) {
    let serial = days + 719_468;
    let era = serial.div_euclid(146_097);
    let day_of_era = serial.rem_euclid(146_097) as u32;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = era * 400 + year_of_era as i32;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    };
    let year = if month <= 2 { year + 1 } else { year };
    (year, month, day)
}

fn format_added_at_from(iso: &str, today: (i32, u32, u32)) -> String {
    let Some((year, month, day)) = parse_ymd(iso) else {
        return format_date(iso);
    };
    let delta = days_from_civil(today.0, today.1, today.2) - days_from_civil(year, month, day);
    match delta {
        0 => "Today".into(),
        1 => "Yesterday".into(),
        2..=29 => format!("{delta} days ago"),
        _ => format_date(iso),
    }
}

/// Tears the id out of `spotify:track:abc` and friends.
pub fn uri_id(uri: &str) -> Option<&str> {
    uri.rsplit(':').next().filter(|id| !id.is_empty())
}

pub fn uri_kind(uri: &str) -> Option<&str> {
    let mut parts = uri.split(':');
    parts.next()?;
    parts.next()
}

/// Whether two context uris name the same thing. Spotify reports a
/// personalised playlist with its owner embedded, `spotify:user:<id>:
/// playlist:<id>`, while cards and rows hold the plain form.
pub fn same_context(a: &str, b: &str) -> bool {
    a == b || canonical_context(a) == canonical_context(b)
}

fn canonical_context(uri: &str) -> String {
    let parts: Vec<&str> = uri.split(':').collect();
    match parts.as_slice() {
        ["spotify", "user", _, kind, id] if *kind == "playlist" => format!("spotify:{kind}:{id}"),
        _ => uri.to_string(),
    }
}

pub fn open_spotify_url(uri: &str) -> Option<String> {
    let kind = uri_kind(uri)?;
    let id = uri_id(uri)?;
    Some(format!("https://open.spotify.com/{kind}/{id}"))
}

pub const BRAND_GRADIENT: [[u8; 3]; 3] =
    [[0x66, 0x66, 0xff], [0xb0, 0x5a, 0x8a], [0xd2, 0x57, 0x2a]];
pub const BRAND_WORDMARK: [u8; 3] = [0xc2, 0x55, 0x1f];
pub const MARK_CENTER: (f32, f32) = (88.75, 80.0);
pub const MARK_SIDE: f32 = 148.0;
pub const MARK_BAR_RADIUS: f32 = 6.0;
pub const MARK_BARS: [(f32, f32, f32); 6] = [
    (26.0, 68.0, 92.0),
    (46.0, 48.0, 112.0),
    (66.0, 30.0, 130.0),
    (86.0, 52.0, 108.0),
    (106.0, 38.0, 122.0),
    (126.0, 62.0, 98.0),
];
pub const MARK_FLAKES: [(f32, f32, f32, f32); 3] = [
    (141.5, 55.5, 7.0, 20.0),
    (148.5, 76.5, 5.0, -15.0),
    (143.0, 103.0, 6.0, 35.0),
];
pub const MARK_DOT: (f32, f32, f32) = (155.0, 90.0, 2.5);

pub fn brand_color_at(x: f32) -> [u8; 3] {
    let t = ((x - 20.0) / 120.0).clamp(0.0, 1.0) * 2.0;
    let (from, to, local) = if t < 1.0 {
        (BRAND_GRADIENT[0], BRAND_GRADIENT[1], t)
    } else {
        (BRAND_GRADIENT[1], BRAND_GRADIENT[2], t - 1.0)
    };
    std::array::from_fn(|c| {
        (from[c] as f32 + (to[c] as f32 - from[c] as f32) * local).round() as u8
    })
}

/// The application icon, shared by the native window and tray.
///
/// macOS template images use only the alpha channel. For that surface we
/// keep the mark's shape, while the OS supplies black or white.
pub fn tray_template_rgba(size: usize) -> Vec<u8> {
    render_app_icon(size, false)
}

/// Rasterises the same mark as `packaging/icons/oxidify.svg`.
/// Four-by-four sampling keeps the taskbar and tray versions clean at 16px.
pub fn app_icon_rgba(size: usize) -> Vec<u8> {
    render_app_icon(size, true)
}

fn render_app_icon(size: usize, colored: bool) -> Vec<u8> {
    const SAMPLES: usize = 4;
    const SAMPLE_COUNT: u32 = (SAMPLES * SAMPLES) as u32;
    let unit = MARK_SIDE / size as f32;
    let left = MARK_CENTER.0 - MARK_SIDE / 2.0;
    let top = MARK_CENTER.1 - MARK_SIDE / 2.0;
    let mut rgba = vec![0u8; size * size * 4];
    for y in 0..size {
        for x in 0..size {
            let mut hits = 0u32;
            let mut sum = [0u32; 3];
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let mx = left + (x as f32 + (sx as f32 + 0.5) / SAMPLES as f32) * unit;
                    let my = top + (y as f32 + (sy as f32 + 0.5) / SAMPLES as f32) * unit;
                    if let Some(color) = mark_color_at(mx, my) {
                        hits += 1;
                        for channel in 0..3 {
                            sum[channel] += u32::from(color[channel]);
                        }
                    }
                }
            }
            if hits == 0 {
                continue;
            }
            let index = (y * size + x) * 4;
            if colored {
                for channel in 0..3 {
                    rgba[index + channel] = ((sum[channel] + hits / 2) / hits) as u8;
                }
            }
            rgba[index + 3] = (hits * 255 / SAMPLE_COUNT) as u8;
        }
    }
    rgba
}

fn mark_color_at(x: f32, y: f32) -> Option<[u8; 3]> {
    let on_bar = MARK_BARS.iter().any(|&(bar_x, top, bottom)| {
        (x - bar_x).hypot(y - y.clamp(top, bottom)) <= MARK_BAR_RADIUS
    });
    if on_bar {
        return Some(brand_color_at(x));
    }
    let on_flake = MARK_FLAKES.iter().any(|&(cx, cy, side, degrees)| {
        let (sin, cos) = degrees.to_radians().sin_cos();
        let (dx, dy) = (x - cx, y - cy);
        (dx * cos + dy * sin).abs() <= side / 2.0 && (dy * cos - dx * sin).abs() <= side / 2.0
    });
    let (dot_x, dot_y, dot_radius) = MARK_DOT;
    (on_flake || (x - dot_x).hypot(y - dot_y) <= dot_radius).then_some(BRAND_GRADIENT[2])
}

pub fn greeting() -> &'static str {
    match local_hour() {
        5..=11 => "Good morning",
        12..=17 => "Good afternoon",
        _ => "Good evening",
    }
}

fn local_hour() -> u8 {
    jiff::Zoned::now().hour() as u8
}

/// Strips the HTML Spotify embeds in playlist descriptions.
pub fn strip_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_tag = false;
    for character in text.chars() {
        match character {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(character),
            _ => {}
        }
    }
    out.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&#x2F;", "/")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A personalised playlist's context carries the owner; the card does
    /// not, and they still mean the same playlist. A user's collection
    /// keeps its owner, since that is what names it.
    #[test]
    fn contexts_match_with_or_without_the_owner() {
        assert!(same_context(
            "spotify:user:spotify:playlist:37i9",
            "spotify:playlist:37i9"
        ));
        assert!(same_context("spotify:album:a", "spotify:album:a"));
        assert!(!same_context("spotify:playlist:a", "spotify:playlist:b"));
        assert!(!same_context(
            "spotify:user:me:collection",
            "spotify:user:you:collection"
        ));
    }

    #[test]
    fn durations() {
        assert_eq!(format_duration_ms(225_000), "3:45");
        assert_eq!(format_duration_ms(3_723_000), "1:02:03");
        assert_eq!(format_total_ms(7_980_000), "2 hr 13 min");
        assert_eq!(format_total_ms(2_712_000), "45 min 12 sec");
        assert_eq!(format_episode_ms(4_320_000), "1 hr 12 min");
    }

    #[test]
    fn counts_and_dates() {
        assert_eq!(format_count(1_234_567), "1,234,567");
        assert_eq!(format_count(12), "12");
        assert_eq!(format_date("2024-01-05T10:00:00Z"), "Jan 5, 2024");
        assert_eq!(format_date("2024-03"), "Mar 2024");
        assert_eq!(format_date("2024"), "2024");
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(ymd_from_unix_days(0), (1970, 1, 1));
        assert_eq!(
            format_added_at_from("2026-09-11T08:00:00Z", (2026, 9, 11)),
            "Today"
        );
        assert_eq!(
            format_added_at_from("2026-09-10", (2026, 9, 11)),
            "Yesterday"
        );
        assert_eq!(
            format_added_at_from("2026-08-20", (2026, 9, 11)),
            "22 days ago"
        );
        assert_eq!(
            format_added_at_from("2026-01-05", (2026, 9, 11)),
            "Jan 5, 2026"
        );
    }

    #[test]
    fn uris() {
        assert_eq!(uri_id("spotify:track:abc"), Some("abc"));
        assert_eq!(uri_kind("spotify:playlist:x"), Some("playlist"));
        assert_eq!(
            open_spotify_url("spotify:album:z").as_deref(),
            Some("https://open.spotify.com/album/z")
        );
    }

    #[test]
    fn html_is_stripped() {
        assert_eq!(
            strip_html("Hi <a href=\"x\">there</a> &amp; you"),
            "Hi there & you"
        );
        assert_eq!(strip_html("ONE&#x2F;TWO&#x2F;THREE"), "ONE/TWO/THREE");
    }

    #[test]
    fn app_icon_is_the_oxide_wave_mark() {
        let rgba = app_icon_rgba(128);
        assert_eq!(rgba.len(), 128 * 128 * 4);
        let (pixels, remainder) = rgba.as_chunks::<4>();
        assert!(remainder.is_empty());
        assert!(
            pixels
                .iter()
                .any(|pixel| pixel == &[0xd2, 0x57, 0x2a, 0xff])
        );
        assert!(
            pixels
                .iter()
                .any(|pixel| pixel[3] == 0xff && pixel[2] > pixel[0])
        );
        assert!(pixels.iter().any(|pixel| pixel[3] == 0));
    }

    #[test]
    fn tray_template_keeps_only_alpha() {
        let rgba = tray_template_rgba(32);
        assert_eq!(rgba.len(), 32 * 32 * 4);
        let (pixels, remainder) = rgba.as_chunks::<4>();
        assert!(remainder.is_empty());
        assert!(pixels.iter().any(|pixel| pixel[3] > 0));
        assert!(pixels.iter().all(|pixel| pixel[..3] == [0, 0, 0]));
    }
}
