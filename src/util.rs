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

/// The application icon, shared by the native window and tray.
///
/// macOS template images use only the alpha channel. For that surface we
/// keep the crystal ring and oxygen dot, while the OS supplies black or white.
pub fn tray_template_rgba(size: usize) -> Vec<u8> {
    render_app_icon(size, false)
}

/// Rasterises the same light-blue crystal mark as `packaging/icons/oxidify.svg`.
/// Four-by-four sampling keeps the taskbar and tray versions clean at 16px.
pub fn app_icon_rgba(size: usize) -> Vec<u8> {
    render_app_icon(size, true)
}

fn render_app_icon(size: usize, include_field: bool) -> Vec<u8> {
    const FIELD: [u8; 3] = [0x91, 0xc4, 0xff];
    const INK: [u8; 3] = [0x0d, 0x3a, 0x73];
    const SAMPLES: usize = 4;
    if size == 0 {
        return Vec::new();
    }
    let mut rgba = vec![0u8; size * size * 4];
    let sample_count = (SAMPLES * SAMPLES) as f32;
    for y in 0..size {
        for x in 0..size {
            let mut field_hits = 0usize;
            let mut ink_hits = 0usize;
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let px = (x as f32 + (sx as f32 + 0.5) / SAMPLES as f32) * 128.0 / size as f32;
                    let py = (y as f32 + (sy as f32 + 0.5) / SAMPLES as f32) * 128.0 / size as f32;
                    if rounded_square_contains(px, py) {
                        field_hits += 1;
                    }
                    if crystal_contains(px, py) {
                        ink_hits += 1;
                    }
                }
            }
            let coverage = if include_field { field_hits } else { ink_hits };
            if coverage == 0 {
                continue;
            }
            let index = (y * size + x) * 4;
            if include_field {
                let ink_fraction = ink_hits as f32 / field_hits.max(1) as f32;
                for channel in 0..3 {
                    rgba[index + channel] = (FIELD[channel] as f32 * (1.0 - ink_fraction)
                        + INK[channel] as f32 * ink_fraction)
                        .round() as u8;
                }
            } else {
                rgba[index..index + 3].fill(0);
            }
            rgba[index + 3] = (coverage as f32 / sample_count * 255.0).round() as u8;
        }
    }
    rgba
}

fn rounded_square_contains(x: f32, y: f32) -> bool {
    const SIZE: f32 = 128.0;
    const RADIUS: f32 = 28.0;
    if !(0.0..SIZE).contains(&x) || !(0.0..SIZE).contains(&y) {
        return false;
    }
    let nearest_x = x.clamp(RADIUS, SIZE - RADIUS);
    let nearest_y = y.clamp(RADIUS, SIZE - RADIUS);
    (x - nearest_x).powi(2) + (y - nearest_y).powi(2) <= RADIUS.powi(2)
}

fn crystal_contains(x: f32, y: f32) -> bool {
    const OUTER_RADIUS: f32 = 42.0;
    const STROKE: f32 = 9.0;
    const DOT_RADIUS: f32 = 8.0;
    let apothem = OUTER_RADIUS * 0.866_025_4;
    let inner_radius = OUTER_RADIUS * (apothem - STROKE / 2.0) / apothem;
    let outer = crystal_points(OUTER_RADIUS);
    let inner = crystal_points(inner_radius);
    let ring = point_in_polygon((x, y), &outer) && !point_in_polygon((x, y), &inner);
    let dot = (x - 64.0).powi(2) + (y - 64.0).powi(2) <= DOT_RADIUS.powi(2);
    ring || dot
}

fn crystal_points(radius: f32) -> [(f32, f32); 6] {
    std::array::from_fn(|step| {
        let angle = std::f32::consts::TAU * step as f32 / 6.0 - std::f32::consts::FRAC_PI_2;
        (64.0 + angle.cos() * radius, 64.0 + angle.sin() * radius)
    })
}

fn point_in_polygon(point: (f32, f32), polygon: &[(f32, f32); 6]) -> bool {
    let mut inside = false;
    let mut previous = polygon.len() - 1;
    for current in 0..polygon.len() {
        let (xi, yi) = polygon[current];
        let (xj, yj) = polygon[previous];
        if (yi > point.1) != (yj > point.1) && point.0 < (xj - xi) * (point.1 - yi) / (yj - yi) + xi
        {
            inside = !inside;
        }
        previous = current;
    }
    inside
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
    fn app_icon_is_the_blue_crystal_mark() {
        let rgba = app_icon_rgba(128);
        assert_eq!(rgba.len(), 128 * 128 * 4);
        let (pixels, remainder) = rgba.as_chunks::<4>();
        assert!(remainder.is_empty());
        assert!(
            pixels
                .iter()
                .any(|pixel| pixel == &[0x91, 0xc4, 0xff, 0xff])
        );
        assert!(
            pixels
                .iter()
                .any(|pixel| pixel == &[0x0d, 0x3a, 0x73, 0xff])
        );
        assert!(pixels.iter().any(|pixel| pixel[3] == 0));
        assert!(
            pixels
                .iter()
                .filter(|pixel| pixel[3] > 0)
                .all(|pixel| pixel[2] > pixel[1])
        );
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
