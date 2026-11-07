//! Shared-memory ring buffer for preview frames (ADR-004): the engine writes, the UI reads.
//!
//! The ring is a file in the private runtime directory, mapped by both processes. One writer,
//! any number of readers. Each slot has a sequence number used as a seqlock: odd while it's
//! being written, `2 × frame + 2` when frame `frame` is complete. A reader copies the slot and
//! accepts it only if the sequence was even and unchanged, so it never returns a torn frame.
//!
//! Layout (little-endian):
//! ```text
//! header (64 bytes): magic "SELAHRNG" | version u32 | slots u32 | slot_capacity u64 |
//!                    latest u64 (frames published; atomic) | reserved
//! slot  (32 + capacity bytes): sequence u64 (atomic) | width u32 | height u32 | stride u32 |
//!                    format u32 | timestamp_ns u64 | pixels…
//! ```
//! This module is listed in `tools/unsafe-allowlist.txt`: mapping a file and treating two
//! aligned words of it as atomics needs `unsafe`.
#![allow(unsafe_code)]

use std::fs::OpenOptions;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering, fence};

use memmap2::MmapMut;

pub const MAGIC: &[u8; 8] = b"SELAHRNG";
pub const VERSION: u32 = 1;
pub const HEADER_BYTES: usize = 64;
pub const SLOT_HEADER_BYTES: usize = 32;
/// Pixel format: 8-bit BGRA, premultiplied alpha.
pub const FORMAT_BGRA8: u32 = 1;
/// Largest slot payload: one 4K BGRA frame.
pub const MAX_SLOT_CAPACITY: u64 = 3840 * 2160 * 4;
pub const MAX_SLOTS: u32 = 8;

const LATEST_OFFSET: usize = 24;

/// One preview frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewFrame {
    pub frame: u64,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub format: u32,
    pub timestamp_ns: u64,
    pub pixels: Vec<u8>,
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn atomic_at(map: &MmapMut, offset: usize) -> &AtomicU64 {
    assert!(offset.is_multiple_of(8) && offset + 8 <= map.len());
    // SAFETY: the mapping is page-aligned, `offset` is a multiple of 8 and in bounds, so the
    // pointer is valid and aligned for an AtomicU64 for as long as `map` is borrowed. All
    // processes access these words only atomically.
    unsafe { AtomicU64::from_ptr(map.as_ptr().add(offset) as *mut u64) }
}

fn slot_offset(index: u32, capacity: u64) -> usize {
    HEADER_BYTES + index as usize * (SLOT_HEADER_BYTES + capacity as usize)
}

fn map_file(path: &Path, create_len: Option<u64>) -> io::Result<MmapMut> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(create_len.is_some())
        .truncate(false)
        .open(path)?;
    if let Some(len) = create_len {
        file.set_len(len)?;
    }
    // SAFETY: the file lives in the user's private runtime directory and is only resized by
    // its single writer before mapping. Concurrent changes by the other process are expected
    // and handled by the seqlock protocol: data is copied out and validated, never referenced.
    unsafe { MmapMut::map_mut(&file) }
}

/// The engine side.
pub struct RingWriter {
    map: MmapMut,
    slots: u32,
    capacity: u64,
    next_frame: u64,
}

impl RingWriter {
    pub fn create(path: &Path, slots: u32, slot_capacity: u64) -> io::Result<Self> {
        if slots == 0
            || slots > MAX_SLOTS
            || slot_capacity == 0
            || slot_capacity > MAX_SLOT_CAPACITY
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "ring size is outside the permitted range",
            ));
        }
        // Round the slot size up so every slot's sequence word stays 8-byte aligned.
        let capacity = slot_capacity.div_ceil(8) * 8;
        let total = slot_offset(slots, capacity) as u64;
        let mut map = map_file(path, Some(total))?;
        map[..HEADER_BYTES].fill(0);
        map[0..8].copy_from_slice(MAGIC);
        map[8..12].copy_from_slice(&VERSION.to_le_bytes());
        map[12..16].copy_from_slice(&slots.to_le_bytes());
        map[16..24].copy_from_slice(&capacity.to_le_bytes());
        for index in 0..slots {
            atomic_at(&map, slot_offset(index, capacity)).store(0, Ordering::Release);
        }
        atomic_at(&map, LATEST_OFFSET).store(0, Ordering::Release);
        Ok(Self {
            map,
            slots,
            capacity,
            next_frame: 0,
        })
    }

    pub fn slot_capacity(&self) -> u64 {
        self.capacity
    }

    /// Publish a frame. Returns its frame number.
    pub fn publish(
        &mut self,
        width: u32,
        height: u32,
        stride: u32,
        timestamp_ns: u64,
        pixels: &[u8],
    ) -> io::Result<u64> {
        let needed = u64::from(stride) * u64::from(height);
        if stride < width.saturating_mul(4)
            || pixels.len() as u64 != needed
            || needed > self.capacity
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "frame doesn't fit the ring slot",
            ));
        }
        let frame = self.next_frame;
        let index = (frame % u64::from(self.slots)) as u32;
        let offset = slot_offset(index, self.capacity);
        let sequence = atomic_at(&self.map, offset);
        sequence.store(frame * 2 + 1, Ordering::Relaxed);
        fence(Ordering::Release);
        let header = offset + 8;
        self.map[header..header + 4].copy_from_slice(&width.to_le_bytes());
        self.map[header + 4..header + 8].copy_from_slice(&height.to_le_bytes());
        self.map[header + 8..header + 12].copy_from_slice(&stride.to_le_bytes());
        self.map[header + 12..header + 16].copy_from_slice(&FORMAT_BGRA8.to_le_bytes());
        self.map[header + 16..header + 24].copy_from_slice(&timestamp_ns.to_le_bytes());
        let data = offset + SLOT_HEADER_BYTES;
        self.map[data..data + pixels.len()].copy_from_slice(pixels);
        atomic_at(&self.map, offset).store(frame * 2 + 2, Ordering::Release);
        atomic_at(&self.map, LATEST_OFFSET).store(frame + 1, Ordering::Release);
        self.next_frame += 1;
        Ok(frame)
    }
}

/// The UI side (also implemented in C# in `Selah.Client`).
pub struct RingReader {
    map: MmapMut,
    slots: u32,
    capacity: u64,
}

impl RingReader {
    pub fn open(path: &Path) -> io::Result<Self> {
        let map = map_file(path, None)?;
        if map.len() < HEADER_BYTES || &map[0..8] != MAGIC {
            return Err(invalid("not a Selah preview ring"));
        }
        let version = u32::from_le_bytes(map[8..12].try_into().unwrap_or_default());
        let slots = u32::from_le_bytes(map[12..16].try_into().unwrap_or_default());
        let capacity = u64::from_le_bytes(map[16..24].try_into().unwrap_or_default());
        if version != VERSION
            || slots == 0
            || slots > MAX_SLOTS
            || capacity == 0
            || capacity > MAX_SLOT_CAPACITY
            || !capacity.is_multiple_of(8)
            || slot_offset(slots, capacity) > map.len()
        {
            return Err(invalid("preview ring header is invalid"));
        }
        Ok(Self {
            map,
            slots,
            capacity,
        })
    }

    /// Number of frames published so far.
    pub fn published(&self) -> u64 {
        atomic_at(&self.map, LATEST_OFFSET).load(Ordering::Acquire)
    }

    /// The newest complete frame, or `None` if nothing is published yet. Retries a few times
    /// if the writer is mid-update.
    pub fn latest(&self) -> Option<PreviewFrame> {
        for _ in 0..8 {
            let published = self.published();
            if published == 0 {
                return None;
            }
            if let Some(frame) = self.read(published - 1) {
                return Some(frame);
            }
            std::hint::spin_loop();
        }
        None
    }

    /// Read a specific frame if its slot still holds it unchanged.
    pub fn read(&self, frame: u64) -> Option<PreviewFrame> {
        let index = (frame % u64::from(self.slots)) as u32;
        let offset = slot_offset(index, self.capacity);
        let expected = frame.checked_mul(2)?.checked_add(2)?;
        let sequence = atomic_at(&self.map, offset);
        if sequence.load(Ordering::Acquire) != expected {
            return None;
        }
        let header = offset + 8;
        let word =
            |at: usize| u32::from_le_bytes(self.map[at..at + 4].try_into().unwrap_or_default());
        let width = word(header);
        let height = word(header + 4);
        let stride = word(header + 8);
        let format = word(header + 12);
        let timestamp_ns = u64::from_le_bytes(
            self.map[header + 16..header + 24]
                .try_into()
                .unwrap_or_default(),
        );
        let length = u64::from(stride) * u64::from(height);
        if length > self.capacity {
            return None;
        }
        let data = offset + SLOT_HEADER_BYTES;
        let pixels = self.map[data..data + length as usize].to_vec();
        fence(Ordering::Acquire);
        if sequence.load(Ordering::Relaxed) != expected {
            return None;
        }
        Some(PreviewFrame {
            frame,
            width,
            height,
            stride,
            format,
            timestamp_ns,
            pixels,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip_and_old_slots_are_not_returned_once_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("preview-0.ring");
        let mut writer = RingWriter::create(&path, 3, 4 * 4 * 2).unwrap();
        let reader = RingReader::open(&path).unwrap();
        assert!(reader.latest().is_none());
        for frame in 0..10_u8 {
            let pixels = vec![frame; 32];
            assert_eq!(
                writer.publish(4, 2, 16, u64::from(frame), &pixels).unwrap(),
                u64::from(frame)
            );
            let latest = reader.latest().unwrap();
            assert_eq!(latest.frame, u64::from(frame));
            assert_eq!(latest.pixels, pixels);
            assert_eq!((latest.width, latest.height, latest.stride), (4, 2, 16));
        }
        // Frame 6 lived in slot 0, which frame 9 has since overwritten.
        assert!(reader.read(6).is_none());
        assert!(reader.read(8).is_some());
    }

    #[test]
    fn oversized_frames_and_bad_rings_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ring");
        let mut writer = RingWriter::create(&path, 2, 64).unwrap();
        assert!(writer.publish(8, 8, 32, 0, &[0; 256]).is_err());
        assert!(
            writer.publish(4, 1, 8, 0, &[0; 8]).is_err(),
            "stride below width × 4"
        );
        assert!(RingWriter::create(&path, 0, 64).is_err());
        assert!(RingWriter::create(&path, 2, MAX_SLOT_CAPACITY + 1).is_err());
        let bogus = dir.path().join("bogus");
        std::fs::write(&bogus, [0_u8; 128]).unwrap();
        assert!(RingReader::open(&bogus).is_err());
    }

    /// The cross-language fixture read by `Selah.Client.Tests`. Regenerate with
    /// `SELAH_UPDATE_FIXTURES=1 cargo test -p selah-ipc ring_fixture`.
    #[test]
    fn ring_fixture_matches_the_committed_copy() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("preview-ring.bin");
        let mut writer = RingWriter::create(&path, 3, 16).unwrap();
        for frame in 0..4_u8 {
            writer
                .publish(2, 2, 8, u64::from(frame) * 1000, &[frame; 16])
                .unwrap();
        }
        drop(writer);
        let bytes = std::fs::read(&path).unwrap();
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/ipc/preview-ring.bin");
        if std::env::var("SELAH_UPDATE_FIXTURES").as_deref() == Ok("1") {
            std::fs::create_dir_all(fixture.parent().unwrap()).unwrap();
            std::fs::write(&fixture, &bytes).unwrap();
        }
        assert_eq!(
            std::fs::read(&fixture).expect("fixture exists"),
            bytes,
            "ring layout changed: update the C# reader and regenerate the fixture"
        );
    }

    #[test]
    fn concurrent_reader_never_sees_a_torn_frame() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ring");
        let mut writer = RingWriter::create(&path, 2, 64 * 64 * 4).unwrap();
        let reader_path = path.clone();
        let reader = std::thread::spawn(move || {
            let reader = RingReader::open(&reader_path).unwrap();
            let mut seen = 0;
            while seen < 2_000 {
                if let Some(frame) = reader.latest() {
                    let first = frame.pixels[0];
                    assert!(frame.pixels.iter().all(|&byte| byte == first), "torn frame");
                    seen += 1;
                }
            }
        });
        let mut frame = 0_u64;
        while !reader.is_finished() {
            let pixels = vec![(frame % 251) as u8; 64 * 64 * 4];
            writer.publish(64, 64, 256, frame, &pixels).unwrap();
            frame += 1;
        }
        reader.join().unwrap();
    }
}
