use blitz_dom::{QualName, ns};

use super::types::Ns;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Control {
    Editor(bool),
    Value,
    Range,
    Checkable,
    Select,
    Default,
    None,
}

pub(super) fn collapse_whitespace(text: &str) -> String {
    text.split_ascii_whitespace().collect::<Vec<_>>().join(" ")
}

pub(super) enum Piece {
    Text(String),
    Break(usize),
}

pub(super) fn namespace(ns: Ns) -> blitz_dom::Namespace {
    match ns {
        Ns::Html => ns!(html),
        Ns::Svg => ns!(svg),
        Ns::MathMl => ns!(mathml),
        Ns::Other => ns!(),
    }
}

pub(super) fn ns_of(name: &QualName) -> Ns {
    match name.ns {
        ns!(html) => Ns::Html,
        ns!(svg) => Ns::Svg,
        ns!(mathml) => Ns::MathMl,
        _ => Ns::Other,
    }
}

pub(super) fn css_context(
    url: &blitz_dom::style::stylesheets::UrlExtraData,
    rule: blitz_dom::style::stylesheets::CssRuleType,
) -> blitz_dom::style::parser::ParserContext<'_> {
    blitz_dom::style::parser::ParserContext::new(
        blitz_dom::style::stylesheets::Origin::Author,
        url,
        Some(rule),
        blitz_dom::style_traits::ParsingMode::DEFAULT,
        blitz_dom::style::context::QuirksMode::NoQuirks,
        Default::default(),
        None,
        None,
        Default::default(),
    )
}

pub(crate) fn valid_float(text: &str) -> Option<f64> {
    let bytes = text.as_bytes();
    let mut i = usize::from(bytes.first() == Some(&b'-'));
    let digits = |i: &mut usize| {
        let start = *i;
        while bytes.get(*i).is_some_and(u8::is_ascii_digit) {
            *i += 1;
        }
        *i > start
    };
    let whole = digits(&mut i);
    let fraction = bytes.get(i) == Some(&b'.') && {
        i += 1;
        digits(&mut i)
    };
    if !whole && !fraction {
        return None;
    }
    if matches!(bytes.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(bytes.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        if !digits(&mut i) {
            return None;
        }
    }
    (i == bytes.len()).then(|| text.parse::<f64>().ok()).flatten().filter(|v| v.is_finite())
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RangeValue {
    pub(crate) min: f64,
    pub(crate) max: f64,
    pub(crate) value: f64,
}

impl RangeValue {
    pub(crate) fn new(min: Option<&str>, max: Option<&str>, step: Option<&str>, value: Option<&str>) -> Self {
        let places = |text: Option<&str>| text.and_then(|t| valid_float(t).map(|_| t)).map_or(0, decimal_places);
        let precision = places(min).max(places(step));
        let min = min.and_then(valid_float).unwrap_or(0.0);
        let max = max.and_then(valid_float).unwrap_or(100.0).max(min);
        let step = match step.map(str::trim) {
            Some(s) if s.eq_ignore_ascii_case("any") => None,
            Some(s) => Some(valid_float(s).filter(|v| *v > 0.0).unwrap_or(1.0)),
            None => Some(1.0),
        };
        let fallback = min + (max - min) / 2.0;
        let mut v = value.and_then(valid_float).unwrap_or(fallback).clamp(min, max);
        if let Some(step) = step {
            let scale = 10f64.powi(precision.min(15) as i32);
            let (lo, hi, unit) = ((min * scale).round(), (max * scale + 1e-6).floor(), (step * scale).round().max(1.0));
            let at = (v * scale).round();
            let mut snapped = lo + ((at - lo) / unit + 0.5).floor() * unit;
            if snapped > hi {
                snapped = lo + ((hi - lo) / unit).floor() * unit;
            }
            v = snapped / scale;
        }
        Self { min, max, value: v }
    }

    pub(crate) fn fraction(&self) -> f64 {
        match self.max > self.min {
            true => (self.value - self.min) / (self.max - self.min),
            false => 0.0,
        }
    }
}

pub(crate) fn number_string(value: f64) -> String {
    if value == 0.0 {
        return "0".into();
    }
    if value.fract() == 0.0 && value.abs() < 1e21 {
        return format!("{value:.0}");
    }
    format!("{value}")
}

pub(super) fn decimal_places(text: &str) -> usize {
    let lower = text.to_ascii_lowercase();
    let (mantissa, exponent) = lower.split_once('e').map_or((lower.as_str(), 0i64), |(m, e)| (m, e.parse().unwrap_or(0)));
    let fraction = mantissa.split_once('.').map_or(0, |(_, f)| f.len()) as i64;
    (fraction - exponent).max(0) as usize
}
