//! Icon assets: the gpui-kit Lucide subset plus a few icons it lacks.
use gpui_kit::{AssetSource, SharedString};
use std::borrow::Cow;

const SVG_HEAD: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">"#;

const EXTRA: &[(&str, &str)] = &[
    ("icons/history.svg", r#"<path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5"/><path d="M12 7v5l4 2"/>"#),
    ("icons/trash-2.svg", r#"<path d="M3 6h18"/><path d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6"/><path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2"/><line x1="10" x2="10" y1="11" y2="17"/><line x1="14" x2="14" y1="11" y2="17"/>"#),
];

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        // The official YouTube icon, also shown in the update/About hero.
        if path == "brand/youtube.svg" {
            return Ok(Some(Cow::Borrowed(include_bytes!("../assets/youtube.svg"))));
        }
        if let Some((_, body)) = EXTRA.iter().find(|(p, _)| *p == path) {
            return Ok(Some(Cow::Owned(format!("{SVG_HEAD}{body}</svg>").into_bytes())));
        }
        gpui_kit::assets::AllAssets.load(path)
    }

    fn list(&self, path: &str) -> anyhow::Result<Vec<SharedString>> {
        gpui_kit::assets::AllAssets.list(path)
    }
}
