use std::marker::PhantomData;

use bytes::{Buf, BufMut, Bytes, BytesMut};

pub trait Length {
    /// Read the length from the bytes, and the size of the length and the length. If the bytes is not enough, return None.
    fn read_from_bytes(bs: &[u8]) -> Option<(usize, usize)>;

    /// Write the length as bytes, and return the size it written. If the length is too long, do
    /// nothing and return 0.
    fn write_to_bytes(length: usize, bs: &mut BytesMut) -> usize;
}

macro_rules! impl_length {
    ($ident:ident) => {
        impl Length for $ident {
            fn read_from_bytes(bs: &[u8]) -> Option<(usize, usize)> {
                const LENGTH: usize = $ident::BITS as usize / 8;

                if bs.len() < LENGTH {
                    return None;
                }
                let mut data = [0; LENGTH];
                data.copy_from_slice(&bs[..LENGTH]);
                Some((LENGTH, $ident::from_be_bytes(data) as usize))
            }

            fn write_to_bytes(length: usize, bs: &mut BytesMut) -> usize {
                const LENGTH: usize = $ident::BITS as usize / 8;

                if length > $ident::MAX as usize {
                    0
                } else {
                    bs.put_slice(&(length as $ident).to_be_bytes());
                    LENGTH
                }
            }
        }
    };
}

impl_length!(u8);
impl_length!(u16);
impl_length!(u32);

/// A reader for length-value pairs.
///
/// This struct reads length-value pairs from a `Bytes` buffer, where the length
/// is encoded in a fixed-size integer (either `u8`, `u16`, or `u32`).
pub struct LvReader<L> {
    /// The underlying `Bytes` buffer.
    data: Bytes,
    /// Total size read.
    read_size: usize,
    phantom_data: PhantomData<L>,
}

impl<L> LvReader<L> {
    /// Creates a new `LvReader` instance.
    ///
    /// # Arguments
    ///
    /// * `data`: The underlying `Bytes` buffer.
    pub fn new(data: Bytes) -> Self {
        Self {
            data,
            read_size: 0,
            phantom_data: Default::default(),
        }
    }
}

impl<L> Iterator for LvReader<L>
where
    L: Length,
{
    type Item = Result<Bytes, usize>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut read_size = 0;
        if self.data.is_empty() {
            return None;
        }

        let Some((length_size, length)) = L::read_from_bytes(self.data.as_ref()) else {
            self.data = Bytes::new();
            return Some(Err(self.read_size));
        };

        self.data.advance(length_size);
        read_size += length_size;

        if length > self.data.len() {
            self.data = Bytes::new();
            return Some(Err(self.read_size));
        }

        let value = self.data.split_to(length);
        self.read_size += read_size;
        Some(Ok(value))
    }
}

/// A writer for length-value pairs.
///
/// This struct writes length-value pairs to a `Bytes` buffer, where the length
/// is encoded in a fixed-size integer (either `u8`, `u16`, or `u32`).
#[derive(Default)]
pub struct LvWriter<L> {
    /// The underlying `Bytes` buffer.
    buf: BytesMut,
    phantom_data: PhantomData<L>,
}

impl<L> LvWriter<L>
where
    L: Length,
{
    /// Writes a data of bytes to the buffer. If the data is too long, nothing will be written.
    ///
    /// # Arguments
    ///
    /// * `data`: The data to write.
    ///
    /// # Returns
    ///
    /// * `usize` the number of bytes written.
    pub fn write<D>(&mut self, data: D) -> usize
    where
        D: AsRef<[u8]>,
    {
        let data = data.as_ref();
        let length_size = L::write_to_bytes(data.len(), &mut self.buf);
        if length_size == 0 {
            return 0;
        }
        self.buf.put(data);
        length_size + data.len()
    }

    pub fn freeze(self) -> Bytes {
        self.buf.freeze()
    }
}
