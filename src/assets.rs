//! Icons, built into the binary. gpui draws an SVG as a mask in the current text color, so only
//! the shapes matter.

use anyhow::Result;
use gpui::{AssetSource, SharedString};
use std::borrow::Cow;

/// name, stroke width, line cap, body
const ICONS: &[(&str, f32, &str, &str)] = &[
    ("logo", 1.75, "round", r#"<circle cx="12" cy="12" r="7"/><path d="M12 2v5M12 17v5M2 12h5M17 12h5"/>"#),
    ("layers", 1.75, "round", r#"<path d="M12 4l8 4.5-8 4.5-8-4.5z"/><path d="M4 12.5l8 4.5 8-4.5"/><path d="M4 16.5l8 4.5 8-4.5"/>"#),
    ("gallery", 1.75, "round", r#"<rect x="4" y="4" width="6.5" height="6.5" rx="1.5"/><rect x="13.5" y="4" width="6.5" height="6.5" rx="1.5"/><rect x="4" y="13.5" width="6.5" height="6.5" rx="1.5"/><rect x="13.5" y="13.5" width="6.5" height="6.5" rx="1.5"/>"#),
    ("settings", 1.75, "round", r#"<path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"/><circle cx="12" cy="12" r="3"/>"#),
    ("info", 1.75, "round", r#"<circle cx="12" cy="12" r="9"/><path d="M12 11v5.5M12 7.6v.01"/>"#),
    ("external", 1.75, "round", r#"<path d="M8 16L16 8M9.5 8H16v6.5"/>"#),
    ("editor", 1.75, "round", r#"<path d="M9.06 11.9l8.07-8.06a2.85 2.85 0 1 1 4.03 4.03l-8.06 8.08"/><path d="M7.07 14.94c-1.66 0-3 1.35-3 3.02 0 1.33-2.5 1.52-2 2.02 1.08 1.1 2.49 2.02 4 2.02 2.2 0 4-1.8 4-4.04a3.01 3.01 0 0 0-3-3.02z"/>"#),
    ("minimize", 1.75, "round", r#"<path d="M5 12h14"/>"#),
    ("maximize", 1.75, "round", r#"<rect x="4.5" y="4.5" width="15" height="15" rx="2"/>"#),
    ("close", 1.75, "round", r#"<path d="M6 6l12 12M18 6L6 18"/>"#),
    ("chevron", 2.5, "round", r#"<path d="M6 9l6 6 6-6"/>"#),
    ("grip", 0.0, "round", r#"<g fill="black" stroke="none"><circle cx="9" cy="6" r="1.4"/><circle cx="15" cy="6" r="1.4"/><circle cx="9" cy="12" r="1.4"/><circle cx="15" cy="12" r="1.4"/><circle cx="9" cy="18" r="1.4"/><circle cx="15" cy="18" r="1.4"/></g>"#),
    ("pencil", 1.75, "round", r#"<path d="M4 20h4L19 9l-4-4L4 16v4z"/><path d="M13.5 6.5l4 4"/>"#),
    ("trash", 1.75, "round", r#"<path d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13M10 11v6M14 11v6"/>"#),
    ("plus", 2.25, "round", r#"<path d="M12 5v14M5 12h14"/>"#),
    ("minus", 2.0, "round", r#"<path d="M5 12h14"/>"#),
    ("check", 2.25, "round", r#"<path d="M5 12.5l4.5 4.5L19 7.5"/>"#),
    ("folder", 1.75, "round", r#"<path d="M3 6.5A1.5 1.5 0 0 1 4.5 5h4.6l2 2.2h8.4A1.5 1.5 0 0 1 21 8.7v9.8a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 18.5z"/>"#),
    ("back", 1.75, "round", r#"<path d="M19 12H5M11 6l-6 6 6 6"/>"#),
    ("shape-cross", 1.75, "square", r#"<path d="M12 3v6M12 15v6M3 12h6M15 12h6"/>"#),
    ("shape-t", 1.75, "square", r#"<path d="M12 15v6M3 12h6M15 12h6"/>"#),
    ("shape-circle", 1.75, "square", r#"<path d="M5 12a7 7 0 1 0 14 0a7 7 0 1 0 -14 0"/>"#),
    ("shape-dot", 1.75, "square", r#"<path d="M10.5 10.5h3v3h-3z"/>"#),
    ("shape-custom", 1.75, "square", r#"<path d="M4 4h4v4H4zM10 10h4v4h-4zM16 4h4v4h-4zM4 16h4v4H4zM16 16h4v4h-4z"/>"#),
];

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        let name = path.strip_prefix("icons/").and_then(|p| p.strip_suffix(".svg"));
        let icon = name.and_then(|name| ICONS.iter().find(|i| i.0 == name));
        Ok(icon.map(|&(_, width, cap, body)| {
            let svg = format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="{width}" stroke-linecap="{cap}" stroke-linejoin="round">{body}</svg>"#
            );
            Cow::Owned(svg.into_bytes())
        }))
    }

    fn list(&self, _path: &str) -> Result<Vec<SharedString>> {
        Ok(ICONS.iter().map(|i| format!("icons/{}.svg", i.0).into()).collect())
    }
}
