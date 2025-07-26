// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

pub mod cmd_parser;
use image::GrayImage;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use tiny_skia::Pixmap;

#[derive(Clone)]
pub struct Bitmap {
    w: u32,
    h: u32,
    pix: Vec<bool>,
}

impl Bitmap {
    /// black (0) pixel will convert to `true`, otherwise to `false`
    pub fn from_gray_image(im: &GrayImage) -> Bitmap {
        im.width();
        im.height();
        // black = true, white = false
        let pix: Vec<bool> = im.par_iter().map(|x| *x == 0).collect();
        Bitmap {
            w: im.width(),
            h: im.height(),
            pix,
        }
    }

    /// black (0) pixel will convert to `true`, otherwise to `false`
    pub fn from_pixmap(im: &Pixmap) -> Bitmap {
        im.width();
        im.height();
        // > 127 black = true, otherwise white = false
        let pix: Vec<bool> = im
            .pixels()
            .par_iter()
            .map(|px| px.demultiply())
            .map(|px| px.red() > 127 || px.green() > 127 || px.blue() > 127)
            .collect();

        Bitmap {
            w: im.width(),
            h: im.height(),
            pix,
        }
    }

    pub fn width(&self) -> u32 {
        self.w
    }

    pub fn height(&self) -> u32 {
        self.h
    }

    pub fn get_pixel(&self, w: u32, h: u32) -> bool {
        self.pix[self.pixel_loc_unchecked(w, h)]
    }

    pub fn get_line(&self, h: u32) -> Vec<bool> {
        let (start, end) = self.line_loc_unchecked(h);
        self.pix[start..end].to_vec()
    }

    pub fn is_line_empty(&self, h: u32) -> bool {
        let (start, end) = self.line_loc_unchecked(h);
        !self.pix[start..end].iter().any(|x| *x)
    }

    pub fn same_lines(&self, h1: u32, h2: u32) -> bool {
        let (start1, end1) = self.line_loc_unchecked(h1);
        let (start2, end2) = self.line_loc_unchecked(h2);
        self.pix[start1..end1] == self.pix[start2..end2]
    }

    pub fn first_black_pixel_in_line(&self, h: u32) -> Option<usize> {
        let (start, end) = self.line_loc_unchecked(h);
        self.pix[start..end]
            .iter()
            .position(|&x| x)
            .map(|i| start + i)
    }

    pub fn last_black_pixel_in_line(&self, h: u32) -> Option<usize> {
        let (start, end) = self.line_loc_unchecked(h);
        self.pix[start..end]
            .iter()
            .rposition(|&x| x)
            .map(|i| start + i)
    }

    pub fn pixel_loc_unchecked(&self, w: u32, h: u32) -> usize {
        h as usize * self.w as usize + w as usize
    }

    /// returns [start..end] of a line
    pub fn line_loc_unchecked(&self, h: u32) -> (usize, usize) {
        let start = self.pixel_loc_unchecked(0, h);
        let end = self.pixel_loc_unchecked(self.w - 1, h);
        (start, end)
    }
}
