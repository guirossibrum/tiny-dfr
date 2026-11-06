//! One shared baseline for text buttons, with ink-centered horizontal placement.
use cairo::Context;

pub fn origin(
    c: &Context,
    text: &str,
    height: f64,
    left: f64,
    width: f64,
    y_shift: f64,
) -> (f64, f64) {
    let ink = c.text_extents(text).unwrap();
    let font = c.font_extents().unwrap();
    (
        (left + (width - ink.width()) / 2.0 - ink.x_bearing()).round(),
        (y_shift + (height + font.ascent() - font.descent()) / 2.0).round(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use cairo::{FontSlant, FontWeight, Format, ImageSurface};

    #[test]
    fn labels_with_descenders_share_a_baseline_and_stay_centered() {
        let surface = ImageSurface::create(Format::ARgb32, 600, 60).unwrap();
        let c = Context::new(&surface).unwrap();
        c.select_font_face("monospace", FontSlant::Normal, FontWeight::Normal);
        c.set_font_size(32.0);
        let mut baseline = None;
        for (i, text) in ["term", "lock", "snip"].iter().enumerate() {
            let left = i as f64 * 200.0;
            let (x, y) = origin(&c, text, 60.0, left, 180.0, 0.0);
            assert_eq!(*baseline.get_or_insert(y), y);
            let ink = c.text_extents(text).unwrap();
            let center = x + ink.x_bearing() + ink.width() / 2.0;
            assert!((center - (left + 90.0)).abs() <= 0.5);
        }
    }
}
