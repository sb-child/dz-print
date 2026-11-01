// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! deku 的 derive 别名。
//!
//! - `CmdTemplate` = `Debug + PartialEq + DekuWrite + Default + Clone`，写侧模板用；
//! - `RespTemplate` = `Debug + PartialEq + DekuRead + Default + Clone`，读侧模板用。
//!
//! `req_template` / `resp_template` 里的结构体统一写成
//! `#[der(..CmdTemplate)]` / `#[der(..RespTemplate)]`，其中 `der` 就是
//! `derive_aliases::derive`。这样模板只需要关心 deku 的字段和 magic。

use derive_aliases::define;

define! {
    #![export_derive_aliases]
    CmdTemplate = ::core::fmt::Debug, ::std::cmp::PartialEq, ::deku::DekuWrite, ::std::default::Default, ::std::clone::Clone;
    RespTemplate = ::core::fmt::Debug, ::std::cmp::PartialEq, ::deku::DekuRead, ::std::default::Default, ::std::clone::Clone;
}
