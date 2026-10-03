//! Color representations and conversions (sRGB, HSL, CIE Lab).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    pub fn hex(&self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }

    pub fn rgb_string(&self) -> String {
        format!("rgb({}, {}, {})", self.r, self.g, self.b)
    }

    pub fn hsl(&self) -> (f64, f64, f64) {
        let r = self.r as f64 / 255.0;
        let g = self.g as f64 / 255.0;
        let b = self.b as f64 / 255.0;
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let l = (max + min) / 2.0;
        let d = max - min;
        if d == 0.0 {
            return (0.0, 0.0, l * 100.0);
        }
        let s = d / (1.0 - (2.0 * l - 1.0).abs());
        let h = if max == r {
            60.0 * (((g - b) / d).rem_euclid(6.0))
        } else if max == g {
            60.0 * ((b - r) / d + 2.0)
        } else {
            60.0 * ((r - g) / d + 4.0)
        };
        (h, s * 100.0, l * 100.0)
    }

    pub fn hsl_string(&self) -> String {
        let (h, s, l) = self.hsl();
        format!("hsl({}, {}%, {}%)", h.round() as i64, s.round() as i64, l.round() as i64)
    }

    /// Relative luminance per WCAG 2.x.
    pub fn luminance(&self) -> f64 {
        fn ch(c: u8) -> f64 {
            let c = c as f64 / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * ch(self.r) + 0.7152 * ch(self.g) + 0.0722 * ch(self.b)
    }

    /// WCAG contrast ratio between two colors, in `[1, 21]`.
    pub fn contrast(&self, other: &Rgb) -> f64 {
        let (a, b) = (self.luminance(), other.luminance());
        let (hi, lo) = if a > b { (a, b) } else { (b, a) };
        (hi + 0.05) / (lo + 0.05)
    }

    pub fn to_lab(&self) -> Lab {
        fn lin(c: u8) -> f64 {
            let c = c as f64 / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        let (r, g, b) = (lin(self.r), lin(self.g), lin(self.b));
        // sRGB D65 -> XYZ, normalized by the D65 white point.
        let x = (0.4124564 * r + 0.3575761 * g + 0.1804375 * b) / 0.95047;
        let y = 0.2126729 * r + 0.7151522 * g + 0.0721750 * b;
        let z = (0.0193339 * r + 0.1191920 * g + 0.9503041 * b) / 1.08883;
        fn f(t: f64) -> f64 {
            if t > 216.0 / 24389.0 {
                t.cbrt()
            } else {
                (24389.0 / 27.0 * t + 16.0) / 116.0
            }
        }
        let (fx, fy, fz) = (f(x), f(y), f(z));
        Lab { l: 116.0 * fy - 16.0, a: 500.0 * (fx - fy), b: 200.0 * (fy - fz) }
    }

    pub fn parse_hex(s: &str) -> Option<Rgb> {
        let s = s.strip_prefix('#').unwrap_or(s);
        let expand = |c: char| c.to_digit(16).map(|v| (v * 17) as u8);
        match s.len() {
            3 => {
                let mut it = s.chars();
                Some(Rgb::new(expand(it.next()?)?, expand(it.next()?)?, expand(it.next()?)?))
            }
            6 => Some(Rgb::new(
                u8::from_str_radix(&s[0..2], 16).ok()?,
                u8::from_str_radix(&s[2..4], 16).ok()?,
                u8::from_str_radix(&s[4..6], 16).ok()?,
            )),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lab {
    pub l: f64,
    pub a: f64,
    pub b: f64,
}

impl Lab {
    /// CIE76 color difference. A ΔE around 2.3 is a "just noticeable difference".
    pub fn delta_e(&self, other: &Lab) -> f64 {
        ((self.l - other.l).powi(2) + (self.a - other.a).powi(2) + (self.b - other.b).powi(2)).sqrt()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        let c = Rgb::new(0x18, 0x18, 0x1B);
        assert_eq!(c.hex(), "#18181B");
        assert_eq!(c.rgb_string(), "rgb(24, 24, 27)");
        assert_eq!(c.hsl_string(), "hsl(240, 6%, 10%)");
        assert_eq!(Rgb::new(124, 58, 237).hsl_string(), "hsl(262, 83%, 58%)");
        assert_eq!(Rgb::parse_hex("#fff"), Some(Rgb::new(255, 255, 255)));
        assert_eq!(Rgb::parse_hex("7C3AED"), Some(Rgb::new(124, 58, 237)));
    }

    #[test]
    fn contrast_and_lab() {
        let black = Rgb::new(0, 0, 0);
        let white = Rgb::new(255, 255, 255);
        assert!((black.contrast(&white) - 21.0).abs() < 1e-9);
        let lw = white.to_lab();
        assert!((lw.l - 100.0).abs() < 0.01 && lw.a.abs() < 0.01 && lw.b.abs() < 0.01);
        assert!(Rgb::new(250, 250, 250).to_lab().delta_e(&Rgb::new(251, 250, 250).to_lab()) < 1.0);
    }
}
