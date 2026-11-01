// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use super::*;

/// 按“每行黑点的 x 坐标”造位图。
fn make(width: usize, rows: &[&[usize]]) -> Bitmap {
    let mut bm = Bitmap::new(width, rows.len()).unwrap();
    for (y, xs) in rows.iter().enumerate() {
        let mut line = bm.line_mut(y).unwrap();
        for &x in *xs {
            line.set(x, true);
        }
    }
    bm
}

#[test]
fn blank_rows_become_one_feed() {
    let bm = make(8, &[&[], &[], &[]]);
    assert_eq!(
        print_bitmap(&bm, RleMode::Off).unwrap(),
        vec![PrintCommand::Feed { lines: 3 }]
    );
}

#[test]
fn feed_skips_trailing_blank_rows_in_one_command() {
    let bm = make(8, &[&[0], &[], &[], &[]]);
    assert_eq!(
        print_bitmap(&bm, RleMode::Off).unwrap(),
        vec![
            PrintCommand::Line {
                skips: 0,
                repeats: 0,
                data: vec![0x80],
            },
            PrintCommand::Feed { lines: 3 },
        ]
    );
}

#[test]
fn leading_whole_bytes_go_to_skips() {
    let bm = make(16, &[&[8]]);
    assert_eq!(
        print_bitmap(&bm, RleMode::Off).unwrap(),
        vec![PrintCommand::Line {
            skips: 1,
            repeats: 0,
            data: vec![0x80],
        }]
    );
}

#[test]
fn sub_byte_leading_blank_stays_in_data() {
    let bm = make(16, &[&[4]]);
    assert_eq!(
        print_bitmap(&bm, RleMode::Off).unwrap(),
        vec![PrintCommand::Line {
            skips: 0,
            repeats: 0,
            data: vec![0x08],
        }]
    );
}

#[test]
fn trailing_blank_bytes_are_trimmed() {
    let bm = make(16, &[&[4, 8]]);
    assert_eq!(
        print_bitmap(&bm, RleMode::Off).unwrap(),
        vec![PrintCommand::Line {
            skips: 0,
            repeats: 0,
            data: vec![0x08, 0x80],
        }]
    );
}

#[test]
fn equal_rows_collapse_into_repeats() {
    let bm = make(8, &[&[0], &[0], &[0], &[7]]);
    assert_eq!(
        print_bitmap(&bm, RleMode::Off).unwrap(),
        vec![
            PrintCommand::Line {
                skips: 0,
                repeats: 2,
                data: vec![0x80],
            },
            PrintCommand::Line {
                skips: 0,
                repeats: 0,
                data: vec![0x01],
            },
        ]
    );
}

#[test]
fn blank_and_line_are_interleaved_in_order() {
    let bm = make(8, &[&[], &[0], &[], &[]]);
    assert_eq!(
        print_bitmap(&bm, RleMode::Off).unwrap(),
        vec![
            PrintCommand::Feed { lines: 1 },
            PrintCommand::Line {
                skips: 0,
                repeats: 0,
                data: vec![0x80],
            },
            PrintCommand::Feed { lines: 2 },
        ]
    );
}

#[test]
fn width_above_limit_errors() {
    let bm = Bitmap::new(MAX_DOTS_PER_LINE + 1, 1).unwrap();
    assert_eq!(
        print_bitmap(&bm, RleMode::Off).unwrap_err(),
        PrintError::LineTooWide {
            width: MAX_DOTS_PER_LINE + 1,
            max: MAX_DOTS_PER_LINE,
        }
    );
}

#[test]
fn max_width_line_still_fits_protocol_limits() {
    let last = MAX_DOTS_PER_LINE - 1;
    let bm = make(MAX_DOTS_PER_LINE, &[&[last]]);
    assert_eq!(
        print_bitmap(&bm, RleMode::Off).unwrap(),
        vec![PrintCommand::Line {
            skips: (last / 8) as u8,
            repeats: 0,
            data: vec![0x80],
        }]
    );
}

#[test]
fn rle_is_skipped_when_not_smaller_than_raw_line() {
    // 一整行黑: 原始 1 字节, RLE4 也是 1 字节, 不赚就回退。
    let bm = make(8, &[&[0, 1, 2, 3, 4, 5, 6, 7]]);
    for mode in [
        RleMode::Byte,
        RleMode::Bits4,
        RleMode::Bits5,
        RleMode::Bits6,
    ] {
        assert_eq!(
            print_bitmap(&bm, mode).unwrap(),
            vec![PrintCommand::Line {
                skips: 0,
                repeats: 0,
                data: vec![0xFF],
            }],
            "{mode:?}"
        );
    }
}

#[test]
fn byte_rle_wins_for_a_solid_row() {
    let mut bm = Bitmap::new(40, 1).unwrap();
    bm.line_mut(0).unwrap().fill(true);
    assert_eq!(
        print_bitmap(&bm, RleMode::Byte).unwrap(),
        vec![PrintCommand::RleByte {
            repeats: 0,
            rle_byte_count: 2,
            data: vec![0xC5, 0xFF],
        }]
    );
}

#[test]
fn bits4_rle_wins_for_a_solid_row() {
    let mut bm = Bitmap::new(32, 1).unwrap();
    bm.line_mut(0).unwrap().fill(true);
    // 32 = 3 + 29 -> 黑,索引2 + 黑,索引6 -> 1010 1110
    assert_eq!(
        print_bitmap(&bm, RleMode::Bits4).unwrap(),
        vec![PrintCommand::Rle4x {
            repeats: 0,
            rle_runs_count: 2,
            data: vec![0xAE],
        }]
    );
}

#[test]
fn rle_d_is_chosen_when_xor_is_smaller() {
    let evens: Vec<usize> = (0..32).step_by(2).collect();
    let odds: Vec<usize> = (1..32).step_by(2).collect();
    let bm = make(32, &[&evens, &odds]);
    assert_eq!(
        print_bitmap(&bm, RleMode::Bits4).unwrap(),
        vec![
            // 第一行棋盘格, RLE 比 4 字节还大, 回退普通行
            PrintCommand::Line {
                skips: 0,
                repeats: 0,
                data: vec![0xAA, 0xAA, 0xAA, 0xAA],
            },
            // 第二行与第一行 XOR 后是全黑, 2 个符号 1 字节
            PrintCommand::Rle4d {
                repeats: 0,
                rle_runs_count: 2,
                data: vec![0xAE],
            },
        ]
    );
}

#[test]
fn x_only_mode_never_emits_d() {
    let evens: Vec<usize> = (0..32).step_by(2).collect();
    let odds: Vec<usize> = (1..32).step_by(2).collect();
    let bm = make(32, &[&evens, &odds]);
    for mode in [
        RleMode::Bits4XOnly,
        RleMode::Bits5XOnly,
        RleMode::Bits6XOnly,
    ] {
        let cmds = print_bitmap(&bm, mode).unwrap();
        assert!(
            !cmds.iter().any(|c| matches!(
                c,
                PrintCommand::Rle4d { .. }
                    | PrintCommand::Rle5d { .. }
                    | PrintCommand::Rle6d { .. }
            )),
            "{mode:?} emitted a D command"
        );
    }
}

#[test]
fn rle_keeps_repeats_of_equal_rows() {
    let mut bm = Bitmap::new(32, 2).unwrap();
    bm.line_mut(0).unwrap().fill(true);
    bm.line_mut(1).unwrap().fill(true);
    assert_eq!(
        print_bitmap(&bm, RleMode::Bits4).unwrap(),
        vec![PrintCommand::Rle4x {
            repeats: 1,
            rle_runs_count: 2,
            data: vec![0xAE],
        }]
    );
}

/// 一组命令实际打印 / 走纸的总行数。
fn printed_lines(cmds: &[PrintCommand]) -> usize {
    cmds.iter()
        .map(|cmd| match cmd {
            PrintCommand::Feed { lines } => *lines as usize,
            PrintCommand::Line { repeats, .. }
            | PrintCommand::RleByte { repeats, .. }
            | PrintCommand::Rle4x { repeats, .. }
            | PrintCommand::Rle4d { repeats, .. }
            | PrintCommand::Rle5x { repeats, .. }
            | PrintCommand::Rle5d { repeats, .. }
            | PrintCommand::Rle6x { repeats, .. }
            | PrintCommand::Rle6d { repeats, .. } => *repeats as usize + 1,
        })
        .sum()
}

#[test]
fn long_bitmap_splits_at_total_lines_limit() {
    let height = MAX_LINES_PER_PAGE + 10;
    let mut bm = Bitmap::new(8, height).unwrap();
    bm.fill(true);

    let pages = print_bitmap_pages(&bm, RleMode::Off, MAX_LINES_PER_PAGE).unwrap();
    assert_eq!(pages.len(), 2);
    assert_eq!(pages[0].lines, MAX_LINES_PER_PAGE);
    assert_eq!(pages[1].lines, 10);
    assert_eq!(printed_lines(&pages[0].commands), MAX_LINES_PER_PAGE);
    assert_eq!(printed_lines(&pages[1].commands), 10);
    assert_eq!(
        pages[0].lines + pages[1].lines,
        bm.height(),
        "pages must cover the whole bitmap"
    );
}

#[test]
fn into_reqs_maps_line_and_rle_commands() {
    let commands = vec![
        PrintCommand::Line {
            skips: 0,
            repeats: 0,
            data: vec![0x80],
        },
        PrintCommand::Rle5x {
            repeats: 0,
            rle_runs_count: 2,
            data: vec![0x1F, 0xC0],
        },
    ];
    let reqs = into_reqs(&commands);
    let refs: Vec<&dyn ReqTrait> = reqs.iter().map(|req| req.as_ref()).collect();
    let bytes = crate::planner::Assembler::new(crate::command::ModelVersion::V2)
        .encode(&refs)
        .unwrap();
    assert_eq!(
        bytes,
        vec![
            0x1f, 0x21, 0x03, 0x00, 0x00, 0x80, 0x5b, // PrintLine
            0x1f, 0x2c, 0x02, 0x1f, 0xc0, 0xf2, // RLE5X
        ]
    );
}

#[test]
fn pages_do_not_reuse_previous_page_for_d() {
    let height = MAX_LINES_PER_PAGE + 2;
    let mut bm = Bitmap::new(32, height).unwrap();
    bm.fill(true);

    let pages = print_bitmap_pages(&bm, RleMode::Bits5, MAX_LINES_PER_PAGE).unwrap();
    assert!(
        matches!(pages[1].commands[0], PrintCommand::Rle5x { .. }),
        "the first line of a page must not XOR against the previous page, got {:?}",
        pages[1].commands[0]
    );
}

#[test]
fn d_is_not_used_after_feed() {
    // 第 0 行整行黑；中间两行空白合并成 Feed；第 3 行只差最后一个点。
    // 如果拿位图里的空白行当 D 基准，或跨 Feed 引用第 0 行，都会算错。
    let mut bm = Bitmap::new(64, 4).unwrap();
    bm.line_mut(0).unwrap().fill(true);
    bm.line_mut(3).unwrap().fill(true);
    bm.line_mut(3).unwrap().set(63, false);

    let cmds = print_bitmap(&bm, RleMode::Bits4).unwrap();
    assert!(
        matches!(cmds[1], PrintCommand::Feed { lines: 2 }),
        "{cmds:?}"
    );
    assert!(
        matches!(cmds[2], PrintCommand::Rle4x { .. }),
        "feed must reset the D base, got {:?}",
        cmds[2]
    );
    assert!(
        !cmds.iter().any(|c| matches!(
            c,
            PrintCommand::Rle4d { .. } | PrintCommand::Rle5d { .. } | PrintCommand::Rle6d { .. }
        )),
        "{cmds:?}"
    );
}

#[test]
fn page_line_limit_must_fit_total_lines() {
    let bm = Bitmap::new(8, 1).unwrap();
    for limit in [0, MAX_LINES_PER_PAGE + 1] {
        assert_eq!(
            print_bitmap_pages(&bm, RleMode::Off, limit),
            Err(PrintError::InvalidPageLines {
                lines: limit,
                max: MAX_LINES_PER_PAGE,
            })
        );
    }
}

#[test]
fn page_start_must_be_inside_the_bitmap() {
    let bm = Bitmap::new(8, 4).unwrap();
    assert_eq!(
        print_bitmap_page(&bm, 4, RleMode::Off, MAX_LINES_PER_PAGE),
        Err(PrintError::Bitmap(BitmapError::LineOutOfRange {
            index: 4,
            height: 4,
        }))
    );

    let page = print_bitmap_page(&bm, 2, RleMode::Off, MAX_LINES_PER_PAGE).unwrap();
    assert_eq!(page.lines, 2);
}
