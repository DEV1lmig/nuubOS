/*
 * Media application of nuubUI (EPIC-027 Jellyfin, EPIC-028 local video and
 * music), redesigned 2026-10-12 on user request as a full media center with
 * its own look (components/media_app.slint, settings-view 65):
 *
 *   Home        shelves: Continue Watching, Videos, Recordings, Music,
 *               Playlists, one per Jellyfin library, Browse (storage,
 *               network shares, the Jellyfin account)
 *   Grid        a video folder, a Jellyfin library or folder, a browsed
 *               folder
 *   Album       an album folder, a playlist or a Jellyfin album
 *   Now Playing music: progress, controls, Music volume, queue, Screen Off
 *
 * Services: nuubos-mediad owns playback (video fullscreen in mpv; music
 * without a window, its queue, shuffle/repeat and Music volume) and keeps
 * playing when the app is left, also behind games (Quick Menu MUSIC
 * section). nuubos-medialib reads the folders (sources, videos, albums,
 * tracks, covers, playlists, resume positions); nuubos-jellyfinctl owns the
 * server account; nuubos-sharesctl the network shares. No policy here.
 */

use crate::gameui::{run_tool, tool_then};
use crate::{handle_settings_action, navigate_settings_view, on_game_session, open_settings_choice,
    open_system_keyboard, play_ui_sound, tr, tr_arg, write_ui_context, AppHint, HomeWindow, MediaShelf, MediaTile,
    MediaTrack};
use slint::{ComponentHandle, Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, Timer, TimerMode, VecModel};
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

pub const KEYBOARD_SERVER: i32 = 22;
pub const KEYBOARD_JF_USER: i32 = 23;
pub const KEYBOARD_JF_PASSWORD: i32 = 24;
pub const KEYBOARD_PLAYLIST: i32 = 31;
pub const KEYBOARD_PLAYLIST_RENAME: i32 = 32;

pub const VIEW: i32 = 65;

const MEDIA_SOCKET: &str = "/run/nuubos/mediad.sock";
const MEDIALIB: &str = "/usr/bin/nuubos-medialib";
const FILESCTL: &str = "/usr/bin/nuubos-filesctl";
const SHARESCTL: &str = "/usr/sbin/nuubos-sharesctl";
const JELLYFIN: &str = "/usr/bin/nuubos-jellyfinctl";
const DISPLAYCTL: &str = "/usr/bin/nuubos-displayctl";

/* The Media app owns Home input. */
pub static ACTIVE: AtomicBool = AtomicBool::new(false);
/* A session exists (video or music). */
pub static PLAYING: AtomicBool = AtomicBool::new(false);
/* A video plays: the player window covers nuubUI and gets the controller. */
pub static PLAYING_VIDEO: AtomicBool = AtomicBool::new(false);
/* Music plays or is paused (behind nuubUI or a game). */
pub static MUSIC: AtomicBool = AtomicBool::new(false);
/* Screen Off: the next button only turns the screen back on. */
pub static SCREEN_OFF: AtomicBool = AtomicBool::new(false);
/* Jellyfin item being played, for the stop position report. */
static JELLYFIN_ITEM: Mutex<String> = Mutex::new(String::new());

const VIDEO_EXTS: &[&str] = &["mkv", "mp4", "m4v", "avi", "webm", "mov", "ts", "mpg", "mpeg"];
const AUDIO_EXTS: &[&str] = &["mp3", "flac", "ogg", "opus", "m4a", "aac", "wav", "alac"];

/* Tile kinds (media_app.slint). */
const K_VIDEO: i32 = 0;
const K_FOLDER: i32 = 1;
const K_ALBUM: i32 = 2;
const K_LIBRARY: i32 = 3;
const K_ACTION: i32 = 4;
const K_SONG: i32 = 5;
const K_STORAGE: i32 = 6;
const K_INFO: i32 = 7;
const K_SHUFFLE: i32 = 9;

/* Home sections (user request 2026-10-13), switched with L2/R2. */
const MUSIC_SECTION: usize = 0;
const VIDEO_SECTION: usize = 1;
/* The app's options (user request 2026-10-14: a third tab). */
const SETTINGS_SECTION: usize = 2;
const SECTIONS: usize = 3;

/* Shelf / grid shapes. */
const POSTER: i32 = 0;
const SQUARE: i32 = 1;
const WIDE: i32 = 2;

/* Now Playing zones. */
const Z_PROGRESS: i32 = 0;
const Z_CONTROLS: i32 = 1;
/* 2 was the Music volume (removed 2026-10-14: the Quick Menu MUSIC
 * section has it). */
const Z_QUEUE: i32 = 3;

/* ---------------------------------------------------------------- */
/* Player snapshot (nuubos-mediad)                                   */
/* ---------------------------------------------------------------- */

#[derive(Clone, Default)]
struct Snapshot {
    state: String,
    kind: String,
    title: String,
    artist: String,
    album: String,
    item: String,
    index: i64,
    count: i64,
    position: f64,
    duration: f64,
    shuffle: bool,
    repeat: String,
    volume: i32,
    codec: String,
    rate: i64,
    bits: i32,
    queue_id: i64,
}

impl Snapshot {
    fn active(&self) -> bool {
        matches!(self.state.as_str(), "playing" | "paused")
    }
    fn music(&self) -> bool {
        self.active() && self.kind == "audio"
    }
}

fn parse_snapshot(block: &str) -> Snapshot {
    let mut s = Snapshot { volume: 80, ..Default::default() };
    for line in block.lines() {
        let Some((k, v)) = line.split_once('=') else { continue };
        match k {
            "state" => s.state = v.into(),
            "kind" => s.kind = v.into(),
            "title" => s.title = v.into(),
            "artist" => s.artist = v.into(),
            "album" => s.album = v.into(),
            "item" => s.item = v.into(),
            "index" => s.index = v.parse().unwrap_or(0),
            "count" => s.count = v.parse().unwrap_or(0),
            "position" => s.position = v.parse().unwrap_or(0.0),
            "duration" => s.duration = v.parse().unwrap_or(0.0),
            "shuffle" => s.shuffle = v == "1",
            "repeat" => s.repeat = v.into(),
            "volume" => s.volume = v.parse().unwrap_or(80),
            "codec" => s.codec = v.into(),
            "rate" => s.rate = v.parse().unwrap_or(0),
            "bits" => s.bits = v.parse().unwrap_or(0),
            "queue_id" => s.queue_id = v.parse().unwrap_or(0),
            _ => {}
        }
    }
    s
}

fn player(line: &str) -> String {
    let Ok(mut stream) = UnixStream::connect(MEDIA_SOCKET) else { return String::new() };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
    let _ = stream.write_all(line.as_bytes());
    let _ = stream.write_all(b"\n");
    let mut reply = String::new();
    let _ = BufReader::new(stream).read_line(&mut reply);
    reply
}

fn send(line: String) {
    thread::spawn(move || {
        let _ = player(&line);
    });
}

/* A queue for nuubos-mediad: PLAYLIST then one line per track. */
fn play_list(ui: &HomeWindow, items: Vec<(String, String)>, start: usize, shuffle: bool) {
    if items.is_empty() {
        return;
    }
    let weak = ui.as_weak();
    thread::spawn(move || {
        let mut body = format!("PLAYLIST\t{}\t{}\t{}\n", start, items.len(), if shuffle { 1 } else { 0 });
        for (path, title) in &items {
            body.push_str(&format!("{path}\t{title}\n"));
        }
        let ok = UnixStream::connect(MEDIA_SOCKET)
            .and_then(|mut s| {
                s.set_read_timeout(Some(Duration::from_secs(3)))?;
                s.write_all(body.as_bytes())?;
                let mut reply = String::new();
                BufReader::new(s).read_line(&mut reply)?;
                Ok(reply.starts_with("OK"))
            })
            .unwrap_or(false);
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = weak.upgrade() {
                if ok {
                    *JELLYFIN_ITEM.lock().unwrap() = String::new();
                    open_now_playing(&ui);
                } else {
                    notice(&ui, tr(&ui, 667, "The media could not be played"));
                }
            }
        });
    });
}

/* ---------------------------------------------------------------- */
/* Model                                                             */
/* ---------------------------------------------------------------- */

#[derive(Clone, Default)]
struct Tile {
    key: String,
    title: String,
    subtitle: String,
    /* Artwork: a local image path, or "jf:<id>" (fetched by jellyfinctl). */
    art: String,
    kind: i32,
    progress: f32,
    badge: String,
}

struct Shelf {
    id: &'static str,
    section: usize,
    /* Focused tile of this shelf. */
    focus: usize,
    title: String,
    tiles: Vec<Tile>,
    shape: i32,
    model: Rc<VecModel<MediaTile>>,
}

#[derive(Clone, Default)]
struct Track {
    /* Local path, or a Jellyfin item id for AlbumSource::Jellyfin. */
    path: String,
    title: String,
    detail: String,
    number: String,
}

#[derive(Clone, PartialEq)]
enum AlbumSource {
    Dir(String),
    Playlist(String),
    Jellyfin(String),
}

enum View {
    Grid { title: String, source: String, tiles: Vec<Tile>, index: usize, shape: i32, loading: bool,
        model: Rc<VecModel<MediaTile>> },
    Album { title: String, artist: String, meta: String, art: String, tracks: Vec<Track>, index: i32,
        source: AlbumSource, model: Rc<VecModel<MediaTrack>> },
}

enum Art {
    Loading,
    Ready(Image),
    Failed,
}

#[derive(Default)]
struct State {
    /* Every shelf of both sections; the home shows the current section's. */
    shelves: Vec<Shelf>,
    section: usize,
    /* Focused shelf, per section (position among that section's shelves). */
    shelf_index: [usize; SECTIONS],
    shelves_model: Option<Rc<VecModel<MediaShelf>>>,
    indices_model: Option<Rc<VecModel<i32>>>,
    views: Vec<View>,
    np_open: bool,
    np_zone: i32,
    np_control: i32,
    np_queue_index: usize,
    queue: Vec<(String, String)>,
    queue_id: i64,
    queue_model: Option<Rc<VecModel<MediaTrack>>>,
    snap: Snapshot,
    snap_at: Option<Instant>,
    np_cover_item: String,
    np_cover: String,
    art: HashMap<String, Art>,
    backdrops: HashMap<String, Image>,
    jf_signed: bool,
    jf_name: String,
    draft: (String, String),
    /* Tracks waiting for "Add to Playlist" → New Playlist. */
    pending_add: Vec<String>,
    /* Playlist file being renamed. */
    renaming: String,
    /* Play the album being opened (shuffled or not) once it is loaded. */
    autoplay: Option<bool>,
    loading: bool,
    notice_timer: Option<Timer>,
    tick: Option<Timer>,
    generation: u64,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

static ART_QUEUE: OnceLock<Mutex<mpsc::Sender<(String, String, u32)>>> = OnceLock::new();

fn ext_of(name: &str) -> String {
    name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default()
}

fn is_video(name: &str) -> bool {
    VIDEO_EXTS.contains(&ext_of(name).as_str())
}

fn is_audio(name: &str) -> bool {
    AUDIO_EXTS.contains(&ext_of(name).as_str())
}

fn base_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn duration(seconds: f64) -> String {
    let s = seconds.max(0.0) as i64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/* A stable colour per title for tiles without artwork. */
fn tint(title: &str) -> slint::Color {
    const PALETTE: [(u8, u8, u8); 8] = [(0x3a, 0x86, 0xff), (0x8e, 0x5c, 0xe6), (0xe0, 0x5a, 0x7a), (0xf0, 0x8c, 0x3c),
        (0x2e, 0xb8, 0x8a), (0x29, 0xa3, 0xc6), (0xd4, 0xa2, 0x2e), (0x6c, 0x7a, 0x99)];
    let h = title.bytes().fold(2166136261u32, |h, b| (h ^ b as u32).wrapping_mul(16777619));
    let (r, g, b) = PALETTE[(h % PALETTE.len() as u32) as usize];
    slint::Color::from_rgb_u8(r, g, b)
}

fn parse_lines<'a>(out: &'a str, key: &str) -> Vec<Vec<&'a str>> {
    out.lines()
        .filter_map(|l| l.strip_prefix(key).and_then(|r| r.strip_prefix('=')))
        .map(|v| v.split('\t').collect())
        .collect()
}

fn media_tile(st: &State, t: &Tile) -> MediaTile {
    let (art, has) = match st.art.get(&t.art) {
        Some(Art::Ready(img)) => (img.clone(), true),
        _ => (Image::default(), false),
    };
    MediaTile {
        title: t.title.clone().into(),
        subtitle: t.subtitle.clone().into(),
        art,
        has_art: has,
        kind: t.kind,
        progress: t.progress,
        badge: t.badge.clone().into(),
        tint: tint(&t.title),
    }
}

fn model<T: Clone + 'static>(v: Vec<T>) -> Rc<VecModel<T>> {
    Rc::new(VecModel::from(v))
}

fn hint(kind: &str, role: i32, label: String) -> AppHint {
    AppHint { kind: kind.into(), role, label: label.into() }
}

/* ---------------------------------------------------------------- */
/* Artwork                                                           */
/* ---------------------------------------------------------------- */

/* Decoded at the tile height in physical pixels, bucketed. */
fn art_target(ui: &HomeWindow) -> u32 {
    let h = ui.window().size().height.max(480) as f32;
    ((h * 0.42) as u32).div_ceil(32) * 32
}

pub fn start_art_loader(ui: &HomeWindow) {
    let (tx, rx) = mpsc::channel::<(String, String, u32)>();
    let _ = ART_QUEUE.set(Mutex::new(tx));
    let weak = ui.as_weak();
    thread::spawn(move || {
        for (key, user, target) in rx {
            let path = match key.strip_prefix("jf:") {
                Some(id) => run_tool(JELLYFIN, &["image", &user, id], None)
                    .lines()
                    .find_map(|l| l.strip_prefix("image="))
                    .unwrap_or("")
                    .to_owned(),
                None => key.clone(),
            };
            let img = if path.is_empty() { None } else { Image::load_from_path(Path::new(&path)).ok() };
            let shaped = img.as_ref().and_then(|i| crate::library::shape_cover(i, target, 0.0));
            let weak = weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = weak.upgrade() else { return };
                let art = match shaped {
                    Some((buf, _)) => Art::Ready(Image::from_rgba8_premultiplied(buf)),
                    None => Art::Failed,
                };
                STATE.with(|st| st.borrow_mut().art.insert(key.clone(), art));
                art_arrived(&ui, &key);
            });
        }
    });
}

fn request_art(ui: &HomeWindow, key: &str) {
    if key.is_empty() {
        return;
    }
    let new = STATE.with(|st| {
        let mut st = st.borrow_mut();
        if st.art.contains_key(key) {
            return false;
        }
        /* A small cache: artwork far from the focus is decoded again. */
        if st.art.len() > 400 {
            st.art.retain(|_, a| matches!(a, Art::Loading));
            st.backdrops.clear();
        }
        st.art.insert(key.to_owned(), Art::Loading);
        true
    });
    if new {
        if let Some(q) = ART_QUEUE.get() {
            let _ = q.lock().unwrap().send((key.to_owned(), ui.get_active_user_id().to_string(), art_target(ui)));
        }
    }
}

/* Updates only the rows showing this artwork. */
fn art_arrived(ui: &HomeWindow, key: &str) {
    STATE.with(|st| {
        let st = st.borrow();
        for shelf in &st.shelves {
            for (j, t) in shelf.tiles.iter().enumerate() {
                if t.art == key {
                    shelf.model.set_row_data(j, media_tile(&st, t));
                }
            }
        }
        for v in &st.views {
            if let View::Grid { tiles, model, .. } = v {
                for (j, t) in tiles.iter().enumerate() {
                    if t.art == key {
                        model.set_row_data(j, media_tile(&st, t));
                    }
                }
            }
        }
    });
    refresh_header(ui);
    refresh_np(ui);
}

/* The focused artwork as a backdrop: a tiny copy stretched by Slint (a
 * cheap blur), computed once per artwork. */
fn backdrop(key: &str) -> Option<Image> {
    STATE.with(|st| {
        let mut st = st.borrow_mut();
        if let Some(img) = st.backdrops.get(key) {
            return Some(img.clone());
        }
        let Some(Art::Ready(img)) = st.art.get(key) else { return None };
        let b = img.to_rgba8_premultiplied()?;
        let (w, h) = (b.width(), b.height());
        if w == 0 || h == 0 {
            return None;
        }
        let (ow, oh) = (24u32, ((24 * h) / w).max(1));
        let mut out = SharedPixelBuffer::<Rgba8Pixel>::new(ow, oh);
        let px = b.as_slice();
        for (i, o) in out.make_mut_slice().iter_mut().enumerate() {
            let (x, y) = (i as u32 % ow, i as u32 / ow);
            let mut acc = [0u32; 4];
            let mut n = 0u32;
            for sy in (y * h / oh)..((y + 1) * h / oh).max(y * h / oh + 1).min(h) {
                for sx in (x * w / ow)..((x + 1) * w / ow).max(x * w / ow + 1).min(w) {
                    let p = px[(sy * w + sx) as usize];
                    acc[0] += p.r as u32;
                    acc[1] += p.g as u32;
                    acc[2] += p.b as u32;
                    acc[3] += p.a as u32;
                    n += 1;
                }
            }
            let n = n.max(1);
            *o = Rgba8Pixel { r: (acc[0] / n) as u8, g: (acc[1] / n) as u8, b: (acc[2] / n) as u8, a: (acc[3] / n) as u8 };
        }
        let img = Image::from_rgba8_premultiplied(out);
        st.backdrops.insert(key.to_owned(), img.clone());
        Some(img)
    })
}

/* ---------------------------------------------------------------- */
/* Rendering                                                         */
/* ---------------------------------------------------------------- */

/* Indices (in st.shelves) of the current section's shelves, in order. */
fn visible(st: &State) -> Vec<usize> {
    st.shelves.iter().enumerate().filter(|(_, s)| s.section == st.section).map(|(i, _)| i).collect()
}

fn current_shelf(st: &State) -> Option<usize> {
    visible(st).get(st.shelf_index[st.section]).copied()
}

fn focused_tile(st: &State) -> Option<Tile> {
    match st.views.last() {
        Some(View::Grid { tiles, index, .. }) => tiles.get(*index).cloned(),
        Some(View::Album { .. }) => None,
        None => current_shelf(st).and_then(|i| st.shelves[i].tiles.get(st.shelves[i].focus).cloned()),
    }
}

/* Hero line, backdrop and artwork requests around the focus. */
fn refresh_header(ui: &HomeWindow) {
    let (tile, wanted) = STATE.with(|st| {
        let st = st.borrow();
        let mut wanted = Vec::new();
        match st.views.last() {
            Some(View::Grid { tiles, index, .. }) => {
                let lo = index.saturating_sub(18);
                wanted.extend(tiles.iter().skip(lo).take(40).map(|t| t.art.clone()));
            }
            Some(View::Album { art, .. }) => wanted.push(art.clone()),
            None => {
                let at = st.shelf_index[st.section];
                for (p, i) in visible(&st).into_iter().enumerate() {
                    if p + 1 < at || p > at + 3 {
                        continue;
                    }
                    let shelf = &st.shelves[i];
                    wanted.extend(shelf.tiles.iter().skip(shelf.focus.saturating_sub(2)).take(12).map(|t| t.art.clone()));
                }
            }
        }
        (focused_tile(&st), wanted)
    });
    for key in wanted {
        request_art(ui, &key);
    }
    match &tile {
        Some(t) => {
            ui.set_media_hero_title(t.title.clone().into());
            ui.set_media_hero_subtitle(t.subtitle.clone().into());
            match backdrop(&t.art) {
                Some(img) => {
                    ui.set_media_backdrop(img);
                    ui.set_media_has_backdrop(true);
                }
                None => ui.set_media_has_backdrop(false),
            }
        }
        None => {
            let album_art = STATE.with(|st| match st.borrow().views.last() {
                Some(View::Album { art, .. }) => art.clone(),
                _ => String::new(),
            });
            match backdrop(&album_art) {
                Some(img) => {
                    ui.set_media_backdrop(img);
                    ui.set_media_has_backdrop(true);
                }
                None => ui.set_media_has_backdrop(false),
            }
            if let Some(Art::Ready(img)) = STATE.with(|st| match st.borrow().art.get(&album_art) {
                Some(Art::Ready(i)) => Some(Art::Ready(i.clone())),
                _ => None,
            }) {
                ui.set_media_album_art(img);
                ui.set_media_album_has_art(true);
            }
        }
    }
}

/* Rebuilds the shelf models (after loading). */
fn publish_shelves(ui: &HomeWindow) {
    STATE.with(|st| {
        let mut st = st.borrow_mut();
        let vis = visible(&st);
        let shelves: Vec<MediaShelf> = vis
            .iter()
            .map(|&i| &st.shelves[i])
            .map(|s| MediaShelf { title: s.title.clone().into(), tiles: ModelRc::from(s.model.clone()), shape: s.shape })
            .collect();
        let indices: Vec<i32> = vis.iter().map(|&i| st.shelves[i].focus as i32).collect();
        let sm = model(shelves);
        let im = model(indices);
        ui.set_media_shelves(ModelRc::from(sm.clone()));
        ui.set_media_shelf_indices(ModelRc::from(im.clone()));
        st.shelves_model = Some(sm);
        st.indices_model = Some(im);
    });
}

fn show_view(ui: &HomeWindow) {
    STATE.with(|st| {
        let st = st.borrow();
        if st.np_open {
            ui.set_media_screen(3);
            return;
        }
        match st.views.last() {
            None => {
                ui.set_media_screen(0);
                ui.set_media_section(st.section as i32);
                ui.set_media_shelf_index(st.shelf_index[st.section] as i32);
                ui.set_media_empty_text(if st.loading { tr(ui, 1156, "Loading…") } else {
                    tr(ui, 1121, "Put videos in the Videos folder and music in the Music folder.") }.into());
            }
            Some(View::Grid { title, index, shape, model, loading, .. }) => {
                ui.set_media_screen(1);
                ui.set_media_grid_title(title.to_uppercase().into());
                ui.set_media_grid_tiles(ModelRc::from(model.clone()));
                ui.set_media_grid_index(*index as i32);
                ui.set_media_grid_shape(*shape);
                ui.set_media_empty_text(if *loading { tr(ui, 1156, "Loading…") } else { tr(ui, 621, "Empty folder") }.into());
            }
            Some(View::Album { title, artist, meta, index, model, source, .. }) => {
                ui.set_media_screen(2);
                ui.set_media_album_title(title.clone().into());
                ui.set_media_album_artist(artist.clone().into());
                ui.set_media_album_meta(meta.clone().into());
                ui.set_media_album_tracks(ModelRc::from(model.clone()));
                ui.set_media_album_index(*index);
                ui.set_media_album_has_art(false);
                if model.row_count() == 0 && matches!(source, AlbumSource::Playlist(_)) {
                    ui.set_media_album_meta(tr(ui, 1126, "This playlist is empty.").into());
                }
            }
        }
    });
    refresh_header(ui);
    refresh_np(ui);
    hints(ui);
}

fn hints(ui: &HomeWindow) {
    let music = MUSIC.load(Ordering::SeqCst);
    let mut h = Vec::new();
    let (np, confirm, options) = STATE.with(|st| {
        let st = st.borrow();
        if st.np_open {
            return (true, String::new(), false);
        }
        if st.views.is_empty() && st.section == SETTINGS_SECTION {
            let label = if ui.get_lighting_supported() { tr(ui, 185, "Select") } else { String::new() };
            return (false, label, false);
        }
        match st.views.last() {
            Some(View::Album { index, source, .. }) => {
                (false, tr(ui, 1164, "Play"), *index >= 0 || matches!(source, AlbumSource::Playlist(_)))
            }
            _ => {
                let t = focused_tile(&st);
                let kind = t.as_ref().map(|t| t.kind).unwrap_or(K_INFO);
                let label = match kind {
                    K_VIDEO | K_SONG | K_SHUFFLE => tr(ui, 1164, "Play"),
                    K_INFO => String::new(),
                    _ => tr(ui, 212, "Open"),
                };
                let key = t.map(|t| t.key).unwrap_or_default();
                (false, label, kind == K_VIDEO || kind == K_ALBUM || key.starts_with("plist:")
                    || key.starts_with("share:") || key == "jfaccount")
            }
        }
    });
    if np {
        h.push(hint("face", 0, tr(ui, 185, "Select")));
        h.push(hint("face", 1, tr(ui, 186, "Back")));
        h.push(hint("face", 3, tr(ui, 1117, "Screen Off")));
    } else {
        if !confirm.is_empty() {
            h.push(hint("face", 0, confirm));
        }
        h.push(hint("face", 1, tr(ui, 186, "Back")));
        if options {
            h.push(hint("face", 3, tr(ui, 1131, "Options")));
        }
        if music {
            h.push(hint("face", 2, tr(ui, 369, "Now Playing")));
        }
    }
    ui.set_app_hints(ModelRc::from(model(h)));
}

pub(crate) fn render(ui: &HomeWindow) {
    if ACTIVE.load(Ordering::SeqCst) {
        show_view(ui);
    }
}

fn notice(ui: &HomeWindow, text: String) {
    ui.set_media_notice(text.into());
    let weak = ui.as_weak();
    let timer = Timer::default();
    timer.start(TimerMode::SingleShot, Duration::from_millis(3200), move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_media_notice("".into());
        }
    });
    STATE.with(|st| st.borrow_mut().notice_timer = Some(timer));
}

/* ---------------------------------------------------------------- */
/* Loading the home shelves                                          */
/* ---------------------------------------------------------------- */

struct Loaded {
    /* (id, section, title for Jellyfin libraries, tiles, shape) */
    shelves: Vec<(&'static str, usize, String, Vec<Tile>, i32)>,
    jf: Option<(bool, String)>,
}

fn video_tiles(out: &str, ui_strings: &HashMap<i32, String>) -> Vec<Tile> {
    let mut tiles = Vec::new();
    for f in parse_lines(out, "folder") {
        if f.len() >= 4 {
            tiles.push(Tile { key: format!("vdir:{}", f[0]), title: f[1].into(), art: f[2].into(), kind: K_FOLDER,
                progress: -1.0, badge: f[3].into(), subtitle: ui_strings[&1113].replace("{0}", f[3]), ..Default::default() });
        }
    }
    for f in parse_lines(out, "video") {
        if f.len() >= 5 {
            let resume: f64 = f[3].parse().unwrap_or(0.0);
            tiles.push(Tile { key: format!("video:{}", f[0]), title: f[1].into(), art: f[2].into(), kind: K_VIDEO,
                progress: -1.0,
                subtitle: if resume > 0.0 { ui_strings[&1111].replace("{0}", &duration(resume)) } else { String::new() },
                ..Default::default() });
        }
    }
    tiles
}

fn jellyfin_tiles(out: &str, ui_strings: &HashMap<i32, String>) -> Vec<Tile> {
    parse_lines(out, "item")
        .into_iter()
        .filter(|f| f.len() >= 6)
        .map(|f| {
            let (id, name, kind, folder) = (f[0], f[1], f[2], f[3] == "1");
            let resume: f64 = f[4].parse().unwrap_or(0.0);
            let length: f64 = f[5].parse().unwrap_or(0.0);
            let sub = f.get(6).copied().unwrap_or("");
            let number: i64 = f.get(7).and_then(|n| n.parse().ok()).unwrap_or(0);
            let (key, tkind) = match kind {
                "MusicAlbum" => (format!("jfalbum:{id}"), K_ALBUM),
                "Audio" => (format!("jfitem:{id}:0:audio"), K_SONG),
                "CollectionFolder" | "UserView" => (format!("jfdir:{id}"), K_LIBRARY),
                _ if folder => (format!("jfdir:{id}"), K_FOLDER),
                _ => (format!("jfitem:{id}:{resume}:video"), K_VIDEO),
            };
            let mut subtitle = sub.to_owned();
            if kind == "Episode" && number > 0 {
                subtitle = format!("{} • {}", sub, ui_strings[&1127].replace("{0}", &number.to_string()));
            }
            if length > 0.0 {
                subtitle = if subtitle.is_empty() { duration(length) } else { format!("{subtitle} • {}", duration(length)) };
            }
            Tile {
                key,
                title: name.into(),
                subtitle,
                art: format!("jf:{id}"),
                kind: tkind,
                progress: if resume > 0.0 && length > 0.0 { (resume / length) as f32 } else { -1.0 },
                badge: String::new(),
            }
        })
        .collect()
}

/* The strings the loader thread needs (it cannot read the UI). */
fn strings(ui: &HomeWindow) -> HashMap<i32, String> {
    [(1111, "Resume from {0}"), (1113, "{0} videos"), (1112, "{0} tracks"), (1119, "Songs"), (1127, "Episode {0}"),
     (1101, "Shuffle All"), (1102, "New Playlist"), (610, "Internal Storage"), (64, "SD2"), (1122, "Jellyfin"),
     (1123, "Sign in to your Jellyfin server"), (1124, "Signed in as {0}"), (1125, "Connect to open it"),
     (1128, "Network Location"), (1016, "USB Drive"), (622, "{0} free"), (1120, "Unknown Artist"),
     (1168, "Put your music in a Music folder"), (1169, "Put your videos in a Videos folder"),
     (1170, "No music yet"), (1171, "No videos yet")]
        .iter()
        .map(|(i, f)| (*i, tr(ui, *i as usize, f)))
        .collect()
}

fn human(bytes: i64) -> String {
    let b = bytes as f64;
    if b >= 1e9 { format!("{:.1} GB", b / 1e9) } else if b >= 1e6 { format!("{:.1} MB", b / 1e6) } else { format!("{:.0} KB", b / 1e3) }
}

fn load_home(ui: &HomeWindow) {
    let user = ui.get_active_user_id().to_string();
    let strings = strings(ui);
    let generation = STATE.with(|st| {
        let mut st = st.borrow_mut();
        st.generation += 1;
        st.loading = st.shelves.is_empty();
        st.generation
    });
    let weak = ui.as_weak();
    thread::spawn(move || {
        let s = &strings;
        let sources = run_tool(MEDIALIB, &["sources", &user], None);
        let mut cont = Vec::new();
        for f in parse_lines(&run_tool(MEDIALIB, &["resume", &user], None), "resume") {
            if f.len() >= 5 {
                let secs: f64 = f[1].parse().unwrap_or(0.0);
                cont.push(Tile { key: format!("video:{}", f[0]), title: f[3].into(), art: f[4].into(), kind: K_VIDEO,
                    progress: -1.0, subtitle: s[&1111].replace("{0}", &duration(secs)), ..Default::default() });
            }
        }
        let mut videos = Vec::new();
        let mut recordings = Vec::new();
        let mut albums = Vec::new();
        let mut music_dirs = Vec::new();
        for f in parse_lines(&sources, "source") {
            if f.len() < 3 {
                continue;
            }
            match f[0] {
                "videos" => videos.extend(video_tiles(&run_tool(MEDIALIB, &["videos", f[1], &user], None), s)),
                "recordings" => recordings.extend(video_tiles(&run_tool(MEDIALIB, &["videos", f[1], &user], None), s)),
                "music" => {
                    music_dirs.push(f[1].to_owned());
                    for a in parse_lines(&run_tool(MEDIALIB, &["albums", f[1]], None), "album") {
                        if a.len() >= 5 {
                            let title = if a[1].is_empty() { s[&1119].clone() } else { a[1].to_owned() };
                            let tile = Tile { key: format!("album:{}", a[0]), title, art: a[3].into(), kind: K_ALBUM,
                                progress: -1.0, subtitle: if a[2].is_empty() { s[&1112].replace("{0}", a[4]) }
                                    else { format!("{} • {}", a[2], s[&1112].replace("{0}", a[4])) },
                                ..Default::default() };
                            albums.push(tile);
                        }
                    }
                }
                _ => {}
            }
        }
        if albums.is_empty() {
            albums.push(Tile { key: "info:music".into(), title: s[&1170].clone(), subtitle: s[&1168].clone(),
                kind: K_INFO, progress: -1.0, ..Default::default() });
        } else {
            albums.insert(0, Tile { key: format!("shuffleall:{}", music_dirs.join("\t")), title: s[&1101].clone(),
                kind: K_SHUFFLE, progress: -1.0, ..Default::default() });
        }
        if videos.is_empty() && recordings.is_empty() && cont.is_empty() {
            videos.push(Tile { key: "info:videos".into(), title: s[&1171].clone(), subtitle: s[&1169].clone(),
                kind: K_INFO, progress: -1.0, ..Default::default() });
        }
        let mut playlists = Vec::new();
        for p in parse_lines(&run_tool(MEDIALIB, &["playlists", &user], None), "playlist") {
            if p.len() >= 4 {
                playlists.push(Tile { key: format!("plist:{}", p[0]), title: p[1].into(), art: p[3].into(), kind: K_ALBUM,
                    progress: -1.0, subtitle: s[&1112].replace("{0}", p[2]), ..Default::default() });
            }
        }
        playlists.push(Tile { key: "newplist".into(), title: s[&1102].clone(), kind: K_ACTION, progress: -1.0, ..Default::default() });

        /* Browse: storage, shares, the Jellyfin account. */
        let mut browse = Vec::new();
        for f in parse_lines(&run_tool(FILESCTL, &["roots"], None), "root") {
            if f.len() < 4 || f[0] == "share" {
                continue;
            }
            let free: i64 = f[3].parse().unwrap_or(0);
            let title = match f[0] {
                "userdata" => s[&610].clone(),
                "sd2" => s[&64].clone(),
                _ => base_name(f[1]).to_owned(),
            };
            browse.push(Tile { key: format!("root:{}", f[1]), title, kind: K_STORAGE, progress: -1.0,
                subtitle: s[&622].replace("{0}", &human(free)), ..Default::default() });
        }
        for f in parse_lines(&run_tool(SHARESCTL, &["list"], None), "share") {
            if f.len() >= 4 {
                browse.push(Tile { key: format!("share:{}:{}", if f[3] == "1" { 1 } else { 0 }, f[0]), title: f[0].into(),
                    kind: K_STORAGE, progress: -1.0,
                    subtitle: if f[3] == "1" { s[&1128].clone() } else { s[&1125].clone() }, ..Default::default() });
            }
        }
        let jst = run_tool(JELLYFIN, &["status", &user], None);
        let signed = jst.lines().any(|l| l == "signed=1");
        let jname = jst.lines().find_map(|l| l.strip_prefix("name=")).unwrap_or("").to_owned();
        let server = jst.lines().find_map(|l| l.strip_prefix("server=")).unwrap_or("").to_owned();
        browse.push(if signed {
            Tile { key: "jfaccount".into(), title: s[&1122].clone(), subtitle: format!("{} • {}", s[&1124].replace("{0}", &jname), server),
                kind: K_LIBRARY, progress: -1.0, ..Default::default() }
        } else {
            Tile { key: "jfsignin".into(), title: s[&1122].clone(), subtitle: s[&1123].clone(), kind: K_LIBRARY,
                progress: -1.0, ..Default::default() }
        });
        let local = Loaded {
            shelves: vec![
                ("albums", MUSIC_SECTION, String::new(), albums, SQUARE),
                ("playlists", MUSIC_SECTION, String::new(), playlists, SQUARE),
                ("mbrowse", MUSIC_SECTION, String::new(), browse.clone(), SQUARE),
                ("continue", VIDEO_SECTION, String::new(), cont.clone(), WIDE),
                ("videos", VIDEO_SECTION, String::new(), videos, POSTER),
                ("recordings", VIDEO_SECTION, String::new(), recordings, WIDE),
                ("browse", VIDEO_SECTION, String::new(), browse.clone(), SQUARE),
            ],
            jf: Some((signed, jname.clone())),
        };
        let w = weak.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = w.upgrade() {
                apply_loaded(&ui, local, generation, false);
            }
        });
        if !signed {
            return;
        }
        /* Jellyfin, after the local shelves are on screen: Continue
         * Watching from the server and one shelf per library. */
        let mut jf_shelves = Vec::new();
        let resume = jellyfin_tiles(&run_tool(JELLYFIN, &["resume", &user], None), s);
        let views = run_tool(JELLYFIN, &["views", &user], None);
        let mut all_cont = cont;
        all_cont.extend(resume);
        jf_shelves.push(("continue", VIDEO_SECTION, String::new(), all_cont, WIDE));
        for v in parse_lines(&views, "item") {
            if v.len() < 3 {
                continue;
            }
            let items = jellyfin_tiles(&run_tool(JELLYFIN, &["items", &user, v[0]], None), s);
            if items.is_empty() {
                continue;
            }
            /* The library kind (jellyfinctl subtitle) picks the section. */
            let music = v.get(6).map(|k| *k == "music").unwrap_or(false)
                || items.iter().all(|t| t.kind == K_ALBUM || t.kind == K_SONG);
            let title = format!("{} • {}", s[&1122], v[1]);
            if music {
                jf_shelves.push(("jfmusic", MUSIC_SECTION, title, items, SQUARE));
            } else {
                jf_shelves.push(("jellyfin", VIDEO_SECTION, title, items, POSTER));
            }
        }
        let loaded = Loaded { shelves: jf_shelves, jf: None};
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = weak.upgrade() {
                apply_loaded(&ui, loaded, generation, true);
            }
        });
    });
}

fn shelf_title(ui: &HomeWindow, id: &str, title: &str) -> String {
    match id {
        "continue" => tr(ui, 1095, "Continue Watching"),
        "videos" => tr(ui, 1096, "Videos"),
        "recordings" => tr(ui, 1097, "Recordings"),
        "albums" => tr(ui, 1165, "Albums"),
        "playlists" => tr(ui, 1099, "Playlists"),
        "browse" | "mbrowse" => tr(ui, 1100, "Browse"),
        _ => title.to_owned(),
    }
}

/* Local shelves first; Jellyfin's are merged in when they arrive (the
 * Continue shelf is replaced, libraries go before the section's Browse).
 * Each shelf keeps its focused tile and each section its focused shelf. */
fn apply_loaded(ui: &HomeWindow, loaded: Loaded, generation: u64, merge: bool) {
    let stale = STATE.with(|st| st.borrow().generation != generation);
    if stale {
        return;
    }
    let titles: Vec<String> = loaded.shelves.iter().map(|(id, _, t, _, _)| shelf_title(ui, id, t)).collect();
    STATE.with(|st| {
        let mut st = st.borrow_mut();
        /* What had the focus, by shelf title and tile key. */
        let old_focus: HashMap<String, String> = st
            .shelves
            .iter()
            .filter_map(|s| s.tiles.get(s.focus).map(|t| (s.title.clone(), t.key.clone())))
            .collect();
        let old_current: [String; SECTIONS] = [0, 1, 2].map(|sec| {
            let vis: Vec<&Shelf> = st.shelves.iter().filter(|s| s.section == sec).collect();
            vis.get(st.shelf_index[sec]).map(|s| s.title.clone()).unwrap_or_default()
        });
        let multi = |id: &str| id == "jellyfin" || id == "jfmusic";
        let mut shelves: Vec<Shelf> = if merge {
            std::mem::take(&mut st.shelves).into_iter().filter(|s| !multi(s.id)).collect()
        } else {
            Vec::new()
        };
        for ((id, section, _, tiles, shape), title) in loaded.shelves.into_iter().zip(titles) {
            if tiles.is_empty() && id != "playlists" {
                if merge {
                    shelves.retain(|s| s.id != id);
                }
                continue;
            }
            let focus = old_focus.get(&title).and_then(|k| tiles.iter().position(|t| &t.key == k)).unwrap_or(0);
            let tiles_model = model(tiles.iter().map(|t| media_tile(&st, t)).collect::<Vec<_>>());
            let shelf = Shelf { id, section, focus, title, tiles, shape, model: tiles_model };
            if let Some(pos) = shelves.iter().position(|s| s.id == id && !multi(id)) {
                shelves[pos] = shelf;
            } else if multi(id) {
                let browse = if id == "jfmusic" { "mbrowse" } else { "browse" };
                let at = shelves.iter().position(|s| s.id == browse).unwrap_or(shelves.len());
                let at = shelves.iter().rposition(|s| s.id == id).map(|p| p + 1).unwrap_or(at);
                shelves.insert(at, shelf);
            } else if id == "continue" {
                let at = shelves.iter().position(|s| s.section == VIDEO_SECTION).unwrap_or(shelves.len());
                shelves.insert(at, shelf);
            } else {
                shelves.push(shelf);
            }
        }
        for sec in 0..SECTIONS {
            let vis: Vec<&Shelf> = shelves.iter().filter(|s| s.section == sec).collect();
            st.shelf_index[sec] = vis.iter().position(|s| s.title == old_current[sec]).unwrap_or(0);
        }
        st.shelves = shelves;
        st.loading = false;
        if let Some((signed, name)) = loaded.jf {
            st.jf_signed = signed;
            st.jf_name = name;
        }
    });
    publish_shelves(ui);
    show_view(ui);
}

/* L2/R2 on the home: the previous/next section. */
fn switch_section(ui: &HomeWindow, to: usize) {
    let changed = STATE.with(|st| {
        let mut st = st.borrow_mut();
        let changed = st.section != to;
        st.section = to;
        changed
    });
    if changed {
        if to == SETTINGS_SECTION {
            refresh_settings(ui);
        }
        publish_shelves(ui);
        show_view(ui);
    }
}

/* ---------------------------------------------------------------- */
/* Settings tab (user request 2026-10-14: like Moonlight's settings)  */
/* ---------------------------------------------------------------- */

/* Rows: 0 Show track progress, 1 Keep lit with the screen off. The
 * options are lightingd's; Home only reads and sets them. */
const SETTINGS_ROWS: i32 = 2;

fn refresh_settings(ui: &HomeWindow) {
    let weak = ui.as_weak();
    thread::spawn(move || {
        let follow = crate::lighting::media_progress();
        let keep = crate::lighting::screen_off_lighting();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = weak.upgrade() {
                if let Some(v) = follow {
                    ui.set_media_rgb_follow(v);
                }
                if let Some(v) = keep {
                    ui.set_media_rgb_keep(v);
                }
            }
        });
    });
}

/* true = handled here (the rest, tabs and Back, as on the other tabs). */
fn settings_action(ui: &HomeWindow, action: &str) -> bool {
    if !ui.get_lighting_supported() {
        return matches!(action, "menu_up" | "menu_down" | "menu_left" | "menu_right" | "menu_confirm" | "face_west");
    }
    let index = ui.get_media_settings_index();
    match action {
        "menu_up" => ui.set_media_settings_index((index - 1).max(0)),
        "menu_down" => ui.set_media_settings_index((index + 1).min(SETTINGS_ROWS - 1)),
        "menu_left" | "menu_right" | "face_west" => {}
        "menu_confirm" => {
            let keep = index == 1;
            let on = if keep { !ui.get_media_rgb_keep() } else { !ui.get_media_rgb_follow() };
            if keep {
                ui.set_media_rgb_keep(on);
            } else {
                ui.set_media_rgb_follow(on);
            }
            let weak = ui.as_weak();
            thread::spawn(move || {
                let ok = if keep {
                    crate::lighting::set_screen_off_lighting(on)
                } else {
                    crate::lighting::set_media_progress(on)
                };
                if !ok {
                    eprintln!("home: Lighting option row={} failed", keep as i32);
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = weak.upgrade() {
                            refresh_settings(&ui);
                        }
                    });
                }
            });
        }
        _ => return false,
    }
    hints(ui);
    true
}

/* ---------------------------------------------------------------- */
/* Grids and albums                                                  */
/* ---------------------------------------------------------------- */

fn push_grid(ui: &HomeWindow, title: String, source: String, shape: i32) {
    STATE.with(|st| {
        st.borrow_mut().views.push(View::Grid { title, source: source.clone(), tiles: Vec::new(), index: 0, shape,
            loading: true, model: model(Vec::new()) })
    });
    show_view(ui);
    load_grid(ui, source);
}

fn load_grid(ui: &HomeWindow, source: String) {
    let user = ui.get_active_user_id().to_string();
    let strings = strings(ui);
    let weak = ui.as_weak();
    thread::spawn(move || {
        let s = &strings;
        let tiles = if let Some(dir) = source.strip_prefix("vdir:") {
            video_tiles(&run_tool(MEDIALIB, &["videos", dir, &user], None), s)
        } else if let Some(id) = source.strip_prefix("jfdir:") {
            jellyfin_tiles(&run_tool(JELLYFIN, &["items", &user, id], None), s)
        } else if let Some(dir) = source.strip_prefix("bdir:") {
            /* Any folder: subfolders, videos, and its songs as one tile. */
            let out = run_tool(FILESCTL, &["list", dir], None);
            let mut dirs = Vec::new();
            let mut vids = Vec::new();
            let mut songs = 0;
            for f in parse_lines(&out, "entry") {
                if f.len() < 4 {
                    continue;
                }
                let path = format!("{dir}/{}", f[3]);
                if f[0] == "d" {
                    dirs.push(Tile { key: format!("bdir:{path}"), title: f[3].into(), kind: K_FOLDER, progress: -1.0,
                        ..Default::default() });
                } else if is_video(f[3]) {
                    vids.push(Tile { key: format!("video:{path}"), title: f[3].into(), kind: K_VIDEO, progress: -1.0,
                        ..Default::default() });
                } else if is_audio(f[3]) {
                    songs += 1;
                }
            }
            dirs.sort_by_key(|t| t.title.to_lowercase());
            vids.sort_by_key(|t| t.title.to_lowercase());
            let mut tiles = Vec::new();
            if songs > 0 {
                tiles.push(Tile { key: format!("album:{dir}"), title: s[&1119].clone(), kind: K_ALBUM, progress: -1.0,
                    badge: songs.to_string(), subtitle: s[&1112].replace("{0}", &songs.to_string()), ..Default::default() });
            }
            tiles.extend(dirs);
            tiles.extend(vids);
            tiles
        } else {
            Vec::new()
        };
        let _ = slint::invoke_from_event_loop(move || {
            let Some(ui) = weak.upgrade() else { return };
            STATE.with(|st| {
                let mut st = st.borrow_mut();
                let rows: Vec<MediaTile> = tiles.iter().map(|t| media_tile(&st, t)).collect();
                if let Some(View::Grid { source: src, tiles: ts, model: m, loading, index, .. }) = st.views.last_mut() {
                    if *src == source {
                        *index = (*index).min(tiles.len().saturating_sub(1));
                        *ts = tiles;
                        m.set_vec(rows);
                        *loading = false;
                    }
                }
            });
            show_view(&ui);
        });
    });
}

fn push_album(ui: &HomeWindow, title: String, artist: String, art: String, source: AlbumSource) {
    STATE.with(|st| {
        st.borrow_mut().views.push(View::Album { title, artist, meta: String::new(), art, tracks: Vec::new(), index: -2,
            source: source.clone(), model: model(Vec::new()) })
    });
    show_view(ui);
    load_album(ui, source);
}

fn load_album(ui: &HomeWindow, source: AlbumSource) {
    let user = ui.get_active_user_id().to_string();
    let tracks_label = tr(ui, 1112, "{0} tracks");
    let weak = ui.as_weak();
    thread::spawn(move || {
        let mut tracks = Vec::new();
        let mut cover = String::new();
        match &source {
            AlbumSource::Dir(dir) | AlbumSource::Playlist(dir) => {
                let out = match &source {
                    AlbumSource::Dir(_) => run_tool(MEDIALIB, &["tracks", dir], None),
                    _ => run_tool(MEDIALIB, &["playlist", dir], None),
                };
                for (i, f) in parse_lines(&out, "track").into_iter().enumerate() {
                    if f.len() >= 3 {
                        let n: i64 = f[2].parse().unwrap_or(0);
                        let number = if matches!(source, AlbumSource::Dir(_)) && n > 0 { n.to_string() } else { (i + 1).to_string() };
                        tracks.push(Track { path: f[0].into(), title: f[1].into(), detail: String::new(), number });
                    }
                }
                if let Some(first) = tracks.first() {
                    cover = run_tool(MEDIALIB, &["cover", &first.path], None).trim().strip_prefix("cover=").unwrap_or("").to_owned();
                }
            }
            AlbumSource::Jellyfin(id) => {
                for f in parse_lines(&run_tool(JELLYFIN, &["items", &user, id], None), "item") {
                    if f.len() >= 6 && f[3] != "1" {
                        let length: f64 = f[5].parse().unwrap_or(0.0);
                        let n: i64 = f.get(7).and_then(|n| n.parse().ok()).unwrap_or(0);
                        tracks.push(Track { path: f[0].into(), title: f[1].into(),
                            detail: if length > 0.0 { duration(length) } else { String::new() },
                            number: if n > 0 { n.to_string() } else { (tracks.len() + 1).to_string() } });
                    }
                }
            }
        }
        let _ = slint::invoke_from_event_loop(move || {
            let Some(ui) = weak.upgrade() else { return };
            STATE.with(|st| {
                let mut st = st.borrow_mut();
                if let Some(View::Album { source: src, tracks: ts, model: m, meta, art, index, .. }) = st.views.last_mut() {
                    if *src == source {
                        m.set_vec(tracks.iter().map(|t| MediaTrack { number: t.number.clone().into(), title: t.title.clone().into(),
                            detail: t.detail.clone().into(), current: false }).collect::<Vec<_>>());
                        *meta = tracks_label.replace("{0}", &tracks.len().to_string());
                        *ts = tracks;
                        *index = (*index).min(ts.len() as i32 - 1).max(-2);
                        if art.is_empty() && !cover.is_empty() {
                            *art = cover;
                        }
                    }
                }
            });
            show_view(&ui);
            mark_current(&ui);
            if let Some(shuffle) = STATE.with(|st| st.borrow_mut().autoplay.take()) {
                play_album(&ui, 0, shuffle);
            }
        });
    });
}

/* The playing track highlighted in the album and the queue. */
fn mark_current(ui: &HomeWindow) {
    let snap = STATE.with(|st| st.borrow().snap.clone());
    STATE.with(|st| {
        let st = st.borrow();
        if let Some(View::Album { tracks, model, .. }) = st.views.last() {
            for (i, t) in tracks.iter().enumerate() {
                let cur = snap.music() && (snap.item == t.path || snap.item.contains(&format!("/Items/{}/", t.path)));
                if let Some(mut row) = model.row_data(i) {
                    if row.current != cur {
                        row.current = cur;
                        model.set_row_data(i, row);
                    }
                }
            }
        }
        if let Some(q) = &st.queue_model {
            for i in 0..q.row_count() {
                if let Some(mut row) = q.row_data(i) {
                    let cur = i as i64 == snap.index;
                    if row.current != cur {
                        row.current = cur;
                        q.set_row_data(i, row);
                    }
                }
            }
        }
    });
    let _ = ui;
}

/* The album's tracks as a queue: Jellyfin ids become stream URLs. */
fn play_album(ui: &HomeWindow, start: usize, shuffle: bool) {
    let Some((tracks, source)) = STATE.with(|st| match st.borrow().views.last() {
        Some(View::Album { tracks, source, .. }) => Some((tracks.clone(), source.clone())),
        _ => None,
    }) else { return };
    if tracks.is_empty() {
        return;
    }
    if let AlbumSource::Jellyfin(_) = source {
        let user = ui.get_active_user_id().to_string();
        let weak = ui.as_weak();
        thread::spawn(move || {
            let mut args = vec!["stream".to_owned(), user];
            args.extend(tracks.iter().map(|t| t.path.clone()));
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            let urls: Vec<String> = run_tool(JELLYFIN, &refs, None).lines().filter(|l| l.starts_with("http")).map(str::to_owned).collect();
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = weak.upgrade() else { return };
                if urls.len() != tracks.len() {
                    notice(&ui, tr(&ui, 668, "The server could not be reached"));
                    return;
                }
                let items = urls.into_iter().zip(tracks.iter()).map(|(u, t)| (u, t.title.clone())).collect();
                play_list(&ui, items, start, shuffle);
            });
        });
    } else {
        play_list(ui, tracks.into_iter().map(|t| (t.path, String::new())).collect(), start, shuffle);
    }
}

fn shuffle_all(ui: &HomeWindow, dirs: String) {
    let weak = ui.as_weak();
    thread::spawn(move || {
        let mut items = Vec::new();
        for d in dirs.split('\t') {
            for f in parse_lines(&run_tool(MEDIALIB, &["tracks", d, "all"], None), "track") {
                if !f.is_empty() {
                    items.push((f[0].to_owned(), String::new()));
                }
            }
        }
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = weak.upgrade() {
                let start = if items.is_empty() { 0 } else { (Instant::now().elapsed().subsec_nanos() as usize) % items.len() };
                play_list(&ui, items, start, true);
            }
        });
    });
}

fn play_video(ui: &HomeWindow, path: &str, from_start: bool) {
    *JELLYFIN_ITEM.lock().unwrap() = String::new();
    send_osd_labels(ui);
    let line = if from_start { format!("PLAY\t{path}\t0") } else { format!("PLAY\t{path}") };
    start(ui, line);
}

fn play_jellyfin(ui: &HomeWindow, id: String, resume: String, audio: bool) {
    let user = ui.get_active_user_id().to_string();
    let weak = ui.as_weak();
    thread::spawn(move || {
        let url = run_tool(JELLYFIN, &["stream", &user, &id], None).trim().to_owned();
        let _ = slint::invoke_from_event_loop(move || {
            let Some(ui) = weak.upgrade() else { return };
            if !url.starts_with("http") {
                notice(&ui, tr(&ui, 668, "The server could not be reached"));
                return;
            }
            *JELLYFIN_ITEM.lock().unwrap() = if audio { String::new() } else { id };
            if !audio {
                send_osd_labels(&ui);
            }
            start(&ui, if audio { format!("PLAY\t{url}\t0\taudio") } else { format!("PLAY\t{url}\t{resume}") });
            if audio {
                open_now_playing(&ui);
            }
        });
    });
}

fn start(ui: &HomeWindow, line: String) {
    let weak = ui.as_weak();
    thread::spawn(move || {
        let reply = player(&line);
        if !reply.starts_with("OK") {
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = weak.upgrade() {
                    notice(&ui, tr(&ui, 667, "The media could not be played"));
                }
            });
        }
    });
}

/* ---------------------------------------------------------------- */
/* Open / leave, confirm, options                                    */
/* ---------------------------------------------------------------- */

/* Home → Applications → Media. */
pub fn open(ui: &HomeWindow) {
    ACTIVE.store(true, Ordering::SeqCst);
    refresh_settings(ui);
    STATE.with(|st| {
        let mut st = st.borrow_mut();
        st.views.clear();
        st.np_open = false;
    });
    ui.set_media_open(true);
    ui.set_app_kicker(tr(ui, 665, "Media").to_uppercase().into());
    ui.set_details_active(true);
    ui.set_details_shell_title(tr(ui, 665, "Media").into());
    ui.set_details_has_cover(false);
    navigate_settings_view(ui, VIEW);
    ui.set_settings_open(true);
    write_ui_context("settings");
    show_view(ui);
    load_home(ui);
    update_tick(ui);
}

fn leave(ui: &HomeWindow) {
    ACTIVE.store(false, Ordering::SeqCst);
    navigate_settings_view(ui, 0);
    ui.set_settings_selected_index(0);
    ui.set_settings_open(false);
    ui.set_details_active(false);
    ui.set_details_shell_title("".into());
    ui.set_media_open(false);
    ui.set_app_hints(ModelRc::default());
    write_ui_context("home");
    update_tick(ui);
}

fn confirm_tile(ui: &HomeWindow, t: Tile) {
    let k = t.key.as_str();
    if let Some(path) = k.strip_prefix("video:") {
        play_video(ui, path, false);
    } else if let Some(dir) = k.strip_prefix("vdir:") {
        push_grid(ui, t.title.clone(), format!("vdir:{dir}"), POSTER);
    } else if let Some(dir) = k.strip_prefix("album:") {
        let artist = t.subtitle.split(" • ").next().filter(|_| t.subtitle.contains(" • ")).unwrap_or("").to_owned();
        push_album(ui, t.title.clone(), artist, t.art.clone(), AlbumSource::Dir(dir.into()));
    } else if let Some(file) = k.strip_prefix("plist:") {
        push_album(ui, t.title.clone(), tr(ui, 1099, "Playlists"), t.art.clone(), AlbumSource::Playlist(file.into()));
    } else if let Some(dirs) = k.strip_prefix("shuffleall:") {
        shuffle_all(ui, dirs.to_owned());
    } else if k == "newplist" {
        STATE.with(|st| st.borrow_mut().pending_add.clear());
        open_system_keyboard(ui, &tr(ui, 1103, "Playlist name"), KEYBOARD_PLAYLIST, VIEW, "text", "");
    } else if let Some(path) = k.strip_prefix("root:") {
        push_grid(ui, t.title.clone(), format!("bdir:{path}"), SQUARE);
    } else if let Some(rest) = k.strip_prefix("share:") {
        let (mounted, name) = rest.split_once(':').unwrap_or(("0", rest));
        if mounted == "1" {
            push_grid(ui, name.to_owned(), format!("bdir:/run/nuubos/shares/{name}"), SQUARE);
        } else {
            let name = name.to_owned();
            let weak = ui.as_weak();
            thread::spawn(move || {
                let out = run_tool(SHARESCTL, &["mount", &name], None);
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(ui) = weak.upgrade() else { return };
                    if out.contains("auth") {
                        notice(&ui, tr(&ui, 1179, "Wrong user name or password"));
                    } else if !out.starts_with("OK") {
                        notice(&ui, tr_arg(&ui, 1181, "Cannot reach {0}: check the address and that the computer is on", &name));
                    }
                    load_home(&ui);
                });
            });
        }
    } else if let Some(path) = k.strip_prefix("bdir:") {
        push_grid(ui, t.title.clone(), format!("bdir:{path}"), SQUARE);
    } else if let Some(id) = k.strip_prefix("jfdir:") {
        push_grid(ui, t.title.clone(), format!("jfdir:{id}"), POSTER);
    } else if let Some(id) = k.strip_prefix("jfalbum:") {
        push_album(ui, t.title.clone(), t.subtitle.clone(), t.art.clone(), AlbumSource::Jellyfin(id.into()));
    } else if let Some(rest) = k.strip_prefix("jfitem:") {
        let mut p = rest.splitn(3, ':');
        let (id, resume, kind) = (p.next().unwrap_or(""), p.next().unwrap_or("0"), p.next().unwrap_or("video"));
        play_jellyfin(ui, id.to_owned(), resume.to_owned(), kind == "audio");
    } else if k == "jfsignin" {
        STATE.with(|st| st.borrow_mut().draft = Default::default());
        open_system_keyboard(ui, &tr(ui, 666, "Server address (http://server:8096)"), KEYBOARD_SERVER, VIEW, "text", "http://");
    } else if k == "jfaccount" {
        options(ui);
    }
}

fn options(ui: &HomeWindow) {
    let (tile, album) = STATE.with(|st| {
        let st = st.borrow();
        let album = match st.views.last() {
            Some(View::Album { index, source, tracks, title, .. }) => Some((*index, source.clone(), tracks.len(), title.clone())),
            _ => None,
        };
        (focused_tile(&st), album)
    });
    let mut opts: Vec<(String, String)> = Vec::new();
    let mut title = String::new();
    if let Some((index, source, count, name)) = album {
        title = name;
        match &source {
            AlbumSource::Playlist(_) => {
                if index >= 0 {
                    opts.push(("plist-remove".into(), tr(ui, 1105, "Remove from Playlist")));
                }
                opts.push(("plist-rename".into(), tr(ui, 1107, "Rename Playlist")));
                opts.push(("plist-delete".into(), tr(ui, 1106, "Delete Playlist")));
            }
            AlbumSource::Dir(_) if count > 0 => opts.push(("add-playlist".into(), tr(ui, 1104, "Add to Playlist"))),
            _ => {}
        }
    } else if let Some(t) = tile {
        title = t.title.clone();
        if t.key.starts_with("video:") {
            if t.subtitle.is_empty() {
                return;
            }
            opts.push(("resume".into(), tr(ui, 416, "Resume")));
            opts.push(("from-start".into(), tr(ui, 1110, "Play from the Beginning")));
        } else if t.key.starts_with("album:") {
            opts.push(("album-play".into(), tr(ui, 1164, "Play")));
            opts.push(("album-shuffle".into(), tr(ui, 1109, "Shuffle")));
            opts.push(("add-playlist".into(), tr(ui, 1104, "Add to Playlist")));
        } else if t.key.starts_with("plist:") {
            opts.push(("album-play".into(), tr(ui, 1164, "Play")));
            opts.push(("album-shuffle".into(), tr(ui, 1109, "Shuffle")));
            opts.push(("plist-rename".into(), tr(ui, 1107, "Rename Playlist")));
            opts.push(("plist-delete".into(), tr(ui, 1106, "Delete Playlist")));
        } else if t.key.starts_with("share:1:") {
            opts.push(("share-umount".into(), tr(ui, 169, "Disconnect")));
        } else if t.key == "jfaccount" {
            opts.push(("jf-signout".into(), tr(ui, 567, "Sign Out")));
        }
    }
    if !opts.is_empty() {
        open_settings_choice(ui, "media-opt", &title, opts, "");
    }
}

fn focused_key(ui: &HomeWindow) -> Tile {
    let _ = ui;
    STATE.with(|st| focused_tile(&st.borrow())).unwrap_or_default()
}

/* Paths the focused album / album track / album tile adds to a playlist. */
fn add_paths() -> Vec<String> {
    STATE.with(|st| {
        let st = st.borrow();
        match st.views.last() {
            Some(View::Album { tracks, index, source: AlbumSource::Dir(dir), .. }) => {
                if *index >= 0 {
                    tracks.get(*index as usize).map(|t| vec![t.path.clone()]).unwrap_or_default()
                } else {
                    vec![dir.clone()]
                }
            }
            Some(View::Album { .. }) => Vec::new(),
            _ => focused_tile(&st)
                .and_then(|t| t.key.strip_prefix("album:").map(|d| vec![d.to_owned()]))
                .unwrap_or_default(),
        }
    })
}

fn add_to_playlist(ui: &HomeWindow, file: String, paths: Vec<String>) {
    let name = base_name(&file).trim_end_matches(".m3u").to_owned();
    let weak = ui.as_weak();
    thread::spawn(move || {
        let mut args = vec!["playlist-add", file.as_str()];
        args.extend(paths.iter().map(String::as_str));
        let out = run_tool(MEDIALIB, &args, None);
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = weak.upgrade() {
                notice(&ui, if out.starts_with("OK") { tr_arg(&ui, 846, "Added to {0}", &name) } else { tr(&ui, 93, "Failed") });
                load_home(&ui);
            }
        });
    });
}

/* Dropdown choices opened by this module. */
pub fn apply_choice(ui: &HomeWindow, context: &str, value: String) {
    match context {
        "media-opt" => match value.as_str() {
            "resume" | "from-start" => {
                let t = focused_key(ui);
                if let Some(path) = t.key.strip_prefix("video:") {
                    play_video(ui, path, value == "from-start");
                }
            }
            "album-play" | "album-shuffle" => {
                /* The album opens and plays once its tracks are loaded. */
                let t = focused_key(ui);
                STATE.with(|st| st.borrow_mut().autoplay = Some(value == "album-shuffle"));
                confirm_tile(ui, t);
            }
            "add-playlist" => {
                let paths = add_paths();
                if paths.is_empty() {
                    return;
                }
                let user = ui.get_active_user_id().to_string();
                let out = run_tool(MEDIALIB, &["playlists", &user], None);
                let mut opts: Vec<(String, String)> = parse_lines(&out, "playlist")
                    .into_iter()
                    .filter(|p| p.len() >= 2)
                    .map(|p| (p[0].to_owned(), p[1].to_owned()))
                    .collect();
                opts.push(("new".into(), tr(ui, 1102, "New Playlist")));
                STATE.with(|st| st.borrow_mut().pending_add = paths);
                open_settings_choice(ui, "media-addto", &tr(ui, 1104, "Add to Playlist"), opts, "");
            }
            "plist-remove" => {
                let target = STATE.with(|st| match st.borrow().views.last() {
                    Some(View::Album { source: AlbumSource::Playlist(f), tracks, index, .. }) if *index >= 0 =>
                        tracks.get(*index as usize).map(|_| (f.clone(), *index)),
                    _ => None,
                });
                if let Some((file, index)) = target {
                    /* Positions in the file: missing tracks are skipped in the
                     * list, so remove by the listed position. */
                    let weak = ui.as_weak();
                    thread::spawn(move || {
                        let out = run_tool(MEDIALIB, &["playlist", &file], None);
                        let pos = parse_lines(&out, "track").get(index as usize).and_then(|f| f.get(2).map(|s| s.to_string()));
                        if let Some(pos) = pos {
                            let _ = run_tool(MEDIALIB, &["playlist-remove", &file, &pos], None);
                        }
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = weak.upgrade() {
                                notice(&ui, tr(&ui, 1108, "Removed from the playlist"));
                                load_album(&ui, AlbumSource::Playlist(file));
                            }
                        });
                    });
                }
            }
            "plist-rename" | "plist-delete" => {
                let file = STATE.with(|st| {
                    let st = st.borrow();
                    match st.views.last() {
                        Some(View::Album { source: AlbumSource::Playlist(f), .. }) => Some(f.clone()),
                        Some(View::Album { .. }) => None,
                        _ => focused_tile(&st).and_then(|t| t.key.strip_prefix("plist:").map(str::to_owned)),
                    }
                });
                let Some(file) = file else { return };
                let name = base_name(&file).trim_end_matches(".m3u").to_owned();
                if value == "plist-rename" {
                    STATE.with(|st| st.borrow_mut().renaming = file);
                    open_system_keyboard(ui, &tr(ui, 1107, "Rename Playlist"), KEYBOARD_PLAYLIST_RENAME, VIEW, "text", &name);
                } else {
                    STATE.with(|st| st.borrow_mut().renaming = file);
                    let opts = vec![("delete".to_owned(), tr(ui, 618, "Delete")), ("cancel".to_owned(), tr(ui, 619, "Cancel"))];
                    open_settings_choice(ui, "media-plist-delete", &tr_arg(ui, 620, "Delete \"{0}\"?", &name), opts, "cancel");
                }
            }
            "share-umount" => {
                let t = focused_key(ui);
                if let Some(name) = t.key.strip_prefix("share:1:") {
                    tool_then(ui, SHARESCTL, vec!["umount".into(), name.to_owned()], None, |ui, _| load_home(ui));
                }
            }
            "jf-signout" => {
                tool_then(ui, JELLYFIN, vec!["logout".into(), ui.get_active_user_id().to_string()], None, |ui, _| load_home(ui));
            }
            _ => {}
        },
        "media-addto" => {
            let paths = STATE.with(|st| std::mem::take(&mut st.borrow_mut().pending_add));
            if value == "new" {
                STATE.with(|st| st.borrow_mut().pending_add = paths);
                open_system_keyboard(ui, &tr(ui, 1103, "Playlist name"), KEYBOARD_PLAYLIST, VIEW, "text", "");
            } else {
                add_to_playlist(ui, value, paths);
            }
        }
        "media-plist-delete" if value == "delete" => {
            let file = STATE.with(|st| std::mem::take(&mut st.borrow_mut().renaming));
            tool_then(ui, MEDIALIB, vec!["playlist-delete".into(), file], None, |ui, _| {
                STATE.with(|st| {
                    let mut st = st.borrow_mut();
                    if matches!(st.views.last(), Some(View::Album { source: AlbumSource::Playlist(_), .. })) {
                        st.views.pop();
                    }
                });
                show_view(ui);
                load_home(ui);
            });
        }
        _ => {}
    }
}

/* ---------------------------------------------------------------- */
/* Input                                                             */
/* ---------------------------------------------------------------- */

/* Same as the grid in media_app.slint (content = window minus the top
 * and hint bars, minus the mini player while music plays). */
fn grid_cols(ui: &HomeWindow, shape: i32) -> usize {
    let scale = ui.window().scale_factor();
    let size = ui.window().size();
    let w = size.width as f32 / scale;
    let h = size.height as f32 / scale;
    let home_s = (h / 480.0).clamp(1.0, 1.4);
    let s = home_s.min((w / 640.0).max(1.0));
    let (pad, gap) = (18.0 * s, 12.0 * s);
    let mini = if MUSIC.load(Ordering::SeqCst) { 56.0 * s } else { 0.0 };
    let area = h - 102.0 - mini - 78.0 * s;
    let ratio = match shape { WIDE => 16.0 / 9.0, SQUARE => 1.0, _ => 2.0 / 3.0 };
    let target = (match shape { WIDE => 200.0, SQUARE => 130.0, _ => 112.0 } * s).min(area * 0.78 * ratio);
    (((w - 2.0 * pad + gap) / (target + gap)).floor() as usize).max(2)
}

fn move_home(ui: &HomeWindow, action: &str) {
    let changed = STATE.with(|st| {
        let mut st = st.borrow_mut();
        let vis = visible(&st);
        let n = vis.len();
        if n == 0 {
            return None;
        }
        let sec = st.section;
        let p = st.shelf_index[sec].min(n - 1);
        let i = vis[p];
        match action {
            "menu_up" if p > 0 => st.shelf_index[sec] -= 1,
            "menu_down" if p + 1 < n => st.shelf_index[sec] += 1,
            "menu_left" if st.shelves[i].focus > 0 => st.shelves[i].focus -= 1,
            "menu_right" if st.shelves[i].focus + 1 < st.shelves[i].tiles.len() => st.shelves[i].focus += 1,
            _ => return None,
        }
        Some((st.shelf_index[sec], p, st.shelves[i].focus))
    });
    if let Some((now, row, f)) = changed {
        ui.set_media_shelf_index(now as i32);
        STATE.with(|st| {
            if let Some(m) = &st.borrow().indices_model {
                m.set_row_data(row, f as i32);
            }
        });
        refresh_header(ui);
        hints(ui);
    }
}

fn move_grid(ui: &HomeWindow, action: &str) {
    let shape = STATE.with(|st| match st.borrow().views.last() {
        Some(View::Grid { shape, .. }) => *shape,
        _ => POSTER,
    });
    let cols = grid_cols(ui, shape);
    let moved = STATE.with(|st| {
        let mut st = st.borrow_mut();
        let Some(View::Grid { index, tiles, .. }) = st.views.last_mut() else { return None };
        let n = tiles.len();
        let i = *index;
        let next = match action {
            "menu_left" if i % cols > 0 => i - 1,
            "menu_right" if i + 1 < n && (i + 1) % cols > 0 => i + 1,
            "menu_up" if i >= cols => i - cols,
            "menu_down" if i + cols < n => i + cols,
            "menu_down" if i / cols + 1 < n.div_ceil(cols) => n - 1,
            _ => return None,
        };
        *index = next;
        Some(next)
    });
    if let Some(i) = moved {
        ui.set_media_grid_index(i as i32);
        refresh_header(ui);
        hints(ui);
    }
}

fn move_album(ui: &HomeWindow, action: &str) {
    let moved = STATE.with(|st| {
        let mut st = st.borrow_mut();
        let Some(View::Album { index, tracks, .. }) = st.views.last_mut() else { return None };
        let n = tracks.len() as i32;
        let i = *index;
        let next = match action {
            "menu_left" if i == -1 => -2,
            "menu_right" if i == -2 => -1,
            "menu_up" if i > 0 => i - 1,
            "menu_up" if i == 0 => -2,
            "menu_down" if i < 0 && n > 0 => 0,
            "menu_down" if i + 1 < n => i + 1,
            _ => return None,
        };
        *index = next;
        Some(next)
    });
    if let Some(i) = moved {
        ui.set_media_album_index(i);
        hints(ui);
    }
}

fn confirm(ui: &HomeWindow) {
    enum Act {
        Tile(Tile),
        Album(i32),
        None,
    }
    let act = STATE.with(|st| {
        let st = st.borrow();
        match st.views.last() {
            Some(View::Album { index, .. }) => Act::Album(*index),
            _ => focused_tile(&st).map(Act::Tile).unwrap_or(Act::None),
        }
    });
    match act {
        Act::Tile(t) => confirm_tile(ui, t),
        Act::Album(-2) => play_album(ui, 0, false),
        Act::Album(-1) => play_album(ui, 0, true),
        Act::Album(i) if i >= 0 => play_album(ui, i as usize, false),
        _ => {}
    }
}

fn back(ui: &HomeWindow) {
    let leave_app = STATE.with(|st| {
        let mut st = st.borrow_mut();
        if st.np_open {
            st.np_open = false;
            return false;
        }
        st.views.pop().is_none()
    });
    if leave_app {
        leave(ui);
    } else {
        show_view(ui);
        update_tick(ui);
    }
}

/* Input while the Media app is open. */
pub fn handle_action(ui: &HomeWindow, action: &str, settings_active: &Arc<AtomicBool>) {
    if ui.get_settings_choice_open() || ui.get_settings_view() == 6 {
        handle_settings_action(ui, action, settings_active);
        return;
    }
    play_ui_sound(action);
    let np = STATE.with(|st| st.borrow().np_open);
    if np {
        np_action(ui, action);
        return;
    }
    let screen = STATE.with(|st| match st.borrow().views.last() {
        None => 0,
        Some(View::Grid { .. }) => 1,
        Some(View::Album { .. }) => 2,
    });
    let section = STATE.with(|st| st.borrow().section);
    if screen == 0 && section == SETTINGS_SECTION && settings_action(ui, action) {
        return;
    }
    match action {
        "tab_prev" | "tab_next" if screen == 0 => {
            switch_section(ui, if action == "tab_prev" { section.saturating_sub(1) } else { (section + 1).min(SECTIONS - 1) });
        }
        "menu_up" | "menu_down" | "menu_left" | "menu_right" => match screen {
            0 => move_home(ui, action),
            1 => move_grid(ui, action),
            _ => move_album(ui, action),
        },
        "menu_confirm" => confirm(ui),
        "menu_back" => back(ui),
        "face_west" => options(ui),
        "face_north" if MUSIC.load(Ordering::SeqCst) => open_now_playing(ui),
        _ => {}
    }
}

/* ---------------------------------------------------------------- */
/* Now Playing                                                       */
/* ---------------------------------------------------------------- */

fn open_now_playing(ui: &HomeWindow) {
    if !ACTIVE.load(Ordering::SeqCst) {
        return;
    }
    STATE.with(|st| {
        let mut st = st.borrow_mut();
        st.np_open = true;
        st.np_zone = Z_CONTROLS;
        st.np_control = 2;
        st.queue_id = -1;
    });
    show_view(ui);
    fetch_queue(ui);
    update_tick(ui);
}

fn fetch_queue(ui: &HomeWindow) {
    let weak = ui.as_weak();
    thread::spawn(move || {
        let mut items = Vec::new();
        if let Ok(mut s) = UnixStream::connect(MEDIA_SOCKET) {
            let _ = s.set_read_timeout(Some(Duration::from_secs(3)));
            if s.write_all(b"QUEUE\n").is_ok() {
                for line in BufReader::new(s).lines() {
                    let Ok(line) = line else { break };
                    if line == "end=1" {
                        break;
                    }
                    if let Some(v) = line.strip_prefix("track=") {
                        let f: Vec<&str> = v.split('\t').collect();
                        if f.len() >= 3 {
                            let title = if f[2].is_empty() {
                                let name = base_name(f[1]);
                                name.rsplit_once('.').map(|(b, _)| b).unwrap_or(name).to_owned()
                            } else {
                                f[2].to_owned()
                            };
                            items.push((f[1].to_owned(), title));
                        }
                    }
                }
            }
        }
        let _ = slint::invoke_from_event_loop(move || {
            let Some(ui) = weak.upgrade() else { return };
            let rows: Vec<MediaTrack> = items
                .iter()
                .enumerate()
                .map(|(i, (_, t))| MediaTrack { number: (i + 1).to_string().into(), title: t.clone().into(), detail: "".into(), current: false })
                .collect();
            let m = model(rows);
            ui.set_media_np_queue(ModelRc::from(m.clone()));
            STATE.with(|st| {
                let mut st = st.borrow_mut();
                st.queue = items;
                st.queue_model = Some(m);
                st.queue_id = st.snap.queue_id;
                st.np_queue_index = st.snap.index.max(0) as usize;
            });
            mark_current(&ui);
            refresh_np(&ui);
        });
    });
}

fn np_position(st: &State) -> f64 {
    let snap = &st.snap;
    let mut p = snap.position;
    if snap.state == "playing" {
        if let Some(at) = st.snap_at {
            p += at.elapsed().as_secs_f64();
        }
    }
    if snap.duration > 0.0 { p.min(snap.duration) } else { p }
}

fn format_label(snap: &Snapshot) -> String {
    let mut parts = Vec::new();
    if !snap.codec.is_empty() {
        parts.push(snap.codec.to_uppercase());
    }
    if snap.bits > 0 {
        parts.push(format!("{}-bit", snap.bits));
    }
    if snap.rate > 0 {
        let khz = snap.rate as f64 / 1000.0;
        parts.push(if snap.rate % 1000 == 0 { format!("{} kHz", snap.rate / 1000) } else { format!("{khz:.1} kHz") });
    }
    parts.join(" • ")
}

/* Now Playing and the mini player from the snapshot. */
fn refresh_np(ui: &HomeWindow) {
    let (snap, pos, zone, control, qi, cover) = STATE.with(|st| {
        let st = st.borrow();
        (st.snap.clone(), np_position(&st), st.np_zone, st.np_control, st.np_queue_index, st.np_cover.clone())
    });
    let music = snap.music();
    ui.set_media_np_active(music);
    if !music {
        return;
    }
    ui.set_media_np_playing(snap.state == "playing");
    ui.set_media_np_title(snap.title.clone().into());
    ui.set_media_np_artist(if snap.artist.is_empty() { tr(ui, 1120, "Unknown Artist") } else { snap.artist.clone() }.into());
    let fmt = format_label(&snap);
    ui.set_media_np_album(match (snap.album.is_empty(), fmt.is_empty()) {
        (false, false) => format!("{} • {}", snap.album, fmt),
        (false, true) => snap.album.clone(),
        _ => fmt,
    }.into());
    ui.set_media_np_position(pos as f32);
    ui.set_media_np_duration(snap.duration as f32);
    ui.set_media_np_elapsed(duration(pos).into());
    ui.set_media_np_total(if snap.duration > 0.0 { duration(snap.duration) } else { String::new() }.into());
    ui.set_media_np_shuffle(snap.shuffle);
    ui.set_media_np_repeat(match snap.repeat.as_str() { "all" => 1, "one" => 2, _ => 0 });
    ui.set_media_np_zone(zone);
    ui.set_media_np_control(control);
    ui.set_media_np_queue_index(qi as i32);
    match STATE.with(|st| match st.borrow().art.get(&cover) {
        Some(Art::Ready(img)) => Some(img.clone()),
        _ => None,
    }) {
        Some(img) => {
            ui.set_media_np_art(img);
            ui.set_media_np_has_art(true);
            if STATE.with(|st| st.borrow().np_open) {
                if let Some(bd) = backdrop(&cover) {
                    ui.set_media_backdrop(bd);
                    ui.set_media_has_backdrop(true);
                }
            }
        }
        None => ui.set_media_np_has_art(false),
    }
}

/* The cover of the playing track (folder picture or embedded). */
fn np_cover(ui: &HomeWindow, item: String) {
    let changed = STATE.with(|st| {
        let mut st = st.borrow_mut();
        if st.np_cover_item == item {
            return false;
        }
        st.np_cover_item = item.clone();
        st.np_cover.clear();
        true
    });
    if !changed || item.is_empty() {
        return;
    }
    let weak = ui.as_weak();
    thread::spawn(move || {
        let cover = if item.starts_with("http") {
            /* A Jellyfin stream: …/Items/<id>/Download… */
            item.split("/Items/").nth(1).and_then(|r| r.split('/').next()).map(|id| format!("jf:{id}")).unwrap_or_default()
        } else {
            run_tool(MEDIALIB, &["cover", &item], None).trim().strip_prefix("cover=").unwrap_or("").to_owned()
        };
        let _ = slint::invoke_from_event_loop(move || {
            let Some(ui) = weak.upgrade() else { return };
            let current = STATE.with(|st| {
                let mut st = st.borrow_mut();
                if st.np_cover_item != item {
                    return false;
                }
                st.np_cover = cover.clone();
                true
            });
            if current {
                request_art(&ui, &cover);
                refresh_np(&ui);
            }
        });
    });
}

/* One redraw a second for the progress, only while it is visible and
 * moving (Now Playing or the mini player, screen on). */
fn update_tick(ui: &HomeWindow) {
    let want = ACTIVE.load(Ordering::SeqCst) && MUSIC.load(Ordering::SeqCst) && !SCREEN_OFF.load(Ordering::SeqCst)
        && STATE.with(|st| st.borrow().snap.state == "playing");
    STATE.with(|st| {
        let mut st = st.borrow_mut();
        if !want {
            st.tick = None;
            return;
        }
        if st.tick.is_some() {
            return;
        }
        let weak = ui.as_weak();
        let timer = Timer::default();
        timer.start(TimerMode::Repeated, Duration::from_secs(1), move || {
            if let Some(ui) = weak.upgrade() {
                let pos = STATE.with(|st| np_position(&st.borrow()));
                ui.set_media_np_position(pos as f32);
                ui.set_media_np_elapsed(duration(pos).into());
            }
        });
        st.tick = Some(timer);
    });
}

/* Square screens show Now Playing stacked, without the queue list
 * (media_app.slint: content height > 0.8 × width). */
fn np_stacked(ui: &HomeWindow) -> bool {
    let scale = ui.window().scale_factor();
    let size = ui.window().size();
    let (w, h) = (size.width as f32 / scale, size.height as f32 / scale - 102.0);
    h > w * 0.8
}

fn np_action(ui: &HomeWindow, action: &str) {
    let (zone, control, qlen, qi, pos) = STATE.with(|st| {
        let st = st.borrow();
        (st.np_zone, st.np_control, st.queue.len(), st.np_queue_index, np_position(&st))
    });
    let set = |zone: i32, control: i32, qi: usize| {
        STATE.with(|st| {
            let mut st = st.borrow_mut();
            st.np_zone = zone;
            st.np_control = control;
            st.np_queue_index = qi;
        });
    };
    match action {
        "menu_up" => {
            if zone == Z_QUEUE && qi > 0 {
                set(zone, control, qi - 1);
            } else if zone == Z_QUEUE {
                set(Z_CONTROLS, control, qi);
            } else if zone > Z_PROGRESS {
                set(zone - 1, control, qi);
            }
        }
        "menu_down" => {
            if zone == Z_QUEUE {
                set(zone, control, (qi + 1).min(qlen.saturating_sub(1)));
            } else if zone == Z_PROGRESS {
                set(Z_CONTROLS, control, qi);
            } else if zone == Z_CONTROLS && qlen > 1 && !np_stacked(ui) {
                set(Z_QUEUE, control, qi);
            }
        }
        "menu_left" | "menu_right" => {
            let d = if action == "menu_left" { -1 } else { 1 };
            match zone {
                Z_PROGRESS => send(format!("SEEKTO\t{:.0}", (pos + 10.0 * d as f64).max(0.0))),
                Z_CONTROLS => set(zone, (control + d).clamp(0, 4), qi),
                _ => {}
            }
        }
        "menu_confirm" => match zone {
            Z_PROGRESS => send("PAUSE".into()),
            Z_CONTROLS => send(match control { 0 => "SHUFFLE", 1 => "PREV", 2 => "PAUSE", 3 => "NEXT", _ => "REPEAT" }.into()),
            _ => send(format!("JUMP\t{qi}")),
        },
        "menu_back" => back(ui),
        "face_west" => screen_off(ui),
        _ => {}
    }
    refresh_np(ui);
}

/* ---------------------------------------------------------------- */
/* Screen Off (the console as a music player)                        */
/* ---------------------------------------------------------------- */

fn screen_off(ui: &HomeWindow) {
    notice(ui, tr(ui, 1118, "The music keeps playing. Press POWER or START to turn the screen back on."));
    let weak = ui.as_weak();
    let timer = Timer::default();
    timer.start(TimerMode::SingleShot, Duration::from_millis(1800), move || {
        SCREEN_OFF.store(true, Ordering::SeqCst);
        let _ = run_tool(DISPLAYCTL, &["screen", "off"], None);
        if let Some(ui) = weak.upgrade() {
            ui.set_media_notice("".into());
            update_tick(&ui);
        }
    });
    STATE.with(|st| st.borrow_mut().notice_timer = Some(timer));
}

const SCREEN_OFF_FILE: &str = "/run/nuubos/screen-off";

/* Turns the screen back on (Start here, the end of playback). */
pub fn wake_screen(weak: &slint::Weak<HomeWindow>) -> bool {
    if !SCREEN_OFF.swap(false, Ordering::SeqCst) {
        return false;
    }
    thread::spawn(|| {
        let _ = run_tool(DISPLAYCTL, &["screen", "on"], None);
    });
    let weak = weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = weak.upgrade() {
            refresh_np(&ui);
            update_tick(&ui);
        }
    });
    true
}

/* Double L1 / double R1 = previous / next track while music plays, only
 * inside the Media app, screen on or off (user request 2026-10-14: not on
 * the games Home). A single press does nothing else there. */
const DOUBLE_TAP_MS: u128 = 450;
static LAST_PAGE_TAP: Mutex<Option<(bool, Instant)>> = Mutex::new(None);

pub fn music_skip_input(action: &str) -> bool {
    let next = match action {
        "page_next" => true,
        "page_prev" => false,
        _ => return false,
    };
    if !MUSIC.load(Ordering::SeqCst) || PLAYING_VIDEO.load(Ordering::SeqCst)
        || !(ACTIVE.load(Ordering::SeqCst) || SCREEN_OFF.load(Ordering::SeqCst))
    {
        return false;
    }
    let now = Instant::now();
    let mut last = LAST_PAGE_TAP.lock().unwrap_or_else(|e| e.into_inner());
    match *last {
        Some((side, at)) if side == next && now.duration_since(at).as_millis() <= DOUBLE_TAP_MS => {
            *last = None;
            send(if next { "NEXT" } else { "PREV" }.into());
        }
        _ => *last = Some((next, now)),
    }
    true
}

/* Screen Off, like Sleep (user request 2026-10-13): only Start (here) or
 * Power (the Quick Menu) turn the screen back on; every other button is
 * ignored. Called from route_input_action before anything else; true =
 * the action was consumed. */
pub fn screen_off_input(weak: &slint::Weak<HomeWindow>, action: &str) -> bool {
    if !SCREEN_OFF.load(Ordering::SeqCst) {
        return false;
    }
    if !Path::new(SCREEN_OFF_FILE).exists() {
        /* Woken elsewhere (Power in the Quick Menu, a Sleep). */
        SCREEN_OFF.store(false, Ordering::SeqCst);
        let weak = weak.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = weak.upgrade() {
                refresh_np(&ui);
                update_tick(&ui);
            }
        });
        return false;
    }
    if action == "settings" {
        wake_screen(weak);
    }
    true
}

/* ---------------------------------------------------------------- */
/* Video player input, keyboard results                              */
/* ---------------------------------------------------------------- */

/* Controller actions while a video plays (the player window covers
 * nuubUI): nuubos-mediad shows its on-screen controls on the first one and
 * acts on the next ones. */
pub fn handle_player(action: &str) {
    if matches!(action, "menu_confirm" | "menu_back" | "menu_left" | "menu_right" | "menu_up" | "menu_down"
        | "face_north" | "face_west") {
        send(format!("KEY\t{action}"));
    }
}

/* The video OSD's words and face-button positions (sent when a video
 * starts: language and mapping may have changed). */
fn send_osd_labels(ui: &HomeWindow) {
    let words = [tr(ui, 1133, "Pause"), tr(ui, 1164, "Play"), tr(ui, 1163, "Subtitles"), tr(ui, 1225, "Audio track"),
        tr(ui, 141, "Stop"), tr(ui, 54, "Off")];
    let pos = [ui.get_select_face_position(), ui.get_back_face_position(), ui.get_context_face_position(),
        ui.get_details_face_position()];
    let mut line = String::from("LABELS");
    for w in words {
        line.push('\t');
        line.push_str(&w.replace(['\t', '\n'], " "));
    }
    for p in pos {
        line.push_str(&format!("\t{}", p.clamp(0, 3)));
    }
    send(line);
}

/* System keyboard results (Jellyfin sign-in, playlist names). */
pub fn keyboard_done(ui: &HomeWindow, purpose: i32, value: String) {
    match purpose {
        KEYBOARD_SERVER => {
            let v = value.trim().trim_end_matches('/').to_owned();
            if v.len() <= 8 {
                navigate_settings_view(ui, VIEW);
                show_view(ui);
                return;
            }
            STATE.with(|st| st.borrow_mut().draft.0 = v);
            open_system_keyboard(ui, &tr(ui, 285, "Username"), KEYBOARD_JF_USER, VIEW, "text", "");
        }
        KEYBOARD_JF_USER => {
            STATE.with(|st| st.borrow_mut().draft.1 = value.trim().to_owned());
            open_system_keyboard(ui, &tr(ui, 172, "Password"), KEYBOARD_JF_PASSWORD, VIEW, "password", "");
        }
        KEYBOARD_JF_PASSWORD => {
            ui.set_keyboard_value("".into());
            navigate_settings_view(ui, VIEW);
            show_view(ui);
            let (server, name) = STATE.with(|st| std::mem::take(&mut st.borrow_mut().draft));
            let user = ui.get_active_user_id().to_string();
            tool_then(ui, JELLYFIN, vec!["login".into(), user, server, name], Some(value), |ui, out| {
                if out.starts_with("ERR auth") {
                    notice(ui, tr(ui, 589, "Sign-in failed"));
                } else if out.starts_with("ERR") {
                    notice(ui, tr(ui, 668, "The server could not be reached"));
                }
                load_home(ui);
            });
        }
        KEYBOARD_PLAYLIST => {
            navigate_settings_view(ui, VIEW);
            show_view(ui);
            let name = value.trim().to_owned();
            if name.is_empty() {
                return;
            }
            let user = ui.get_active_user_id().to_string();
            let paths = STATE.with(|st| std::mem::take(&mut st.borrow_mut().pending_add));
            let weak = ui.as_weak();
            thread::spawn(move || {
                let out = run_tool(MEDIALIB, &["playlist-new", &user, &name], None);
                let file = out.trim().strip_prefix("OK ").map(str::to_owned);
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(ui) = weak.upgrade() else { return };
                    match file {
                        Some(f) if !paths.is_empty() => add_to_playlist(&ui, f, paths),
                        Some(_) => load_home(&ui),
                        None => notice(&ui, tr(&ui, 93, "Failed")),
                    }
                });
            });
        }
        KEYBOARD_PLAYLIST_RENAME => {
            navigate_settings_view(ui, VIEW);
            let file = STATE.with(|st| std::mem::take(&mut st.borrow_mut().renaming));
            let name = value.trim().to_owned();
            if name.is_empty() || file.is_empty() {
                show_view(ui);
                return;
            }
            let weak = ui.as_weak();
            thread::spawn(move || {
                let out = run_tool(MEDIALIB, &["playlist-rename", &file, &name], None);
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(ui) = weak.upgrade() else { return };
                    if let Some(to) = out.trim().strip_prefix("OK ") {
                        STATE.with(|st| {
                            if let Some(View::Album { source, title, .. }) = st.borrow_mut().views.last_mut() {
                                if *source == AlbumSource::Playlist(file.clone()) {
                                    *source = AlbumSource::Playlist(to.to_owned());
                                    *title = name.clone();
                                }
                            }
                        });
                    } else {
                        notice(&ui, tr(&ui, 93, "Failed"));
                    }
                    show_view(&ui);
                    load_home(&ui);
                });
            });
        }
        _ => {}
    }
}

/* ---------------------------------------------------------------- */
/* nuubos-mediad listener                                            */
/* ---------------------------------------------------------------- */

fn on_snapshot(ui: &HomeWindow, snap: Snapshot, last_item: String, last_position: String) {
    let was = STATE.with(|st| st.borrow().snap.clone());
    let video = snap.active() && snap.kind == "video" || (snap.state == "stopping" && was.kind == "video");
    let music = snap.music();
    let active = snap.active() || snap.state == "stopping";
    PLAYING.store(active, Ordering::SeqCst);
    let video_was = PLAYING_VIDEO.swap(video, Ordering::SeqCst);
    let music_was = MUSIC.swap(music, Ordering::SeqCst);
    /* A video is like a game for Home (input, Home Music). */
    if video != video_was {
        on_game_session(ui, video);
        if video {
            send_osd_labels(ui);
        }
    }
    /* Music: Home Music stops for it and comes back after it. */
    if music && !music_was {
        thread::spawn(crate::stop_home_music_session);
    } else if !music && music_was && !video && !crate::library::GAME_RUNNING.load(Ordering::SeqCst) {
        thread::spawn(crate::start_home_music_session);
    }
    if !active && was.active() {
        let item = std::mem::take(&mut *JELLYFIN_ITEM.lock().unwrap());
        if !item.is_empty() && last_item.starts_with("http") && last_position.parse::<f64>().unwrap_or(-1.0) >= 0.0 {
            let user = ui.get_active_user_id().to_string();
            thread::spawn(move || {
                let _ = run_tool(JELLYFIN, &["progress", &user, &item, &last_position], None);
            });
        }
        if SCREEN_OFF.load(Ordering::SeqCst) {
            /* Nothing plays any more: the screen comes back. */
            wake_screen(&ui.as_weak());
        }
    }
    let reload = !active && was.kind == "video" && was.active() && ACTIVE.load(Ordering::SeqCst);
    let queue_changed = STATE.with(|st| {
        let mut st = st.borrow_mut();
        let changed = music && snap.queue_id != st.queue_id;
        if !music && st.np_open {
            st.np_open = false;
        }
        if snap.index != was.index {
            st.np_queue_index = snap.index.max(0) as usize;
        }
        st.snap = snap.clone();
        st.snap_at = Some(Instant::now());
        changed
    });
    np_cover(ui, if music { snap.item.clone() } else { String::new() });
    if ACTIVE.load(Ordering::SeqCst) {
        if queue_changed && STATE.with(|st| st.borrow().np_open) {
            fetch_queue(ui);
        }
        if was.music() != music {
            show_view(ui);
        } else {
            refresh_np(ui);
        }
        mark_current(ui);
        hints(ui);
    }
    update_tick(ui);
    if reload {
        /* Resume positions changed. */
        load_home(ui);
    }
}

pub fn start_listener(ui: &HomeWindow) {
    start_art_loader(ui);
    let weak = ui.as_weak();
    thread::spawn(move || loop {
        if let Ok(mut stream) = UnixStream::connect(MEDIA_SOCKET) {
            if stream.write_all(b"SUBSCRIBE\n").is_ok() {
                let mut block = String::new();
                for line in BufReader::new(stream).lines() {
                    let Ok(line) = line else { break };
                    block.push_str(&line);
                    block.push('\n');
                    if line != "end=1" {
                        continue;
                    }
                    let snap = parse_snapshot(&block);
                    let last_item = block.lines().find_map(|l| l.strip_prefix("last_item=")).unwrap_or("").to_owned();
                    let last_position = block.lines().find_map(|l| l.strip_prefix("last_position=")).unwrap_or("").to_owned();
                    block.clear();
                    let weak = weak.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = weak.upgrade() {
                            on_snapshot(&ui, snap, last_item, last_position);
                        }
                    });
                }
            }
        }
        let weak = weak.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = weak.upgrade() {
                on_snapshot(&ui, Snapshot { state: "idle".into(), ..Default::default() }, String::new(), String::new());
            }
        });
        thread::sleep(Duration::from_millis(500));
    });
}
