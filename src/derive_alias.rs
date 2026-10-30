// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use derive_aliases::define;

define! {
    #![export_derive_aliases]
    CmdTemplate = ::core::fmt::Debug, ::std::cmp::PartialEq, ::deku::DekuWrite, ::std::default::Default, ::std::clone::Clone;
}
