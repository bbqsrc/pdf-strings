extern crate ttf_parser;

use std::fs;

fn main() {
    let font_data = fs::read("/tmp/extras/ttf/Inter-SemiBold.ttf").expect("Failed to read font");
    let face = ttf_parser::Face::parse(&font_data, 0).expect("Failed to parse font");

    println!("Inter-SemiBold has {} glyphs", face.number_of_glyphs());

    let target_gids = [1374, 1408, 1441];

    for &gid_num in &target_gids {
        let gid = ttf_parser::GlyphId(gid_num);

        println!("\n=== GID {} ===", gid_num);

        if let Some(name) = face.glyph_name(gid) {
            println!("  Name: {}", name);
        } else {
            println!("  Name: (none)");
        }

        if let Some(cmap_table) = face.tables().cmap {
            for subtable in cmap_table.subtables {
                subtable.codepoints(|codepoint| {
                    if let Some(mapped_gid) = subtable.glyph_index(codepoint) {
                        if mapped_gid.0 == gid_num {
                            println!(
                                "  Unicode: U+{:04X} '{}'",
                                codepoint,
                                char::from_u32(codepoint).unwrap_or('?')
                            );
                        }
                    }
                });
            }
        }

        if let Some(width) = face.glyph_hor_advance(gid) {
            let units_per_em = face.units_per_em() as f32;
            let width_normalized = (width as f32 / units_per_em) * 1000.0;
            println!(
                "  Width: {} units ({:.2} normalized)",
                width, width_normalized
            );
        }
    }
}
