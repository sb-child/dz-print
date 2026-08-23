use crate::{bitmap::BitmapLine, rle::RleTransformError};

#[derive(Debug, PartialEq, Clone)]
pub struct Run<V> {
    pub value: V,
    pub count: usize,
}

impl TryInto<Vec<Run<bool>>> for &BitmapLine {
    type Error = RleTransformError;

    fn try_into(self) -> Result<Vec<Run<bool>>, Self::Error> {
        let len = self.data.len();
        let first_bit: bool = (*(self.data.first().ok_or(Self::Error::EmptyData)?)).into();
        let mut shifted = self.data.clone();
        shifted.shift_end(1);
        let xored = self.data.clone() ^ shifted;
        let prepend_zero = if !first_bit { Some(0) } else { None };
        let res: Vec<Run<bool>> = prepend_zero
            .into_iter()
            .chain(xored.iter_ones())
            .chain(std::iter::once(len))
            .map_windows(|[a, b]| b - a)
            .enumerate()
            .map(|(idx, num)| {
                let bitval = (idx % 2 != 0) ^ first_bit;
                Run {
                    value: bitval,
                    count: num,
                }
            })
            .collect();
        Ok(res)
    }
}
