use std::io::Read;

use acde::render::Options;
use flate2::read::GzDecoder;

mod common;
use common::Doc;

struct Recording(Vec<u8>);

impl Recording {
    fn load(name: &str) -> Self {
        let path = format!("{}/tests/fixtures/replay/{name}.ops.gz", env!("CARGO_MANIFEST_DIR"));
        let mut bytes = Vec::new();
        GzDecoder::new(std::fs::File::open(&path).unwrap()).read_to_end(&mut bytes).unwrap();
        Self(bytes)
    }

    fn batches(&self) -> impl Iterator<Item = &[u8]> {
        let mut rest = &self.0[..];
        std::iter::from_fn(move || {
            let len = u32::from_le_bytes(rest.get(..4)?.try_into().ok()?) as usize;
            let (batch, tail) = rest[12..].split_at(len);
            rest = tail;
            Some(batch)
        })
    }

    fn replay(&self) -> Replayed {
        let doc = Doc::new(1280, 800, 1.0, &Options { verify_paint: Some(true), ..Options::default() }).unwrap();
        let mut player = Player { doc, ids: vec![0] };
        for (i, batch) in self.batches().enumerate() {
            player.batch(batch);
            for (x, y) in [(640.0, 400.0), (300.0, 200.0), (900.0, 300.0), (640.0, 700.0), (100.0, 50.0)] {
                let _ = player.doc.hit_test(x, y);
            }
            if i % 4 == 3 {
                player.doc.scene();
            }
        }
        player.doc.scene();
        Replayed { doc: player.doc, ids: player.ids }
    }
}

struct Replayed {
    doc: Doc,
    ids: Vec<u32>,
}

struct Player {
    doc: Doc,
    ids: Vec<u32>,
}

impl Player {
    fn id(&self, recorded: u32) -> u32 {
        self.ids.get(recorded as usize).copied().unwrap_or(acde::render::NONE)
    }

    fn batch(&mut self, mut b: &[u8]) {
        fn u32_(b: &mut &[u8]) -> u32 {
            let (head, tail) = b.split_at(4);
            *b = tail;
            u32::from_le_bytes(head.try_into().unwrap())
        }
        fn str_<'a>(b: &mut &'a [u8]) -> &'a str {
            let len = u32_(b) as usize;
            let (head, tail) = b.split_at(len);
            *b = tail;
            std::str::from_utf8(head).unwrap()
        }
        fn created(ids: &mut Vec<u32>, id: acde::render::Result<u32>) -> acde::render::Result<()> {
            ids.push(id.unwrap_or(acde::render::NONE));
            Ok(())
        }
        while let Some((&op, rest)) = b.split_first() {
            b = rest;
            let doc = &self.doc;
            let _ = match op {
                1 => created(&mut self.ids, doc.create_element(str_(&mut b))),
                2 => created(&mut self.ids, doc.create_text(str_(&mut b))),
                3 => {
                    let id = self.id(u32_(&mut b));
                    doc.set_attribute(id, str_(&mut b), str_(&mut b)).map(drop)
                }
                4 => doc.remove_attribute(self.id(u32_(&mut b)), str_(&mut b)).map(drop),
                5 => doc.set_data(self.id(u32_(&mut b)), str_(&mut b)),
                6 => doc.append_child(self.id(u32_(&mut b)), self.id(u32_(&mut b))),
                7 => {
                    let (parent, child, reference) = (u32_(&mut b), u32_(&mut b), u32_(&mut b));
                    match reference {
                        u32::MAX => doc.append_child(self.id(parent), self.id(child)),
                        r if self.id(r) == acde::render::NONE => Ok(()),
                        r => doc.insert_before(self.id(parent), self.id(child), Some(self.id(r))),
                    }
                }
                8 => doc.remove(self.id(u32_(&mut b))),
                9 => doc.add_stylesheet(str_(&mut b)),
                10 => {
                    let ns = b[0];
                    b = &b[1..];
                    created(&mut self.ids, doc.create_element_ns(ns, str_(&mut b)))
                }
                11 => doc.release(self.id(u32_(&mut b))),
                12 => {
                    let id = self.id(u32_(&mut b));
                    let len = u32_(&mut b) as usize;
                    let (commands, tail) = b.split_at(len);
                    b = tail;
                    doc.canvas(id, commands)
                }
                op => panic!("unknown opcode {op}"),
            };
        }
    }
}

#[test]
fn closing_the_editor_leaves_no_removed_boxes() {
    let stats = Recording::load("editor-close").replay().doc.stats();
    assert_eq!((stats.panics, stats.paint_mismatches), (0, 0), "{stats:?}");
}

#[test]
fn icons_in_a_window_opened_after_hidden_are_drawn() {
    let Replayed { doc, ids } = Recording::load("open-browser").replay();
    for (id, what) in [(2499, "title icon"), (576, "address bar lock")] {
        let capture = doc.capture(ids[id], 0.0).unwrap();
        let drawn = capture.as_ref().map_or(0, |c| c.pixels().chunks(4).filter(|p| p[3] > 64).count());
        assert!(drawn > 8, "{what} (node {id}) draws {drawn} pixels");
    }
    let stats = doc.stats();
    assert_eq!((stats.panics, stats.paint_mismatches), (0, 0), "{stats:?}");
}
