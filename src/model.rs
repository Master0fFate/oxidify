//! Interface-side state: what is open, what is loaded, what was asked for.

use std::collections::HashMap;
use std::time::Instant;

use crate::api::models::*;

/// Every screen the central panel can show.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Page {
    Home,
    TopSongs,
    Search,
    LikedSongs,
    Albums,
    Artists,
    Podcasts,
    Episodes,
    Playlist(String),
    Album(String),
    Artist(String),
    Show(String),
    Queue,
    Settings,
}

impl Page {
    pub fn encode(&self) -> String {
        match self {
            Page::Home => "home".into(),
            Page::TopSongs => "top-songs".into(),
            Page::Search => "search".into(),
            Page::LikedSongs => "liked".into(),
            Page::Albums => "albums".into(),
            Page::Artists => "artists".into(),
            Page::Podcasts => "podcasts".into(),
            Page::Episodes => "episodes".into(),
            Page::Playlist(id) => format!("playlist:{id}"),
            Page::Album(id) => format!("album:{id}"),
            Page::Artist(id) => format!("artist:{id}"),
            Page::Show(id) => format!("show:{id}"),
            Page::Queue => "queue".into(),
            Page::Settings => "settings".into(),
        }
    }

    pub fn decode(text: &str) -> Option<Self> {
        Some(match text {
            "home" => Page::Home,
            "top-songs" => Page::TopSongs,
            "search" => Page::Search,
            "liked" => Page::LikedSongs,
            "albums" => Page::Albums,
            "artists" => Page::Artists,
            "podcasts" => Page::Podcasts,
            "episodes" => Page::Episodes,
            "queue" => Page::Queue,
            "settings" => Page::Settings,
            other => {
                let (kind, id) = other.split_once(':')?;
                match kind {
                    "playlist" => Page::Playlist(id.into()),
                    "album" => Page::Album(id.into()),
                    "artist" => Page::Artist(id.into()),
                    "show" => Page::Show(id.into()),
                    _ => return None,
                }
            }
        })
    }

    /// Opens whatever a Spotify URI points at.
    pub fn from_uri(uri: &str) -> Option<Self> {
        let mut parts = uri.split(':');
        let _ = parts.next()?;
        let kind = parts.next()?;
        let id = parts.next()?.to_string();
        Some(match kind {
            "playlist" => Page::Playlist(id),
            "album" => Page::Album(id),
            "artist" => Page::Artist(id),
            "show" => Page::Show(id),
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Loadable<T> {
    #[default]
    NotLoaded,
    Loading,
    Loaded(T),
    Failed(String),
}

impl<T> Loadable<T> {
    pub fn get(&self) -> Option<&T> {
        match self {
            Loadable::Loaded(value) => Some(value),
            _ => None,
        }
    }

    pub fn get_mut(&mut self) -> Option<&mut T> {
        match self {
            Loadable::Loaded(value) => Some(value),
            _ => None,
        }
    }

    pub fn is_loading(&self) -> bool {
        matches!(self, Loadable::Loading)
    }

    pub fn needs_load(&self) -> bool {
        matches!(self, Loadable::NotLoaded | Loadable::Failed(_))
    }

    pub fn from_result<E: std::fmt::Display>(result: Result<T, E>) -> Self {
        match result {
            Ok(value) => Loadable::Loaded(value),
            Err(error) => Loadable::Failed(error.to_string()),
        }
    }

    /// Keeps an already loaded value when a refresh fails.
    pub fn refresh<E: std::fmt::Display>(&mut self, result: Result<T, E>) {
        if result.is_ok() || self.get().is_none() {
            *self = Self::from_result(result);
        }
    }
}

// Recreated lists must not reuse the revision of a cached view of the old list.
pub(crate) fn next_view_revision() -> u64 {
    static REVISION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    REVISION.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// How many pages of one list are asked for at once. Spotify answers each
/// page in its own round trip, so a long playlist fetched one page at a
/// time took one round trip per fifty songs; a few in flight together
/// fill it in a fraction of the time without flooding the rate limit.
pub const PAGE_WINDOW: usize = 4;

/// How many items a list fetches on its own, ahead of the scroll. Past
/// this the rest comes as the listener scrolls, so a library of many
/// thousands of liked songs does not cost hundreds of requests on open.
pub const EAGER_ITEMS: u32 = 1_500;

/// An offset-paginated list. Pages are fetched several at a time and may
/// land in any order: a page that arrives ahead of the contiguous end
/// waits in `parked` until the gap before it is filled, so `items` is
/// always a prefix of the list and rows never show with holes.
#[derive(Clone, Debug)]
pub struct PagedList<T> {
    pub items: Vec<T>,
    pub total: Option<u32>,
    /// The first offset not yet held or parked, if any is known to exist.
    pub next_offset: Option<u32>,
    /// Something is on its way.
    pub loading: bool,
    pub error: Option<String>,
    pub loaded_once: bool,
    pub revision: u64,
    /// Offsets requested and not yet answered.
    pub pending: std::collections::BTreeSet<u32>,
    /// Pages that arrived before the ones ahead of them.
    parked: std::collections::BTreeMap<u32, Vec<T>>,
    /// The page size Spotify answered with, once known.
    page_size: u32,
}

impl<T> Default for PagedList<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            total: None,
            next_offset: Some(0),
            loading: false,
            error: None,
            loaded_once: false,
            revision: next_view_revision(),
            pending: std::collections::BTreeSet::new(),
            parked: std::collections::BTreeMap::new(),
            page_size: 50,
        }
    }
}

impl<T> PagedList<T> {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Whether a page can be asked for now: nothing in flight and more
    /// known to exist. The scroll path asks one page at a time this way;
    /// [`Self::fill_ahead`] asks for several.
    pub fn can_load_more(&self) -> bool {
        !self.loading && self.next_offset.is_some()
    }

    pub fn is_complete(&self) -> bool {
        self.loaded_once && self.next_offset.is_none() && self.pending.is_empty()
    }

    /// Marks one offset as requested.
    pub fn begin(&mut self, offset: u32) {
        self.pending.insert(offset);
        self.loading = true;
    }

    /// The offsets to ask for now so that up to `window` pages are in
    /// flight, marked as requested. Nothing past `limit` items is asked for
    /// here; the scroll path fetches the rest. An empty answer means
    /// nothing more is wanted right now.
    pub fn fill_ahead(&mut self, window: usize, limit: u32) -> Vec<u32> {
        let mut offsets = Vec::new();
        let Some(total) = self.total else {
            // Nothing known yet: the first page tells the size.
            if self.pending.is_empty() && self.next_offset == Some(0) {
                self.begin(0);
                offsets.push(0);
            }
            return offsets;
        };
        let page = self.page_size.max(1);
        let mut offset = self.items.len() as u32;
        while self.pending.len() + offsets.len() < window && offset < total && offset < limit {
            if !self.pending.contains(&offset) && !self.parked.contains_key(&offset) {
                offsets.push(offset);
            }
            offset += page;
        }
        for offset in &offsets {
            self.begin(*offset);
        }
        offsets
    }

    pub fn absorb(&mut self, offset: u32, page: Page_<T>) {
        self.pending.remove(&offset);
        if page.limit > 0 {
            self.page_size = page.limit;
        }
        self.total = Some(page.total);
        self.loaded_once = true;
        self.error = None;
        let held = self.items.len() as u32;
        if offset == 0 {
            // A fresh start: whatever was parked belongs to the old list
            // only if it was fetched under the same total; keep it, since
            // the offsets still mean the same rows.
            self.items.clear();
            self.items.extend(page.items);
        } else if offset < held {
            self.items.truncate(offset as usize);
            self.items.extend(page.items);
        } else if offset == held {
            self.items.extend(page.items);
        } else {
            self.parked.insert(offset, page.items);
        }
        // Pages that waited for this one follow it in.
        while let Some(items) = self.parked.remove(&(self.items.len() as u32)) {
            self.items.extend(items);
        }
        self.parked
            .retain(|parked, _| *parked > self.items.len() as u32);
        self.settle();
        self.revision = next_view_revision();
    }

    /// Works out what is still to come from what is held, parked and in
    /// flight.
    fn settle(&mut self) {
        let held = self.items.len() as u32;
        let page = self.page_size.max(1);
        self.next_offset = match self.total {
            Some(total) if held < total => {
                let mut offset = held;
                while self.pending.contains(&offset) || self.parked.contains_key(&offset) {
                    offset += page;
                }
                (offset < total).then_some(offset)
            }
            Some(_) => None,
            None => Some(held),
        };
        self.loading = !self.pending.is_empty();
    }

    pub fn retain<F>(&mut self, f: F)
    where
        F: FnMut(&T) -> bool,
    {
        self.items.retain(f);
        self.revision = next_view_revision();
    }

    pub fn reorder(&mut self, from: usize, to: usize) {
        if from < self.items.len() && to <= self.items.len() {
            let item = self.items.remove(from);
            let insert_at = if to > from { to - 1 } else { to };
            self.items.insert(insert_at.min(self.items.len()), item);
            self.revision = next_view_revision();
        }
    }

    pub fn set_cached(&mut self, items: Vec<T>) {
        self.total = Some(items.len() as u32);
        self.items = items;
        self.next_offset = None;
        self.pending.clear();
        self.parked.clear();
        self.loading = false;
        self.loaded_once = true;
        self.error = None;
        self.revision = next_view_revision();
    }

    /// One page failed: the others in flight keep going, and the failed
    /// offset can be asked for again.
    pub fn fail_page(&mut self, offset: u32, error: String) {
        self.pending.remove(&offset);
        self.error = Some(error);
        self.loaded_once = true;
        self.settle();
    }

    pub fn fail(&mut self, error: String) {
        self.pending.clear();
        self.loading = false;
        self.error = Some(error);
        self.loaded_once = true;
    }
}

type Page_<T> = crate::api::models::Page<T>;

/// A cursor-paginated list (followed artists).
#[derive(Clone, Debug)]
pub struct CursorList<T> {
    pub items: Vec<T>,
    pub after: Option<String>,
    pub loading: bool,
    pub error: Option<String>,
    pub loaded_once: bool,
    pub complete: bool,
}

impl<T> Default for CursorList<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            after: None,
            loading: false,
            error: None,
            loaded_once: false,
            complete: false,
        }
    }
}

impl<T> CursorList<T> {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn can_load_more(&self) -> bool {
        !self.loading && !self.complete
    }
}

/// The rows a table draws, built once per change of the list behind them
/// rather than on every frame.
pub type TableItem = (PlayableItem, Option<String>, Option<String>);

#[derive(Clone)]
pub struct TableRows {
    pub items_revision: u64,
    pub names_revision: u64,
    pub rows: std::sync::Arc<Vec<TableItem>>,
}

#[derive(Default)]
pub struct Library {
    pub playlists: Loadable<Vec<Playlist>>,
    /// The pages of the playlist list as they come in; `playlists` is
    /// published from it.
    pub playlist_fetch: PagedList<Playlist>,
    /// `playlists` came from disk and the live list is still on its way.
    pub playlists_cached: bool,
    pub liked: PagedList<SavedTrack>,
    /// `liked` came from disk and the live first page has yet to confirm it.
    pub liked_cached: bool,
    pub albums: PagedList<SavedAlbum>,
    pub artists: CursorList<Artist>,
    pub shows: PagedList<SavedShow>,
    pub episodes: PagedList<SavedEpisode>,
    pub filter: String,
}

#[derive(Default)]
pub struct HomeData {
    pub recently_played: Loadable<Vec<PlayHistory>>,
    pub top_artists: Loadable<Vec<Artist>>,
    /// The 20-track preview shown on Home.
    pub top_tracks: Loadable<Vec<Track>>,
    /// The separately loaded, complete ranking shown by the Top Songs page.
    pub top_songs: Loadable<Vec<Track>>,
    pub top_songs_loading: bool,
    pub top_songs_complete: bool,
    pub recommendations: Loadable<Vec<Track>>,
    pub discover: HashMap<String, Loadable<Vec<Playlist>>>,
    pub discover_pending: HashMap<String, Loadable<Vec<Playlist>>>,
    pub generation: u64,
    pub top_songs_generation: u64,
    pub top_songs_revision: u64,
    pub requested: bool,
    pub loaded_at: Option<Instant>,
    /// Which of Home's shelves are showing.
    pub filter: HomeFilter,
}

pub const DISCOVER_TERMS: &[&str] = &["Discover Weekly", "Release Radar", "Daily Mix", "daylist"];

/// The chips above Home: everything, music only, or podcasts only.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HomeFilter {
    #[default]
    All,
    Music,
    Podcasts,
}

impl HomeFilter {
    pub const ALL: [HomeFilter; 3] = [Self::All, Self::Music, Self::Podcasts];

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Music => "Music",
            Self::Podcasts => "Podcasts",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SearchFilter {
    #[default]
    All,
    Songs,
    Artists,
    Albums,
    Playlists,
    Podcasts,
    Episodes,
}

impl SearchFilter {
    pub const ALL: [SearchFilter; 7] = [
        Self::All,
        Self::Songs,
        Self::Artists,
        Self::Albums,
        Self::Playlists,
        Self::Podcasts,
        Self::Episodes,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Songs => "Songs",
            Self::Artists => "Artists",
            Self::Albums => "Albums",
            Self::Playlists => "Playlists",
            Self::Podcasts => "Podcasts",
            Self::Episodes => "Episodes",
        }
    }
}

#[derive(Default)]
pub struct SearchState {
    pub query: String,
    pub committed: String,
    pub serial: u64,
    pub results: Loadable<SearchResults>,
    /// The search the loaded results answer, so a half of a newer search
    /// replaces them rather than merging into another query's answers.
    pub results_serial: u64,
    /// The catalogue half (songs, artists, albums, podcasts, episodes) of
    /// the current search is still on its way.
    pub catalogue_pending: bool,
    /// The playlist half of the current search is still on its way.
    pub playlists_pending: bool,
    pub filter: SearchFilter,
    pub typed_at: Option<Instant>,
    pub focus_requested: bool,
}

impl SearchState {
    /// Something of the current search is still on its way.
    pub fn pending(&self) -> bool {
        self.catalogue_pending || self.playlists_pending
    }
}

#[derive(Default)]
pub struct PlaylistPage {
    pub generation: u64,
    pub playlist: Loadable<Playlist>,
    pub items: PagedList<PlaylistItem>,
    pub filter: String,
    /// Ids of everyone who added songs, from the pages seen so far and one
    /// look at the tail.
    pub contributors: std::collections::BTreeSet<String>,
    /// Whether the tail was sampled for who added its songs.
    pub tail_checked: bool,
    /// The whole list came from disk and matches the live snapshot.
    pub cache_complete: bool,
    /// Items read from disk, waiting for the live snapshot to confirm.
    pub pending_cache: Option<(String, Vec<PlaylistItem>)>,
    /// The rows on show came from disk under `cache_snapshot` and the live
    /// playlist has yet to say whether they are still true.
    pub cache_provisional: bool,
    pub cache_snapshot: Option<String>,
    /// Live pages that landed while the disk rows were provisional, kept
    /// in case the snapshot turns out to have moved on.
    pub held_live: Vec<(u32, Page_<PlaylistItem>)>,
    pub table_rows: Option<TableRows>,
}

#[derive(Default)]
pub struct AlbumPage {
    pub album: Loadable<Album>,
    pub tracks: PagedList<Track>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DiscographyFilter {
    #[default]
    All,
    Albums,
    Singles,
    AppearsOn,
}

impl DiscographyFilter {
    pub const ALL: [DiscographyFilter; 4] =
        [Self::All, Self::Albums, Self::Singles, Self::AppearsOn];

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Albums => "Albums",
            Self::Singles => "Singles & EPs",
            Self::AppearsOn => "Appears On",
        }
    }

    pub fn groups(self) -> &'static str {
        match self {
            Self::All => "album,single,compilation",
            Self::Albums => "album",
            Self::Singles => "single",
            Self::AppearsOn => "appears_on",
        }
    }
}

#[derive(Default)]
pub struct ArtistPage {
    pub artist: Loadable<Artist>,
    pub top_tracks: Loadable<Vec<Track>>,
    pub albums: HashMap<String, PagedList<Album>>,
    pub related: Loadable<Vec<Artist>>,
    pub filter: DiscographyFilter,
    pub show_all_top: bool,
}

#[derive(Default)]
pub struct ShowPage {
    pub show: Loadable<Show>,
    pub episodes: PagedList<Episode>,
}

/// A table's sort, chosen by clicking a column heading.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TableSort {
    pub column: SortColumn,
    pub ascending: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SortColumn {
    Title,
    Album,
    Added,
    Duration,
    AddedBy,
    /// The list's own order, for playing it reversed from the # heading.
    Index,
}

/// One of the things a track row can be part of, for playback context and
/// for the actions the row offers.
#[derive(Clone, Debug, PartialEq)]
pub enum RowContext {
    /// A Spotify context (playlist, album) that can be played from an offset.
    Context {
        uri: String,
        /// The playlist id when the user owns it, enabling removal.
        editable_playlist: Option<(String, Option<String>)>,
    },
    /// A loose list of tracks, played as a queue of URIs.
    Uris(Vec<String>),
    /// A sorted or filtered view of a context: plays exactly the list on
    /// screen, while the context stays what the interface calls playing.
    View {
        uris: Vec<String>,
        context_uri: String,
        /// Removal is by URI and remains safe when display order changes.
        /// Reordering still requires a Context with actual server positions.
        editable_playlist: Option<(String, Option<String>)>,
    },
}

/// The track in hand while a row is dragged, until a sidebar row takes it.
#[derive(Clone, Debug)]
pub struct DragTrack {
    pub uri: String,
    pub title: String,
    /// Small cover art for the chip that rides the pointer.
    pub image: Option<String>,
    /// Where the drag began when it began on an editable playlist: that
    /// playlist's id and the row's real index, so the same table can move
    /// the row instead of copying it. The sidebar ignores this.
    pub from: Option<(String, u32)>,
}

/// A sidebar row in hand while it is dragged to a new place in the
/// pinned block.
#[derive(Clone, Debug)]
pub struct DragEntry {
    pub uri: String,
    pub title: String,
    pub image: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Dialog {
    CreatePlaylist {
        name: String,
        public: bool,
        add_uris: Vec<String>,
    },
    EditPlaylist {
        id: String,
        name: String,
        description: String,
        public: bool,
    },
    ConfirmDeletePlaylist {
        id: String,
        name: String,
        owned: bool,
    },
    ConfirmAddToPlaylist {
        playlist_id: String,
        playlist_name: String,
        uris: Vec<String>,
        title: String,
        position: Option<u32>,
    },
    Shortcuts,
    /// The Jump palette: find anything already loaded, offline.
    Jump,
}

/// What kind of thing a Jump entry is, in the order ties are broken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JumpKind {
    Song,
    Liked,
    Playlist,
    Album,
    Artist,
    Podcast,
    Command,
}

impl JumpKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Song => "Song",
            Self::Liked | Self::Playlist => "Playlist",
            Self::Album => "Album",
            Self::Artist => "Artist",
            Self::Podcast => "Podcast",
            Self::Command => "Command",
        }
    }

    pub fn icon(self) -> crate::theme::Icon {
        use crate::theme::Icon;
        match self {
            Self::Song => Icon::Music,
            Self::Liked | Self::Playlist => Icon::ListMusic,
            Self::Album => Icon::Disc,
            Self::Artist => Icon::User,
            Self::Podcast => Icon::Mic,
            Self::Command => Icon::Sparkles,
        }
    }

    pub fn rank(self) -> u8 {
        match self {
            Self::Song => 0,
            Self::Liked => 1,
            Self::Playlist => 2,
            Self::Album => 3,
            Self::Artist => 4,
            Self::Podcast => 5,
            Self::Command => 6,
        }
    }
}

/// One thing the Jump palette can open or play.
#[derive(Clone, Debug)]
pub struct JumpEntry {
    /// What makes it unique, so a song in two playlists is listed once.
    pub key: String,
    pub kind: JumpKind,
    pub name: String,
    pub subtitle: String,
    pub image: Option<String>,
    pub icon: Option<crate::theme::Icon>,
    /// What Enter does: play a song, open anything else.
    pub primary: Action,
    /// What Ctrl+Enter does, for things that can be played whole.
    pub play: Option<Action>,
}

/// The Jump palette's state while it is open.
#[derive(Default)]
pub struct JumpState {
    pub query: String,
    pub selected: usize,
    pub focus_pending: bool,
    /// Everything that can be jumped to, built when the palette opens.
    pub entries: Option<Vec<JumpEntry>>,
    /// The matches for the query they were ranked for.
    pub ranked: Option<(String, Vec<usize>)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Error,
}

#[derive(Clone, Debug)]
pub struct Toast {
    pub message: String,
    pub kind: ToastKind,
    pub created: Instant,
}

/// What views ask the app to do. Collected while drawing, applied after, so
/// a view can iterate over app data without fighting the borrow checker.
#[derive(Clone, Debug)]
pub enum Action {
    Open(Page),
    OpenUri(String),
    Back,
    Forward,
    PlayContext {
        uri: String,
        offset_uri: Option<String>,
        offset_index: Option<u32>,
    },
    /// Play an episode from the saved place shown by the requesting row.
    PlayEpisode {
        uri: String,
        resume_ms: Option<u32>,
    },
    PlayUris {
        uris: Vec<String>,
        index: u32,
    },
    PlayFromRow {
        context: RowContext,
        uri: String,
        index: u32,
        /// An episode's saved place, without discarding its playlist context.
        resume_ms: Option<u32>,
    },
    ShufflePlay(String),
    SelectLocalPlayback,
    TogglePlay,
    Next,
    Previous,
    Seek(u32),
    SeekBy(i64),
    SetVolume(u8),
    /// The slider mid-drag: heard at once, told to Spotify on release.
    PreviewVolume(u8),
    VolumeBy(i8),
    ToggleMute,
    ToggleShuffle,
    CycleRepeat,
    SetShuffle(bool),
    SetRepeat(crate::player::RepeatMode),
    AddToQueue {
        uri: String,
        label: String,
    },
    ToggleSaved(String),
    AddToPlaylist {
        playlist_id: String,
        playlist_name: String,
        uris: Vec<String>,
        position: Option<u32>,
        confirmed: bool,
    },
    RemoveFromPlaylist {
        playlist_id: String,
        uris: Vec<String>,
    },
    MoveInPlaylist {
        playlist_id: String,
        from: u32,
        to: u32,
    },
    ShowDialog(Dialog),
    CloseDialog,
    CreatePlaylist {
        name: String,
        public: bool,
        add_uris: Vec<String>,
    },
    UpdatePlaylist {
        id: String,
        name: String,
        description: String,
        public: bool,
    },
    DeletePlaylist(String),
    Transfer(String),
    /// Hand the account to a receiver found on the local network.
    ActivateReceiver(Box<crate::zeroconf::Receiver>),
    RefreshDevices,
    RefreshQueue,
    CopyLink(String),
    CopySignInLink(String),
    /// A web page, in the browser.
    OpenUrl(String),
    OpenInSpotify(String),
    Search(String),
    SetSearchFilter(SearchFilter),
    FocusSearch,
    LoadMore(Page),
    LoadMoreArtistAlbums(String),
    SetDiscographyFilter {
        artist_id: String,
        filter: DiscographyFilter,
    },
    ToggleShowAllTop(String),
    Reload(Page),
    SignIn,
    CancelSignIn,
    SignOut,
    /// Add, replace, or remove the optional personal Web API app.
    ConfigurePersonalWebApp,
    ToggleSidebar,
    /// Fold Your Library down to a rail of covers, or open it out again.
    ToggleSidebarCollapsed,
    ToggleQueuePanel,
    ToggleLyricsPanel,
    /// Open or close the Now Playing view beside the page.
    ToggleNowPlayingPanel,
    /// Which of Home's shelves to show.
    SetHomeFilter(HomeFilter),
    ToggleDevicesPopup,
    CheckForUpdates,
    SettingsChanged,
    RestartEngine,
    EnablePlayback,
    ShowWindow,
    HideWindow,
    ClearArtCache,
    /// Open or close the Winamp window.
    ToggleWinampWindow,
    /// Wear a skin from the skins folder, or the built-in one for `None`.
    SetSkin(Option<String>),
    /// Copy a skin file into the skins folder and wear it.
    InstallSkin(std::path::PathBuf),
    /// Screen pixels per skin pixel in the Winamp window.
    SetSkinScale(u8),
    ToggleWinampOnTop,
    OpenSkinsFolder,
    /// Bars, then the scope, then nothing, in the mini player's display.
    CycleVisualiser,
    /// Open or close the playlist window under the mini player.
    ToggleWinampPlaylist,
    /// The playlist window's height, in skin pixels.
    SetPlaylistHeight(u32),
    /// Open or close the equalizer window under the mini player.
    ToggleWinampEq,
    /// Switch the equalizer's effect on the sound on or off.
    ToggleEq,
    SetEqBand(usize, f32),
    /// Lay every equalizer band flat.
    FlattenEq,
    SetEqPreamp(f32),
    /// One of Winamp's presets, by its place in the list.
    ApplyEqPreset(usize),
    /// The balance, -1 all left to 1 all right.
    SetBalance(f32),
    ToggleMono,
    /// Roll the playlist window up to its title bar, or down again.
    ToggleWinampPlaylistShade,
    /// Roll the main window up to its title bar, or down again.
    ToggleWinampShade,
    Quit,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(offset: u32, total: u32, items: Vec<u32>) -> Page_<u32> {
        let limit = 50;
        let next = (offset + limit < total).then(|| "next".to_string());
        Page_ {
            items,
            total,
            limit,
            offset,
            next,
        }
    }

    /// Several pages go out together; a page that lands before the one
    /// ahead of it waits, so the rows on show are always a prefix of the
    /// list, and the fill stops at the eager bound until a scroll asks on.
    #[test]
    fn pages_in_flight_together_land_in_order() {
        let mut list: PagedList<u32> = PagedList::default();
        assert_eq!(list.fill_ahead(4, EAGER_ITEMS), vec![0]);
        assert!(list.loading);
        assert_eq!(
            list.fill_ahead(4, EAGER_ITEMS),
            Vec::<u32>::new(),
            "the first page tells the size"
        );
        list.absorb(0, page(0, 230, (0..50).collect()));
        assert_eq!(list.fill_ahead(4, EAGER_ITEMS), vec![50, 100, 150, 200]);
        assert_eq!(list.pending.len(), 4);
        // The third page arrives first and waits out of sight.
        list.absorb(150, page(150, 230, (150..200).collect()));
        assert_eq!(list.items.len(), 50);
        assert!(list.loading);
        list.absorb(50, page(50, 230, (50..100).collect()));
        assert_eq!(list.items.len(), 100);
        list.absorb(100, page(100, 230, (100..150).collect()));
        assert_eq!(
            list.items.len(),
            200,
            "the parked page followed its predecessor in"
        );
        assert_eq!(list.next_offset, None, "the last page is still in flight");
        assert!(!list.is_complete());
        list.absorb(200, page(200, 230, (200..230).collect()));
        assert!(list.is_complete());
        assert_eq!(list.items, (0..230).collect::<Vec<_>>());
        assert!(!list.loading);
    }

    #[test]
    fn the_eager_fill_stops_at_its_bound_and_a_scroll_continues() {
        let mut list: PagedList<u32> = PagedList::default();
        list.begin(0);
        list.absorb(0, page(0, 10_000, (0..50).collect()));
        let offsets = list.fill_ahead(4, 100);
        assert_eq!(offsets, vec![50]);
        list.absorb(50, page(50, 10_000, (50..100).collect()));
        assert!(
            list.fill_ahead(4, 100).is_empty(),
            "eager loading ends at the bound"
        );
        assert_eq!(list.next_offset, Some(100));
        assert!(list.can_load_more());
        assert_eq!(list.fill_ahead(4, u32::MAX), vec![100, 150, 200, 250]);
    }

    #[test]
    fn a_failed_page_leaves_the_others_in_flight_and_can_be_asked_again() {
        let mut list: PagedList<u32> = PagedList::default();
        list.begin(0);
        list.absorb(0, page(0, 150, (0..50).collect()));
        assert_eq!(list.fill_ahead(4, u32::MAX), vec![50, 100]);
        list.fail_page(50, "rate limited".into());
        assert!(list.loading, "the other page is still coming");
        assert_eq!(list.next_offset, Some(50));
        list.absorb(100, page(100, 150, (100..150).collect()));
        assert_eq!(list.items.len(), 50);
        assert_eq!(list.fill_ahead(4, u32::MAX), vec![50]);
        list.absorb(50, page(50, 150, (50..100).collect()));
        assert!(list.is_complete());
        assert_eq!(list.items.len(), 150);
    }
}
