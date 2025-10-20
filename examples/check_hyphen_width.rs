extern crate ttf_parser;

use std::fs;

fn main() {
    let font_data = fs::read("/tmp/extras/ttf/Inter-SemiBold.ttf").expect("Failed to read font");
    let face = ttf_parser::Face::parse(&font_data, 0).expect("Failed to parse font");

    println!("Checking dash/hyphen characters in original Inter-SemiBold:\n");

    let chars_to_check = [
        ('-', "hyphen-minus U+002D"),
        ('–', "en-dash U+2013"),
        ('—', "em-dash U+2014"),
    ];

    for (ch, name) in chars_to_check.iter() {
        if let Some(cmap_table) = face.tables().cmap {
            for subtable in cmap_table.subtables {
                if let Some(gid) = subtable.glyph_index(*ch as u32) {
                    if let Some(width) = face.glyph_hor_advance(gid) {
                        let units_per_em = face.units_per_em() as f32;
                        let width_normalized = (width as f32 / units_per_em) * 1000.0;
                        println!(
                            "{}: GID {} → width {} ({:.2} normalized)",
                            name, gid.0, width, width_normalized
                        );
                    }
                    break;
                }
            }
        }
    }

    println!("\nPDF embedded font GID 1374 has normalized width: 465.91");
    println!("Does this match any of the above?");
}
