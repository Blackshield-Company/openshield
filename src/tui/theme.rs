//! OpenShark Theme System
//!
//! Multi-theme support with presets inspired by Omarchy and synthwave aesthetics.
//! Themes define colors for backgrounds, text, accents, borders, and semantic roles.
#![allow(dead_code)]

use ratatui::style::{Color, Modifier, Style};

// ---------------------------------------------------------------------------
// Theme definition
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub name: String,
    pub background: Color,
    pub foreground: Color,
    pub accent: Color,
    pub accent_secondary: Color,
    pub highlight: Color,
    pub muted: Color,
    pub error: Color,
    pub success: Color,
    pub border_focused: Color,
    pub border_unfocused: Color,
    pub title: Color,
    pub selected_bg: Color,
    pub selected_fg: Color,
    pub row_alt_bg: Color,
    pub tool: Color,
    pub reasoning: Color,
    pub user_name: Color,
    pub agent_name: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self::synthwave84()
    }
}

impl Theme {
    // -----------------------------------------------------------------------
    // Core synthwave theme (default)
    // -----------------------------------------------------------------------
    pub fn synthwave84() -> Self {
        Self {
            name: "synthwave84".to_string(),
            background: Color::Rgb(26, 0, 51),
            foreground: Color::Rgb(248, 250, 252),
            accent: Color::Rgb(34, 211, 238),
            accent_secondary: Color::Rgb(236, 72, 153),
            highlight: Color::Rgb(253, 224, 71),
            muted: Color::Rgb(148, 163, 184),
            error: Color::Rgb(236, 72, 153),
            success: Color::Rgb(34, 211, 238),
            border_focused: Color::Rgb(34, 211, 238),
            border_unfocused: Color::Rgb(147, 51, 234),
            title: Color::Rgb(250, 204, 21),
            selected_bg: Color::Rgb(34, 211, 238),
            selected_fg: Color::Rgb(0, 0, 0),
            row_alt_bg: Color::Rgb(35, 0, 60),
            tool: Color::Rgb(250, 204, 21),
            reasoning: Color::Rgb(180, 140, 255),
            user_name: Color::Rgb(34, 211, 238),
            agent_name: Color::Rgb(253, 224, 71),
        }
    }

    // -----------------------------------------------------------------------
    // Light themes
    // -----------------------------------------------------------------------
    pub fn white() -> Self {
        Self {
            name: "white".to_string(),
            background: Color::Rgb(255, 255, 255),
            foreground: Color::Rgb(0, 0, 0),
            accent: Color::Rgb(26, 26, 26),
            accent_secondary: Color::Rgb(110, 110, 110),
            highlight: Color::Rgb(26, 26, 26),
            muted: Color::Rgb(128, 128, 128),
            error: Color::Rgb(180, 0, 0),
            success: Color::Rgb(0, 100, 0),
            border_focused: Color::Rgb(0, 0, 0),
            border_unfocused: Color::Rgb(192, 192, 192),
            title: Color::Rgb(0, 0, 0),
            selected_bg: Color::Rgb(26, 26, 26),
            selected_fg: Color::Rgb(255, 255, 255),
            row_alt_bg: Color::Rgb(245, 245, 245),
            tool: Color::Rgb(58, 58, 58),
            reasoning: Color::Rgb(180, 140, 255),
            user_name: Color::Rgb(0, 0, 0),
            agent_name: Color::Rgb(26, 26, 26),
        }
    }

    // -----------------------------------------------------------------------
    // Dark themes (Omarchy stock)
    // -----------------------------------------------------------------------
    pub fn catppuccin() -> Self {
        Self {
            name: "catppuccin".to_string(),
            background: Color::Rgb(30, 30, 46),
            foreground: Color::Rgb(205, 214, 244),
            accent: Color::Rgb(137, 180, 250),
            accent_secondary: Color::Rgb(245, 194, 231),
            highlight: Color::Rgb(249, 226, 175),
            muted: Color::Rgb(186, 194, 222),
            error: Color::Rgb(243, 139, 168),
            success: Color::Rgb(148, 226, 213),
            border_focused: Color::Rgb(137, 180, 250),
            border_unfocused: Color::Rgb(69, 71, 90),
            title: Color::Rgb(249, 226, 175),
            selected_bg: Color::Rgb(137, 180, 250),
            selected_fg: Color::Rgb(30, 30, 46),
            row_alt_bg: Color::Rgb(35, 35, 52),
            tool: Color::Rgb(249, 226, 175),
            reasoning: Color::Rgb(180, 140, 255),
            user_name: Color::Rgb(137, 180, 250),
            agent_name: Color::Rgb(245, 194, 231),
        }
    }

    pub fn tokyo_night() -> Self {
        Self {
            name: "tokyo-night".to_string(),
            background: Color::Rgb(26, 27, 38),
            foreground: Color::Rgb(169, 177, 214),
            accent: Color::Rgb(122, 162, 247),
            accent_secondary: Color::Rgb(173, 142, 230),
            highlight: Color::Rgb(224, 175, 104),
            muted: Color::Rgb(120, 124, 153),
            error: Color::Rgb(247, 118, 142),
            success: Color::Rgb(158, 206, 106),
            border_focused: Color::Rgb(122, 162, 247),
            border_unfocused: Color::Rgb(50, 52, 74),
            title: Color::Rgb(224, 175, 104),
            selected_bg: Color::Rgb(122, 162, 247),
            selected_fg: Color::Rgb(26, 27, 38),
            row_alt_bg: Color::Rgb(32, 33, 46),
            tool: Color::Rgb(255, 158, 100),
            reasoning: Color::Rgb(180, 140, 255),
            user_name: Color::Rgb(122, 162, 247),
            agent_name: Color::Rgb(187, 154, 247),
        }
    }

    pub fn gruvbox() -> Self {
        Self {
            name: "gruvbox".to_string(),
            background: Color::Rgb(40, 40, 40),
            foreground: Color::Rgb(212, 190, 152),
            accent: Color::Rgb(125, 174, 163),
            accent_secondary: Color::Rgb(211, 134, 155),
            highlight: Color::Rgb(216, 166, 87),
            muted: Color::Rgb(168, 153, 132),
            error: Color::Rgb(234, 105, 98),
            success: Color::Rgb(169, 182, 101),
            border_focused: Color::Rgb(125, 174, 163),
            border_unfocused: Color::Rgb(60, 56, 54),
            title: Color::Rgb(216, 166, 87),
            selected_bg: Color::Rgb(125, 174, 163),
            selected_fg: Color::Rgb(40, 40, 40),
            row_alt_bg: Color::Rgb(48, 48, 48),
            tool: Color::Rgb(216, 166, 87),
            reasoning: Color::Rgb(180, 140, 255),
            user_name: Color::Rgb(125, 174, 163),
            agent_name: Color::Rgb(211, 134, 155),
        }
    }

    pub fn nord() -> Self {
        Self {
            name: "nord".to_string(),
            background: Color::Rgb(46, 52, 64),
            foreground: Color::Rgb(216, 222, 233),
            accent: Color::Rgb(129, 161, 193),
            accent_secondary: Color::Rgb(180, 142, 173),
            highlight: Color::Rgb(235, 203, 139),
            muted: Color::Rgb(143, 188, 187),
            error: Color::Rgb(191, 97, 106),
            success: Color::Rgb(163, 190, 140),
            border_focused: Color::Rgb(129, 161, 193),
            border_unfocused: Color::Rgb(59, 66, 82),
            title: Color::Rgb(235, 203, 139),
            selected_bg: Color::Rgb(129, 161, 193),
            selected_fg: Color::Rgb(46, 52, 64),
            row_alt_bg: Color::Rgb(52, 58, 72),
            tool: Color::Rgb(235, 203, 139),
            reasoning: Color::Rgb(180, 140, 255),
            user_name: Color::Rgb(129, 161, 193),
            agent_name: Color::Rgb(180, 142, 173),
        }
    }

    pub fn everforest() -> Self {
        Self {
            name: "everforest".to_string(),
            background: Color::Rgb(45, 53, 59),
            foreground: Color::Rgb(211, 198, 170),
            accent: Color::Rgb(167, 192, 128),
            accent_secondary: Color::Rgb(227, 171, 157),
            highlight: Color::Rgb(219, 188, 127),
            muted: Color::Rgb(167, 192, 128),
            error: Color::Rgb(230, 126, 128),
            success: Color::Rgb(167, 192, 128),
            border_focused: Color::Rgb(167, 192, 128),
            border_unfocused: Color::Rgb(60, 70, 66),
            title: Color::Rgb(219, 188, 127),
            selected_bg: Color::Rgb(167, 192, 128),
            selected_fg: Color::Rgb(45, 53, 59),
            row_alt_bg: Color::Rgb(52, 61, 66),
            tool: Color::Rgb(219, 188, 127),
            reasoning: Color::Rgb(180, 140, 255),
            user_name: Color::Rgb(167, 192, 128),
            agent_name: Color::Rgb(227, 171, 157),
        }
    }

    pub fn kanagawa() -> Self {
        Self {
            name: "kanagawa".to_string(),
            background: Color::Rgb(31, 31, 40),
            foreground: Color::Rgb(220, 215, 186),
            accent: Color::Rgb(126, 156, 216),
            accent_secondary: Color::Rgb(210, 126, 153),
            highlight: Color::Rgb(232, 191, 120),
            muted: Color::Rgb(146, 131, 116),
            error: Color::Rgb(255, 98, 90),
            success: Color::Rgb(152, 187, 108),
            border_focused: Color::Rgb(126, 156, 216),
            border_unfocused: Color::Rgb(55, 55, 65),
            title: Color::Rgb(232, 191, 120),
            selected_bg: Color::Rgb(126, 156, 216),
            selected_fg: Color::Rgb(31, 31, 40),
            row_alt_bg: Color::Rgb(38, 38, 48),
            tool: Color::Rgb(232, 191, 120),
            reasoning: Color::Rgb(180, 140, 255),
            user_name: Color::Rgb(126, 156, 216),
            agent_name: Color::Rgb(210, 126, 153),
        }
    }

    pub fn rose_pine() -> Self {
        Self {
            name: "rose-pine".to_string(),
            background: Color::Rgb(25, 23, 36),
            foreground: Color::Rgb(224, 222, 244),
            accent: Color::Rgb(156, 207, 216),
            accent_secondary: Color::Rgb(235, 188, 186),
            highlight: Color::Rgb(246, 193, 119),
            muted: Color::Rgb(144, 140, 170),
            error: Color::Rgb(235, 111, 146),
            success: Color::Rgb(156, 207, 216),
            border_focused: Color::Rgb(156, 207, 216),
            border_unfocused: Color::Rgb(64, 61, 82),
            title: Color::Rgb(246, 193, 119),
            selected_bg: Color::Rgb(156, 207, 216),
            selected_fg: Color::Rgb(25, 23, 36),
            row_alt_bg: Color::Rgb(33, 31, 45),
            tool: Color::Rgb(246, 193, 119),
            reasoning: Color::Rgb(180, 140, 255),
            user_name: Color::Rgb(156, 207, 216),
            agent_name: Color::Rgb(235, 188, 186),
        }
    }

    // -----------------------------------------------------------------------
    // Specialty themes
    // -----------------------------------------------------------------------
    pub fn hackerman() -> Self {
        Self {
            name: "hackerman".to_string(),
            background: Color::Rgb(11, 12, 22),
            foreground: Color::Rgb(221, 247, 255),
            accent: Color::Rgb(130, 251, 156),
            accent_secondary: Color::Rgb(124, 248, 247),
            highlight: Color::Rgb(130, 251, 156),
            muted: Color::Rgb(106, 110, 149),
            error: Color::Rgb(255, 100, 100),
            success: Color::Rgb(130, 251, 156),
            border_focused: Color::Rgb(130, 251, 156),
            border_unfocused: Color::Rgb(40, 42, 60),
            title: Color::Rgb(130, 251, 156),
            selected_bg: Color::Rgb(130, 251, 156),
            selected_fg: Color::Rgb(11, 12, 22),
            row_alt_bg: Color::Rgb(18, 19, 30),
            tool: Color::Rgb(124, 248, 247),
            reasoning: Color::Rgb(180, 140, 255),
            user_name: Color::Rgb(130, 251, 156),
            agent_name: Color::Rgb(124, 248, 247),
        }
    }

    pub fn vantablack() -> Self {
        Self {
            name: "vantablack".to_string(),
            background: Color::Rgb(0, 0, 0),
            foreground: Color::Rgb(255, 255, 255),
            accent: Color::Rgb(141, 141, 141),
            accent_secondary: Color::Rgb(155, 155, 155),
            highlight: Color::Rgb(255, 255, 255),
            muted: Color::Rgb(128, 128, 128),
            error: Color::Rgb(200, 200, 200),
            success: Color::Rgb(180, 180, 180),
            border_focused: Color::Rgb(255, 255, 255),
            border_unfocused: Color::Rgb(64, 64, 64),
            title: Color::Rgb(255, 255, 255),
            selected_bg: Color::Rgb(255, 255, 255),
            selected_fg: Color::Rgb(0, 0, 0),
            row_alt_bg: Color::Rgb(16, 16, 16),
            tool: Color::Rgb(180, 180, 180),
            reasoning: Color::Rgb(180, 140, 255),
            user_name: Color::Rgb(200, 200, 200),
            agent_name: Color::Rgb(255, 255, 255),
        }
    }

    pub fn retro82() -> Self {
        Self {
            name: "retro-82".to_string(),
            background: Color::Rgb(5, 24, 46),
            foreground: Color::Rgb(246, 220, 172),
            accent: Color::Rgb(250, 169, 104),
            accent_secondary: Color::Rgb(140, 191, 184),
            highlight: Color::Rgb(250, 169, 104),
            muted: Color::Rgb(167, 201, 198),
            error: Color::Rgb(248, 85, 37),
            success: Color::Rgb(2, 131, 145),
            border_focused: Color::Rgb(250, 169, 104),
            border_unfocused: Color::Rgb(48, 52, 66),
            title: Color::Rgb(250, 169, 104),
            selected_bg: Color::Rgb(250, 169, 104),
            selected_fg: Color::Rgb(5, 24, 46),
            row_alt_bg: Color::Rgb(10, 35, 60),
            tool: Color::Rgb(233, 123, 60),
            reasoning: Color::Rgb(180, 140, 255),
            user_name: Color::Rgb(250, 169, 104),
            agent_name: Color::Rgb(140, 191, 184),
        }
    }

    pub fn matte_black() -> Self {
        Self {
            name: "matte-black".to_string(),
            background: Color::Rgb(20, 20, 20),
            foreground: Color::Rgb(200, 200, 200),
            accent: Color::Rgb(160, 160, 160),
            accent_secondary: Color::Rgb(120, 120, 120),
            highlight: Color::Rgb(220, 220, 220),
            muted: Color::Rgb(100, 100, 100),
            error: Color::Rgb(180, 80, 80),
            success: Color::Rgb(80, 180, 80),
            border_focused: Color::Rgb(180, 180, 180),
            border_unfocused: Color::Rgb(50, 50, 50),
            title: Color::Rgb(200, 200, 200),
            selected_bg: Color::Rgb(180, 180, 180),
            selected_fg: Color::Rgb(20, 20, 20),
            row_alt_bg: Color::Rgb(28, 28, 28),
            tool: Color::Rgb(160, 160, 160),
            reasoning: Color::Rgb(180, 140, 255),
            user_name: Color::Rgb(180, 180, 180),
            agent_name: Color::Rgb(200, 200, 200),
        }
    }

    pub fn miasma() -> Self {
        Self {
            name: "miasma".to_string(),
            background: Color::Rgb(33, 37, 30),
            foreground: Color::Rgb(199, 199, 176),
            accent: Color::Rgb(136, 166, 96),
            accent_secondary: Color::Rgb(174, 155, 120),
            highlight: Color::Rgb(187, 176, 130),
            muted: Color::Rgb(120, 130, 110),
            error: Color::Rgb(180, 90, 80),
            success: Color::Rgb(136, 166, 96),
            border_focused: Color::Rgb(136, 166, 96),
            border_unfocused: Color::Rgb(50, 55, 45),
            title: Color::Rgb(187, 176, 130),
            selected_bg: Color::Rgb(136, 166, 96),
            selected_fg: Color::Rgb(33, 37, 30),
            row_alt_bg: Color::Rgb(40, 44, 36),
            tool: Color::Rgb(187, 176, 130),
            reasoning: Color::Rgb(180, 140, 255),
            user_name: Color::Rgb(136, 166, 96),
            agent_name: Color::Rgb(174, 155, 120),
        }
    }

    pub fn ethereal() -> Self {
        Self {
            name: "ethereal".to_string(),
            background: Color::Rgb(35, 35, 50),
            foreground: Color::Rgb(210, 210, 230),
            accent: Color::Rgb(180, 160, 220),
            accent_secondary: Color::Rgb(220, 160, 180),
            highlight: Color::Rgb(200, 180, 240),
            muted: Color::Rgb(150, 150, 170),
            error: Color::Rgb(230, 120, 140),
            success: Color::Rgb(140, 200, 180),
            border_focused: Color::Rgb(180, 160, 220),
            border_unfocused: Color::Rgb(55, 55, 70),
            title: Color::Rgb(200, 180, 240),
            selected_bg: Color::Rgb(180, 160, 220),
            selected_fg: Color::Rgb(35, 35, 50),
            row_alt_bg: Color::Rgb(42, 42, 58),
            tool: Color::Rgb(200, 180, 240),
            reasoning: Color::Rgb(180, 140, 255),
            user_name: Color::Rgb(180, 160, 220),
            agent_name: Color::Rgb(220, 160, 180),
        }
    }

    pub fn lumon() -> Self {
        Self {
            name: "lumon".to_string(),
            background: Color::Rgb(230, 230, 220),
            foreground: Color::Rgb(40, 40, 40),
            accent: Color::Rgb(60, 80, 100),
            accent_secondary: Color::Rgb(100, 80, 60),
            highlight: Color::Rgb(50, 70, 90),
            muted: Color::Rgb(120, 120, 110),
            error: Color::Rgb(150, 60, 60),
            success: Color::Rgb(60, 100, 60),
            border_focused: Color::Rgb(60, 80, 100),
            border_unfocused: Color::Rgb(180, 180, 170),
            title: Color::Rgb(50, 70, 90),
            selected_bg: Color::Rgb(60, 80, 100),
            selected_fg: Color::Rgb(230, 230, 220),
            row_alt_bg: Color::Rgb(220, 220, 210),
            tool: Color::Rgb(80, 100, 120),
            reasoning: Color::Rgb(180, 140, 255),
            user_name: Color::Rgb(60, 80, 100),
            agent_name: Color::Rgb(80, 60, 50),
        }
    }

    pub fn osaka_jade() -> Self {
        Self {
            name: "osaka-jade".to_string(),
            background: Color::Rgb(25, 35, 35),
            foreground: Color::Rgb(190, 210, 200),
            accent: Color::Rgb(100, 180, 160),
            accent_secondary: Color::Rgb(160, 180, 140),
            highlight: Color::Rgb(120, 200, 180),
            muted: Color::Rgb(120, 140, 130),
            error: Color::Rgb(200, 100, 100),
            success: Color::Rgb(100, 180, 160),
            border_focused: Color::Rgb(100, 180, 160),
            border_unfocused: Color::Rgb(45, 55, 55),
            title: Color::Rgb(120, 200, 180),
            selected_bg: Color::Rgb(100, 180, 160),
            selected_fg: Color::Rgb(25, 35, 35),
            row_alt_bg: Color::Rgb(32, 42, 42),
            tool: Color::Rgb(140, 200, 160),
            reasoning: Color::Rgb(180, 140, 255),
            user_name: Color::Rgb(100, 180, 160),
            agent_name: Color::Rgb(160, 180, 140),
        }
    }

    pub fn ristretto() -> Self {
        Self {
            name: "ristretto".to_string(),
            background: Color::Rgb(35, 30, 30),
            foreground: Color::Rgb(210, 195, 180),
            accent: Color::Rgb(180, 140, 110),
            accent_secondary: Color::Rgb(160, 130, 120),
            highlight: Color::Rgb(200, 160, 120),
            muted: Color::Rgb(140, 130, 120),
            error: Color::Rgb(180, 90, 80),
            success: Color::Rgb(130, 160, 120),
            border_focused: Color::Rgb(180, 140, 110),
            border_unfocused: Color::Rgb(55, 50, 50),
            title: Color::Rgb(200, 160, 120),
            selected_bg: Color::Rgb(180, 140, 110),
            selected_fg: Color::Rgb(35, 30, 30),
            row_alt_bg: Color::Rgb(42, 37, 37),
            tool: Color::Rgb(200, 160, 120),
            reasoning: Color::Rgb(180, 140, 255),
            user_name: Color::Rgb(180, 140, 110),
            agent_name: Color::Rgb(200, 170, 140),
        }
    }

    pub fn flexoki_light() -> Self {
        Self {
            name: "flexoki-light".to_string(),
            background: Color::Rgb(250, 248, 245),
            foreground: Color::Rgb(50, 50, 50),
            accent: Color::Rgb(100, 100, 100),
            accent_secondary: Color::Rgb(120, 100, 80),
            highlight: Color::Rgb(80, 80, 80),
            muted: Color::Rgb(130, 130, 130),
            error: Color::Rgb(180, 60, 60),
            success: Color::Rgb(60, 140, 60),
            border_focused: Color::Rgb(80, 80, 80),
            border_unfocused: Color::Rgb(210, 208, 205),
            title: Color::Rgb(80, 80, 80),
            selected_bg: Color::Rgb(80, 80, 80),
            selected_fg: Color::Rgb(250, 248, 245),
            row_alt_bg: Color::Rgb(242, 240, 237),
            tool: Color::Rgb(100, 100, 100),
            reasoning: Color::Rgb(180, 140, 255),
            user_name: Color::Rgb(80, 80, 80),
            agent_name: Color::Rgb(100, 100, 100),
        }
    }

    // -----------------------------------------------------------------------
    // Generic light / dark
    // -----------------------------------------------------------------------
    pub fn light() -> Self {
        Self {
            name: "light".to_string(),
            background: Color::Rgb(250, 250, 250),
            foreground: Color::Rgb(30, 30, 30),
            accent: Color::Rgb(0, 100, 200),
            accent_secondary: Color::Rgb(180, 60, 120),
            highlight: Color::Rgb(0, 80, 160),
            muted: Color::Rgb(120, 120, 120),
            error: Color::Rgb(200, 50, 50),
            success: Color::Rgb(40, 140, 40),
            border_focused: Color::Rgb(0, 100, 200),
            border_unfocused: Color::Rgb(200, 200, 200),
            title: Color::Rgb(0, 80, 160),
            selected_bg: Color::Rgb(0, 100, 200),
            selected_fg: Color::Rgb(250, 250, 250),
            row_alt_bg: Color::Rgb(240, 240, 240),
            tool: Color::Rgb(80, 80, 80),
            reasoning: Color::Rgb(180, 140, 255),
            user_name: Color::Rgb(0, 100, 200),
            agent_name: Color::Rgb(0, 80, 160),
        }
    }

    pub fn dark() -> Self {
        Self {
            name: "dark".to_string(),
            background: Color::Rgb(30, 30, 30),
            foreground: Color::Rgb(220, 220, 220),
            accent: Color::Rgb(100, 150, 220),
            accent_secondary: Color::Rgb(220, 120, 160),
            highlight: Color::Rgb(120, 170, 240),
            muted: Color::Rgb(140, 140, 140),
            error: Color::Rgb(240, 80, 80),
            success: Color::Rgb(80, 200, 80),
            border_focused: Color::Rgb(100, 150, 220),
            border_unfocused: Color::Rgb(60, 60, 60),
            title: Color::Rgb(120, 170, 240),
            selected_bg: Color::Rgb(100, 150, 220),
            selected_fg: Color::Rgb(30, 30, 30),
            row_alt_bg: Color::Rgb(40, 40, 40),
            tool: Color::Rgb(160, 160, 160),
            reasoning: Color::Rgb(180, 140, 255),
            user_name: Color::Rgb(100, 150, 220),
            agent_name: Color::Rgb(120, 170, 240),
        }
    }

    // -----------------------------------------------------------------------
    // More synthwave variants
    // -----------------------------------------------------------------------
    pub fn outrun() -> Self {
        Self {
            name: "outrun".to_string(),
            background: Color::Rgb(15, 0, 40),
            foreground: Color::Rgb(255, 220, 240),
            accent: Color::Rgb(255, 50, 150),
            accent_secondary: Color::Rgb(180, 50, 255),
            highlight: Color::Rgb(255, 200, 50),
            muted: Color::Rgb(150, 120, 160),
            error: Color::Rgb(255, 60, 60),
            success: Color::Rgb(50, 255, 180),
            border_focused: Color::Rgb(255, 50, 150),
            border_unfocused: Color::Rgb(60, 0, 80),
            title: Color::Rgb(255, 200, 50),
            selected_bg: Color::Rgb(255, 50, 150),
            selected_fg: Color::Rgb(15, 0, 40),
            row_alt_bg: Color::Rgb(25, 0, 55),
            tool: Color::Rgb(255, 200, 50),
            reasoning: Color::Rgb(180, 140, 255),
            user_name: Color::Rgb(255, 50, 150),
            agent_name: Color::Rgb(255, 200, 50),
        }
    }

    pub fn hotline() -> Self {
        Self {
            name: "hotline".to_string(),
            background: Color::Rgb(20, 0, 10),
            foreground: Color::Rgb(255, 230, 230),
            accent: Color::Rgb(255, 80, 80),
            accent_secondary: Color::Rgb(255, 160, 40),
            highlight: Color::Rgb(255, 220, 80),
            muted: Color::Rgb(180, 140, 140),
            error: Color::Rgb(255, 40, 40),
            success: Color::Rgb(80, 220, 120),
            border_focused: Color::Rgb(255, 80, 80),
            border_unfocused: Color::Rgb(80, 0, 20),
            title: Color::Rgb(255, 220, 80),
            selected_bg: Color::Rgb(255, 80, 80),
            selected_fg: Color::Rgb(20, 0, 10),
            row_alt_bg: Color::Rgb(35, 0, 18),
            tool: Color::Rgb(255, 160, 40),
            reasoning: Color::Rgb(180, 140, 255),
            user_name: Color::Rgb(255, 80, 80),
            agent_name: Color::Rgb(255, 220, 80),
        }
    }

    pub fn sunset() -> Self {
        Self {
            name: "sunset".to_string(),
            background: Color::Rgb(40, 15, 30),
            foreground: Color::Rgb(255, 230, 210),
            accent: Color::Rgb(255, 140, 80),
            accent_secondary: Color::Rgb(255, 80, 120),
            highlight: Color::Rgb(255, 200, 100),
            muted: Color::Rgb(180, 150, 140),
            error: Color::Rgb(255, 60, 80),
            success: Color::Rgb(100, 220, 160),
            border_focused: Color::Rgb(255, 140, 80),
            border_unfocused: Color::Rgb(80, 30, 50),
            title: Color::Rgb(255, 200, 100),
            selected_bg: Color::Rgb(255, 140, 80),
            selected_fg: Color::Rgb(40, 15, 30),
            row_alt_bg: Color::Rgb(55, 20, 40),
            tool: Color::Rgb(255, 180, 80),
            reasoning: Color::Rgb(180, 140, 255),
            user_name: Color::Rgb(255, 140, 80),
            agent_name: Color::Rgb(255, 200, 100),
        }
    }

    pub fn midnight() -> Self {
        Self {
            name: "midnight".to_string(),
            background: Color::Rgb(10, 10, 25),
            foreground: Color::Rgb(200, 210, 255),
            accent: Color::Rgb(100, 120, 255),
            accent_secondary: Color::Rgb(180, 100, 255),
            highlight: Color::Rgb(140, 180, 255),
            muted: Color::Rgb(120, 130, 170),
            error: Color::Rgb(255, 80, 120),
            success: Color::Rgb(80, 220, 180),
            border_focused: Color::Rgb(100, 120, 255),
            border_unfocused: Color::Rgb(30, 30, 50),
            title: Color::Rgb(140, 180, 255),
            selected_bg: Color::Rgb(100, 120, 255),
            selected_fg: Color::Rgb(10, 10, 25),
            row_alt_bg: Color::Rgb(18, 18, 38),
            tool: Color::Rgb(180, 160, 255),
            reasoning: Color::Rgb(180, 140, 255),
            user_name: Color::Rgb(100, 120, 255),
            agent_name: Color::Rgb(140, 180, 255),
        }
    }

    pub fn cyberpunk() -> Self {
        Self {
            name: "cyberpunk".to_string(),
            background: Color::Rgb(10, 0, 15),
            foreground: Color::Rgb(220, 255, 220),
            accent: Color::Rgb(0, 255, 120),
            accent_secondary: Color::Rgb(255, 0, 200),
            highlight: Color::Rgb(255, 255, 0),
            muted: Color::Rgb(120, 160, 120),
            error: Color::Rgb(255, 40, 40),
            success: Color::Rgb(0, 255, 100),
            border_focused: Color::Rgb(0, 255, 120),
            border_unfocused: Color::Rgb(40, 0, 40),
            title: Color::Rgb(255, 255, 0),
            selected_bg: Color::Rgb(0, 255, 120),
            selected_fg: Color::Rgb(10, 0, 15),
            row_alt_bg: Color::Rgb(20, 0, 25),
            tool: Color::Rgb(255, 200, 0),
            reasoning: Color::Rgb(180, 140, 255),
            user_name: Color::Rgb(0, 255, 120),
            agent_name: Color::Rgb(255, 255, 0),
        }
    }

    // -----------------------------------------------------------------------
    // All presets
    // -----------------------------------------------------------------------
    pub fn all_presets() -> Vec<Self> {
        vec![
            Self::synthwave84(),
            Self::light(),
            Self::dark(),
            Self::white(),
            Self::catppuccin(),
            Self::tokyo_night(),
            Self::gruvbox(),
            Self::nord(),
            Self::everforest(),
            Self::kanagawa(),
            Self::rose_pine(),
            Self::outrun(),
            Self::hotline(),
            Self::sunset(),
            Self::midnight(),
            Self::cyberpunk(),
            Self::hackerman(),
            Self::vantablack(),
            Self::retro82(),
            Self::matte_black(),
            Self::miasma(),
            Self::ethereal(),
            Self::lumon(),
            Self::osaka_jade(),
            Self::ristretto(),
            Self::flexoki_light(),
        ]
    }

    pub fn by_name(name: &str) -> Option<Self> {
        Self::all_presets().into_iter().find(|t| t.name == name)
    }

    pub fn names() -> Vec<String> {
        Self::all_presets().into_iter().map(|t| t.name).collect()
    }
}

// ---------------------------------------------------------------------------
// Global theme instance (set at app startup, changed via TUI)
// ---------------------------------------------------------------------------

use std::sync::RwLock;

static CURRENT_THEME: RwLock<Option<Theme>> = RwLock::new(None);

pub fn set_theme(theme: Theme) {
    if let Ok(mut guard) = CURRENT_THEME.write() {
        *guard = Some(theme);
    }
}

pub fn current_theme() -> Theme {
    if let Ok(guard) = CURRENT_THEME.read() {
        guard.clone().unwrap_or_default()
    } else {
        Theme::default()
    }
}

// ---------------------------------------------------------------------------
// Style helpers — dynamic based on current theme
// ---------------------------------------------------------------------------

pub fn bg_style() -> Style {
    let t = current_theme();
    Style::default().bg(t.background)
}

pub fn text_style() -> Style {
    let t = current_theme();
    Style::default().fg(t.foreground)
}

pub fn muted_style() -> Style {
    let t = current_theme();
    Style::default().fg(t.muted)
}

/// Style for reasoning/thinking content — same muted color as streaming,
/// so saved reasoning messages match the live-streaming appearance.
pub fn reasoning_style() -> Style {
    let t = current_theme();
    Style::default().fg(t.muted)
}

pub fn highlight_style() -> Style {
    let t = current_theme();
    Style::default()
        .fg(t.highlight)
        .add_modifier(Modifier::BOLD)
}

pub fn accent_style() -> Style {
    let t = current_theme();
    Style::default().fg(t.accent).add_modifier(Modifier::BOLD)
}

pub fn secondary_accent_style() -> Style {
    let t = current_theme();
    Style::default().fg(t.accent_secondary)
}

pub fn error_style() -> Style {
    let t = current_theme();
    Style::default().fg(t.error).add_modifier(Modifier::BOLD)
}

pub fn success_style() -> Style {
    let t = current_theme();
    Style::default().fg(t.success).add_modifier(Modifier::BOLD)
}

pub fn focused_border_style() -> Style {
    let t = current_theme();
    Style::default().fg(t.border_focused)
}

pub fn border_style() -> Style {
    let t = current_theme();
    Style::default().fg(t.border_unfocused)
}

pub fn title_style() -> Style {
    let t = current_theme();
    Style::default().fg(t.title).add_modifier(Modifier::BOLD)
}

pub fn shark_style() -> Style {
    let t = current_theme();
    Style::default().fg(t.accent).add_modifier(Modifier::BOLD)
}

pub fn prompt_style() -> Style {
    let t = current_theme();
    Style::default()
        .fg(t.highlight)
        .add_modifier(Modifier::BOLD)
}

pub fn tool_style() -> Style {
    let t = current_theme();
    Style::default().fg(t.tool).add_modifier(Modifier::BOLD)
}

pub fn selected_style() -> Style {
    let t = current_theme();
    Style::default()
        .fg(t.selected_fg)
        .bg(t.selected_bg)
        .add_modifier(Modifier::BOLD)
}

/// Style for mouse-drag text selection in the chat area.
pub fn selection_style() -> Style {
    let t = current_theme();
    Style::default().fg(t.foreground).bg(t.border_unfocused)
}

pub fn header_style() -> Style {
    let t = current_theme();
    Style::default()
        .fg(t.title)
        .bg(t.border_unfocused)
        .add_modifier(Modifier::BOLD)
}

pub fn row_even_style() -> Style {
    let t = current_theme();
    Style::default().fg(t.foreground)
}

pub fn row_odd_style() -> Style {
    let t = current_theme();
    Style::default().fg(t.foreground).bg(t.row_alt_bg)
}

pub fn user_name_style() -> Style {
    let t = current_theme();
    Style::default()
        .fg(t.user_name)
        .add_modifier(Modifier::BOLD)
}

pub fn agent_name_style() -> Style {
    let t = current_theme();
    Style::default()
        .fg(t.agent_name)
        .add_modifier(Modifier::BOLD)
}

// ---------------------------------------------------------------------------
// Low-level ANSI helpers (theme-aware)
// ---------------------------------------------------------------------------

fn hex_to_rgb(hex: &str) -> (u8, u8, u8) {
    let h = hex.trim_start_matches('#');
    let r = u8::from_str_radix(&h[0..2], 16).unwrap_or(0);
    let g = u8::from_str_radix(&h[2..4], 16).unwrap_or(0);
    let b = u8::from_str_radix(&h[4..6], 16).unwrap_or(0);
    (r, g, b)
}

fn color_to_hex(c: Color) -> String {
    match c {
        Color::Rgb(r, g, b) => format!("#{:02x}{:02x}{:02x}", r, g, b),
        _ => "#ffffff".to_string(),
    }
}

pub fn fg(text: &str, hex: &str) -> String {
    let (r, g, b) = hex_to_rgb(hex);
    format!("\x1b[38;2;{};{};{}m{}\x1b[0m", r, g, b, text)
}

pub fn bg_ansi(text: &str, hex: &str) -> String {
    let (r, g, b) = hex_to_rgb(hex);
    format!("\x1b[48;2;{};{};{}m{}\x1b[0m", r, g, b, text)
}

pub fn bold_fg(text: &str, hex: &str) -> String {
    let (r, g, b) = hex_to_rgb(hex);
    format!("\x1b[1;38;2;{};{};{}m{}\x1b[0m", r, g, b, text)
}

pub fn italic_fg(text: &str, hex: &str) -> String {
    let (r, g, b) = hex_to_rgb(hex);
    format!("\x1b[3;38;2;{};{};{}m{}\x1b[0m", r, g, b, text)
}

pub fn dim(text: &str) -> String {
    format!("\x1b[2m{}\x1b[0m", text)
}

pub fn underline_fg(text: &str, hex: &str) -> String {
    let (r, g, b) = hex_to_rgb(hex);
    format!("\x1b[4;38;2;{};{};{}m{}\x1b[0m", r, g, b, text)
}

// ---------------------------------------------------------------------------
// Semantic helpers
// ---------------------------------------------------------------------------

pub fn shark(text: &str) -> String {
    let t = current_theme();
    bold_fg(text, &color_to_hex(t.accent))
}

pub fn prompt(text: &str) -> String {
    let t = current_theme();
    bold_fg(text, &color_to_hex(t.highlight))
}

pub fn highlight(text: &str) -> String {
    let t = current_theme();
    bold_fg(text, &color_to_hex(t.highlight))
}

pub fn accent(text: &str) -> String {
    let t = current_theme();
    fg(text, &color_to_hex(t.accent_secondary))
}

pub fn soft_accent(text: &str) -> String {
    let t = current_theme();
    fg(text, &color_to_hex(t.accent_secondary))
}

pub fn purple(text: &str) -> String {
    let t = current_theme();
    fg(text, &color_to_hex(t.accent))
}

pub fn bright_purple(text: &str) -> String {
    let t = current_theme();
    bold_fg(text, &color_to_hex(t.accent))
}

pub fn interactive(text: &str) -> String {
    let t = current_theme();
    bold_fg(text, &color_to_hex(t.accent))
}

pub fn body(text: &str) -> String {
    let t = current_theme();
    fg(text, &color_to_hex(t.foreground))
}

pub fn muted(text: &str) -> String {
    let t = current_theme();
    fg(text, &color_to_hex(t.muted))
}

pub fn error(text: &str) -> String {
    let t = current_theme();
    bold_fg(text, &color_to_hex(t.error))
}

pub fn success(text: &str) -> String {
    let t = current_theme();
    bold_fg(text, &color_to_hex(t.success))
}

pub fn tool(text: &str) -> String {
    let t = current_theme();
    bold_fg(text, &color_to_hex(t.tool))
}

pub fn gradient(text: &str) -> String {
    use std::fmt::Write as _;
    let t = current_theme();
    let colors = [
        color_to_hex(t.accent),
        color_to_hex(t.accent_secondary),
        color_to_hex(t.highlight),
        color_to_hex(t.accent),
    ];
    let mut out = String::with_capacity(text.len() * 2);
    for (i, ch) in text.chars().enumerate() {
        let color = &colors[i % colors.len()];
        let (r, g, b) = hex_to_rgb(color);
        let _ = write!(out, "\x1b[1;38;2;{};{};{}m{}\x1b[0m", r, g, b, ch);
    }
    out
}

pub fn hr(width: usize) -> String {
    let t = current_theme();
    let left = fg("▛", &color_to_hex(t.border_unfocused));
    let right = fg("▜", &color_to_hex(t.border_unfocused));
    let bar: String = "▀".repeat(width.saturating_sub(2));
    let bar = fg(&bar, &color_to_hex(t.row_alt_bg));
    format!("{}{}{}", left, bar, right)
}

pub fn border_line(width: usize) -> String {
    let t = current_theme();
    let line: String = "═".repeat(width);
    fg(&line, &color_to_hex(t.border_unfocused))
}

pub fn box_top(width: usize) -> String {
    let t = current_theme();
    let mut s = String::new();
    s.push_str(&fg("╔", &color_to_hex(t.border_unfocused)));
    s.push_str(&fg(
        &"═".repeat(width.saturating_sub(2)),
        &color_to_hex(t.border_focused),
    ));
    s.push_str(&fg("╗", &color_to_hex(t.border_unfocused)));
    s
}

pub fn box_bottom(width: usize) -> String {
    let t = current_theme();
    let mut s = String::new();
    s.push_str(&fg("╚", &color_to_hex(t.border_unfocused)));
    s.push_str(&fg(
        &"═".repeat(width.saturating_sub(2)),
        &color_to_hex(t.border_focused),
    ));
    s.push_str(&fg("╝", &color_to_hex(t.border_unfocused)));
    s
}

pub fn box_mid(text: &str, width: usize) -> String {
    let pad = width.saturating_sub(text.len() + 2);
    let right = " ".repeat(pad);
    format!(
        "{}{}{}{}{}",
        fg("║", &color_to_hex(current_theme().border_unfocused)),
        body(text),
        right,
        fg("║", &color_to_hex(current_theme().border_unfocused)),
        "\x1b[0m"
    )
}
