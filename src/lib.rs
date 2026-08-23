// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

// rle模块需要
#![feature(iter_map_windows)]

// pub mod backend;
pub mod bitmap;
pub mod command;
pub mod derive_alias;
pub mod frontend;
pub mod image_proc;
pub mod info;
pub mod param;
pub mod planner;
pub mod rle;
pub mod scheduler;

#[cfg(test)]
mod tests {
    // use super::*;

    #[test]
    fn test() {}
}
