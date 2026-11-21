use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

use peniko::{Blob, FontData};

const MASKS: [u8; 4] = *b"EBLC";
const HIDDEN: [u8; 4] = *b"EBL ";
const OUTLINES: [[u8; 4]; 3] = [*b"glyf", *b"CFF ", *b"CFF2"];

static SEEN: LazyLock<Mutex<HashMap<u64, Option<FontData>>>> = LazyLock::new(Mutex::default);

pub fn outlined(font: &FontData) -> Option<FontData> {
    let mut seen = SEEN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    seen.entry(font.data.id()).or_insert_with(|| without_masks(font)).clone()
}

fn be16(bytes: &[u8], at: usize) -> Option<usize> {
    Some(usize::from(u16::from_be_bytes(bytes.get(at..at + 2)?.try_into().ok()?)))
}

fn be32(bytes: &[u8], at: usize) -> Option<usize> {
    usize::try_from(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?)).ok()
}

fn directories(bytes: &[u8]) -> Vec<usize> {
    if !bytes.starts_with(b"ttcf") {
        return vec![0];
    }
    (0..be32(bytes, 8).unwrap_or(0)).map_while(|i| be32(bytes, 12 + 4 * i)).collect()
}

fn records(bytes: &[u8], directory: usize) -> impl Iterator<Item = (usize, [u8; 4])> + '_ {
    (0..be16(bytes, directory + 4).unwrap_or(0)).map_while(move |i| {
        let at = directory + 12 + 16 * i;
        Some((at, bytes.get(at..at + 4)?.try_into().ok()?))
    })
}

fn without_masks(font: &FontData) -> Option<FontData> {
    let bytes = font.data.data();
    let all = directories(bytes);
    let own = *all.get(usize::try_from(font.index).ok()?)?;
    let has = |tag: [u8; 4]| records(bytes, own).any(|(_, t)| t == tag);
    if !has(MASKS) || !OUTLINES.into_iter().any(has) {
        return None;
    }
    let mut patched = bytes.to_vec();
    for directory in all {
        for (at, tag) in records(bytes, directory) {
            if tag == MASKS {
                patched[at..at + 4].copy_from_slice(&HIDDEN);
            }
        }
    }
    Some(FontData::new(Blob::new(Arc::new(patched)), font.index))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn font(tags: &[&[u8; 4]]) -> Vec<u8> {
        let mut bytes = vec![0, 1, 0, 0];
        bytes.extend((tags.len() as u16).to_be_bytes());
        bytes.extend([0; 6]);
        for tag in tags {
            bytes.extend(*tag);
            bytes.extend([0; 12]);
        }
        bytes
    }

    fn data(bytes: Vec<u8>) -> FontData {
        FontData::new(Blob::new(Arc::new(bytes)), 0)
    }

    #[test]
    fn mask_strikes_are_hidden_only_where_outlines_exist() {
        let outlined = without_masks(&data(font(&[b"EBDT", b"EBLC", b"glyf", b"head"]))).expect("an outline font with mask strikes");
        let tags: Vec<[u8; 4]> = records(outlined.data.data(), 0).map(|(_, t)| t).collect();
        assert_eq!(tags, [*b"EBDT", *b"EBL ", *b"glyf", *b"head"]);
        assert!(without_masks(&data(font(&[b"EBDT", b"EBLC", b"head"]))).is_none(), "bitmap-only fonts keep their strikes");
        assert!(without_masks(&data(font(&[b"CBDT", b"CBLC", b"glyf"]))).is_none(), "color strikes stay");
        assert!(without_masks(&data(font(&[b"glyf", b"head"]))).is_none());
    }
}
