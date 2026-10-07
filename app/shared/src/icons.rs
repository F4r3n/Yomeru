//! Inline SVG icons, so every icon shares one stroke style and follows the
//! surrounding text colour (`currentColor`) through themes, hover and
//! active states — which the Unicode glyphs they replace did unevenly
//! across platform fonts.
//!
//! Path data is from Lucide (https://lucide.dev), ISC License:
//! Copyright (c) for portions of Lucide are held by Cole Bemis 2013-2022 as
//! part of Feather (MIT). All other copyright (c) for Lucide are held by
//! Lucide Contributors 2022. Permission to use, copy, modify, and/or
//! distribute this software for any purpose with or without fee is hereby
//! granted, provided that the above copyright notice and this permission
//! notice appear in all copies.

use dioxus::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconName {
    Play,
    Sparkles,
    List,
    Chart,
    Search,
    Settings,
    Info,
    Sun,
    Moon,
    Eye,
    EyeOff,
    Loader,
    CircleCheck,
}

impl IconName {
    /// The SVG children for a 24×24, stroke-only icon.
    fn body(self) -> &'static str {
        match self {
            Self::Play => r#"<polygon points="6 3 20 12 6 21 6 3"/>"#,
            Self::Sparkles => concat!(
                r#"<path d="M9.937 15.5A2 2 0 0 0 8.5 14.063l-6.135-1.582a.5.5 0 0 1 0-.962L8.5 9.936A2 2 0 0 0 9.937 8.5l1.582-6.135a.5.5 0 0 1 .963 0L14.063 8.5A2 2 0 0 0 15.5 9.937l6.135 1.581a.5.5 0 0 1 0 .964L15.5 14.063a2 2 0 0 0-1.437 1.437l-1.582 6.135a.5.5 0 0 1-.963 0z"/>"#,
                r#"<path d="M20 3v4"/><path d="M22 5h-4"/><path d="M4 17v2"/><path d="M5 18H3"/>"#,
            ),
            Self::List => concat!(
                r#"<path d="M3 12h.01"/><path d="M3 18h.01"/><path d="M3 6h.01"/>"#,
                r#"<path d="M8 12h13"/><path d="M8 18h13"/><path d="M8 6h13"/>"#,
            ),
            Self::Chart => concat!(
                r#"<path d="M3 3v16a2 2 0 0 0 2 2h16"/>"#,
                r#"<path d="M18 17V9"/><path d="M13 17V5"/><path d="M8 17v-3"/>"#,
            ),
            Self::Search => r#"<circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/>"#,
            Self::Settings => concat!(
                r#"<path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"/>"#,
                r#"<circle cx="12" cy="12" r="3"/>"#,
            ),
            Self::Info => {
                r#"<circle cx="12" cy="12" r="10"/><path d="M12 16v-4"/><path d="M12 8h.01"/>"#
            }
            Self::Sun => concat!(
                r#"<circle cx="12" cy="12" r="4"/>"#,
                r#"<path d="M12 2v2"/><path d="M12 20v2"/>"#,
                r#"<path d="m4.93 4.93 1.41 1.41"/><path d="m17.66 17.66 1.41 1.41"/>"#,
                r#"<path d="M2 12h2"/><path d="M20 12h2"/>"#,
                r#"<path d="m6.34 17.66-1.41 1.41"/><path d="m19.07 4.93-1.41 1.41"/>"#,
            ),
            Self::Moon => r#"<path d="M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9Z"/>"#,
            Self::Eye => concat!(
                r#"<path d="M2.062 12.348a1 1 0 0 1 0-.696 10.75 10.75 0 0 1 19.876 0 1 1 0 0 1 0 .696 10.75 10.75 0 0 1-19.876 0"/>"#,
                r#"<circle cx="12" cy="12" r="3"/>"#,
            ),
            Self::EyeOff => concat!(
                r#"<path d="M10.733 5.076a10.744 10.744 0 0 1 11.205 6.575 1 1 0 0 1 0 .696 10.747 10.747 0 0 1-1.444 2.49"/>"#,
                r#"<path d="M14.084 14.158a3 3 0 0 1-4.242-4.242"/>"#,
                r#"<path d="M17.479 17.499a10.75 10.75 0 0 1-15.417-5.151 1 1 0 0 1 0-.696 10.75 10.75 0 0 1 4.446-5.143"/>"#,
                r#"<path d="m2 2 20 20"/>"#,
            ),
            Self::Loader => r#"<path d="M21 12a9 9 0 1 1-6.219-8.56"/>"#,
            Self::CircleCheck => r#"<circle cx="12" cy="12" r="10"/><path d="m9 12 2 2 4-4"/>"#,
        }
    }
}

/// A stroke icon sized in CSS pixels. Decorative (`aria-hidden`): callers put
/// the accessible name on the surrounding button or link.
#[component]
pub fn Icon(name: IconName, #[props(default = 16)] size: u32) -> Element {
    rsx! {
        svg {
            class: "icon",
            width: "{size}",
            height: "{size}",
            view_box: "0 0 24 24",
            fill: "none",
            stroke: "currentColor",
            stroke_width: "2",
            stroke_linecap: "round",
            stroke_linejoin: "round",
            "aria-hidden": "true",
            dangerous_inner_html: name.body(),
        }
    }
}
