//! The Dapple document theme — one source of truth for every color, size, and
//! brand constant used by both output emitters (Typst → PDF and HTML).
//!
//! Palette provenance: `--dapple-green` and the slate ink ramp mirror the
//! dapple-ui theme tokens (`packages/ui-demo/app/globals.css`) so a rendered
//! report reads as the same product as the journey panes. Admonition colors
//! follow the ui `NoticeBlock` convention (colored rail + pale tint) extended
//! across the twelve mdbook-admonish directive families.

use crate::admonish::AdmonitionKind;

/// Dapple brand green (`--dapple-green` in dapple-ui). Title, H1/H2, links,
/// rails, and the divider motif.
pub const DAPPLE_GREEN: &str = "#49a013";

/// Ink ramp — Tailwind slate, the dapple-ui prose colors.
pub const SLATE_800: &str = "#1d293d";
pub const SLATE_700: &str = "#314158";
pub const SLATE_600: &str = "#45556c";
pub const SLATE_500: &str = "#62748e";
pub const SLATE_300: &str = "#cad5e2";
pub const SLATE_200: &str = "#e2e8f0";
pub const SLATE_100: &str = "#f1f5f9";
pub const SLATE_50: &str = "#f8fafc";

/// Closing colophon line, appended to every rendered document.
pub const COLOPHON_TEXT: &str = "Contextualized by Dapple AI";

/// Series palette for rendered charts ([`crate::chart`]), cycled in order.
///
/// Brand green leads; the rest are the admonition rail colors, so a chart sits
/// beside a callout without introducing a second palette. Ordered for contrast
/// between adjacent series rather than by hue.
pub const CHART_SERIES_COLORS: &[&str] = &[
    DAPPLE_GREEN,
    "#3b82f6", // note blue
    "#f59e0b", // warning amber
    "#7c3aed", // question violet
    "#0891b2", // abstract cyan
    "#e11d48", // failure rose
];

/// Embedded font family names as registered with the Typst `FontBook`.
/// (The HTML emitter uses a system font stack instead — no font embedding.)
pub const FONT_SERIF: &str = "DejaVu Serif";
pub const FONT_SANS: &str = "Liberation Sans";
pub const FONT_MONO: &str = "DejaVu Sans Mono";

/// Typographic settings for a branding key. Pure data — the Typst preamble and
/// the HTML stylesheet both read from it. Sizes are points.
#[derive(Clone, Debug)]
pub struct BrandStyle {
    pub body_font_size: u8,
    pub heading_font_size: u8,
    pub subheading_font_size: u8,
    pub h3_font_size: u8,
    pub h4_font_size: u8,
    pub h5_font_size: u8,
    pub h6_font_size: u8,
    pub title_font_size: u8,
    pub subtitle_font_size: u8,
    pub code_font_size: u8,
    /// Line spacing multiplier (1.0 = single). Mapped onto Typst `par.leading`.
    pub line_spacing: f64,
}

/// Get the brand style for a branding key.
///
/// Supported keys: `"consumer"`, `"technical"`, `"internal"`. Unknown keys
/// log a warning and fall back to the consumer style (existing behavior).
pub fn brand_style(branding: &str) -> BrandStyle {
    match branding {
        // Heading sizes follow a ~1.25 modular scale off the body size so each
        // level is clearly larger than the one below. A flat ramp renders as
        // "bold body" and the hierarchy disappears — headings must step visibly.
        "consumer" => BrandStyle {
            body_font_size: 11,
            heading_font_size: 21,
            subheading_font_size: 17,
            h3_font_size: 14,
            h4_font_size: 13,
            h5_font_size: 12,
            h6_font_size: 11,
            title_font_size: 28,
            subtitle_font_size: 16,
            code_font_size: 10,
            line_spacing: 1.4,
        },
        "technical" => BrandStyle {
            body_font_size: 10,
            heading_font_size: 20,
            subheading_font_size: 16,
            h3_font_size: 13,
            h4_font_size: 12,
            h5_font_size: 11,
            h6_font_size: 10,
            title_font_size: 26,
            subtitle_font_size: 15,
            code_font_size: 9,
            line_spacing: 1.2,
        },
        "internal" => BrandStyle {
            body_font_size: 10,
            heading_font_size: 20,
            subheading_font_size: 16,
            h3_font_size: 13,
            h4_font_size: 12,
            h5_font_size: 11,
            h6_font_size: 10,
            title_font_size: 26,
            subtitle_font_size: 15,
            code_font_size: 9,
            line_spacing: 1.3,
        },
        _ => {
            log::warn!("Unknown branding key '{branding}', falling back to consumer style");
            brand_style("consumer")
        }
    }
}

/// Colors for one admonition family: the rail/title accent and the pale tint
/// behind the title bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdmonitionColors {
    pub rail: &'static str,
    pub tint: &'static str,
}

/// The Dapple admonition palette — Tailwind families chosen to match the ui
/// `NoticeBlock` variants where one exists (info/warning/danger/success) and
/// to stay mutually distinguishable across all twelve kinds. `tip` is the one
/// brand-green callout.
pub fn admonition_colors(kind: AdmonitionKind) -> AdmonitionColors {
    use AdmonitionKind::*;
    match kind {
        Note => AdmonitionColors {
            rail: "#3b82f6",
            tint: "#eff6ff",
        }, // blue
        Abstract => AdmonitionColors {
            rail: "#0891b2",
            tint: "#ecfeff",
        }, // cyan
        Info => AdmonitionColors {
            rail: "#0284c7",
            tint: "#f0f9ff",
        }, // sky
        Tip => AdmonitionColors {
            rail: DAPPLE_GREEN,
            tint: "#f0fdf4",
        }, // dapple green
        Success => AdmonitionColors {
            rail: "#16a34a",
            tint: "#f0fdf4",
        }, // green
        Question => AdmonitionColors {
            rail: "#7c3aed",
            tint: "#f5f3ff",
        }, // violet
        Warning => AdmonitionColors {
            rail: "#f59e0b",
            tint: "#fffbeb",
        }, // amber
        Failure => AdmonitionColors {
            rail: "#e11d48",
            tint: "#fff1f2",
        }, // rose
        Danger => AdmonitionColors {
            rail: "#dc2626",
            tint: "#fef2f2",
        }, // red
        Bug => AdmonitionColors {
            rail: "#c026d3",
            tint: "#fdf4ff",
        }, // fuchsia
        Example => AdmonitionColors {
            rail: "#4f46e5",
            tint: "#eef2ff",
        }, // indigo
        Quote => AdmonitionColors {
            rail: SLATE_500,
            tint: SLATE_100,
        }, // slate
        // Color bands: persistent per-entity identity (e.g. one company per
        // color in a two-company report). Rails deliberately clear of every
        // callout rail above and of brand green.
        Orange => AdmonitionColors {
            rail: "#ea580c",
            tint: "#fff7ed",
        }, // orange
        Teal => AdmonitionColors {
            rail: "#0d9488",
            tint: "#f0fdfa",
        }, // teal
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every branding's heading ramp must step by a visible margin so the
    /// hierarchy survives onto the page (ported guard from the genpdf-era
    /// style module — the regression it protects against is unchanged).
    #[test]
    fn heading_ramp_steps_are_visibly_distinct() {
        for branding in ["consumer", "technical", "internal"] {
            let s = brand_style(branding);
            assert!(s.body_font_size < s.h3_font_size, "{branding}: h3 vs body");
            assert!(
                s.h3_font_size < s.subheading_font_size,
                "{branding}: h2 vs h3"
            );
            assert!(
                s.subheading_font_size < s.heading_font_size,
                "{branding}: h1 vs h2"
            );
            assert!(
                s.heading_font_size < s.title_font_size,
                "{branding}: title vs h1"
            );
            assert!(
                s.h3_font_size - s.body_font_size >= 2,
                "{branding}: h3 too close to body"
            );
            assert!(
                s.subheading_font_size - s.h3_font_size >= 2,
                "{branding}: h2 too close to h3"
            );
            assert!(
                s.heading_font_size - s.subheading_font_size >= 2,
                "{branding}: h1 too close to h2"
            );
            assert!(s.h3_font_size > s.h4_font_size, "{branding}: h4 below h3");
            assert!(s.h4_font_size > s.h5_font_size, "{branding}: h5 below h4");
            assert!(
                s.h5_font_size >= s.h6_font_size,
                "{branding}: h6 not above h5"
            );
            assert!(
                s.h6_font_size >= s.body_font_size,
                "{branding}: h6 not below body"
            );
        }
    }

    #[test]
    fn unknown_branding_falls_back_to_consumer() {
        let s = brand_style("no-such-brand");
        let consumer = brand_style("consumer");
        assert_eq!(s.body_font_size, consumer.body_font_size);
        assert_eq!(s.title_font_size, consumer.title_font_size);
    }

    /// All admonition families must have distinct rail colors — a collision
    /// would make two kinds visually indistinguishable.
    #[test]
    fn admonition_rails_are_distinct() {
        use AdmonitionKind::*;
        let kinds = [
            Note, Abstract, Info, Tip, Success, Question, Warning, Failure, Danger, Bug, Example,
            Quote, Orange, Teal,
        ];
        let rails: Vec<&str> = kinds.iter().map(|k| admonition_colors(*k).rail).collect();
        let mut deduped = rails.clone();
        deduped.sort();
        deduped.dedup();
        assert_eq!(rails.len(), deduped.len(), "duplicate rail color");
    }
}
