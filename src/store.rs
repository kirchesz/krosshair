//! Files on disk: the configuration and the gallery folder with crosshair PNGs and GIFs.

use crate::model::{presets, Config, LayerRef, Profile, TrayMode};
use crate::raster::{self, Anim, Bitmap, Picture};
use anyhow::{Context as _, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// `%APPDATA%\krosshair\config.json`, or the file named by the `KROSSHAIR_CONFIG` environment
/// variable (used for testing with a separate configuration).
fn config_path() -> PathBuf {
    if let Some(path) = std::env::var_os("KROSSHAIR_CONFIG") {
        return PathBuf::from(path);
    }
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    base.join("krosshair").join("config.json")
}

/// `Documents\Krosshair\crosshairs`
fn default_gallery_dir() -> PathBuf {
    let docs = documents_dir().unwrap_or_else(|| {
        std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(".")).join("Documents")
    });
    docs.join("Krosshair").join("crosshairs")
}

fn documents_dir() -> Option<PathBuf> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{FOLDERID_Documents, SHGetKnownFolderPath, KF_FLAG_DEFAULT};
    unsafe {
        let p = SHGetKnownFolderPath(&FOLDERID_Documents, KF_FLAG_DEFAULT, None).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as _));
        s.map(PathBuf::from)
    }
}

fn default_config() -> Config {
    Config {
        gallery_dir: default_gallery_dir(),
        mask: 0xffffff,
        power: true,
        scale: 1.0,
        rotation: 0,
        tray: TrayMode::OnMinimize,
        tray_asked: false,
        check_updates: true,
        current: 0,
        profiles: vec![Profile {
            name: "Default".into(),
            layers: vec![LayerRef::new("cross-gap.png".into(), 4)],
        }],
    }
}

/// A configuration from its file. Files from before the tray setting had three choices carry
/// `hide_to_tray: false` for "never"; true, or no value, was "on minimize", the default.
fn parse_config(text: &str) -> Option<Config> {
    let mut value: serde_json::Value = serde_json::from_str(text).ok()?;
    if let Some(map) = value.as_object_mut()
        && !map.contains_key("tray")
        && let Some(old) = map.remove("hide_to_tray")
        && old == serde_json::Value::Bool(false)
    {
        map.insert("tray".into(), serde_json::to_value(TrayMode::Never).ok()?);
    }
    serde_json::from_value(value).ok()
}

/// Reads the configuration. On the first start it creates the default one and fills the gallery
/// folder with the preset crosshairs.
pub fn load() -> Config {
    let path = config_path();
    let text = fs::read_to_string(&path).ok();
    let loaded = text.as_deref().and_then(parse_config);
    let mut cfg = match loaded {
        Some(cfg) => cfg,
        None => {
            if text.is_some() {
                // The file is there but unreadable. Keep it aside instead of overwriting it.
                let _ = fs::rename(&path, path.with_extension("broken.json"));
            }
            let cfg = default_config();
            if !cfg.gallery_dir.exists() {
                seed_presets(&cfg.gallery_dir);
            }
            cfg
        }
    };
    if cfg.profiles.is_empty() {
        cfg.profiles.push(Profile { name: "Default".into(), layers: Vec::new() });
    }
    cfg.current = cfg.current.min(cfg.profiles.len() - 1);
    cfg
}

pub fn save(cfg: &Config) -> Result<()> {
    let path = config_path();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(&path, serde_json::to_string_pretty(cfg)?).with_context(|| format!("can't write {}", path.display()))
}

fn seed_presets(dir: &Path) {
    if fs::create_dir_all(dir).is_err() {
        return;
    }
    for (name, layers) in presets() {
        let _ = raster::render(&layers).save_png(&dir.join(format!("{name}.png")));
    }
}

#[derive(Clone)]
pub struct GalleryItem {
    pub file: String,
    pub path: PathBuf,
    /// The picture, or the first frame of an animation.
    pub bmp: Arc<Bitmap>,
    pub anim: Option<Arc<Anim>>,
}

impl GalleryItem {
    pub fn picture(&self) -> Picture {
        Picture { still: self.bmp.clone(), anim: self.anim.clone() }
    }
}

/// Every readable PNG and GIF in the folder, sorted by name. A GIF with one frame, or one too
/// big to animate, is a still picture.
pub fn scan_gallery(dir: &Path) -> Vec<GalleryItem> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    let mut items: Vec<GalleryItem> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let path = e.path();
            let ext = path.extension()?.to_str()?.to_ascii_lowercase();
            let anim = match ext.as_str() {
                "png" => None,
                "gif" => Anim::load_gif(&path).map(Arc::new),
                _ => return None,
            };
            let bmp = match &anim {
                Some(anim) => anim.frames[0].clone(),
                None => Arc::new(Bitmap::load(&path).ok()?),
            };
            let file = e.file_name().to_str()?.to_owned();
            Some(GalleryItem { file, path, bmp, anim })
        })
        .collect();
    items.sort_by_key(|i| i.file.to_lowercase());
    items
}

/// Saves a new crosshair as `crosshair-NNN.png` with the first free number and returns the name.
pub fn save_crosshair(dir: &Path, bmp: &Bitmap) -> Result<String> {
    fs::create_dir_all(dir).with_context(|| format!("can't create {}", dir.display()))?;
    for n in 1..10_000 {
        let name = format!("crosshair-{n:03}.png");
        let path = dir.join(&name);
        if !path.exists() {
            bmp.save_png(&path)?;
            return Ok(name);
        }
    }
    anyhow::bail!("no free file name in {}", dir.display())
}

/// What [`import`] did with each file.
#[derive(Debug, Default, PartialEq)]
pub struct Imported {
    /// Names the new files got in the gallery folder.
    pub added: Vec<String>,
    /// Files already in the gallery: the same file, or one with the same content.
    pub present: Vec<String>,
    /// Files left out, with why.
    pub skipped: Vec<(String, &'static str)>,
}

/// Copies PNG and GIF files into the gallery folder. A name already taken by a different file
/// gets " (2)", " (3)" and so on; a file with the same content as one already there is not
/// copied again.
pub fn import(dir: &Path, paths: &[PathBuf]) -> Imported {
    let mut out = Imported::default();
    if fs::create_dir_all(dir).is_err() {
        out.skipped = paths.iter().map(|p| (display_name(p), "the gallery folder can't be created")).collect();
        return out;
    }
    for path in paths {
        let name = display_name(path);
        let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
        if !matches!(ext.as_deref(), Some("png" | "gif")) {
            out.skipped.push((name, if path.is_dir() { "a folder" } else { "not a PNG or GIF" }));
            continue;
        }
        let Ok(bytes) = fs::read(path) else {
            out.skipped.push((name, "can't be read"));
            continue;
        };
        if image::load_from_memory(&bytes).is_err() {
            out.skipped.push((name, "not a picture"));
            continue;
        }
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("crosshair").to_owned();
        let ext = ext.unwrap();
        let mut copied = false;
        for n in 1..1000 {
            let file = if n == 1 { format!("{stem}.{ext}") } else { format!("{stem} ({n}).{ext}") };
            let target = dir.join(&file);
            if target.exists() {
                if fs::read(&target).is_ok_and(|have| have == bytes) {
                    out.present.push(file);
                    copied = true;
                    break;
                }
                continue;
            }
            match fs::write(&target, &bytes) {
                Ok(()) => out.added.push(file),
                Err(_) => out.skipped.push((name.clone(), "can't be written to the gallery folder")),
            }
            copied = true;
            break;
        }
        if !copied {
            out.skipped.push((name, "no free file name"));
        }
    }
    out
}

fn display_name(path: &Path) -> String {
    path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
}

/// Moves the crosshair file to the Recycle Bin.
pub fn delete_crosshair(dir: &Path, file: &str) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::{
        SHFileOperationW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FO_DELETE, SHFILEOPSTRUCTW,
    };
    let path = dir.join(file);
    // The path list ends with two zero characters.
    let from: Vec<u16> = path.as_os_str().encode_wide().chain([0, 0]).collect();
    let mut op = SHFILEOPSTRUCTW {
        wFunc: FO_DELETE,
        pFrom: PCWSTR(from.as_ptr()),
        fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_SILENT).0 as u16,
        ..Default::default()
    };
    let code = unsafe { SHFileOperationW(&mut op) };
    anyhow::ensure!(code == 0 && !path.exists(), "can't delete {}", path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh empty folder for a test, removed when it ends.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            let dir = std::env::temp_dir().join(format!("krosshair-test-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn png(path: &Path, color: [u8; 4]) {
        let mut b = Bitmap::new(2, 2);
        b.px.fill(color);
        b.save_png(path).unwrap();
    }

    #[test]
    fn tray_setting_from_old_and_new_files() {
        let with = |extra: &str| {
            let mut json = serde_json::to_value(default_config()).unwrap();
            let map = json.as_object_mut().unwrap();
            map.remove("tray");
            if !extra.is_empty() {
                let extra: serde_json::Value = serde_json::from_str(extra).unwrap();
                map.extend(extra.as_object().unwrap().clone());
            }
            parse_config(&json.to_string()).map(|cfg| cfg.tray)
        };
        // Older files: the yes/no setting.
        assert_eq!(with(r#"{"hide_to_tray": false}"#), Some(TrayMode::Never));
        assert_eq!(with(r#"{"hide_to_tray": true}"#), Some(TrayMode::OnMinimize));
        assert_eq!(with(""), Some(TrayMode::OnMinimize));
        // Newer files, and a new value wins over a leftover old one.
        assert_eq!(with(r#"{"tray": "on_close"}"#), Some(TrayMode::OnClose));
        assert_eq!(with(r#"{"tray": "never", "hide_to_tray": true}"#), Some(TrayMode::Never));
        // What is written reads back the same.
        let mut cfg = default_config();
        cfg.tray = TrayMode::OnClose;
        cfg.tray_asked = true;
        assert_eq!(parse_config(&serde_json::to_string(&cfg).unwrap()), Some(cfg));
    }

    #[test]
    fn layers_from_older_files_have_no_scale_or_tint_of_their_own() {
        let mut json = serde_json::to_value(default_config()).unwrap();
        let layer = &mut json["profiles"][0]["layers"][0];
        layer.as_object_mut().unwrap().remove("scale");
        layer.as_object_mut().unwrap().remove("mask");
        let cfg = parse_config(&json.to_string()).unwrap();
        let layer = &cfg.profiles[0].layers[0];
        assert_eq!((layer.scale, layer.mask), (1.0, crate::model::WHITE));
    }

    #[test]
    fn files_without_the_tray_answer_ask_once() {
        let mut json = serde_json::to_value(default_config()).unwrap();
        json.as_object_mut().unwrap().remove("tray_asked");
        assert_eq!(parse_config(&json.to_string()).map(|cfg| cfg.tray_asked), Some(false));
        assert!(!default_config().tray_asked);
    }

    #[test]
    fn import_copies_pictures_and_explains_the_rest() {
        let src = Scratch::new("src");
        let gallery = Scratch::new("gallery");
        png(&src.0.join("a.png"), [255, 0, 0, 255]);
        png(&src.0.join("B.PNG"), [0, 255, 0, 255]);
        fs::write(src.0.join("notes.txt"), "hi").unwrap();
        fs::write(src.0.join("fake.png"), "not a png").unwrap();
        let paths: Vec<PathBuf> = ["a.png", "B.PNG", "notes.txt", "fake.png"].iter().map(|n| src.0.join(n)).collect();
        let r = import(&gallery.0, &paths);
        assert_eq!(r.added, ["a.png", "B.png"]);
        assert_eq!(r.skipped, [("notes.txt".to_string(), "not a PNG or GIF"), ("fake.png".to_string(), "not a picture")]);
        assert_eq!(scan_gallery(&gallery.0).len(), 2);

        // The same file again is already there; a different one with a taken name gets " (2)".
        png(&src.0.join("a.png"), [0, 0, 255, 255]);
        let r = import(&gallery.0, &[gallery.0.join("B.png"), src.0.join("a.png")]);
        assert_eq!(r.present, ["B.png"]);
        assert_eq!(r.added, ["a (2).png"]);
    }
}