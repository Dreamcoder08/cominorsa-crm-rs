//! Locks WCAG AA contrast for every solid text and background token pair.

const STYLE: &str = include_str!("../src/style.css");

const PAIRS: [(&str, &str); 15] = [
    ("--text", "--canvas"),
    ("--text", "--surface"),
    ("--text", "--subtle"),
    ("--muted", "--canvas"),
    ("--muted", "--surface"),
    ("--muted", "--subtle"),
    ("--accent", "--canvas"),
    ("--accent", "--surface"),
    ("--on-accent", "--accent"),
    ("--overdue", "--overdue-soft"),
    ("--overdue", "--surface"),
    ("--today", "--today-soft"),
    ("--today", "--surface"),
    ("--closed", "--closed-soft"),
    ("--closed", "--surface"),
];

fn token(name: &str) -> (String, String) {
    let line = STYLE
        .lines()
        .find(|line| line.trim_start().starts_with(&format!("{name}:")))
        .unwrap_or_else(|| panic!("missing token {name}"));
    let inner = line
        .split("light-dark(")
        .nth(1)
        .and_then(|rest| rest.split(')').next())
        .unwrap_or_else(|| panic!("{name} must use light-dark()"));
    let (light, dark) = inner.split_once(',').unwrap();

    (light.trim().to_owned(), dark.trim().to_owned())
}

fn luminance(hex: &str) -> f64 {
    let channel = |range: std::ops::Range<usize>| {
        let value = f64::from(u8::from_str_radix(&hex[range], 16).unwrap()) / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };

    0.2126 * channel(1..3) + 0.7152 * channel(3..5) + 0.0722 * channel(5..7)
}

fn ratio(first: &str, second: &str) -> f64 {
    let (a, b) = (luminance(first), luminance(second));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

#[test]
fn every_token_pair_meets_aa_in_both_themes() {
    for (foreground, background) in PAIRS {
        let (fg_light, fg_dark) = token(foreground);
        let (bg_light, bg_dark) = token(background);

        for (theme, fg, bg) in [("light", fg_light, bg_light), ("dark", fg_dark, bg_dark)] {
            let measured = ratio(&fg, &bg);
            assert!(
                measured >= 4.5,
                "{foreground} on {background} in {theme}: {measured:.2}"
            );
        }
    }
}
