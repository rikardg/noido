//! The bundled fonts, unpacked once, and the parley context that knows only
//! them.

use std::io::Read;
use std::sync::{Arc, OnceLock};

use flate2::read::ZlibDecoder;
use parley::FontContext;
use parley::fontique::{Blob, Collection, CollectionOptions};

/// The families the house style sets, each with its fallbacks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Family {
    Body,
    Heading,
    Code,
}

impl Family {
    /// The parley font stack, in CSS list syntax.
    pub(crate) fn stack(self) -> &'static str {
        match self {
            Family::Body => "Libron, Libertinus Serif, NewComputerModern",
            Family::Heading => "Libertinus Sans, Libertinus Serif, NewComputerModern",
            Family::Code => "Monaspace Neon, NewComputerModern",
        }
    }
}

/// An index into the unpacked faces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FaceId(pub u16);

/// The bundled faces, inflated, and a font collection holding them and
/// nothing else (no system fonts).
pub(crate) struct Fonts {
    collection: Collection,
    faces: Vec<Arc<Vec<u8>>>,
}

impl Fonts {
    fn unpack() -> Self {
        let mut collection = Collection::new(CollectionOptions {
            shared: false,
            system_fonts: false,
        });
        let mut faces = Vec::new();
        for face in noido_fonts::faces() {
            let mut data = Vec::new();
            ZlibDecoder::new(face.deflated)
                .read_to_end(&mut data)
                .expect("a bundled font inflates");
            let data = Arc::new(data);
            collection.register_fonts(Blob::new(data.clone()), None);
            faces.push(data);
        }
        Fonts { collection, faces }
    }

    /// A parley font context that knows the bundled fonts only. It starts
    /// with an empty source cache: keep one per layout pass, not one per
    /// paragraph.
    pub(crate) fn context(&self) -> FontContext {
        FontContext {
            collection: self.collection.clone(),
            source_cache: Default::default(),
        }
    }

    /// Which face a shaped run's font data is: the data parley hands back
    /// is the registered buffer itself.
    pub(crate) fn face_of(&self, data: &[u8]) -> Option<FaceId> {
        let i = self
            .faces
            .iter()
            .position(|f| std::ptr::eq(f.as_ptr(), data.as_ptr()) && f.len() == data.len())
            .or_else(|| self.faces.iter().position(|f| f.as_slice() == data))?;
        Some(FaceId(i as u16))
    }

    /// The font file's bytes.
    pub(crate) fn data(&self, face: FaceId) -> &[u8] {
        &self.faces[usize::from(face.0)]
    }
}

/// The bundled fonts, unpacked on the first call.
pub(crate) fn fonts() -> &'static Fonts {
    static FONTS: OnceLock<Fonts> = OnceLock::new();
    FONTS.get_or_init(Fonts::unpack)
}

#[cfg(test)]
mod tests {
    use super::*;
    use parley::{FontFamily, LayoutContext, StyleProperty};

    /// The font data of the first run of "a" shaped with `stack`.
    fn shape_a(stack: &str) -> Option<Vec<u8>> {
        let mut font_cx = fonts().context();
        let mut layout_cx: LayoutContext<()> = LayoutContext::new();
        let mut b = layout_cx.ranged_builder(&mut font_cx, "a", 1.0, true);
        b.push_default(StyleProperty::FontFamily(FontFamily::from(stack)));
        let mut layout = b.build("a");
        layout.break_all_lines(None);
        let line = layout.lines().next()?;
        let run = line.runs().next()?;
        Some(run.font().data.data().to_vec())
    }

    fn is_ours(data: &[u8]) -> bool {
        let f = fonts();
        (0..f.faces.len() as u16).any(|i| f.data(FaceId(i)) == data)
    }

    #[test]
    fn fonts_unpack_once() {
        assert!(std::ptr::eq(fonts(), fonts()));
    }

    /// The bundled family a font file belongs to.
    fn family_of(data: &[u8]) -> Option<&'static str> {
        let f = fonts();
        let i = (0..f.faces.len() as u16).find(|&i| f.data(FaceId(i)) == data)?;
        Some(noido_fonts::faces()[usize::from(i)].family)
    }

    #[test]
    fn every_family_stack_resolves_to_its_first_family() {
        for (family, first) in [
            (Family::Body, "Libron"),
            (Family::Heading, "Libertinus Sans"),
            (Family::Code, "Monaspace Neon"),
        ] {
            let data = shape_a(family.stack()).expect("a run");
            assert_eq!(family_of(&data), Some(first), "{family:?}");
        }
    }

    #[test]
    fn no_system_font_is_seen() {
        if let Some(data) = shape_a("Helvetica") {
            assert!(is_ours(&data));
        }
    }
}
