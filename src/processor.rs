use std::collections::HashMap;
use std::marker::PhantomData;
use std::rc::Rc;

use euclid::Transform2D;
use lopdf::content::Content;
use lopdf::{Dictionary, Document, Object, Stream};
use tracing::{debug, warn};

use crate::error::OutputError;
use crate::fonts::{PdfFont, make_font};
use crate::output::BoundingBoxOutput;
use crate::types::{MediaBox, Transform};
use crate::utils::*;

#[derive(Clone)]
pub(crate) struct TextState<'a> {
    pub(crate) font: Option<Rc<dyn PdfFont + 'a>>,
    pub(crate) font_size: f32,
    pub(crate) character_spacing: f32,
    pub(crate) word_spacing: f32,
    pub(crate) horizontal_scaling: f32,
    pub(crate) leading: f32,
    pub(crate) rise: f32,
    pub(crate) tm: Transform,
}

#[derive(Clone)]
pub(crate) struct GraphicsState<'a> {
    pub(crate) ctm: Transform,
    pub(crate) ts: TextState<'a>,
    pub(crate) smask: Option<Dictionary>,
    pub(crate) line_width: f32,
}

fn show_text(
    gs: &mut GraphicsState,
    s: &[u8],
    _tlm: &Transform,
    _flip_ctm: &Transform,
    output: &mut BoundingBoxOutput,
) -> Result<(), OutputError> {
    let ts = &mut gs.ts;
    let font = ts.font.as_ref().unwrap();
    debug!("{:?}", font.decode(s));
    debug!("{:?}", font.decode(s).as_bytes());
    debug!("{:?}", s);
    output.begin_word()?;

    for (c, length) in font.char_codes(s) {
        // 5.3.3 Text Space Details
        let tsm = Transform2D::new(ts.horizontal_scaling, 0., 0., 1.0, 0., ts.rise);
        // Trm = Tsm × Tm × CTM
        let trm = tsm.then(&ts.tm.then(&gs.ctm));

        let w0 = font.get_width(c) / 1000.;

        let mut spacing = ts.character_spacing;
        // "Word spacing is applied to every occurrence of the single-byte character code 32 in a
        //  string when using a simple font or a composite font that defines code 32 as a
        //  single-byte code. It does not apply to occurrences of the byte value 32 in
        //  multiple-byte codes."
        let is_space = c == 32 && length == 1;
        if is_space {
            spacing += ts.word_spacing
        }

        output.output_character(
            &trm,
            w0,
            spacing,
            font.get_font_name(),
            ts.font_size,
            &font.decode_char(c),
        )?;
        let tj = 0.;
        let ty = 0.;
        let tx = ts.horizontal_scaling * ((w0 - tj / 1000.) * ts.font_size + spacing);
        debug!(
            "horizontal {} adjust {} {} {} {}",
            ts.horizontal_scaling, tx, w0, ts.font_size, spacing
        );
        ts.tm = Transform2D::translation(tx, ty).then(&ts.tm);
        let _trm = gs.ctm.then(&ts.tm);
    }
    output.end_word()?;
    Ok(())
}

fn apply_state(doc: &Document, gs: &mut GraphicsState, state: &Dictionary) {
    for (k, v) in state.iter() {
        let k: &[u8] = k.as_ref();
        match k {
            b"SMask" => match maybe_deref(doc, v) {
                &Object::Name(ref name) => {
                    if name == b"None" {
                        gs.smask = None;
                    } else {
                        panic!("unexpected smask name")
                    }
                }
                &Object::Dictionary(ref dict) => {
                    gs.smask = Some(dict.clone());
                }
                _ => {
                    panic!("unexpected smask type {:?}", v)
                }
            },
            b"Type" => match v {
                &Object::Name(ref name) => {
                    assert_eq!(name, b"ExtGState")
                }
                _ => {
                    panic!("unexpected type")
                }
            },
            _ => {
                debug!("unapplied state: {:?} {:?}", k, v);
            }
        }
    }
}

#[derive(Debug)]
pub(crate) struct Processor<'a> {
    _none: PhantomData<&'a ()>,
}

impl<'a> Processor<'a> {
    pub(crate) fn new() -> Processor<'a> {
        Processor { _none: PhantomData }
    }

    pub(crate) fn process_stream(
        &mut self,
        doc: &'a Document,
        content: Vec<u8>,
        resources: &'a Dictionary,
        media_box: &MediaBox,
        output: &mut BoundingBoxOutput,
        page_num: u32,
    ) -> Result<(), OutputError> {
        let content = match Content::decode(&content) {
            Ok(content) => content,
            Err(e) => {
                warn!(
                    "Failed to decode content stream for page {}: {}. Skipping this content.",
                    page_num, e
                );
                return Ok(());
            }
        };
        let mut font_table = HashMap::new();
        let mut gs: GraphicsState = GraphicsState {
            ts: TextState {
                font: None,
                font_size: std::f32::NAN,
                character_spacing: 0.,
                word_spacing: 0.,
                horizontal_scaling: 100. / 100.,
                leading: 0.,
                rise: 0.,
                tm: Transform2D::identity(),
            },
            line_width: 1.,
            ctm: Transform2D::identity(),
            smask: None,
        };
        let mut gs_stack = Vec::new();
        let mut mc_stack = Vec::new();
        let mut tlm = Transform2D::identity();
        let flip_ctm = Transform2D::new(1., 0., 0., -1., 0., media_box.ury - media_box.lly);
        debug!("MediaBox {:?}", media_box);
        for operation in &content.operations {
            match operation.operator.as_ref() {
                "BT" => {
                    tlm = Transform2D::identity();
                    gs.ts.tm = tlm;
                }
                "ET" => {
                    tlm = Transform2D::identity();
                    gs.ts.tm = tlm;
                }
                "cm" => {
                    assert!(operation.operands.len() == 6);
                    let m = Transform2D::new(
                        as_num(&operation.operands[0]),
                        as_num(&operation.operands[1]),
                        as_num(&operation.operands[2]),
                        as_num(&operation.operands[3]),
                        as_num(&operation.operands[4]),
                        as_num(&operation.operands[5]),
                    );
                    gs.ctm = m.then(&gs.ctm);
                    debug!("matrix {:?}", gs.ctm);
                }
                "TJ" => match operation.operands[0] {
                    Object::Array(ref array) => {
                        for e in array {
                            match e {
                                &Object::String(ref s, _) => {
                                    show_text(&mut gs, s, &tlm, &flip_ctm, output)?;
                                }
                                &Object::Integer(i) => {
                                    let ts = &mut gs.ts;
                                    let w0 = 0.;
                                    let tj = i as f32;
                                    let ty = 0.;
                                    let tx =
                                        ts.horizontal_scaling * ((w0 - tj / 1000.) * ts.font_size);
                                    ts.tm = Transform2D::translation(tx, ty).then(&ts.tm);
                                    debug!("adjust text by: {} {:?}", i, ts.tm);
                                }
                                &Object::Real(i) => {
                                    let ts = &mut gs.ts;
                                    let w0 = 0.;
                                    let tj = i as f32;
                                    let ty = 0.;
                                    let tx =
                                        ts.horizontal_scaling * ((w0 - tj / 1000.) * ts.font_size);
                                    ts.tm = Transform2D::translation(tx, ty).then(&ts.tm);
                                    debug!("adjust text by: {} {:?}", i, ts.tm);
                                }
                                _ => {
                                    debug!("kind of {:?}", e);
                                }
                            }
                        }
                    }
                    _ => {}
                },
                "Tj" => match operation.operands[0] {
                    Object::String(ref s, _) => {
                        show_text(&mut gs, s, &tlm, &flip_ctm, output)?;
                    }
                    _ => {
                        panic!("unexpected Tj operand {:?}", operation)
                    }
                },
                "Tc" => {
                    gs.ts.character_spacing = as_num(&operation.operands[0]);
                }
                "Tw" => {
                    gs.ts.word_spacing = as_num(&operation.operands[0]);
                }
                "Tz" => {
                    gs.ts.horizontal_scaling = as_num(&operation.operands[0]) / 100.;
                }
                "TL" => {
                    gs.ts.leading = as_num(&operation.operands[0]);
                }
                "Tf" => {
                    let fonts: &Dictionary = get(&doc, resources, b"Font");
                    let name = operation.operands[0].as_name().unwrap();
                    let font = font_table
                        .entry(name.to_owned())
                        .or_insert_with(|| make_font(doc, get::<&Dictionary>(doc, fonts, name)))
                        .clone();
                    gs.ts.font = Some(font);

                    gs.ts.font_size = as_num(&operation.operands[1]);
                    debug!(
                        "font {} size: {} {:?}",
                        pdf_to_utf8(name),
                        gs.ts.font_size,
                        operation
                    );
                }
                "Ts" => {
                    gs.ts.rise = as_num(&operation.operands[0]);
                }
                "Tm" => {
                    assert!(operation.operands.len() == 6);
                    tlm = Transform2D::new(
                        as_num(&operation.operands[0]),
                        as_num(&operation.operands[1]),
                        as_num(&operation.operands[2]),
                        as_num(&operation.operands[3]),
                        as_num(&operation.operands[4]),
                        as_num(&operation.operands[5]),
                    );
                    gs.ts.tm = tlm;
                    debug!("Tm: matrix {:?}", gs.ts.tm);
                    output.end_line()?;
                }
                "Td" => {
                    /* Move to the start of the next line, offset from the start of the current line by (tx , ty ).
                      tx and ty are numbers expressed in unscaled text space units.
                      More precisely, this operator performs the following assignments:
                    */
                    assert!(operation.operands.len() == 2);
                    let tx = as_num(&operation.operands[0]);
                    let ty = as_num(&operation.operands[1]);
                    debug!("translation: {} {}", tx, ty);

                    tlm = Transform2D::translation(tx, ty).then(&tlm);
                    gs.ts.tm = tlm;
                    debug!("Td matrix {:?}", gs.ts.tm);
                    output.end_line()?;
                }

                "TD" => {
                    /* Move to the start of the next line, offset from the start of the current line by (tx , ty ).
                      As a side effect, this operator sets the leading parameter in the text state.
                    */
                    assert!(operation.operands.len() == 2);
                    let tx = as_num(&operation.operands[0]);
                    let ty = as_num(&operation.operands[1]);
                    debug!("translation: {} {}", tx, ty);
                    gs.ts.leading = -ty;

                    tlm = Transform2D::translation(tx, ty).then(&tlm);
                    gs.ts.tm = tlm;
                    debug!("TD matrix {:?}", gs.ts.tm);
                    output.end_line()?;
                }

                "T*" => {
                    let tx = 0.0;
                    let ty = -gs.ts.leading;

                    tlm = Transform2D::translation(tx, ty).then(&tlm);
                    gs.ts.tm = tlm;
                    debug!("T* matrix {:?}", gs.ts.tm);
                    output.end_line()?;
                }
                "q" => {
                    gs_stack.push(gs.clone());
                }
                "Q" => {
                    let s = gs_stack.pop();
                    if let Some(s) = s {
                        gs = s;
                    } else {
                        warn!("No state to pop");
                    }
                }
                "gs" => {
                    let ext_gstate: &Dictionary = get(doc, resources, b"ExtGState");
                    let name = operation.operands[0].as_name().unwrap();
                    let state: &Dictionary = get(doc, ext_gstate, name);
                    apply_state(doc, &mut gs, state);
                }
                "i" => {
                    debug!(
                        "unhandled graphics state flattness operator {:?}",
                        operation
                    );
                }
                "w" => {
                    gs.line_width = as_num(&operation.operands[0]);
                }
                "J" | "j" | "M" | "d" | "ri" | "m" | "l" | "c" | "v" | "y" | "h" | "re" | "s"
                | "f*" | "B" | "B*" | "b" | "S" | "F" | "f" | "W" | "w*" | "n" => {
                    // Path operations - ignored (no SVG/HTML output)
                }
                "BMC" | "BDC" => {
                    mc_stack.push(operation);
                }
                "EMC" => {
                    mc_stack.pop();
                }
                "Do" => {
                    // `Do` process an entire subdocument, so we do a recursive call to `process_stream`
                    // with the subdocument content and resources
                    let xobject: &Dictionary = get(&doc, resources, b"XObject");
                    let name = operation.operands[0].as_name().unwrap();
                    let xf: &Stream = get(&doc, xobject, name);
                    let resources = maybe_get_obj(&doc, &xf.dict, b"Resources")
                        .and_then(|n| n.as_dict().ok())
                        .unwrap_or(resources);
                    let contents = get_contents(xf);
                    self.process_stream(&doc, contents, resources, &media_box, output, page_num)?;
                }
                _ => {
                    debug!("unknown operation {:?}", operation);
                }
            }
        }
        Ok(())
    }
}
