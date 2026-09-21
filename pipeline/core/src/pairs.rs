pub struct Pairs {
    pub keys: Vec<u64>,
    pub counts: Vec<u32>,
}

impl Pairs {
    pub fn key(hi: u32, lo: u32) -> u64 {
        ((hi as u64) << 32) | lo as u64
    }
    fn hi(key: u64) -> u32 {
        (key >> 32) as u32
    }
    fn lo(key: u64) -> u32 {
        key as u32
    }
    pub fn iter(&self) -> impl Iterator<Item = (u32, u32, u32)> + Clone + '_ {
        self.keys
            .iter()
            .zip(&self.counts)
            .map(|(&k, &c)| (Pairs::hi(k), Pairs::lo(k), c))
    }
}
