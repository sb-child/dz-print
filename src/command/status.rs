// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

pub enum PrinterErrorCode {
    Cancelled = 12,
    VolTooLow = 30,
    VolTooHigh = 31,
    TphNotFound = 32,
    TphTooHot = 33,
    CoverOpened = 34,
    NoPaper = 35,
    TphOpened = 36,
    NoRibbon = 37,
    UnmatchedRibbon = 38,
    TphTooCold = 39,
    UsedupRibbon = 40,
    UsedupRibbon2 = 41,
    LabelCanOpend = 50,
}
