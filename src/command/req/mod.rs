// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

pub mod page_start;

use crate::command::{envelope::Envelope, models::ModelVersion};

pub trait ReqTrait {
    fn pack(&self, cv: ModelVersion) -> Vec<Envelope>; // 需要再设计一下
}
