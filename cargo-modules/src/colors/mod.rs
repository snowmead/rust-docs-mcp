// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![allow(dead_code)]

//! Color utilities for terminal output.

pub mod cli {
    use yansi::{Color, Style};
    
    pub fn color_palette() -> ColorPalette {
        ColorPalette::default()
    }
    
    #[derive(Debug, Clone)]
    pub struct ColorPalette {
        pub normal: Style,
        pub bold: Style,
        pub dim: Style,
        pub red: Color,
        pub green: Color,
        pub yellow: Color,
        pub blue: Color,
        pub magenta: Color,
        pub cyan: Color,
        pub orange: Color,
    }
    
    impl Default for ColorPalette {
        fn default() -> Self {
            Self {
                normal: Style::new(),
                bold: Style::new().bold(),
                dim: Style::new().dim(),
                red: Color::Red,
                green: Color::Green,
                yellow: Color::Yellow,
                blue: Color::Blue,
                magenta: Color::Magenta,
                cyan: Color::Cyan,
                orange: Color::Rgb(255, 165, 0), // RGB for orange
            }
        }
    }
}