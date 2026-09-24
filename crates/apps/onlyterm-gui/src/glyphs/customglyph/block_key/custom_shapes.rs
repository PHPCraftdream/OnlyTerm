use super::super::types::*;

pub(super) fn lookup(c: u32) -> Option<BlockKey> {
    Some(match c {
        // [🮘] UPPER LEFT TO LOWER RIGHT FILL
        // NOTE: This is a quick placeholder which doesn't scale correctly
        0x1fb98 => BlockKey::Poly(&[
            Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 10)),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 6), BlockCoord::Zero),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::OutlineThin,
            },
            Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(3, 10)),
                    PolyCommand::LineTo(BlockCoord::Frac(3, 6), BlockCoord::Zero),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::OutlineThin,
            },
            Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(5, 10)),
                    PolyCommand::LineTo(BlockCoord::Frac(5, 6), BlockCoord::Zero),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::OutlineThin,
            },
            Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(7, 10)),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 10)),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::OutlineThin,
            },
            Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(9, 10)),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(3, 10)),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::OutlineThin,
            },
            Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Frac(1, 6), BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(5, 10)),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::OutlineThin,
            },
            Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Frac(3, 6), BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(7, 10)),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::OutlineThin,
            },
            Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Frac(5, 6), BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(9, 10)),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::OutlineThin,
            },
        ]),
        // [🮙] UPPER RIGHT TO LOWER LEFT FILL
        // NOTE: This is a quick placeholder which doesn't scale correctly
        0x1fb99 => BlockKey::Poly(&[
            Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::One, BlockCoord::Frac(1, 10)),
                    PolyCommand::LineTo(BlockCoord::Frac(5, 6), BlockCoord::Zero),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::OutlineThin,
            },
            Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::One, BlockCoord::Frac(3, 10)),
                    PolyCommand::LineTo(BlockCoord::Frac(3, 6), BlockCoord::Zero),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::OutlineThin,
            },
            Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::One, BlockCoord::Frac(5, 10)),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 6), BlockCoord::Zero),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::OutlineThin,
            },
            Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::One, BlockCoord::Frac(7, 10)),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Frac(1, 10)),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::OutlineThin,
            },
            Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::One, BlockCoord::Frac(9, 10)),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Frac(3, 10)),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::OutlineThin,
            },
            Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Frac(5, 6), BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Frac(5, 10)),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::OutlineThin,
            },
            Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Frac(3, 6), BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Frac(7, 10)),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::OutlineThin,
            },
            Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Frac(1, 6), BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Frac(9, 10)),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::OutlineThin,
            },
        ]),
        // [🮚] UPPER AND LOWER TRIANGULAR HALF BLOCK
        0x1fb9a => BlockKey::Triangles(Triangle::UPPER | Triangle::LOWER, BlockAlpha::Full),
        // [🮛] LEFT AND RIGHT TRIANGULAR HALF BLOCK
        0x1fb9b => BlockKey::Triangles(Triangle::LEFT | Triangle::RIGHT, BlockAlpha::Full),
        // [🮜] UPPER UPPER LEFT TRIANGULAR MEDIUM SHADE
        0x1fb9c => BlockKey::Triangles(Triangle::LEFT | Triangle::UPPER, BlockAlpha::Medium),
        // [🮝] UPPER RIGHT TRIANGULAR MEDIUM SHADE
        0x1fb9d => BlockKey::Triangles(Triangle::RIGHT | Triangle::UPPER, BlockAlpha::Medium),
        // [🮞] LOWER RIGHT TRIANGULAR MEDIUM SHADE
        0x1fb9e => BlockKey::Triangles(Triangle::RIGHT | Triangle::LOWER, BlockAlpha::Medium),
        // [🮟] LOWER LEFT TRIANGULAR MEDIUM SHADE
        0x1fb9f => BlockKey::Triangles(Triangle::LEFT | Triangle::LOWER, BlockAlpha::Medium),
        // [🮠] BOX DRAWINGS LIGHT DIAGONAL UPPER CENTRE TO MIDDLE LEFT
        0x1fba0 => BlockKey::CellDiagonals(CellDiagonal::UPPER_LEFT),
        // [🮡] BOX DRAWINGS LIGHT DIAGONAL UPPER CENTRE TO MIDDLE RIGHT
        0x1fba1 => BlockKey::CellDiagonals(CellDiagonal::UPPER_RIGHT),
        // [🮢] BOX DRAWINGS LIGHT DIAGONAL MIDDLE LEFT TO LOWER CENTRE
        0x1fba2 => BlockKey::CellDiagonals(CellDiagonal::LOWER_LEFT),
        // [🮣] BOX DRAWINGS LIGHT DIAGONAL MIDDLE RIGHT TO LOWER CENTRE
        0x1fba3 => BlockKey::CellDiagonals(CellDiagonal::LOWER_RIGHT),
        // [🮤] BOX DRAWINGS LIGHT DIAGONAL UPPER CENTRE TO MIDDLE LEFT TO LOWER CENTRE
        0x1fba4 => BlockKey::CellDiagonals(CellDiagonal::UPPER_LEFT | CellDiagonal::LOWER_LEFT),
        // [🮥] BOX DRAWINGS LIGHT DIAGONAL UPPER CENTRE TO MIDDLE RIGHT TO LOWER CENTRE
        0x1fba5 => BlockKey::CellDiagonals(CellDiagonal::UPPER_RIGHT | CellDiagonal::LOWER_RIGHT),
        // [🮦] BOX DRAWINGS LIGHT DIAGONAL MIDDLE LEFT TO LOWER CENTRE TO MIDDLE RIGHT
        0x1fba6 => BlockKey::CellDiagonals(CellDiagonal::LOWER_LEFT | CellDiagonal::LOWER_RIGHT),
        // [🮧] BOX DRAWINGS LIGHT DIAGONAL MIDDLE LEFT TO UPPER CENTRE TO MIDDLE RIGHT
        0x1fba7 => BlockKey::CellDiagonals(CellDiagonal::UPPER_LEFT | CellDiagonal::UPPER_RIGHT),
        // [🮨] BOX DRAWINGS LIGHT DIAGONAL UPPER CENTRE TO MIDDLE LEFT AND MIDDLE RIGHT TO LOWER CENTRE
        0x1fba8 => BlockKey::CellDiagonals(CellDiagonal::UPPER_LEFT | CellDiagonal::LOWER_RIGHT),
        // [🮩] BOX DRAWINGS LIGHT DIAGONAL UPPER CENTRE TO MIDDLE RIGHT AND MIDDLE LEFT TO LOWER CENTRE
        0x1fba9 => BlockKey::CellDiagonals(CellDiagonal::UPPER_RIGHT | CellDiagonal::LOWER_LEFT),
        // [🮪] BOX DRAWINGS LIGHT DIAGONAL UPPER CENTRE TO MIDDLE RIGHT TO LOWER CENTRE TO MIDDLE LEFT
        0x1fbaa => BlockKey::CellDiagonals(
            CellDiagonal::UPPER_RIGHT | CellDiagonal::LOWER_LEFT | CellDiagonal::LOWER_RIGHT,
        ),
        // [🮫] BOX DRAWINGS LIGHT DIAGONAL UPPER CENTRE TO MIDDLE LEFT TO LOWER CENTRE TO MIDDLE RIGHT
        0x1fbab => BlockKey::CellDiagonals(
            CellDiagonal::UPPER_LEFT | CellDiagonal::LOWER_LEFT | CellDiagonal::LOWER_RIGHT,
        ),
        // [🮬] BOX DRAWINGS LIGHT DIAGONAL MIDDLE LEFT TO UPPER CENTRE TO MIDDLE RIGHT TO LOWER CENTRE
        0x1fbac => BlockKey::CellDiagonals(
            CellDiagonal::UPPER_LEFT | CellDiagonal::UPPER_RIGHT | CellDiagonal::LOWER_RIGHT,
        ),
        // [🮭] BOX DRAWINGS LIGHT DIAGONAL MIDDLE RIGHT TO UPPER CENTRE TO MIDDLE LEFT TO LOWER CENTRE
        0x1fbad => BlockKey::CellDiagonals(
            CellDiagonal::UPPER_LEFT | CellDiagonal::UPPER_RIGHT | CellDiagonal::LOWER_LEFT,
        ),
        // [🮮] BOX DRAWINGS LIGHT DIAGONAL DIAMOND
        0x1fbae => BlockKey::CellDiagonals(
            CellDiagonal::UPPER_LEFT
                | CellDiagonal::UPPER_RIGHT
                | CellDiagonal::LOWER_LEFT
                | CellDiagonal::LOWER_RIGHT,
        ),
        // [🮯] BOX DRAWINGS LIGHT HORIZONTAL WITH VERTICAL STROKE
        0x1fbaf => BlockKey::Poly(&[
            Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            },
            Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            },
        ]),

        // Braille dot patterns
        // ⠀ ⠁ ⠂ ⠃ ⠄ ⠅ ⠆ ⠇ ⠈ ⠉ ⠊ ⠋ ⠌ ⠍ ⠎ ⠏
        // ⠐ ⠑ ⠒ ⠓ ⠔ ⠕ ⠖ ⠗ ⠘ ⠙ ⠚ ⠛ ⠜ ⠝ ⠞ ⠟
        // ⠠ ⠡ ⠢ ⠣ ⠤ ⠥ ⠦ ⠧ ⠨ ⠩ ⠪ ⠫ ⠬ ⠭ ⠮ ⠯
        // ⠰ ⠱ ⠲ ⠳ ⠴ ⠵ ⠶ ⠷ ⠸ ⠹ ⠺ ⠻ ⠼ ⠽ ⠾ ⠿
        // ⡀ ⡁ ⡂ ⡃ ⡄ ⡅ ⡆ ⡇ ⡈ ⡉ ⡊ ⡋ ⡌ ⡍ ⡎ ⡏
        // ⡐ ⡑ ⡒ ⡓ ⡔ ⡕ ⡖ ⡗ ⡘ ⡙ ⡚ ⡛ ⡜ ⡝ ⡞ ⡟
        // ⡠ ⡡ ⡢ ⡣ ⡤ ⡥ ⡦ ⡧ ⡨ ⡩ ⡪ ⡫ ⡬ ⡭ ⡮ ⡯
        // ⡰ ⡱ ⡲ ⡳ ⡴ ⡵ ⡶ ⡷ ⡸ ⡹ ⡺ ⡻ ⡼ ⡽ ⡾ ⡿
        // ⢀ ⢁ ⢂ ⢃ ⢄ ⢅ ⢆ ⢇ ⢈ ⢉ ⢊ ⢋ ⢌ ⢍ ⢎ ⢏
        // ⢐ ⢑ ⢒ ⢓ ⢔ ⢕ ⢖ ⢗ ⢘ ⢙ ⢚ ⢛ ⢜ ⢝ ⢞ ⢟
        // ⢠ ⢡ ⢢ ⢣ ⢤ ⢥ ⢦ ⢧ ⢨ ⢩ ⢪ ⢫ ⢬ ⢭ ⢮ ⢯
        // ⢰ ⢱ ⢲ ⢳ ⢴ ⢵ ⢶ ⢷ ⢸ ⢹ ⢺ ⢻ ⢼ ⢽ ⢾ ⢿
        // ⣀ ⣁ ⣂ ⣃ ⣄ ⣅ ⣆ ⣇ ⣈ ⣉ ⣊ ⣋ ⣌ ⣍ ⣎ ⣏
        // ⣐ ⣑ ⣒ ⣓ ⣔ ⣕ ⣖ ⣗ ⣘ ⣙ ⣚ ⣛ ⣜ ⣝ ⣞ ⣟
        // ⣠ ⣡ ⣢ ⣣ ⣤ ⣥ ⣦ ⣧ ⣨ ⣩ ⣪ ⣫ ⣬ ⣭ ⣮ ⣯
        // ⣰ ⣱ ⣲ ⣳ ⣴ ⣵ ⣶ ⣷ ⣸ ⣹ ⣺ ⣻ ⣼ ⣽ ⣾ ⣿
        n @ 0x2800..=0x28ff => BlockKey::Braille((n & 0xff) as u8),
        // [] Powerline filled right arrow
        0xe0b0 => BlockKey::Poly(&[Poly {
            path: &[
                PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Zero),
                PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::One),
                PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                PolyCommand::Close,
            ],
            intensity: BlockAlpha::Full,
            style: PolyStyle::Fill,
        }]),
        // [] Powerline outline right arrow
        0xe0b1 => BlockKey::Poly(&[Poly {
            path: &[
                PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Zero),
                PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::One),
            ],
            intensity: BlockAlpha::Full,
            style: PolyStyle::Outline,
        }]),
        // [] Powerline filled left arrow
        0xe0b2 => BlockKey::Poly(&[Poly {
            path: &[
                PolyCommand::MoveTo(BlockCoord::One, BlockCoord::Zero),
                PolyCommand::LineTo(BlockCoord::One, BlockCoord::One),
                PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                PolyCommand::Close,
            ],
            intensity: BlockAlpha::Full,
            style: PolyStyle::Fill,
        }]),
        // [] Powerline outline left arrow
        0xe0b3 => BlockKey::Poly(&[Poly {
            path: &[
                PolyCommand::MoveTo(BlockCoord::One, BlockCoord::Zero),
                PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                PolyCommand::LineTo(BlockCoord::One, BlockCoord::One),
            ],
            intensity: BlockAlpha::Full,
            style: PolyStyle::Outline,
        }]),

        // [] Powerline filled left semicircle
        0xe0b4 => BlockKey::Poly(&[Poly {
            path: &[
                PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Zero),
                PolyCommand::QuadTo {
                    control: (BlockCoord::One, BlockCoord::Zero),
                    to: (BlockCoord::One, BlockCoord::Frac(1, 2)),
                },
                PolyCommand::QuadTo {
                    control: (BlockCoord::One, BlockCoord::One),
                    to: (BlockCoord::Zero, BlockCoord::One),
                },
                PolyCommand::Close,
            ],
            intensity: BlockAlpha::Full,
            style: PolyStyle::Fill,
        }]),
        // [] Powerline outline left semicircle
        0xe0b5 => BlockKey::Poly(&[Poly {
            path: &[
                PolyCommand::MoveTo(BlockCoord::Frac(-1, 4), BlockCoord::Frac(-1, 3)),
                PolyCommand::QuadTo {
                    control: (BlockCoord::Frac(7, 4), BlockCoord::Frac(1, 2)),
                    to: (BlockCoord::Frac(-1, 4), BlockCoord::Frac(4, 3)),
                },
            ],
            intensity: BlockAlpha::Full,
            style: PolyStyle::Outline,
        }]),
        // [] Powerline filled right semicircle
        0xe0b6 => BlockKey::Poly(&[Poly {
            path: &[
                PolyCommand::MoveTo(BlockCoord::One, BlockCoord::Zero),
                PolyCommand::QuadTo {
                    control: (BlockCoord::Zero, BlockCoord::Zero),
                    to: (BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                },
                PolyCommand::QuadTo {
                    control: (BlockCoord::Zero, BlockCoord::One),
                    to: (BlockCoord::One, BlockCoord::One),
                },
                PolyCommand::Close,
            ],
            intensity: BlockAlpha::Full,
            style: PolyStyle::Fill,
        }]),
        // [] Powerline outline right semicircle
        0xe0b7 => BlockKey::Poly(&[Poly {
            path: &[
                PolyCommand::MoveTo(BlockCoord::Frac(5, 4), BlockCoord::Frac(-1, 3)),
                PolyCommand::QuadTo {
                    control: (BlockCoord::Frac(-3, 4), BlockCoord::Frac(1, 2)),
                    to: (BlockCoord::Frac(5, 4), BlockCoord::Frac(4, 3)),
                },
            ],
            intensity: BlockAlpha::Full,
            style: PolyStyle::Outline,
        }]),

        // [] Powerline filled bottom left half triangle
        0xe0b8 => BlockKey::Triangles(Triangle::LEFT | Triangle::LOWER, BlockAlpha::Full),
        // [] Powerline outline bottom left half triangle
        0xe0b9 => BlockKey::Poly(&[Poly {
            path: &[
                PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Zero),
                PolyCommand::LineTo(BlockCoord::One, BlockCoord::One),
            ],
            intensity: BlockAlpha::Full,
            style: PolyStyle::Outline,
        }]),
        // [] Powerline filled bottom right half triangle
        0xe0ba => BlockKey::Triangles(Triangle::RIGHT | Triangle::LOWER, BlockAlpha::Full),
        // [] Powerline outline bottom right half triangle
        0xe0bb => BlockKey::Poly(&[Poly {
            path: &[
                PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::One),
                PolyCommand::LineTo(BlockCoord::One, BlockCoord::Zero),
            ],
            intensity: BlockAlpha::Full,
            style: PolyStyle::Outline,
        }]),
        // [] Powerline filled top left half triangle
        0xe0bc => BlockKey::Poly(&[Poly {
            path: &[
                PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Zero),
                PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::One),
                PolyCommand::LineTo(BlockCoord::One, BlockCoord::Zero),
                PolyCommand::Close,
            ],
            intensity: BlockAlpha::Full,
            style: PolyStyle::Fill,
        }]),
        // [] Powerline outline top left half triangle
        0xe0bd => BlockKey::Poly(&[Poly {
            path: &[
                PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::One),
                PolyCommand::LineTo(BlockCoord::One, BlockCoord::Zero),
            ],
            intensity: BlockAlpha::Full,
            style: PolyStyle::Outline,
        }]),
        // [] Powerline filled top right half triangle
        0xe0be => BlockKey::Triangles(Triangle::RIGHT | Triangle::UPPER, BlockAlpha::Full),
        // [] Powerline outline top right half triangle
        0xe0bf => BlockKey::Poly(&[Poly {
            path: &[
                PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Zero),
                PolyCommand::LineTo(BlockCoord::One, BlockCoord::One),
            ],
            intensity: BlockAlpha::Full,
            style: PolyStyle::Outline,
        }]),
        // [] Progress chunk - left empty
        0xee00 => BlockKey::Progress(ProgressChunk::LEFT),
        // [] Progress chunk - middle empty
        0xee01 => BlockKey::Progress(ProgressChunk::MIDDLE),
        // [] Progress chunk - right empty
        0xee02 => BlockKey::Progress(ProgressChunk::RIGHT),
        // [] Progress chunk - left full
        0xee03 => BlockKey::Progress(ProgressChunk::LEFT | ProgressChunk::FULL),
        // [] Progress chunk - middle full
        0xee04 => BlockKey::Progress(ProgressChunk::MIDDLE | ProgressChunk::FULL),
        // [] Progress chunk - right full
        0xee05 => BlockKey::Progress(ProgressChunk::RIGHT | ProgressChunk::FULL),
        n @ 0xee06..=0xee0b => BlockKey::Spinner((n & 0xff) as u8 - 6),
        // [] Branch drawing horizontal
        0xF5D0 => BlockKey::Branches(Branch::HORIZONTAL),
        // [] Branch drawing vertical
        0xF5D1 => BlockKey::Branches(Branch::VERTICAL),
        // [] Branch drawing fade out to right
        0xF5D2 => BlockKey::Poly(&[Poly {
            path: &[
                PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                PolyCommand::LineTo(BlockCoord::Frac(5, 30), BlockCoord::Frac(1, 2)),
                PolyCommand::MoveTo(BlockCoord::Frac(6, 30), BlockCoord::Frac(1, 2)),
                PolyCommand::LineTo(BlockCoord::Frac(10, 30), BlockCoord::Frac(1, 2)),
                PolyCommand::MoveTo(BlockCoord::Frac(12, 30), BlockCoord::Frac(1, 2)),
                PolyCommand::LineTo(BlockCoord::Frac(15, 30), BlockCoord::Frac(1, 2)),
                PolyCommand::MoveTo(BlockCoord::Frac(18, 30), BlockCoord::Frac(1, 2)),
                PolyCommand::LineTo(BlockCoord::Frac(20, 30), BlockCoord::Frac(1, 2)),
                PolyCommand::MoveTo(BlockCoord::Frac(24, 30), BlockCoord::Frac(1, 2)),
                PolyCommand::LineTo(BlockCoord::Frac(25, 30), BlockCoord::Frac(1, 2)),
            ],
            intensity: BlockAlpha::Full,
            style: PolyStyle::OutlineHeavy,
        }]),
        // [] Branch drawing fade out to left
        0xF5D3 => BlockKey::Poly(&[Poly {
            path: &[
                PolyCommand::MoveTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                PolyCommand::LineTo(BlockCoord::Frac(30 - 5, 30), BlockCoord::Frac(1, 2)),
                PolyCommand::MoveTo(BlockCoord::Frac(30 - 6, 30), BlockCoord::Frac(1, 2)),
                PolyCommand::LineTo(BlockCoord::Frac(30 - 10, 30), BlockCoord::Frac(1, 2)),
                PolyCommand::MoveTo(BlockCoord::Frac(30 - 12, 30), BlockCoord::Frac(1, 2)),
                PolyCommand::LineTo(BlockCoord::Frac(30 - 15, 30), BlockCoord::Frac(1, 2)),
                PolyCommand::MoveTo(BlockCoord::Frac(30 - 18, 30), BlockCoord::Frac(1, 2)),
                PolyCommand::LineTo(BlockCoord::Frac(30 - 20, 30), BlockCoord::Frac(1, 2)),
                PolyCommand::MoveTo(BlockCoord::Frac(30 - 24, 30), BlockCoord::Frac(1, 2)),
                PolyCommand::LineTo(BlockCoord::Frac(30 - 25, 30), BlockCoord::Frac(1, 2)),
            ],
            intensity: BlockAlpha::Full,
            style: PolyStyle::OutlineHeavy,
        }]),
        // [] Branch drawing fade out to lower
        0xF5D4 => BlockKey::Poly(&[Poly {
            path: &[
                PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(5, 30)),
                PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(6, 30)),
                PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(10, 30)),
                PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(12, 30)),
                PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(15, 30)),
                PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(18, 30)),
                PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(20, 30)),
                PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(24, 30)),
                PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(25, 30)),
            ],
            intensity: BlockAlpha::Full,
            style: PolyStyle::OutlineHeavy,
        }]),
        // [] Branch drawing fade out to upper
        0xF5D5 => BlockKey::Poly(&[Poly {
            path: &[
                PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(30 - 5, 30)),
                PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(30 - 6, 30)),
                PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(30 - 10, 30)),
                PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(30 - 12, 30)),
                PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(30 - 15, 30)),
                PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(30 - 18, 30)),
                PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(30 - 20, 30)),
                PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(30 - 24, 30)),
                PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(30 - 25, 30)),
            ],
            intensity: BlockAlpha::Full,
            style: PolyStyle::OutlineHeavy,
        }]),
        // [] Branch drawing arc down and right
        0xF5D6 => BlockKey::Branches(Branch::RIGHT_TO_DOWN),
        // [] Branch drawing arc down and left
        0xF5D7 => BlockKey::Branches(Branch::LEFT_TO_DOWN),
        // [] Branch drawing arc up and right
        0xF5D8 => BlockKey::Branches(Branch::RIGHT_TO_UP),
        // [] Branch drawing arc up and left
        0xF5D9 => BlockKey::Branches(Branch::LEFT_TO_UP),
        // [] Branch drawing merge from lower and right
        0xF5DA => BlockKey::Branches(Branch::VERTICAL | Branch::RIGHT_TO_UP),
        // [] Branch drawing branch to upper and right
        0xF5DB => BlockKey::Branches(Branch::VERTICAL | Branch::RIGHT_TO_DOWN),
        // [] Branch drawing upper to right and lower to right
        0xF5DC => BlockKey::Branches(Branch::RIGHT_TO_UP | Branch::RIGHT_TO_DOWN),
        // [] Branch drawing merge from left and down
        0xF5DD => BlockKey::Branches(Branch::VERTICAL | Branch::LEFT_TO_UP),
        // [] Branch drawing branch to left and up
        0xF5DE => BlockKey::Branches(Branch::VERTICAL | Branch::LEFT_TO_DOWN),
        // [] Branch drawing upper to left and lower to left
        0xF5DF => BlockKey::Branches(Branch::LEFT_TO_UP | Branch::LEFT_TO_DOWN),
        // [] Branch drawing from left to right and left to down
        0xF5E0 => BlockKey::Branches(Branch::HORIZONTAL | Branch::LEFT_TO_DOWN),
        // [] Branch drawing from right to left and right to down
        0xF5E1 => BlockKey::Branches(Branch::HORIZONTAL | Branch::RIGHT_TO_DOWN),
        // [] Branch drawing from down to left and down to right
        0xF5E2 => BlockKey::Branches(Branch::LEFT_TO_DOWN | Branch::RIGHT_TO_DOWN),
        // [] Branch drawing from left to up and left to right
        0xF5E3 => BlockKey::Branches(Branch::HORIZONTAL | Branch::LEFT_TO_UP),
        // [] Branch drawing from right to up and right to left
        0xF5E4 => BlockKey::Branches(Branch::HORIZONTAL | Branch::RIGHT_TO_UP),
        // [] Branch drawing from up to left and up to right
        0xF5E5 => BlockKey::Branches(Branch::LEFT_TO_UP | Branch::RIGHT_TO_UP),
        // [] Branch drawing from up to left, up to right, and up to down
        0xF5E6 => BlockKey::Branches(Branch::VERTICAL | Branch::LEFT_TO_UP | Branch::RIGHT_TO_UP),
        // [] Branch drawing from down to left, down to right, and down to up
        0xF5E7 => {
            BlockKey::Branches(Branch::VERTICAL | Branch::LEFT_TO_DOWN | Branch::RIGHT_TO_DOWN)
        }
        // [] Branch drawing from left to right, left to up, and left to down
        0xF5E8 => {
            BlockKey::Branches(Branch::HORIZONTAL | Branch::LEFT_TO_UP | Branch::LEFT_TO_DOWN)
        }
        // [] Branch drawing from right to left, right to up, and right to down
        0xF5E9 => {
            BlockKey::Branches(Branch::HORIZONTAL | Branch::RIGHT_TO_DOWN | Branch::RIGHT_TO_UP)
        }
        // [] Branch drawing from up to down, up to left, and down to right
        0xF5EA => BlockKey::Branches(Branch::VERTICAL | Branch::LEFT_TO_UP | Branch::RIGHT_TO_DOWN),
        // [] Branch drawing from up to down, up to right, and down to left
        0xF5EB => BlockKey::Branches(Branch::VERTICAL | Branch::LEFT_TO_DOWN | Branch::RIGHT_TO_UP),
        // [] Branch drawing from left to right, left to up, and right to down
        0xF5EC => {
            BlockKey::Branches(Branch::HORIZONTAL | Branch::LEFT_TO_UP | Branch::RIGHT_TO_DOWN)
        }
        // [] Branch drawing from left to right, left to down, and right to up
        0xF5ED => {
            BlockKey::Branches(Branch::HORIZONTAL | Branch::LEFT_TO_DOWN | Branch::RIGHT_TO_UP)
        }
        // [] Branch drawing filled circle
        0xF5EE => BlockKey::Branches(Branch::CIRCLE_FILLED),
        // [] Branch drawing outline circle
        0xF5EF => BlockKey::Branches(Branch::CIRCLE_OUTLINE),
        // [] Branch drawing filled circle connected to right
        0xF5F0 => BlockKey::Branches(Branch::CIRCLE_FILLED | Branch::RIGHT),
        // [] Branch drawing outline circle connected to right
        0xF5F1 => BlockKey::Branches(Branch::CIRCLE_OUTLINE | Branch::RIGHT),
        // [] Branch drawing filled circle connected to left
        0xF5F2 => BlockKey::Branches(Branch::CIRCLE_FILLED | Branch::LEFT),
        // [] Branch drawing outline circle connected to left
        0xF5F3 => BlockKey::Branches(Branch::CIRCLE_OUTLINE | Branch::LEFT),
        // [] Branch drawing filled circle connected to left and right
        0xF5F4 => BlockKey::Branches(Branch::CIRCLE_FILLED | Branch::LEFT | Branch::RIGHT),
        // [] Branch drawing outline circle connected to left and right
        0xF5F5 => BlockKey::Branches(Branch::CIRCLE_OUTLINE | Branch::LEFT | Branch::RIGHT),
        // [] Branch drawing filled circle connected to down
        0xF5F6 => BlockKey::Branches(Branch::CIRCLE_FILLED | Branch::DOWN),
        // [] Branch drawing outline circle connected to down
        0xF5F7 => BlockKey::Branches(Branch::CIRCLE_OUTLINE | Branch::DOWN),
        // [] Branch drawing filled circle connected to up
        0xF5F8 => BlockKey::Branches(Branch::CIRCLE_FILLED | Branch::UP),
        // [] Branch drawing outline circle connected to up
        0xF5F9 => BlockKey::Branches(Branch::CIRCLE_OUTLINE | Branch::UP),
        // [] Branch drawing filled circle connected to up and down
        0xF5FA => BlockKey::Branches(Branch::CIRCLE_FILLED | Branch::UP | Branch::DOWN),
        // [] Branch drawing outline circle connected to up and down
        0xF5FB => BlockKey::Branches(Branch::CIRCLE_OUTLINE | Branch::UP | Branch::DOWN),
        // [] Branch drawing filled circle connected to right and down
        0xF5FC => BlockKey::Branches(Branch::CIRCLE_FILLED | Branch::RIGHT | Branch::DOWN),
        // [] Branch drawing outline circle connected to right and down
        0xF5FD => BlockKey::Branches(Branch::CIRCLE_OUTLINE | Branch::RIGHT | Branch::DOWN),
        // [] Branch drawing filled circle connected to left and down
        0xF5FE => BlockKey::Branches(Branch::CIRCLE_FILLED | Branch::LEFT | Branch::DOWN),
        // [] Branch drawing outline circle connected to left and down
        0xF5FF => BlockKey::Branches(Branch::CIRCLE_OUTLINE | Branch::LEFT | Branch::DOWN),
        // [] Branch drawing filled circle connected to right and up
        0xF600 => BlockKey::Branches(Branch::CIRCLE_FILLED | Branch::RIGHT | Branch::UP),
        // [] Branch drawing outline circle connected to right and up
        0xF601 => BlockKey::Branches(Branch::CIRCLE_OUTLINE | Branch::RIGHT | Branch::UP),
        // [] Branch drawing filled circle connected to left and up
        0xF602 => BlockKey::Branches(Branch::CIRCLE_FILLED | Branch::LEFT | Branch::UP),
        // [] Branch drawing outline circle connected to left and up
        0xF603 => BlockKey::Branches(Branch::CIRCLE_OUTLINE | Branch::LEFT | Branch::UP),
        // [] Branch drawing filled circle connected to right, up, and down
        0xF604 => {
            BlockKey::Branches(Branch::CIRCLE_FILLED | Branch::RIGHT | Branch::UP | Branch::DOWN)
        }
        // [] Branch drawing outline circle connected to right, up, and down
        0xF605 => {
            BlockKey::Branches(Branch::CIRCLE_OUTLINE | Branch::RIGHT | Branch::UP | Branch::DOWN)
        }
        // [] Branch drawing filled circle connected to left, up, and down
        0xF606 => {
            BlockKey::Branches(Branch::CIRCLE_FILLED | Branch::LEFT | Branch::UP | Branch::DOWN)
        }
        // [] Branch drawing outline circle connected to left, up, and down
        0xF607 => {
            BlockKey::Branches(Branch::CIRCLE_OUTLINE | Branch::LEFT | Branch::UP | Branch::DOWN)
        }
        // [] Branch drawing filled circle connected to left, right, and down
        0xF608 => {
            BlockKey::Branches(Branch::CIRCLE_FILLED | Branch::LEFT | Branch::RIGHT | Branch::DOWN)
        }
        // [] Branch drawing outline circle connected to left, right, and down
        0xF609 => {
            BlockKey::Branches(Branch::CIRCLE_OUTLINE | Branch::LEFT | Branch::RIGHT | Branch::DOWN)
        }
        // [] Branch drawing filled circle connected to left, right, and up
        0xF60A => {
            BlockKey::Branches(Branch::CIRCLE_FILLED | Branch::LEFT | Branch::RIGHT | Branch::UP)
        }
        // [] Branch drawing outline circle connected to left, right, and up
        0xF60B => {
            BlockKey::Branches(Branch::CIRCLE_OUTLINE | Branch::LEFT | Branch::RIGHT | Branch::UP)
        }
        // [] Branch drawing filled circle connected to left, right, up, and down
        0xF60C => BlockKey::Branches(
            Branch::CIRCLE_FILLED | Branch::LEFT | Branch::RIGHT | Branch::UP | Branch::DOWN,
        ),
        // [] Branch drawing outline circle connected to left, right, up, and down
        0xF60D => BlockKey::Branches(
            Branch::CIRCLE_OUTLINE | Branch::LEFT | Branch::RIGHT | Branch::UP | Branch::DOWN,
        ),
        _ => return None,
    })
}
