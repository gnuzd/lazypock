//! The app's icon set: one inline SVG each, drawn with `currentColor`.

use nsh_core::element::ImageSrc;
use nsh_core::prelude::*;

macro_rules! svg_icon {
    ($($body:literal)*) => {
        concat!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">"#,
            $($body,)*
            "</svg>"
        )
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Overview,
    Collections,
    Logs,
    Settings,
    Search,
    Logout,
    ChevronRight,
    Plus,
    Close,
}

impl Icon {
    fn markup(self) -> &'static str {
        match self {
            Icon::Overview => svg_icon!(
                r#"<rect x="3" y="3" width="7" height="7" rx="1"/>"#
                r#"<rect x="14" y="3" width="7" height="7" rx="1"/>"#
                r#"<rect x="14" y="14" width="7" height="7" rx="1"/>"#
                r#"<rect x="3" y="14" width="7" height="7" rx="1"/>"#
            ),
            Icon::Collections => svg_icon!(
                r#"<ellipse cx="12" cy="5" rx="9" ry="3"/>"#
                r#"<path d="M21 12c0 1.66-4 3-9 3s-9-1.34-9-3"/>"#
                r#"<path d="M3 5v14c0 1.66 4 3 9 3s9-1.34 9-3V5"/>"#
            ),
            Icon::Logs => svg_icon!(
                r#"<line x1="8" y1="6" x2="21" y2="6"/>"#
                r#"<line x1="8" y1="12" x2="21" y2="12"/>"#
                r#"<line x1="8" y1="18" x2="21" y2="18"/>"#
                r#"<line x1="3" y1="6" x2="3.01" y2="6"/>"#
                r#"<line x1="3" y1="12" x2="3.01" y2="12"/>"#
                r#"<line x1="3" y1="18" x2="3.01" y2="18"/>"#
            ),
            Icon::Settings => svg_icon!(
                r#"<line x1="4" y1="21" x2="4" y2="14"/>"#
                r#"<line x1="4" y1="10" x2="4" y2="3"/>"#
                r#"<line x1="12" y1="21" x2="12" y2="12"/>"#
                r#"<line x1="12" y1="8" x2="12" y2="3"/>"#
                r#"<line x1="20" y1="21" x2="20" y2="16"/>"#
                r#"<line x1="20" y1="12" x2="20" y2="3"/>"#
                r#"<line x1="1" y1="14" x2="7" y2="14"/>"#
                r#"<line x1="9" y1="8" x2="15" y2="8"/>"#
                r#"<line x1="17" y1="16" x2="23" y2="16"/>"#
            ),
            Icon::Search => svg_icon!(
                r#"<circle cx="11" cy="11" r="8"/>"#
                r#"<line x1="21" y1="21" x2="16.65" y2="16.65"/>"#
            ),
            Icon::Logout => svg_icon!(
                r#"<path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4"/>"#
                r#"<polyline points="16 17 21 12 16 7"/>"#
                r#"<line x1="21" y1="12" x2="9" y2="12"/>"#
            ),
            Icon::ChevronRight => svg_icon!(r#"<polyline points="9 18 15 12 9 6"/>"#),
            Icon::Plus => svg_icon!(r#"<path d="M12 5v14"/><path d="M5 12h14"/>"#),
            Icon::Close => svg_icon!(
                r#"<path d="M18 6 6 18"/>"#
                r#"<path d="m6 6 12 12"/>"#
            ),
        }
    }
}

/// `name`, drawn at `size` points in the surrounding text color.
pub fn icon(name: Icon, size: f32) -> Svg {
    svg(ImageSrc::svg_markup(name.markup())).size(px(size))
}
