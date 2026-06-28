use crate::fsutil;
use crate::{LovelyError, Result};
use std::fs::{self, File};
use std::io::{Seek, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveEntry {
    pub name: String,
    pub bytes: Vec<u8>,
}

impl ArchiveEntry {
    pub fn file(name: impl Into<String>, bytes: impl Into<Vec<u8>>) -> Result<Self> {
        let name = name.into();
        validate_archive_name(&name)?;
        Ok(Self {
            name,
            bytes: bytes.into(),
        })
    }
}

pub fn create_love_archive(
    source: &Path,
    output: &Path,
    includes: &[String],
    excludes: &[String],
) -> Result<Vec<ArchiveEntry>> {
    let mut entries = Vec::new();
    for file in fsutil::collect_included_files(source, includes, excludes)? {
        let rel = fsutil::relative_path(source, &file)?;
        let name = fsutil::normalize_slashes(&rel);
        validate_archive_name(&name)?;
        let bytes = fs::read(&file).map_err(|err| LovelyError::io(&file, err))?;
        entries.push(ArchiveEntry { name, bytes });
    }

    entries.sort_by(|a, b| a.name.cmp(&b.name));
    write_zip(output, &entries)?;
    Ok(entries)
}

pub fn write_zip(output: &Path, entries: &[ArchiveEntry]) -> Result<()> {
    if let Some(parent) = output.parent() {
        fsutil::ensure_dir(parent)?;
    }

    let mut file = File::create(output).map_err(|err| LovelyError::io(output, err))?;
    let mut central_records = Vec::new();

    for entry in entries {
        validate_archive_name(&entry.name)?;
        let offset = file.stream_position().map_err(LovelyError::plain_io)? as u32;
        let crc = crc32(&entry.bytes);
        let size = checked_u32(entry.bytes.len(), "file is too large for ZIP32")?;
        let name = entry.name.as_bytes();
        let name_len = checked_u16(name.len(), "file name is too long")?;

        write_u32(&mut file, 0x0403_4b50)?;
        write_u16(&mut file, 20)?;
        write_u16(&mut file, 0)?;
        write_u16(&mut file, 0)?;
        write_u16(&mut file, 0)?;
        write_u16(&mut file, 33)?;
        write_u32(&mut file, crc)?;
        write_u32(&mut file, size)?;
        write_u32(&mut file, size)?;
        write_u16(&mut file, name_len)?;
        write_u16(&mut file, 0)?;
        file.write_all(name).map_err(LovelyError::plain_io)?;
        file.write_all(&entry.bytes)
            .map_err(LovelyError::plain_io)?;

        central_records.push(CentralRecord {
            name: entry.name.clone(),
            crc,
            size,
            offset,
        });
    }

    let central_offset = file.stream_position().map_err(LovelyError::plain_io)? as u32;
    for record in &central_records {
        let name = record.name.as_bytes();
        let name_len = checked_u16(name.len(), "file name is too long")?;

        write_u32(&mut file, 0x0201_4b50)?;
        write_u16(&mut file, 20)?;
        write_u16(&mut file, 20)?;
        write_u16(&mut file, 0)?;
        write_u16(&mut file, 0)?;
        write_u16(&mut file, 0)?;
        write_u16(&mut file, 33)?;
        write_u32(&mut file, record.crc)?;
        write_u32(&mut file, record.size)?;
        write_u32(&mut file, record.size)?;
        write_u16(&mut file, name_len)?;
        write_u16(&mut file, 0)?;
        write_u16(&mut file, 0)?;
        write_u16(&mut file, 0)?;
        write_u16(&mut file, 0)?;
        write_u32(&mut file, 0o100644 << 16)?;
        write_u32(&mut file, record.offset)?;
        file.write_all(name).map_err(LovelyError::plain_io)?;
    }

    let central_size =
        file.stream_position().map_err(LovelyError::plain_io)? as u32 - central_offset;
    write_u32(&mut file, 0x0605_4b50)?;
    write_u16(&mut file, 0)?;
    write_u16(&mut file, 0)?;
    write_u16(
        &mut file,
        checked_u16(central_records.len(), "too many ZIP entries")?,
    )?;
    write_u16(
        &mut file,
        checked_u16(central_records.len(), "too many ZIP entries")?,
    )?;
    write_u32(&mut file, central_size)?;
    write_u32(&mut file, central_offset)?;
    write_u16(&mut file, 0)?;

    Ok(())
}

pub fn write_tar(output: &Path, entries: &[ArchiveEntry]) -> Result<()> {
    if let Some(parent) = output.parent() {
        fsutil::ensure_dir(parent)?;
    }
    let mut file = File::create(output).map_err(|err| LovelyError::io(output, err))?;
    for entry in entries {
        validate_archive_name(&entry.name)?;
        write_tar_header(&mut file, entry)?;
        file.write_all(&entry.bytes)
            .map_err(LovelyError::plain_io)?;
        let padding = (512 - (entry.bytes.len() % 512)) % 512;
        if padding > 0 {
            file.write_all(&vec![0; padding])
                .map_err(LovelyError::plain_io)?;
        }
    }
    file.write_all(&[0; 1024]).map_err(LovelyError::plain_io)?;
    Ok(())
}

pub fn archive_entries_from_files(
    root: &Path,
    files: &[PathBuf],
    prefix: &str,
) -> Result<Vec<ArchiveEntry>> {
    let mut entries = Vec::new();
    for file in files {
        let rel = fsutil::relative_path(root, file)?;
        let name = format!(
            "{}/{}",
            prefix.trim_matches('/'),
            fsutil::normalize_slashes(&rel)
        );
        let bytes = fs::read(file).map_err(|err| LovelyError::io(file, err))?;
        entries.push(ArchiveEntry::file(name, bytes)?);
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(entries)
}

fn write_tar_header(mut file: impl Write, entry: &ArchiveEntry) -> Result<()> {
    let mut header = [0u8; 512];
    write_octal(&mut header[100..108], 0o644);
    write_octal(&mut header[108..116], 0);
    write_octal(&mut header[116..124], 0);
    write_octal(&mut header[124..136], entry.bytes.len() as u64);
    write_octal(&mut header[136..148], 0);
    header[156] = b'0';
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");

    let name_bytes = entry.name.as_bytes();
    if name_bytes.len() <= 100 {
        header[0..name_bytes.len()].copy_from_slice(name_bytes);
    } else if let Some(split) = entry.name.rfind('/') {
        let (prefix, name) = entry.name.split_at(split);
        let name = &name[1..];
        if prefix.len() > 155 || name.len() > 100 {
            return Err(LovelyError::Archive(format!(
                "tar path is too long: {}",
                entry.name
            )));
        }
        header[0..name.len()].copy_from_slice(name.as_bytes());
        header[345..345 + prefix.len()].copy_from_slice(prefix.as_bytes());
    } else {
        return Err(LovelyError::Archive(format!(
            "tar path is too long: {}",
            entry.name
        )));
    }

    for byte in &mut header[148..156] {
        *byte = b' ';
    }
    let checksum: u32 = header.iter().map(|byte| *byte as u32).sum();
    write_checksum(&mut header[148..156], checksum);
    file.write_all(&header).map_err(LovelyError::plain_io)?;
    Ok(())
}

fn write_octal(field: &mut [u8], value: u64) {
    let text = format!("{:0width$o}\0", value, width = field.len() - 1);
    field.copy_from_slice(text.as_bytes());
}

fn write_checksum(field: &mut [u8], value: u32) {
    let text = format!("{:06o}\0 ", value);
    field.copy_from_slice(text.as_bytes());
}

fn validate_archive_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.starts_with('/')
        || name.contains('\\')
        || name.split('/').any(|part| part == ".." || part.is_empty())
    {
        return Err(LovelyError::Archive(format!(
            "unsafe archive path: {name:?}"
        )));
    }
    Ok(())
}

fn checked_u16(value: usize, message: &str) -> Result<u16> {
    u16::try_from(value).map_err(|_| LovelyError::Archive(message.to_string()))
}

fn checked_u32(value: usize, message: &str) -> Result<u32> {
    u32::try_from(value).map_err(|_| LovelyError::Archive(message.to_string()))
}

fn write_u16(mut writer: impl Write, value: u16) -> Result<()> {
    writer
        .write_all(&value.to_le_bytes())
        .map_err(LovelyError::plain_io)
}

fn write_u32(mut writer: impl Write, value: u32) -> Result<()> {
    writer
        .write_all(&value.to_le_bytes())
        .map_err(LovelyError::plain_io)
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in bytes {
        crc ^= *byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

struct CentralRecord {
    name: String,
    crc: u32,
    size: u32,
    offset: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unsafe_paths() {
        assert!(ArchiveEntry::file("../x", b"").is_err());
        assert!(ArchiveEntry::file("/x", b"").is_err());
        assert!(ArchiveEntry::file("a\\b", b"").is_err());
    }

    #[test]
    fn crc_known_value() {
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
    }
}
