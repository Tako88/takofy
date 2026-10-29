//! The stations shelf: a two-column band built from the library's own seeds.

use takofy_model::home::{HomeAnswer, HomeAsk};

use crate::message::Failure;

use super::Seed;

/// What the stations shelf holds between opens.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct State;

impl State {
    /// Starts this open, from the page's own seed.
    pub fn open(&mut self, _seed: Seed) {}

    /// Folds one answer the shelf asked for into its state.
    pub fn land(&mut self, _answer: &HomeAnswer) {}

    /// Records that one of the shelf's reads failed.
    pub fn fail(&mut self, _ask: &HomeAsk, _failure: Failure) {}

    /// Drops everything the last open read, so a retry asks again.
    pub fn forget(&mut self) {}
}

/// What the stations shelf draws, borrowed from the library for one frame.
///
/// It owns nothing: everything it draws is a list the library already holds, so
/// a frame costs no copies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Band<'a>(std::marker::PhantomData<&'a ()>);
