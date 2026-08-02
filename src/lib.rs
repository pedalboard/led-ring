//! LED ring animation engine: spatial patterns + temporal modifiers.
//!
//! Generic over ring size (`N` LEDs per ring). Default is 12.
//! No-std compatible, no allocator needed.
//!
//! # Usage
//!
//! ```
//! use led_ring::{LedRing, RingAnimation, Rgb, PEDALBOARD_CLOCK_MAP};
//!
//! // 12-LED ring with pedalboard PCB mapping
//! let mut ring = LedRing::<12>::new(&PEDALBOARD_CLOCK_MAP);
//! ring.set(RingAnimation::solid(Rgb::new(255, 0, 0)));
//! let frame = ring.render(0);
//! assert!(frame.iter().all(|px| *px == Rgb::new(255, 0, 0)));
//! ```

#![no_std]

use serde::{Deserialize, Serialize};

/// RGB color (matches smart_leds::RGB8 layout).
#[derive(Debug, Copy, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    pub const BLACK: Self = Self::new(0, 0, 0);

    /// Scale brightness by factor 0..=255 (255 = full).
    pub fn scale(self, factor: u8) -> Self {
        Self {
            r: ((self.r as u16 * factor as u16) / 255) as u8,
            g: ((self.g as u16 * factor as u16) / 255) as u8,
            b: ((self.b as u16 * factor as u16) / 255) as u8,
        }
    }
}

/// Spatial pattern — which LEDs are lit and what color.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Renderer {
    Off,
    /// All LEDs same color.
    Solid(Rgb),
    /// Arc from rotation anchor, `count` LEDs lit.
    Fill(Rgb, u8),
    /// Potentiometer arc, blue→green→red. `fill` = 0..N.
    Heatmap(u8),
    /// Single LED at position (0..N-1), with neighbors lit.
    Single(Rgb, u8),
    /// N evenly-spaced LEDs.
    Dots(Rgb, u8),
}

/// Temporal modifier — how rendered pixels change over time.
#[derive(Debug, Copy, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Modifier {
    /// No modulation, full brightness.
    #[default]
    Solid,
    /// Static dim (1/8 intensity) — "available but inactive".
    Glow,
    /// On/off toggle at ~4Hz.
    Blink,
    /// Sine-wave fade, period ~1.5s.
    Pulse,
    /// Rotate pattern clockwise, one step per ~100ms.
    Rotate,
    /// Cycle hue over time (ignores original color, keeps lit/unlit pattern).
    ColorCycle,
}

/// Complete ring animation state.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RingAnimation {
    pub renderer: Renderer,
    pub modifier: Modifier,
}

impl Default for RingAnimation {
    fn default() -> Self {
        Self {
            renderer: Renderer::Off,
            modifier: Modifier::Solid,
        }
    }
}

impl RingAnimation {
    pub const fn off() -> Self {
        Self {
            renderer: Renderer::Off,
            modifier: Modifier::Solid,
        }
    }

    pub const fn solid(color: Rgb) -> Self {
        Self {
            renderer: Renderer::Solid(color),
            modifier: Modifier::Solid,
        }
    }

    pub const fn glow(color: Rgb) -> Self {
        Self {
            renderer: Renderer::Solid(color),
            modifier: Modifier::Glow,
        }
    }
}

/// Identity clock map for N LEDs (position i maps to physical index i).
pub const IDENTITY_MAP: [usize; 24] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
];

/// Stateful LED ring that tracks animation and renders frames.
///
/// `N` is the number of LEDs in the ring.
/// `clock_map` maps logical positions (clock hours) to physical LED indices.
#[derive(Copy, Clone)]
pub struct LedRing<const N: usize = 12> {
    pub animation: RingAnimation,
    rotation: u8,
    clock_map: [usize; N],
}

impl<const N: usize> LedRing<N> {
    /// Create a new ring with a clock-position-to-physical-index mapping.
    ///
    /// `clock_map` should have N entries mapping logical position to physical LED index.
    pub const fn new(clock_map: &[usize; N]) -> Self {
        Self {
            animation: RingAnimation::off(),
            rotation: 0,
            clock_map: *clock_map,
        }
    }

    /// Create with a rotation anchor (first lit LED position for Fill patterns).
    pub const fn with_rotation(clock_map: &[usize; N], rotation: u8) -> Self {
        Self {
            animation: RingAnimation::off(),
            rotation,
            clock_map: *clock_map,
        }
    }

    pub fn set(&mut self, anim: RingAnimation) {
        self.animation = anim;
    }

    /// Render current frame using a global tick (shared across all rings).
    pub fn render(&self, tick: u16) -> [Rgb; N] {
        let base = self.render_spatial();
        self.apply_modifier(base, tick)
    }

    fn render_spatial(&self) -> [Rgb; N] {
        let mut frame = [Rgb::BLACK; N];
        match self.animation.renderer {
            Renderer::Off => {}
            Renderer::Solid(c) => {
                for px in &mut frame {
                    *px = c;
                }
            }
            Renderer::Fill(c, count) => {
                for i in 0..(count as usize).min(N) {
                    let pos = (self.rotation as usize + N - i) % N;
                    frame[self.clock_map[pos]] = c;
                }
            }
            Renderer::Heatmap(fill) => {
                // Arc from 7h position clockwise, N-1 positions
                let arc_len = N - 1;
                let lit = ((fill as usize) * arc_len / N.max(1)).min(arc_len);
                for i in 0..lit {
                    // Start at ~7h equivalent position
                    let hour = (N * 7 / 12 + i) % N;
                    frame[self.clock_map[hour]] = heatmap_color(i, arc_len);
                }
            }
            Renderer::Single(c, pos) => {
                let p = (pos as usize) % N;
                frame[self.clock_map[p]] = c;
                // Light neighbors for visibility
                if N > 2 {
                    frame[self.clock_map[(p + N - 1) % N]] = c;
                    frame[self.clock_map[(p + 1) % N]] = c;
                }
            }
            Renderer::Dots(c, count) => {
                let n = (count as usize).clamp(1, N);
                let spacing = N / n;
                for i in 0..n {
                    frame[self.clock_map[(i * spacing) % N]] = c;
                }
            }
        }
        frame
    }

    fn apply_modifier(&self, mut frame: [Rgb; N], tick: u16) -> [Rgb; N] {
        match self.animation.modifier {
            Modifier::Solid => frame,
            Modifier::Glow => {
                for px in frame.iter_mut() {
                    *px = px.scale(2);
                }
                frame
            }
            Modifier::Blink => {
                if (tick / 12) % 2 == 1 {
                    [Rgb::BLACK; N]
                } else {
                    frame
                }
            }
            Modifier::Pulse => {
                let factor = sine_u8(tick % 50, 50);
                for px in &mut frame {
                    *px = px.scale(factor);
                }
                frame
            }
            Modifier::Rotate => {
                let shift = (tick / 25) as usize % N;
                let mut rotated = [Rgb::BLACK; N];
                for i in 0..N {
                    rotated[(i + shift) % N] = frame[i];
                }
                rotated
            }
            Modifier::ColorCycle => {
                let color = hue_to_rgb((tick * 3) as u8);
                for px in frame.iter_mut() {
                    if *px != Rgb::BLACK {
                        *px = color;
                    }
                }
                frame
            }
        }
    }
}

impl Default for LedRing<12> {
    fn default() -> Self {
        Self::with_rotation(&PEDALBOARD_CLOCK_MAP, 8)
    }
}

/// Clock-position mapping for the pedalboard PCB (12 LEDs).
/// PCB layout: D1=3h, D2=2h, ..., D12=4h.
pub const PEDALBOARD_CLOCK_MAP: [usize; 12] = [
    3,  //  0: 12h
    2,  //  1:  1h
    1,  //  2:  2h
    0,  //  3:  3h
    11, //  4:  4h
    10, //  5:  5h
    9,  //  6:  6h
    8,  //  7:  7h
    7,  //  8:  8h
    6,  //  9:  9h
    5,  // 10: 10h
    4,  // 11: 11h
];

// --- Helper functions ---

/// Blue (0) → Green (mid) → Red (max-1)
fn heatmap_color(pos: usize, max: usize) -> Rgb {
    if max <= 1 {
        return Rgb::new(0, 0, 255);
    }
    let t = (pos * 255) / (max - 1);
    if t < 128 {
        let g = (t * 2) as u8;
        Rgb::new(0, g, 255 - g)
    } else {
        let r = ((t - 128) * 2) as u8;
        Rgb::new(r, 255 - r, 0)
    }
}

/// HSV hue (0–255) to RGB at full saturation/value.
pub fn hue_to_rgb(h: u8) -> Rgb {
    let region = h / 43;
    let remainder = (h % 43) * 6;
    match region {
        0 => Rgb::new(255, remainder, 0),
        1 => Rgb::new(255 - remainder, 255, 0),
        2 => Rgb::new(0, 255, remainder),
        3 => Rgb::new(0, 255 - remainder, 255),
        4 => Rgb::new(remainder, 0, 255),
        _ => Rgb::new(255, 0, 255 - remainder),
    }
}

/// Approximate sine wave for LED pulsing (no libm needed).
fn sine_u8(phase: u16, period: u16) -> u8 {
    const TABLE: [u8; 16] = [
        0, 25, 50, 74, 98, 120, 142, 162, 180, 197, 213, 226, 237, 245, 251, 255,
    ];
    let pos = phase % period;
    let half = period / 2;
    let dist = half.abs_diff(pos);
    let idx = ((half - dist) as usize * 15) / half.max(1) as usize;
    TABLE[idx.min(15)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ring12() -> LedRing<12> {
        LedRing::with_rotation(&PEDALBOARD_CLOCK_MAP, 8)
    }

    #[test]
    fn off_renders_black() {
        let ring = ring12();
        let frame = ring.render(0);
        assert!(frame.iter().all(|px| *px == Rgb::BLACK));
    }

    #[test]
    fn solid_renders_all_same() {
        let mut ring = ring12();
        let c = Rgb::new(255, 0, 0);
        ring.set(RingAnimation::solid(c));
        let frame = ring.render(0);
        assert!(frame.iter().all(|px| *px == c));
    }

    #[test]
    fn glow_dims_uniformly() {
        let mut ring = ring12();
        ring.set(RingAnimation::glow(Rgb::new(255, 255, 255)));
        let frame = ring.render(0);
        assert!(frame.iter().all(|px| px.r == 2 && px.g == 2 && px.b == 2));
    }

    #[test]
    fn blink_alternates() {
        let mut ring = ring12();
        let c = Rgb::new(255, 0, 0);
        ring.set(RingAnimation {
            renderer: Renderer::Solid(c),
            modifier: Modifier::Blink,
        });
        assert!(ring.render(0).iter().all(|px| *px == c));
        assert!(ring.render(12).iter().all(|px| *px == Rgb::BLACK));
    }

    #[test]
    fn dots_2_lights_opposing() {
        let mut ring = ring12();
        let c = Rgb::new(0, 255, 0);
        ring.set(RingAnimation {
            renderer: Renderer::Dots(c, 2),
            modifier: Modifier::Solid,
        });
        let frame = ring.render(0);
        let lit = frame.iter().filter(|px| **px != Rgb::BLACK).count();
        assert_eq!(lit, 2);
    }

    #[test]
    fn heatmap_full() {
        let mut ring = ring12();
        ring.set(RingAnimation {
            renderer: Renderer::Heatmap(12),
            modifier: Modifier::Solid,
        });
        let frame = ring.render(0);
        let lit = frame.iter().filter(|px| **px != Rgb::BLACK).count();
        assert_eq!(lit, 11);
    }

    #[test]
    fn rotate_shifts() {
        let mut ring = ring12();
        ring.set(RingAnimation {
            renderer: Renderer::Single(Rgb::new(255, 255, 0), 0),
            modifier: Modifier::Rotate,
        });
        let f0 = ring.render(0);
        let f1 = ring.render(25);
        assert_ne!(f0, f1);
    }

    #[test]
    fn pulse_starts_dim() {
        let mut ring = ring12();
        ring.set(RingAnimation {
            renderer: Renderer::Solid(Rgb::new(255, 255, 255)),
            modifier: Modifier::Pulse,
        });
        let frame = ring.render(0);
        assert!(frame[0].r < 30);
    }

    #[test]
    fn scale_zero_is_black() {
        assert_eq!(Rgb::new(255, 128, 64).scale(0), Rgb::BLACK);
    }

    #[test]
    fn scale_255_is_identity() {
        let c = Rgb::new(200, 100, 50);
        assert_eq!(c.scale(255), c);
    }

    #[test]
    fn struct_sizes() {
        assert_eq!(core::mem::size_of::<Rgb>(), 3);
        assert_eq!(core::mem::size_of::<RingAnimation>(), 6);
        // LedRing<12> = animation(6) + rotation(1) + padding + clock_map([usize;12])
        assert!(core::mem::size_of::<LedRing<12>>() <= 112);
    }

    #[test]
    fn generic_ring_16_leds() {
        let map: [usize; 16] = core::array::from_fn(|i| i);
        let mut ring = LedRing::<16>::new(&map);
        ring.set(RingAnimation::solid(Rgb::new(0, 0, 255)));
        let frame = ring.render(0);
        assert_eq!(frame.len(), 16);
        assert!(frame.iter().all(|px| *px == Rgb::new(0, 0, 255)));
    }

    #[test]
    fn generic_ring_24_leds() {
        let map: [usize; 24] = core::array::from_fn(|i| i);
        let mut ring = LedRing::<24>::new(&map);
        ring.set(RingAnimation {
            renderer: Renderer::Fill(Rgb::new(255, 0, 0), 12),
            modifier: Modifier::Solid,
        });
        let frame = ring.render(0);
        let lit = frame.iter().filter(|px| **px != Rgb::BLACK).count();
        assert_eq!(lit, 12);
    }
}
