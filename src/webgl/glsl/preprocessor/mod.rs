// acde: pp-rs 0.2.1 (Corentin Wallez, BSD-3-Clause, LICENSE.BSD), the preprocessor of this GLSL frontend, see docs/agents/graphics/vendor.md
#[allow(clippy::match_like_matches_macro)]
pub(crate) mod lexer;
pub(crate) mod pp;
pub(crate) mod token;

#[cfg(test)]
mod lexer_tests;
#[cfg(test)]
mod pp_tests;
