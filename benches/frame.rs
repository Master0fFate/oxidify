use std::time::{Duration, Instant};

use oxidify::api::models::{Image, Owner, Playlist};
use oxidify::app::{App, AppOptions};
use oxidify::backend::Waker;
use oxidify::model::Loadable;
use oxidify::settings::Settings;

const PLAYLISTS: usize = 2_000;
const SAVED_ALBUMS: usize = 1_000;
const ARTISTS: usize = 500;
const SHOWS: usize = 300;
const PAGE_TRACKS: usize = 10_000;
const BENCH_PLAYLIST: &str = "pl0";
const WARMUP: usize = 10;
const FRAMES: usize = 120;

fn dirs(tag: &str) -> oxidify::paths::AppDirs {
    let root = std::env::temp_dir().join(format!(
        "oxidify-bench-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock runs")
            .as_nanos()
    ));
    oxidify::paths::AppDirs {
        config: root.join("config"),
        state: root.join("state"),
        cache: root.join("cache"),
    }
}

fn image(url: String) -> Image {
    Image {
        url,
        width: Some(64),
        height: Some(64),
    }
}

fn playlist(index: usize) -> Playlist {
    Playlist {
        id: format!("pl{index}"),
        name: format!("Playlist number {index} for the frame benchmark"),
        uri: format!("spotify:playlist:pl{index}"),
        description: None,
        images: vec![image(format!("https://bench.invalid/{index}.jpg"))],
        owner: Owner {
            id: Some(if index.is_multiple_of(4) {
                "bench".into()
            } else {
                format!("other{}", index % 7)
            }),
            display_name: Some(format!("Owner {}", index % 7)),
            uri: None,
        },
        public: Some(true),
        collaborative: false,
        snapshot_id: None,
        tracks: None,
        items_count: None,
        ..Default::default()
    }
}

fn bench_app() -> App {
    let mut app = App::new(
        &Waker::default(),
        dirs("frame"),
        Settings::default(),
        AppOptions {
            media_controls: false,
            tray: false,
        },
    );
    app.backend.set_offline(true);
    app.auth = oxidify::backend::AuthStatus::Connected {
        username: "bench".into(),
    };
    app.user = Some(oxidify::api::models::User {
        id: "bench".into(),
        display_name: Some("Bench".into()),
        images: Vec::new(),
        product: Some("premium".into()),
        country: Some("DE".into()),
        uri: Some("spotify:user:bench".into()),
    });

    let playlists: Vec<Playlist> = (0..PLAYLISTS).map(playlist).collect();
    app.library.playlists = Loadable::Loaded(playlists);

    let albums: Vec<oxidify::api::models::SavedAlbum> = (0..SAVED_ALBUMS)
        .map(|index| oxidify::api::models::SavedAlbum {
            added_at: Some(format!("2026-01-{:02}T00:00:00Z", 1 + index % 28)),
            album: oxidify::api::models::Album {
                id: format!("al{index}"),
                name: format!("Album {index}"),
                uri: format!("spotify:album:al{index}"),
                images: vec![image(format!("https://bench.invalid/a{index}.jpg"))],
                artists: vec![oxidify::api::models::ArtistRef {
                    id: Some(format!("ar{}", index % ARTISTS)),
                    name: format!("Artist {}", index % ARTISTS),
                    uri: None,
                }],
                ..Default::default()
            },
        })
        .collect();
    app.library.albums.items = albums;

    let artists: Vec<oxidify::api::models::Artist> = (0..ARTISTS)
        .map(|index| oxidify::api::models::Artist {
            id: format!("ar{index}"),
            name: format!("Artist {index}"),
            uri: format!("spotify:artist:ar{index}"),
            images: vec![image(format!("https://bench.invalid/r{index}.jpg"))],
            ..Default::default()
        })
        .collect();
    app.library.artists.items = artists;

    let shows: Vec<oxidify::api::models::SavedShow> = (0..SHOWS)
        .map(|index| oxidify::api::models::SavedShow {
            added_at: None,
            show: oxidify::api::models::Show {
                id: format!("sh{index}"),
                name: format!("Podcast {index}"),
                uri: format!("spotify:show:sh{index}"),
                images: vec![image(format!("https://bench.invalid/s{index}.jpg"))],
                publisher: format!("Publisher {}", index % 9),
                ..Default::default()
            },
        })
        .collect();
    app.library.shows.items = shows;

    app.recent_contexts = (0..40)
        .map(|index| format!("spotify:playlist:pl{}", index * 37 % PLAYLISTS))
        .collect();
    for index in 0..25 {
        app.settings
            .pinned_contexts
            .push(format!("spotify:playlist:pl{}", index * 53 % PLAYLISTS));
    }
    for index in 0..60 {
        app.settings
            .sidebar_order
            .push(format!("spotify:playlist:pl{}", index * 91 % PLAYLISTS));
    }
    app
}

fn open_playlist_page(app: &mut App) {
    let tracks: Vec<oxidify::api::models::PlaylistItem> = (0..PAGE_TRACKS)
        .map(|index| oxidify::api::models::PlaylistItem {
            added_at: Some(format!(
                "2026-0{}-{:02}T10:00:00Z",
                1 + index % 8,
                1 + index % 27
            )),
            added_by: Some(oxidify::api::models::UserRef {
                id: Some(if index.is_multiple_of(3) {
                    "bench".into()
                } else {
                    format!("other{}", index % 7)
                }),
            }),
            is_local: false,
            item: Some(oxidify::api::models::PlayableItem::Track(
                oxidify::api::models::Track {
                    id: Some(format!("tr{index}")),
                    name: format!("Track number {index} in the benchmark playlist"),
                    uri: format!("spotify:track:tr{index}"),
                    duration_ms: 200_000 + index as u32 * 997 % 200_000,
                    artists: vec![oxidify::api::models::ArtistRef {
                        id: Some(format!("ar{}", index % ARTISTS)),
                        name: format!("Artist {}", index % ARTISTS),
                        uri: None,
                    }],
                    album: Some(oxidify::api::models::Album {
                        id: format!("al{}", index % SAVED_ALBUMS),
                        name: format!("Album {}", index % SAVED_ALBUMS),
                        uri: format!("spotify:album:al{}", index % SAVED_ALBUMS),
                        images: vec![image(format!("https://bench.invalid/t{index}.jpg"))],
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            )),
            track: None,
        })
        .collect();
    let mut page = oxidify::model::PlaylistPage {
        playlist: Loadable::Loaded(playlist(0)),
        ..Default::default()
    };
    page.items.absorb(
        0,
        oxidify::api::models::Page {
            items: tracks,
            total: PAGE_TRACKS as u32,
            limit: PAGE_TRACKS as u32,
            offset: 0,
            next: None,
        },
    );
    app.playlist_pages.insert(BENCH_PLAYLIST.into(), page);
    app.open(oxidify::model::Page::Playlist(BENCH_PLAYLIST.into()));
}

fn frame(app: &mut App, ui: &mut egui::Ui) {
    oxidify::ui::show(app, ui);
}

fn stats(samples: &[Duration]) -> (f64, f64, f64, f64) {
    let mut millis: Vec<f64> = samples
        .iter()
        .map(|sample| sample.as_secs_f64() * 1_000.0)
        .collect();
    millis.sort_by(|a, b| a.total_cmp(b));
    let mean = millis.iter().sum::<f64>() / millis.len() as f64;
    let median = millis[millis.len() / 2];
    let p95 = millis[millis.len() * 95 / 100];
    let max = millis[millis.len() - 1];
    (mean, median, p95, max)
}

fn main() {
    let mut app = bench_app();
    let ctx = egui::Context::default();
    ctx.set_pixels_per_point(1.0);
    oxidify::theme::install(&ctx);

    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1280.0, 800.0),
        )),
        ..Default::default()
    };

    let mut at = Instant::now();
    for _ in 0..WARMUP {
        let mut output = ctx.run_ui(input.clone(), |ui| frame(&mut app, ui));
        output.textures_delta.clear();
        at += Duration::from_millis(16);
    }

    let mut samples = Vec::with_capacity(FRAMES);
    for _ in 0..FRAMES {
        let started = Instant::now();
        let mut output = ctx.run_ui(input.clone(), |ui| frame(&mut app, ui));
        output.textures_delta.clear();
        samples.push(started.elapsed());
        at += Duration::from_millis(16);
    }

    let (mean, median, p95, max) = stats(&samples);
    println!(
        "ui::show frame over {PLAYLISTS} playlists, {SAVED_ALBUMS} albums, {ARTISTS} artists, {SHOWS} shows ({FRAMES} frames):"
    );
    println!("  mean   {mean:7.3} ms");
    println!("  median {median:7.3} ms");
    println!("  p95    {p95:7.3} ms");
    println!("  max    {max:7.3} ms");

    app.settings.sidebar_visible = false;
    let mut without = Vec::with_capacity(FRAMES);
    for _ in 0..WARMUP {
        let mut output = ctx.run_ui(input.clone(), |ui| frame(&mut app, ui));
        output.textures_delta.clear();
        at += Duration::from_millis(16);
    }
    for _ in 0..FRAMES {
        let started = Instant::now();
        let mut output = ctx.run_ui(input.clone(), |ui| frame(&mut app, ui));
        output.textures_delta.clear();
        without.push(started.elapsed());
        at += Duration::from_millis(16);
    }
    let (mean, median, p95, max) = stats(&without);
    println!("same frame without the sidebar:");
    println!("  mean   {mean:7.3} ms");
    println!("  median {median:7.3} ms");
    println!("  p95    {p95:7.3} ms");
    println!("  max    {max:7.3} ms");

    open_playlist_page(&mut app);
    let mut playlist_frames = Vec::with_capacity(FRAMES);
    for _ in 0..WARMUP {
        let mut output = ctx.run_ui(input.clone(), |ui| frame(&mut app, ui));
        output.textures_delta.clear();
        at += Duration::from_millis(16);
    }
    for _ in 0..FRAMES {
        let started = Instant::now();
        let mut output = ctx.run_ui(input.clone(), |ui| frame(&mut app, ui));
        output.textures_delta.clear();
        playlist_frames.push(started.elapsed());
        at += Duration::from_millis(16);
    }
    let (mean, median, p95, max) = stats(&playlist_frames);
    println!("playlist page with {PAGE_TRACKS} tracks:");
    println!("  mean   {mean:7.3} ms");
    println!("  median {median:7.3} ms");
    println!("  p95    {p95:7.3} ms");
    println!("  max    {max:7.3} ms");
}
