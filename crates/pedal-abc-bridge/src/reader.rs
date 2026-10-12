//! 二进制读取游标：小端整数、uleb128/sleb128、MUTF-8 字符串
//!
//! @author ljlVink

use crate::error::{AbcError, AbcResult};

/// 只读游标，带绝对偏移与越界检查
pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    /// 基于完整缓冲构造（位置 0）
    pub fn new(data: &'a [u8]) -> Self {
        Reader { data, pos: 0 }
    }

    /// 定位到绝对偏移
    pub fn seek(&mut self, pos: usize) -> AbcResult<()> {
        if pos > self.data.len() {
            return Err(bound(pos, self.data.len(), "seek"));
        }
        self.pos = pos;
        Ok(())
    }

    /// 当前绝对偏移
    pub fn pos(&self) -> usize {
        self.pos
    }

    /// 剩余可读字节数（容量上界推导用）
    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    /// 底层缓冲
    pub fn bytes(&self) -> &'a [u8] {
        self.data
    }

    fn need(&self, n: usize, what: &'static str) -> AbcResult<()> {
        if self.pos + n > self.data.len() {
            return Err(bound(self.pos, self.data.len(), what));
        }
        Ok(())
    }

    /// 读 1 字节
    pub fn u8(&mut self) -> AbcResult<u8> {
        self.need(1, "u8")?;
        let v = self.data[self.pos];
        self.pos += 1;
        Ok(v)
    }

    /// 读 2 字节小端
    pub fn u16(&mut self) -> AbcResult<u16> {
        self.need(2, "u16")?;
        let v = u16::from_le_bytes(self.data[self.pos..self.pos + 2].try_into().unwrap());
        self.pos += 2;
        Ok(v)
    }

    /// 读 4 字节小端
    pub fn u32(&mut self) -> AbcResult<u32> {
        self.need(4, "u32")?;
        let v = u32::from_le_bytes(self.data[self.pos..self.pos + 4].try_into().unwrap());
        self.pos += 4;
        Ok(v)
    }

    /// 读 8 字节小端
    pub fn u64(&mut self) -> AbcResult<u64> {
        self.need(8, "u64")?;
        let v = u64::from_le_bytes(self.data[self.pos..self.pos + 8].try_into().unwrap());
        self.pos += 8;
        Ok(v)
    }

    /// 在绝对偏移处读 4 字节小端（不移动游标）
    pub fn u32_at(&self, offset: usize) -> AbcResult<u32> {
        if offset + 4 > self.data.len() {
            return Err(bound(offset, self.data.len(), "u32_at"));
        }
        Ok(u32::from_le_bytes(
            self.data[offset..offset + 4].try_into().unwrap(),
        ))
    }

    /// 读 uleb128
    pub fn uleb(&mut self) -> AbcResult<u64> {
        let mut result: u64 = 0;
        let mut shift = 0;
        loop {
            let b = self.u8()?;
            result |= u64::from(b & 0x7f) << shift;
            if b & 0x80 == 0 {
                return Ok(result);
            }
            shift += 7;
            if shift >= 64 {
                return Err(AbcError::BadLeb128);
            }
        }
    }

    /// 读 sleb128
    pub fn sleb(&mut self) -> AbcResult<i64> {
        let mut result: i64 = 0;
        let mut shift = 0;
        loop {
            let b = self.u8()?;
            result |= i64::from(b & 0x7f) << shift;
            shift += 7;
            if b & 0x80 == 0 {
                if b & 0x40 != 0 && shift < 64 {
                    result |= -1i64 << shift;
                }
                return Ok(result);
            }
            if shift >= 64 {
                return Err(AbcError::BadLeb128);
            }
        }
    }

    /// 读 abc 字符串：uleb 头（len<<1|is_ascii）+ MUTF-8 字节 + NUL
    pub fn string(&mut self) -> AbcResult<String> {
        let header = self.uleb()?;
        let _utf16_len = (header >> 1) as usize;
        let _is_ascii = header & 1 != 0;
        let start = self.pos;
        let nul = memchr::memchr(self.data, start, 0).ok_or(AbcError::UnterminatedString)?;
        let s = decode_mutf8(&self.data[start..nul]);
        self.pos = nul + 1;
        Ok(s)
    }

    /// 在绝对偏移处读 abc 字符串（不移动游标）
    pub fn string_at(&self, offset: usize) -> AbcResult<String> {
        if offset >= self.data.len() {
            return Err(bound(offset, self.data.len(), "string_at"));
        }
        let mut r = Reader {
            data: self.data,
            pos: offset,
        };
        r.string()
    }

    /// 跳过 N 字节
    pub fn skip(&mut self, n: usize) -> AbcResult<()> {
        self.need(n, "skip")?;
        self.pos += n;
        Ok(())
    }
}

fn bound(offset: usize, len: usize, what: &'static str) -> AbcError {
    AbcError::OutOfBounds { offset, len, what }
}

mod memchr {
    /// 极简 memchr：切片内查找字节
    pub fn memchr(hay: &[u8], from: usize, needle: u8) -> Option<usize> {
        hay[from..]
            .iter()
            .position(|&b| b == needle)
            .map(|i| i + from)
    }
}

/// MUTF-8（CESU-8 变体）解码：代理对以两个 3 字节序列编码
pub fn decode_mutf8(bytes: &[u8]) -> String {
    let mut units = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b0 = bytes[i];
        if b0 < 0x80 {
            units.push(u16::from(b0));
            i += 1;
        } else if b0 & 0xe0 == 0xc0 {
            if i + 1 >= bytes.len() {
                break;
            }
            let b1 = bytes[i + 1];
            units.push((u16::from(b0 & 0x1f) << 6) | u16::from(b1 & 0x3f));
            i += 2;
        } else if b0 & 0xf0 == 0xe0 {
            if i + 2 >= bytes.len() {
                break;
            }
            let b1 = bytes[i + 1];
            let b2 = bytes[i + 2];
            units.push(
                (u16::from(b0 & 0x0f) << 12) | (u16::from(b1 & 0x3f) << 6) | u16::from(b2 & 0x3f),
            );
            i += 3;
        } else {
            // MUTF-8 不出现 4 字节序列；容错跳过
            units.push(0xfffd);
            i += 1;
        }
    }
    String::from_utf16_lossy(&units)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leb_roundtrip() {
        let data = [0xe5, 0x8e, 0x26, 0x00, 0x00];
        let mut r = Reader::new(&data);
        assert_eq!(r.uleb().unwrap(), 624485);
        assert_eq!(r.string().unwrap(), "");
    }

    #[test]
    fn mutf8_ascii() {
        assert_eq!(decode_mutf8(b"hello"), "hello");
    }

    #[test]
    fn mutf8_surrogate_pair() {
        // U+1D11E (𝄞) = D834 DD1E，各编码为 3 字节
        let bytes = [0xed, 0xa0, 0xb4, 0xed, 0xb4, 0x9e];
        assert_eq!(decode_mutf8(&bytes), "\u{1D11E}");
    }
}
