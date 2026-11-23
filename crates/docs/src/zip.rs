//! A zip WRITER, the twin of the bounded reader in `harness-attached`
//! (PLAN.md T97): stored or deflate, correct CRC-32 and sizes, UTF-8 names,
//! a fixed timestamp (the same input gives the same bytes), no zip64, no
//! data descriptors, no directory entries, no extra fields. The size is
//! counted as parts are added, so a package past its ceiling is refused
//! before it is finished. Entry names are the writer's own constants, but
//! they are still checked (no leading slash, no `..`, no backslash, no
//! control character, no duplicate even by letter case), because the editing
//! tools that follow take names from files.

use std::collections::HashSet;
use std::io::Write;

use crate::Result;
use crate::limits::MAX_PART_BYTES;

/// 1980-01-01 00:00:00 as a DOS date and time.
const DOS_DATE: u16 = (1 << 5) | 1;
const DOS_TIME: u16 = 0;

struct Central {
    name: Vec<u8>,
    time: u16,
    date: u16,
    method: u16,
    crc: u32,
    comp: u32,
    size: u32,
    offset: u32,
}

pub struct ZipWriter {
    out: Vec<u8>,
    entries: Vec<Central>,
    names: HashSet<String>,
    max: usize,
}

fn le16(o: &mut Vec<u8>, v: u16) {
    o.extend_from_slice(&v.to_le_bytes());
}

fn le32(o: &mut Vec<u8>, v: u32) {
    o.extend_from_slice(&v.to_le_bytes());
}

/// Whether `name` may be an entry name of a package.
pub fn check_name(name: &str) -> Result<()> {
    let bad = name.is_empty()
        || name.len() > 255
        || name.starts_with('/')
        || name.contains('\\')
        || name.chars().any(|c| c.is_control())
        || name
            .split('/')
            .any(|s| s.is_empty() || s == "." || s == "..");
    if bad {
        return Err("a part of the file has a name a package cannot hold".into());
    }
    Ok(())
}

impl ZipWriter {
    /// A writer whose finished file may hold at most `max` bytes.
    pub fn new(max: usize) -> Self {
        Self {
            out: Vec::new(),
            entries: Vec::new(),
            names: HashSet::new(),
            max,
        }
    }

    /// Bytes written so far.
    pub fn len(&self) -> usize {
        self.out.len()
    }

    pub fn is_empty(&self) -> bool {
        self.out.is_empty()
    }

    /// The finished size of what is held, central directory included.
    fn projected(&self, extra_name: usize) -> usize {
        let central: usize = self.entries.iter().map(|e| 46 + e.name.len()).sum();
        self.out.len() + central + 46 + extra_name + 22
    }

    /// Add a part, deflated when that is smaller, stored otherwise.
    pub fn add(&mut self, name: &str, data: &[u8]) -> Result<()> {
        self.put(name, data, true, DOS_TIME, DOS_DATE)
    }

    /// [`Self::add`] with the timestamp of the entry it replaces.
    pub fn add_at(&mut self, name: &str, data: &[u8], time: u16, date: u16) -> Result<()> {
        self.put(name, data, true, time, date)
    }

    /// Add a part stored as it is.
    pub fn add_stored(&mut self, name: &str, data: &[u8]) -> Result<()> {
        self.put(name, data, false, DOS_TIME, DOS_DATE)
    }

    /// Copy an entry of another zip as it is stored: its compressed bytes,
    /// CRC and sizes (as that zip's directory gave them) and its timestamp.
    /// Nothing is unpacked, so what the entry unpacks to costs nothing here
    /// and its size is not held to [`MAX_PART_BYTES`]: a part this writer did
    /// not change is the other file's own. The caller has checked the method
    /// is 0 or 8 and the sizes fit 32 bits. A directory entry (name ending in
    /// a slash) is copied too.
    #[allow(clippy::too_many_arguments)]
    pub fn put_raw(
        &mut self,
        name: &str,
        method: u16,
        crc: u32,
        comp: u32,
        size: u32,
        time: u16,
        date: u16,
        data: &[u8],
    ) -> Result<()> {
        check_name(name.strip_suffix('/').unwrap_or(name))?;
        if !matches!(method, 0 | 8) || data.len() != comp as usize {
            return Err("a part of the file cannot be copied".into());
        }
        if !self.names.insert(name.to_ascii_lowercase()) {
            return Err("a part of the file would be written twice".into());
        }
        if self.projected(name.len()) + data.len() > self.max {
            return Err(format!(
                "the file would be larger than {} MiB",
                self.max >> 20
            ));
        }
        self.push(name, method, crc, comp, size, time, date, data)
    }

    #[allow(clippy::too_many_arguments)]
    fn push(
        &mut self,
        name: &str,
        method: u16,
        crc: u32,
        comp: u32,
        size: u32,
        time: u16,
        date: u16,
        body: &[u8],
    ) -> Result<()> {
        let offset =
            u32::try_from(self.out.len()).map_err(|_| "the file is too large".to_string())?;
        let o = &mut self.out;
        le32(o, 0x0403_4b50);
        le16(o, 20);
        le16(o, 0x0800);
        le16(o, method);
        le16(o, time);
        le16(o, date);
        le32(o, crc);
        le32(o, comp);
        le32(o, size);
        le16(o, name.len() as u16);
        le16(o, 0);
        o.extend_from_slice(name.as_bytes());
        o.extend_from_slice(body);
        self.entries.push(Central {
            name: name.as_bytes().to_vec(),
            time,
            date,
            method,
            crc,
            comp,
            size,
            offset,
        });
        Ok(())
    }

    fn put(&mut self, name: &str, data: &[u8], deflate: bool, time: u16, date: u16) -> Result<()> {
        check_name(name)?;
        if !self.names.insert(name.to_ascii_lowercase()) {
            return Err("a part of the file would be written twice".into());
        }
        if data.len() > MAX_PART_BYTES {
            return Err("the file has a part larger than a document part may be".into());
        }
        let crc = {
            let mut c = flate2::Crc::new();
            c.update(data);
            c.sum()
        };
        let packed;
        let (method, body): (u16, &[u8]) = if deflate && data.len() > 64 {
            let mut e =
                flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
            e.write_all(data)
                .map_err(|_| "the file could not be packed".to_string())?;
            packed = e
                .finish()
                .map_err(|_| "the file could not be packed".to_string())?;
            if packed.len() < data.len() {
                (8, &packed)
            } else {
                (0, data)
            }
        } else {
            (0, data)
        };
        if self.projected(name.len()) + body.len() > self.max {
            return Err(format!(
                "the file would be larger than {} MiB",
                self.max >> 20
            ));
        }
        let (comp, size) = (body.len() as u32, data.len() as u32);
        self.push(name, method, crc, comp, size, time, date, body)
    }

    /// Write the central directory and the end record.
    pub fn finish(mut self) -> Result<Vec<u8>> {
        let start =
            u32::try_from(self.out.len()).map_err(|_| "the file is too large".to_string())?;
        let entries = std::mem::take(&mut self.entries);
        let o = &mut self.out;
        for e in &entries {
            le32(o, 0x0201_4b50);
            le16(o, 20);
            le16(o, 20);
            le16(o, 0x0800);
            le16(o, e.method);
            le16(o, e.time);
            le16(o, e.date);
            le32(o, e.crc);
            le32(o, e.comp);
            le32(o, e.size);
            le16(o, e.name.len() as u16);
            le16(o, 0);
            le16(o, 0);
            le16(o, 0);
            le16(o, 0);
            le32(o, 0);
            le32(o, e.offset);
            o.extend_from_slice(&e.name);
        }
        let cd = (o.len() as u32).saturating_sub(start);
        le32(o, 0x0605_4b50);
        le16(o, 0);
        le16(o, 0);
        le16(o, entries.len() as u16);
        le16(o, entries.len() as u16);
        le32(o, cd);
        le32(o, start);
        le16(o, 0);
        if self.out.len() > self.max {
            return Err(format!(
                "the file would be larger than {} MiB",
                self.max >> 20
            ));
        }
        Ok(self.out)
    }
}
