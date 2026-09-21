use std::io::{self, Read, Write};

pub trait Le: Copy {
    const SIZE: usize;
    fn put(self, out: &mut Vec<u8>);
    fn get_all(bytes: &[u8]) -> Vec<Self>;
}

macro_rules! le {
    ($t:ty, $n:expr) => {
        impl Le for $t {
            const SIZE: usize = $n;
            fn put(self, out: &mut Vec<u8>) {
                out.extend_from_slice(&self.to_le_bytes());
            }
            fn get_all(bytes: &[u8]) -> Vec<Self> {
                bytes
                    .as_chunks::<$n>()
                    .0
                    .iter()
                    .map(|c| <$t>::from_le_bytes(*c))
                    .collect()
            }
        }
    };
}
le!(u8, 1);
le!(u16, 2);
le!(u32, 4);
le!(u64, 8);
le!(f32, 4);

impl Le for [f32; 2] {
    const SIZE: usize = 8;
    fn put(self, out: &mut Vec<u8>) {
        self[0].put(out);
        self[1].put(out);
    }
    fn get_all(bytes: &[u8]) -> Vec<Self> {
        f32::get_all(bytes).as_chunks::<2>().0.to_vec()
    }
}

pub fn write_all<T: Le, W: Write>(w: &mut W, values: &[T]) -> io::Result<()> {
    let mut buf = Vec::with_capacity((1 << 16) * T::SIZE);
    for chunk in values.chunks(1 << 16) {
        buf.clear();
        for v in chunk {
            v.put(&mut buf);
        }
        w.write_all(&buf)?;
    }
    Ok(())
}

pub fn read_all<T: Le, R: Read>(r: &mut R) -> io::Result<Vec<T>> {
    let mut bytes = Vec::new();
    r.read_to_end(&mut bytes)?;
    if bytes.len() % T::SIZE != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "length is not a multiple of the element size",
        ));
    }
    Ok(T::get_all(&bytes))
}
