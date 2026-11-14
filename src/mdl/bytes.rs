//! Little-endian byte reading and writing for the MDL format.
//! Names are fixed-size, NUL-padded byte fields. They are mapped byte for byte
//! to `char` (Latin-1), so any byte survives a round trip.

use anyhow::{Result, anyhow, bail};

pub type Vec3 = [f32; 3];

/// A read position in the file.
#[derive(Clone)]
pub struct Cur<'a> {
    d: &'a [u8],
    pub pos: usize,
}

impl<'a> Cur<'a> {
    pub fn new(d: &'a [u8], pos: usize) -> Self {
        Cur { d, pos }
    }

    pub fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.d.len());
        let end = end.ok_or_else(|| {
            anyhow!(
                "read past end of file: {} + {n} > {}",
                self.pos,
                self.d.len()
            )
        })?;
        let s = &self.d[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    pub fn i16(&mut self) -> Result<i16> {
        Ok(i16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    pub fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    pub fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    pub fn vec3(&mut self) -> Result<Vec3> {
        Ok([self.f32()?, self.f32()?, self.f32()?])
    }
    pub fn ints<const N: usize>(&mut self) -> Result<[i32; N]> {
        let mut a = [0; N];
        for v in &mut a {
            *v = self.i32()?;
        }
        Ok(a)
    }
    pub fn floats<const N: usize>(&mut self) -> Result<[f32; N]> {
        let mut a = [0.0; N];
        for v in &mut a {
            *v = self.f32()?;
        }
        Ok(a)
    }
    /// A NUL-padded name field of `n` bytes.
    pub fn name(&mut self, n: usize) -> Result<String> {
        let b = self.take(n)?;
        let end = b.iter().position(|&c| c == 0).unwrap_or(n);
        Ok(b[..end].iter().map(|&c| c as char).collect())
    }
}

/// A count or offset from the file, which must not be negative.
pub fn idx(v: i32, what: &str) -> Result<usize> {
    usize::try_from(v).map_err(|_| anyhow!("negative {what}: {v}"))
}

/// The file being written.
#[derive(Default)]
pub struct Out {
    pub buf: Vec<u8>,
}

impl Out {
    pub fn pos(&self) -> usize {
        self.buf.len()
    }
    /// The current position as a file offset field.
    pub fn here(&self) -> Result<i32> {
        i32::try_from(self.buf.len()).map_err(|_| anyhow!("file larger than 2 GB"))
    }
    pub fn align4(&mut self) {
        while !self.buf.len().is_multiple_of(4) {
            self.buf.push(0);
        }
    }
    pub fn zeros(&mut self, n: usize) {
        self.buf.resize(self.buf.len() + n, 0);
    }
    pub fn bytes(&mut self, b: &[u8]) {
        self.buf.extend_from_slice(b);
    }
    pub fn i16(&mut self, v: i16) {
        self.bytes(&v.to_le_bytes());
    }
    pub fn i32(&mut self, v: i32) {
        self.bytes(&v.to_le_bytes());
    }
    pub fn f32(&mut self, v: f32) {
        self.bytes(&v.to_le_bytes());
    }
    pub fn vec3(&mut self, v: Vec3) {
        v.iter().for_each(|&x| self.f32(x));
    }
    pub fn ints(&mut self, v: &[i32]) {
        v.iter().for_each(|&x| self.i32(x));
    }
    pub fn floats(&mut self, v: &[f32]) {
        v.iter().for_each(|&x| self.f32(x));
    }
    /// A NUL-padded name field of `n` bytes; the name must leave room for the NUL.
    pub fn name(&mut self, s: &str, n: usize) -> Result<()> {
        let mut b = Vec::with_capacity(n);
        for c in s.chars() {
            let c = u32::from(c);
            if c > 0xff {
                bail!("name {s:?} has a character outside Latin-1");
            }
            b.push(c as u8);
        }
        if b.len() >= n {
            bail!("name {s:?} is longer than {} bytes", n - 1);
        }
        b.resize(n, 0);
        self.bytes(&b);
        Ok(())
    }
    pub fn patch_i32(&mut self, at: usize, v: i32) {
        self.buf[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }
    pub fn patch_u16(&mut self, at: usize, v: u16) {
        self.buf[at..at + 2].copy_from_slice(&v.to_le_bytes());
    }
}
