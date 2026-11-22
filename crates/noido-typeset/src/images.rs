//! The pictures of a book: where to find them and how big they are drawn.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use image::RgbaImage;
use noido_engine::PdfBytes;

/// Points to a CSS pixel.
const POINT: f32 = 0.75;

/// One image of a book, by the order its size was first read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ImageId(u32);

#[cfg(test)]
impl ImageId {
    pub(crate) fn test(n: u32) -> Self {
        ImageId(n)
    }
}

/// An image and its size in pixels.
type Sized = (ImageId, (u32, u32));

/// What the cache of pixels may hold, in bytes of pixels.
const CACHE_BYTES: usize = 64 * 1024 * 1024;

/// A cached image: a decoded one (the whole image), or one resized to a
/// width and height.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Key {
    Decoded(ImageId),
    Sized(ImageId, u32, u32),
}

/// What is kept of the pictures drawn: decoded and resized images
/// together, the least recently used out first.
struct Cache {
    /// `None` is a decode that failed, remembered. The `u64` is the
    /// last use.
    held: HashMap<Key, (Option<Arc<RgbaImage>>, u64)>,
    clock: u64,
    bytes: usize,
    /// What it may hold, in bytes of pixels.
    budget: usize,
    /// The decodes under way, by image: whoever finds one waits on it
    /// instead of decoding again. Made outside the lock the cache is
    /// under, so that a slow decode holds up only those who want its
    /// image.
    deciding: HashMap<ImageId, Arc<Decoding>>,
}

/// A decode that one thread makes and the others wait for.
type Decoding = OnceLock<Option<Arc<RgbaImage>>>;

impl Default for Cache {
    fn default() -> Self {
        Cache::with_budget(CACHE_BYTES)
    }
}

impl Cache {
    fn with_budget(budget: usize) -> Self {
        Cache {
            held: HashMap::new(),
            clock: 0,
            bytes: 0,
            budget,
            deciding: HashMap::new(),
        }
    }

    fn get(&mut self, key: Key) -> Option<Option<Arc<RgbaImage>>> {
        self.clock += 1;
        let clock = self.clock;
        let (image, used) = self.held.get_mut(&key)?;
        *used = clock;
        Some(image.clone())
    }

    fn put(&mut self, key: Key, image: Option<Arc<RgbaImage>>) {
        self.clock += 1;
        self.bytes += image.as_ref().map_or(0, |i| bytes_of(i));
        // A key put again is counted once: the old image's bytes go.
        if let Some((Some(old), _)) = self.held.insert(key, (image, self.clock)) {
            self.bytes -= bytes_of(&old);
        }
        // The newest stays, even alone over the limit.
        while self.bytes > self.budget && self.held.len() > 1 {
            let oldest = self
                .held
                .iter()
                .min_by_key(|(_, (_, used))| *used)
                .map(|(key, _)| *key);
            if let Some((Some(gone), _)) = oldest.and_then(|key| self.held.remove(&key)) {
                self.bytes -= bytes_of(&gone);
            }
        }
    }
}

fn bytes_of(image: &RgbaImage) -> usize {
    image.width() as usize * image.height() as usize * 4
}

/// The images of a book. Sizes are read while it is laid out, the first
/// time each is asked for; pixels when a page is drawn.
pub(crate) struct Images {
    /// The EPUB's bytes; none for markdown, which has no images.
    bytes: Option<PdfBytes>,
    /// By path in the archive; `None` for an image that cannot be read.
    known: HashMap<String, Option<Sized>>,
    next: u32,
    /// Where each image is, by id.
    paths: Vec<String>,
    cache: Mutex<Cache>,
    /// Decodes made, for the tests.
    #[cfg(test)]
    pub(crate) decodes: std::sync::atomic::AtomicUsize,
    /// Stands in for reading and decoding an image, for the tests.
    #[cfg(test)]
    decoder: Mutex<Option<TestDecoder>>,
}

#[cfg(test)]
type TestDecoder = Arc<dyn Fn(ImageId) -> Option<RgbaImage> + Send + Sync>;

impl Images {
    /// No images: markdown's.
    pub(crate) fn none() -> Self {
        Images {
            bytes: None,
            known: HashMap::new(),
            next: 0,
            paths: Vec::new(),
            cache: Mutex::default(),
            #[cfg(test)]
            decodes: Default::default(),
            #[cfg(test)]
            decoder: Mutex::new(None),
        }
    }

    /// The images of the EPUB made of `bytes`.
    pub(crate) fn of_epub(bytes: PdfBytes) -> Self {
        Images {
            bytes: Some(bytes),
            ..Self::none()
        }
    }

    /// The image at `path` in the archive and its size in pixels; `None`
    /// when there is none, or its header cannot be read.
    pub(crate) fn size(&mut self, path: &str) -> Option<Sized> {
        if let Some(known) = self.known.get(path) {
            return *known;
        }
        let found = self
            .bytes
            .as_ref()
            .and_then(|bytes| noido_epub::image_size(bytes, path))
            .map(|size| {
                self.paths.push(path.to_string());
                self.next += 1;
                (ImageId(self.next - 1), size)
            });
        self.known.insert(path.to_string(), found);
        found
    }
}

impl Images {
    /// The image drawn `width` x `height` pixels, premultiplied; `None`
    /// when it cannot be decoded, which is remembered. Decoded once,
    /// from its full resolution, however many threads ask.
    pub(crate) fn pixels(&self, id: ImageId, width: u32, height: u32) -> Option<Arc<RgbaImage>> {
        let sized = Key::Sized(id, width, height);
        let whole = {
            let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(hit) = cache.get(sized) {
                return hit;
            }
            match cache.get(Key::Decoded(id)) {
                Some(held) => held,
                None => {
                    // One thread decodes, the others wait for it, and
                    // none holds the cache meanwhile.
                    let cell = cache.deciding.entry(id).or_default().clone();
                    drop(cache);
                    cell.get_or_init(|| {
                        let made = self.decode(id).map(Arc::new);
                        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
                        cache.put(Key::Decoded(id), made.clone());
                        cache.deciding.remove(&id);
                        made
                    })
                    .clone()
                }
            }
        }?;
        if whole.dimensions() == (width, height) {
            return Some(whole);
        }
        let drawn = Arc::new(noido_image::resized((*whole).clone(), width, height));
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        // Another thread may have made it meanwhile; keep that one.
        if let Some(Some(made)) = cache.get(sized) {
            return Some(made);
        }
        cache.put(sized, Some(drawn.clone()));
        Some(drawn)
    }

    fn decode(&self, id: ImageId) -> Option<RgbaImage> {
        #[cfg(test)]
        {
            self.decodes
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let decoder = self.decoder.lock().unwrap().clone();
            if let Some(decoder) = decoder {
                return decoder(id);
            }
        }
        let bytes = self.bytes.as_ref()?;
        let data = noido_epub::image_data(bytes, self.paths.get(id.0 as usize)?)?;
        noido_image::decode(&data).ok()
    }
}

/// An image's size in points: its pixels as CSS pixels, unless the
/// `<img>` gave a width or a height (one alone keeps the aspect). `None`
/// when a side is 0.
pub(crate) fn natural(
    px: (u32, u32),
    width: Option<u32>,
    height: Option<u32>,
) -> Option<(f32, f32)> {
    if px.0 == 0 || px.1 == 0 {
        return None;
    }
    let (w, h) = (px.0 as f32, px.1 as f32);
    let (w, h) = match (width, height) {
        (Some(width), Some(height)) => (width as f32, height as f32),
        (Some(width), None) => (width as f32, width as f32 * h / w),
        (None, Some(height)) => (height as f32 * w / h, height as f32),
        (None, None) => (w, h),
    };
    Some((w * POINT, h * POINT))
}

/// `size` shrunk to fit within `max`, the aspect kept; enlarged to fill
/// it when `enlarge`.
pub(crate) fn fit(size: (f32, f32), max: (f32, f32), enlarge: bool) -> (f32, f32) {
    let (w, h) = size;
    let (across, down) = (max.0 / w, max.1 / h);
    if across.min(down) >= 1.0 && !enlarge {
        return size;
    }
    // The side that limits is the maximum itself, not a product that
    // might miss it by a rounding.
    if across <= down {
        (max.0, h * max.0 / w)
    } else {
        (w * max.1 / h, max.1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_is_three_quarters_of_a_pixel() {
        assert_eq!(natural((800, 100), None, None), Some((600.0, 75.0)));
    }

    #[test]
    fn one_attribute_keeps_the_aspect() {
        assert_eq!(natural((20, 20), Some(40), None), Some((30.0, 30.0)));
    }

    #[test]
    fn a_zero_side_has_no_size() {
        assert_eq!(natural((0, 5), None, None), None);
    }

    #[test]
    fn fit_shrinks_and_never_enlarges() {
        assert_eq!(fit((600.0, 75.0), (270.0, 434.0), false), (270.0, 33.75));
        assert_eq!(fit((15.0, 15.0), (270.0, 434.0), false), (15.0, 15.0));
    }

    fn image(side: u32) -> Option<Arc<RgbaImage>> {
        Some(Arc::new(RgbaImage::new(side, side)))
    }

    #[test]
    fn a_replaced_entry_is_counted_once() {
        let mut cache = Cache::with_budget(1000);
        let key = Key::Sized(ImageId(0), 4, 4);
        cache.put(key, image(4));
        cache.put(key, image(4));
        assert_eq!(cache.bytes, 64);
        cache.put(key, None);
        assert_eq!(cache.bytes, 0);
    }

    #[test]
    fn the_least_recently_used_goes_first_and_the_newest_stays() {
        // Each 4 x 4 image is 64 bytes; room for two.
        let mut cache = Cache::with_budget(150);
        let key = |n| Key::Sized(ImageId(n), 4, 4);
        cache.put(key(0), image(4));
        cache.put(key(1), image(4));
        // A use makes 0 newer than 1.
        assert!(cache.get(key(0)).is_some());
        cache.put(key(2), image(4));
        assert!(cache.get(key(1)).is_none(), "1 was the least recently used");
        assert!(cache.get(key(0)).is_some() && cache.get(key(2)).is_some());
        assert_eq!(cache.bytes, 128);
        // One alone over the budget is kept, and the rest go.
        cache.put(key(3), image(10));
        assert!(cache.get(key(3)).is_some());
        assert_eq!(cache.held.len(), 1);
        assert_eq!(cache.bytes, 400);
    }

    fn side(n: u32) -> RgbaImage {
        RgbaImage::new(n, n)
    }

    #[test]
    fn a_decode_in_progress_does_not_hold_the_cache() {
        use std::sync::atomic::Ordering;
        use std::sync::mpsc::channel;
        let images = Images::none();
        let (entered, entered_rx) = channel();
        let (release, release_rx) = channel::<()>();
        let (entered, release_rx) = (Mutex::new(entered), Mutex::new(release_rx));
        *images.decoder.lock().unwrap() = Some(Arc::new(move |id| {
            if id == ImageId(0) {
                // The slow one: say so, and wait to be let go.
                entered.lock().unwrap().send(()).unwrap();
                release_rx.lock().unwrap().recv().unwrap();
            }
            Some(side(8))
        }));
        // Image 1 is held, whole and at a size.
        assert!(images.pixels(ImageId(1), 8, 8).is_some());
        assert!(images.pixels(ImageId(1), 4, 4).is_some());
        std::thread::scope(|s| {
            let slow: Vec<_> = (0..2)
                .map(|_| s.spawn(|| images.pixels(ImageId(0), 8, 8)))
                .collect();
            entered_rx.recv().unwrap();
            // Image 0 is mid-decode: the cache is free, and hits are
            // served, and a new decode of another image is made.
            let free = images.cache.try_lock().is_ok();
            if free {
                let hit = images.pixels(ImageId(1), 4, 4).unwrap();
                assert_eq!(hit.dimensions(), (4, 4));
                let hit = images.pixels(ImageId(1), 2, 2).unwrap();
                assert_eq!(hit.dimensions(), (2, 2));
                assert!(images.pixels(ImageId(2), 8, 8).is_some());
            }
            release.send(()).unwrap();
            for t in slow {
                assert!(t.join().unwrap().is_some());
            }
            assert!(free, "the cache was held by a decode");
        });
        // One decode of each image, however many asked.
        assert_eq!(images.decodes.load(Ordering::SeqCst), 3);
        assert!(images.cache.lock().unwrap().deciding.is_empty());
    }

    #[test]
    fn a_failed_decode_is_remembered() {
        use std::sync::atomic::Ordering;
        let images = Images::none();
        *images.decoder.lock().unwrap() = Some(Arc::new(|_| None));
        assert!(images.pixels(ImageId(0), 8, 8).is_none());
        assert!(images.pixels(ImageId(0), 4, 4).is_none());
        assert_eq!(images.decodes.load(Ordering::SeqCst), 1);
    }
}
