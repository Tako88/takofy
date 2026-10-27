//! The bundled faces, and how a weight reaches a `FontFamily`.
//!
//! This app has no system font fallback, so every glyph it can draw comes from
//! one of these files. A chain reads left to right: the first face that carries
//! the character draws it, which is why the regional order below is the whole
//! answer to which form a shared Han character takes.
//!
//! Every face is a variable font read at one set of coordinates. Nothing is cut
//! apart per weight: one blob is shared by every face that uses it.

use std::sync::Arc;

use egui::epaint::text::{Blob, VariationCoords};
use egui::{FontData, FontDefinitions, FontFamily, FontTweak};

use super::{MonoWeight, Weight};

/// The bundled files, and the weights each one is asked for.
///
/// Archivo carries the interface and JetBrains Mono the numbers and field
/// labels. Both default to a weight nobody wants — Archivo to 600, the Noto faces
/// to 100 — so every face below sets its coordinates explicitly.
const FILES: [(&str, &[u8], &[f32]); 6] = [
    (
        "archivo",
        include_bytes!("../../assets/fonts/Archivo-Variable.ttf"),
        &[400.0, 500.0, 600.0, 700.0, 800.0],
    ),
    (
        "jbmono",
        include_bytes!("../../assets/fonts/JetBrainsMono-Variable.ttf"),
        &[400.0, 500.0, 700.0],
    ),
    (
        "jp",
        include_bytes!("../../assets/fonts/NotoSansJP-Variable.ttf"),
        &[400.0, 500.0, 600.0, 700.0, 800.0],
    ),
    (
        "kr",
        include_bytes!("../../assets/fonts/NotoSansKR-Variable.ttf"),
        &[400.0, 500.0, 600.0, 700.0, 800.0],
    ),
    (
        "sc",
        include_bytes!("../../assets/fonts/NotoSansSC-Variable.ttf"),
        &[400.0, 500.0, 600.0, 700.0, 800.0],
    ),
    (
        "tc",
        include_bytes!("../../assets/fonts/NotoSansTC-Variable.ttf"),
        &[400.0, 500.0, 600.0, 700.0, 800.0],
    ),
];

/// The regional faces in fallback order.
///
/// For a character several faces carry, the first name here wins and the rest are
/// never consulted, so this list decides the form of every shared Han character.
/// Which region one wants cannot be read off the string: kana and hangul give a
/// language away, but every face draws them identically, and telling Chinese from
/// Japanese would take a repertoire table this app does not carry. So Japanese
/// leads by decision. It costs Chinese the form of a character and never the
/// character, which is what `the_han_repertoires_nest` holds this to.
const CJK_ORDER: [&str; 4] = ["jp", "kr", "sc", "tc"];

/// egui's own text faces. Its symbol faces stay, at the end of every chain.
const EGUI_TEXT_FACES: [&str; 2] = ["Ubuntu-Light", "Hack"];

fn blob(bytes: &'static [u8]) -> Blob {
    let blob: Blob = Arc::new(bytes);
    blob
}

fn key(family: &str, weight: f32) -> String {
    format!("{family}-{}", weight as u32)
}

fn face(blob: Blob, weight: f32) -> FontData {
    FontData::from_blob(blob, 0).tweak(FontTweak {
        coords: VariationCoords::new([(b"wght", weight)]),
        ..Default::default()
    })
}

/// The face that stands in where the interface face has no weight of its own.
pub fn mono_weight_for(weight: Weight) -> MonoWeight {
    match weight {
        Weight::Regular => MonoWeight::Regular,
        Weight::Medium | Weight::Semibold => MonoWeight::Medium,
        Weight::Bold | Weight::Black => MonoWeight::Bold,
    }
}

/// The interface face at one weight.
pub fn sans(weight: Weight) -> FontFamily {
    FontFamily::Name(format!("sans-{}", weight.value() as u32).into())
}

/// The monospaced face at one weight.
pub fn mono(weight: MonoWeight) -> FontFamily {
    FontFamily::Name(format!("mono-{}", weight.value() as u32).into())
}

/// Every family this module registers, so a caller can walk them all.
pub fn families() -> Vec<FontFamily> {
    let mut families: Vec<FontFamily> = Weight::ALL.iter().copied().map(sans).collect();
    families.extend(MonoWeight::ALL.iter().copied().map(mono));
    families.push(FontFamily::Proportional);
    families.push(FontFamily::Monospace);
    families
}

pub fn definitions() -> FontDefinitions {
    let mut defs = FontDefinitions::default();

    // egui's symbol faces are kept: its own widgets and emoji use private-use and
    // emoji ranges that cannot collide with text, and dropping them would leave
    // holes in egui's chrome rather than in ours.
    let symbols = take_symbol_faces(&mut defs);

    for (family, bytes, weights) in FILES {
        // One allocation per file: every face at every weight shares the bytes.
        let blob = blob(bytes);
        for weight in weights {
            defs.font_data
                .insert(key(family, *weight), Arc::new(face(blob.clone(), *weight)));
        }
    }

    for weight in Weight::ALL {
        let chain = sans_chain(weight, &symbols);
        defs.families.insert(sans(weight), chain);
    }
    for weight in MonoWeight::ALL {
        let chain = mono_chain(weight, &symbols);
        defs.families.insert(mono(weight), chain);
    }

    // egui's own internals reach for these two, and an empty chain draws nothing.
    let proportional = sans_chain(Weight::Regular, &symbols);
    let monospace = mono_chain(MonoWeight::Regular, &symbols);
    defs.families.insert(FontFamily::Proportional, proportional);
    defs.families.insert(FontFamily::Monospace, monospace);

    defs
}

/// Removes egui's own text faces and returns the symbol faces to end every chain
/// with.
///
/// Whether egui ships any at all depends on its `default_fonts` feature, which the
/// binary gets through eframe and a bare `egui` dependency does not. Both cases
/// are handled here: its widgets keep their icons where they exist, and nothing is
/// invented where they do not.
fn take_symbol_faces(defs: &mut FontDefinitions) -> Vec<String> {
    let symbols: Vec<String> = FontDefinitions::builtin_font_names()
        .iter()
        .copied()
        .filter(|name| !EGUI_TEXT_FACES.contains(name))
        .filter(|name| defs.font_data.contains_key(*name))
        .map(str::to_owned)
        .collect();

    for name in EGUI_TEXT_FACES {
        defs.font_data.remove(name);
    }
    symbols
}

fn with_symbols(mut names: Vec<String>, symbols: &[String]) -> Vec<String> {
    names.extend(symbols.iter().cloned());
    names
}

fn local_face_order(weight: f32) -> Vec<String> {
    CJK_ORDER.iter().map(|region| key(region, weight)).collect()
}

fn sans_chain(weight: Weight, symbols: &[String]) -> Vec<String> {
    let mut names = vec![
        key("archivo", weight.value()),
        key("jbmono", mono_weight_for(weight).value()),
    ];
    names.extend(local_face_order(weight.value()));
    with_symbols(names, symbols)
}

fn mono_chain(weight: MonoWeight, symbols: &[String]) -> Vec<String> {
    let mut names = vec![
        key("jbmono", weight.value()),
        key("archivo", weight.value()),
    ];
    names.extend(local_face_order(weight.value()));
    with_symbols(names, symbols)
}

#[cfg(test)]
mod tests;
