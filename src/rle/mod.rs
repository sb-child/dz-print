// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

pub mod lut;
pub mod run;

use deku::bitvec::BitVec;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum RleTransformError {
    #[error("Data is empty.")]
    EmptyData,
}

pub struct BitmapLine {
    data: BitVec,
}

impl BitmapLine {
    pub fn new(data: BitVec) -> Self {
        Self { data }
    }
}
