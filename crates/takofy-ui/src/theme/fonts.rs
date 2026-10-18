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
mod tests {
    use super::*;
    use egui::FontId;

    /// One string per script the bundled faces are expected to cover.
    const SAMPLES: [(&str, &str); 8] = [
        ("latin", "Hana Sō Ünï Señor"),
        ("cyrillic", "Привет"),
        ("greek", "Ωμέγα"),
        ("kana", "かなカナ"),
        ("han", "漢字"),
        ("simplified", "汉字"),
        ("hangul", "한글"),
        ("punctuation", "·–—…→✓"),
    ];

    fn our_keys() -> Vec<String> {
        let mut keys = Vec::new();
        for (family, _, weights) in FILES {
            for weight in weights {
                keys.push(key(family, *weight));
            }
        }
        keys
    }

    fn axes_of(bytes: &'static [u8]) -> Vec<(f32, f32, f32)> {
        FontData::from_static(bytes)
            .variation_axes()
            .iter()
            .map(|axis| (axis.range.min, axis.range.max, axis.default))
            .collect()
    }

    #[test]
    fn every_file_covers_every_weight_asked_of_it() {
        for (family, bytes, weights) in FILES {
            let axes = axes_of(bytes);
            assert_eq!(
                axes.len(),
                1,
                "{family} should carry a single wght axis, came back with {}",
                axes.len()
            );
            let (min, max, _) = axes[0];
            for weight in weights {
                assert!(
                    min <= *weight && *weight <= max,
                    "{family} cannot be asked for {weight}: its axis is {min}..{max}"
                );
            }
        }
    }

    #[test]
    fn the_default_instances_are_the_ones_we_must_override() {
        let defaults: Vec<(String, f32)> = FILES
            .iter()
            .map(|(family, bytes, _)| {
                let (_, _, default) = axes_of(bytes)[0];
                ((*family).to_owned(), default)
            })
            .collect();

        for (family, default) in defaults {
            let expected = match family.as_str() {
                "archivo" => 600.0,
                "jbmono" => 400.0,
                _ => 100.0,
            };
            assert_eq!(
                default, expected,
                "{family} changed its default instance; no coordinate would draw {expected}"
            );
        }
    }

    #[test]
    fn every_face_sets_its_weight_explicitly() {
        let defs = definitions();
        for key in our_keys() {
            let face = &defs.font_data[&key];
            let weight: f32 = key
                .rsplit('-')
                .next()
                .and_then(|digits| digits.parse().ok())
                .expect("a key ends in its weight");
            assert_eq!(
                face.tweak.coords,
                VariationCoords::new([(b"wght", weight)]),
                "{key} does not pin its weight, so it would draw the default instance"
            );
        }
    }

    #[test]
    fn no_two_faces_share_a_key_or_a_setting() {
        let defs = definitions();
        let keys = our_keys();

        // Nothing may be registered that is neither ours nor a face egui brought
        // with it, and a key that never reached `font_data` would be a silently
        // missing face.
        for key in defs.font_data.keys() {
            let ours = keys.contains(key);
            let egui_symbol = FontDefinitions::builtin_font_names().contains(&key.as_str())
                && !EGUI_TEXT_FACES.contains(&key.as_str());
            assert!(ours || egui_symbol, "{key} is neither ours nor egui's");
        }
        assert_eq!(keys.len(), 28, "the face table changed size");

        for family in ["archivo", "jbmono", "jp", "kr", "sc", "tc"] {
            let settings: Vec<VariationCoords> = keys
                .iter()
                .filter(|key| key.starts_with(&format!("{family}-")))
                .map(|key| defs.font_data[key].tweak.coords.clone())
                .collect();
            assert!(!settings.is_empty(), "{family}");
            for (index, first) in settings.iter().enumerate() {
                for second in &settings[index + 1..] {
                    assert_ne!(
                        first, second,
                        "{family} has two faces at one setting, so one is thrown away"
                    );
                }
            }
        }
    }

    #[test]
    fn every_name_in_every_chain_exists() {
        let defs = definitions();
        for (family, chain) in &defs.families {
            assert!(
                !chain.is_empty(),
                "{family:?} is empty, so it draws nothing"
            );
            for name in chain {
                assert!(
                    defs.font_data.contains_key(name),
                    "{family:?} names {name:?}, which has no font data"
                );
            }
        }
    }

    #[test]
    fn the_generic_families_follow_our_faces() {
        let defs = definitions();
        assert_eq!(
            defs.families[&FontFamily::Proportional],
            defs.families[&sans(Weight::Regular)],
        );
        assert_eq!(
            defs.families[&FontFamily::Monospace],
            defs.families[&mono(MonoWeight::Regular)],
        );
    }

    #[test]
    fn egui_text_faces_are_gone_and_its_symbols_remain() {
        let defs = definitions();

        for name in EGUI_TEXT_FACES {
            assert!(!defs.font_data.contains_key(name), "{name} is still loaded");
            for (family, chain) in &defs.families {
                assert!(!chain.iter().any(|entry| entry == name), "{family:?}");
            }
        }

        // Whether egui ships symbol faces depends on its `default_fonts` feature:
        // the binary gets them through eframe, and a bare `egui` build has no
        // builtin fonts at all. Either way every chain ends with exactly the
        // symbol faces that are loaded, and names none that are not.
        let symbols = take_symbol_faces(&mut FontDefinitions::default());
        for (family, chain) in &defs.families {
            for name in FontDefinitions::builtin_font_names().iter().copied() {
                assert_eq!(
                    chain.iter().any(|entry| entry == name),
                    symbols.iter().any(|entry| entry == name),
                    "{family:?} handles egui's {name} inconsistently"
                );
            }
            if !symbols.is_empty() {
                assert_eq!(
                    &chain[chain.len() - symbols.len()..],
                    symbols.as_slice(),
                    "the symbol faces must end a chain"
                );
            }
        }
    }

    #[test]
    fn the_latin_faces_lead_and_the_regional_ones_follow_in_order() {
        let defs = definitions();
        for family in families() {
            let chain = &defs.families[&family];

            // Two Latin faces, then the four regional ones, then whatever egui
            // ends a chain with. A regional face that moved up the chain would
            // take the form of every character it shares with an earlier one.
            for name in &chain[..2] {
                assert!(
                    name.starts_with("archivo-") || name.starts_with("jbmono-"),
                    "{family:?} starts with {name}, which is not a Latin face"
                );
            }

            let regional = &chain[2..6];
            for (name, region) in regional.iter().zip(CJK_ORDER) {
                assert!(
                    name.starts_with(&format!("{region}-")),
                    "{family:?} puts {name} where {region} belongs"
                );
            }

            let weights: Vec<&str> = regional
                .iter()
                .filter_map(|name| name.rsplit('-').next())
                .collect();
            assert!(
                weights.windows(2).all(|pair| pair[0] == pair[1]),
                "{family:?} mixes regional weights: {weights:?}"
            );
        }
    }

    #[test]
    fn the_monospace_stand_in_is_the_nearest_weight() {
        assert_eq!(mono_weight_for(Weight::Regular), MonoWeight::Regular);
        assert_eq!(mono_weight_for(Weight::Medium), MonoWeight::Medium);
        assert_eq!(mono_weight_for(Weight::Semibold), MonoWeight::Medium);
        assert_eq!(mono_weight_for(Weight::Bold), MonoWeight::Bold);
        assert_eq!(mono_weight_for(Weight::Black), MonoWeight::Bold);
    }

    #[test]
    fn every_family_this_module_names_is_registered() {
        let defs = definitions();
        for family in families() {
            assert!(
                defs.families.contains_key(&family),
                "{family:?} is not a registered family"
            );
        }
    }

    /// A missing glyph is invisible to layout — `.notdef` is a full em wide, the
    /// same as a real ideograph — so coverage has to be asked for directly.
    #[test]
    fn every_chain_can_draw_every_script_it_claims() {
        let ctx = egui::Context::default();
        ctx.set_fonts(definitions());
        ctx.set_missing_glyph_policy(egui::MissingGlyphPolicy::Panic);

        // The policy applies from the next pass, and fonts exist only after one.
        let mut warm_up = ctx.run_ui(egui::RawInput::default(), |_ui| {});
        warm_up.textures_delta.clear();

        for family in families() {
            for (script, sample) in SAMPLES {
                let font_id = FontId::new(super::super::text::BASE, family.clone());

                let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                    ui.label(egui::RichText::new(sample).font(font_id.clone()));
                });
                assert!(
                    !output.shapes.is_empty(),
                    "{family:?} drew nothing for {script}"
                );
                output.textures_delta.clear();

                assert!(
                    ctx.fonts_mut(|fonts| fonts.has_glyphs(&font_id, sample)),
                    "{family:?} has no glyph for {script}: {sample:?}"
                );
            }
        }
    }

    /// The order decides forms only because no order can strand a character: the
    /// Chinese face carries every Han character the other three do, so a Chinese
    /// character never falls through to a face that lacks it. A font swap that
    /// broke that would turn a form preference into a missing glyph, and it has to
    /// fail here rather than in a screenshot.
    #[test]
    fn the_han_repertoires_nest() {
        let mut defs = definitions();
        // A family per face, so `characters` answers for that face alone.
        let probes: [(&str, &str); 4] = [
            ("probe-jp", "jp"),
            ("probe-kr", "kr"),
            ("probe-sc", "sc"),
            ("probe-tc", "tc"),
        ];
        for (probe, region) in probes {
            defs.families
                .insert(FontFamily::Name(probe.into()), vec![key(region, 400.0)]);
        }

        let ctx = egui::Context::default();
        ctx.set_fonts(defs);
        let mut warm_up = ctx.run_ui(egui::RawInput::default(), |_ui| {});
        warm_up.textures_delta.clear();

        let han_of = |probe: &str| -> std::collections::BTreeSet<char> {
            let family = FontFamily::Name(probe.into());
            ctx.fonts_mut(|fonts| {
                fonts
                    .characters(&family)
                    .keys()
                    .copied()
                    .filter(|c| matches!(c, '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}'))
                    .collect()
            })
        };

        let chinese = han_of("probe-sc");
        assert!(
            chinese.len() > SAMPLES.len(),
            "the Chinese face came back with {} Han characters, which is not a repertoire",
            chinese.len()
        );

        for (probe, region) in probes {
            if probe == "probe-sc" {
                continue;
            }
            let theirs = han_of(probe);
            assert!(!theirs.is_empty(), "{region} carries no Han at all");
            let stranded: Vec<char> = theirs.difference(&chinese).copied().take(5).collect();
            assert!(
                stranded.is_empty(),
                "{region} carries Han the Chinese face does not, so an order could strand it: {stranded:?}"
            );
        }
    }

    #[test]
    fn a_character_lands_on_the_region_that_carries_it() {
        let ctx = egui::Context::default();
        ctx.set_fonts(definitions());
        let mut warm_up = ctx.run_ui(egui::RawInput::default(), |_ui| {});
        warm_up.textures_delta.clear();

        for family in families() {
            let families_for = |character: char| -> Vec<String> {
                ctx.fonts_mut(|fonts| {
                    fonts
                        .characters(&family)
                        .get(&character)
                        .cloned()
                        .unwrap_or_default()
                })
            };

            let hangul = families_for('한');
            assert!(
                hangul.iter().any(|name| name.starts_with("kr-")),
                "{family:?}: hangul should reach the Korean face, got {hangul:?}"
            );

            let latin_ext = families_for('ō');
            assert!(
                latin_ext.iter().any(|name| name.starts_with("archivo-")),
                "{family:?}: latin-ext should come from the interface face, got {latin_ext:?}"
            );
        }
    }
}
