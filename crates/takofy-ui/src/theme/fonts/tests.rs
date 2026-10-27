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

/// One string each bundled file must shape on its own.
///
/// epaint parses a blob lazily, the first time a face draws with it, and
/// panics at its own parse site when the file is unreadable — so a corrupted
/// asset is a crash on startup rather than a quiet fallback. Every face
/// drawing its sample here turns that into a test failure.
///
/// Japanese and Traditional Chinese carry no BMP codepoint the other three
/// lack, so their samples come from the shared repertoire and pin only that
/// this file, alone, draws them. Hangul is Korean's and the simplified forms
/// are Chinese's, per the table in `assets/fonts/README.md`.
const FACE_SAMPLES: [(&str, &str); 6] = [
    ("archivo", "Hana Sō Ünï Señor"),
    ("jbmono", "fn main() -> i32 { 0x2A }"),
    ("jp", "かなカナ漢字"),
    ("kr", "한글"),
    ("sc", "汉字"),
    ("tc", "漢字"),
];

/// A missing glyph is invisible to layout — `.notdef` is a full em wide, the
/// same as a real ideograph — so coverage has to be asked for directly.
#[test]
fn every_face_parses_and_shapes_its_sample() {
    // A family per face, so the sample can only be drawn by its own file.
    let mut defs = definitions();
    let keys = our_keys();
    for key in &keys {
        let probe = FontFamily::Name(format!("probe-{key}").into());
        defs.families.insert(probe, vec![key.clone()]);
    }

    let ctx = egui::Context::default();
    ctx.set_fonts(defs);
    ctx.set_missing_glyph_policy(egui::MissingGlyphPolicy::Panic);

    // The policy applies from the next pass, and fonts exist only after one.
    let mut warm_up = ctx.run_ui(egui::RawInput::default(), |_ui| {});
    warm_up.textures_delta.clear();

    for key in keys {
        let family = key.split_once('-').expect("a key ends in its weight").0;
        let sample = FACE_SAMPLES
            .iter()
            .find(|(name, _)| *name == family)
            .map(|(_, sample)| *sample)
            .expect("every bundled family has a sample");
        let probe = FontFamily::Name(format!("probe-{key}").into());
        let font_id = FontId::new(super::super::text::BASE, probe);

        // Shaping the sample is what parses the blob, so this frame is the
        // parse: an unreadable file panics here instead of in the window.
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.label(egui::RichText::new(sample).font(font_id.clone()));
        });
        output.textures_delta.clear();
        assert!(
            !output.shapes.is_empty(),
            "{key} drew nothing for {sample:?}"
        );
        assert!(
            ctx.fonts_mut(|fonts| fonts.has_glyphs(&font_id, sample)),
            "{key} has no glyph for {sample:?}"
        );
    }
}

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
