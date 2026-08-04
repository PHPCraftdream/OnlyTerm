use super::types::*;

// Lookup table from sextant Unicode range 0x1fb00..=0x1fb3b to sextant pattern:
// `pattern` is a byte whose bits corresponds to elements on a 2 by 3 grid.
// The position of a sextant for a bit position (0-indexed) is as follows:
// ╭───┬───╮
// │ 0 │ 1 │
// ├───┼───┤
// │ 2 │ 3 │
// ├───┼───┤
// │ 4 │ 5 │
// ╰───┴───╯
const SEXTANT_PATTERNS: [u8; 60] = [
    0b000001, // [🬀] BLOCK SEXTANT-1
    0b000010, // [🬁] BLOCK SEXTANT-2
    0b000011, // [🬂] BLOCK SEXTANT-12
    0b000100, // [🬃] BLOCK SEXTANT-3
    0b000101, // [🬄] BLOCK SEXTANT-13
    0b000110, // [🬅] BLOCK SEXTANT-23
    0b000111, // [🬆] BLOCK SEXTANT-123
    0b001000, // [🬇] BLOCK SEXTANT-4
    0b001001, // [🬈] BLOCK SEXTANT-14
    0b001010, // [🬉] BLOCK SEXTANT-24
    0b001011, // [🬊] BLOCK SEXTANT-124
    0b001100, // [🬋] BLOCK SEXTANT-34
    0b001101, // [🬌] BLOCK SEXTANT-134
    0b001110, // [🬍] BLOCK SEXTANT-234
    0b001111, // [🬎] BLOCK SEXTANT-1234
    0b010000, // [🬏] BLOCK SEXTANT-5
    0b010001, // [🬐] BLOCK SEXTANT-15
    0b010010, // [🬑] BLOCK SEXTANT-25
    0b010011, // [🬒] BLOCK SEXTANT-125
    0b010100, // [🬓] BLOCK SEXTANT-35
    0b010110, // [🬔] BLOCK SEXTANT-235
    0b010111, // [🬕] BLOCK SEXTANT-1235
    0b011000, // [🬖] BLOCK SEXTANT-45
    0b011001, // [🬗] BLOCK SEXTANT-145
    0b011010, // [🬘] BLOCK SEXTANT-245
    0b011011, // [🬙] BLOCK SEXTANT-1245
    0b011100, // [🬚] BLOCK SEXTANT-345
    0b011101, // [🬛] BLOCK SEXTANT-1345
    0b011110, // [🬜] BLOCK SEXTANT-2345
    0b011111, // [🬝] BLOCK SEXTANT-12345
    0b100000, // [🬞] BLOCK SEXTANT-6
    0b100001, // [🬟] BLOCK SEXTANT-16
    0b100010, // [🬠] BLOCK SEXTANT-26
    0b100011, // [🬡] BLOCK SEXTANT-126
    0b100100, // [🬢] BLOCK SEXTANT-36
    0b100101, // [🬣] BLOCK SEXTANT-136
    0b100110, // [🬤] BLOCK SEXTANT-236
    0b100111, // [🬥] BLOCK SEXTANT-1236
    0b101000, // [🬦] BLOCK SEXTANT-46
    0b101001, // [🬧] BLOCK SEXTANT-146
    0b101011, // [🬨] BLOCK SEXTANT-1246
    0b101100, // [🬩] BLOCK SEXTANT-346
    0b101101, // [🬪] BLOCK SEXTANT-1346
    0b101110, // [🬫] BLOCK SEXTANT-2346
    0b101111, // [🬬] BLOCK SEXTANT-12346
    0b110000, // [🬭] BLOCK SEXTANT-56
    0b110001, // [🬮] BLOCK SEXTANT-156
    0b110010, // [🬯] BLOCK SEXTANT-256
    0b110011, // [🬰] BLOCK SEXTANT-1256
    0b110100, // [🬱] BLOCK SEXTANT-356
    0b110101, // [🬲] BLOCK SEXTANT-1356
    0b110110, // [🬳] BLOCK SEXTANT-2356
    0b110111, // [🬴] BLOCK SEXTANT-12356
    0b111000, // [🬵] BLOCK SEXTANT-456
    0b111001, // [🬶] BLOCK SEXTANT-1456
    0b111010, // [🬷] BLOCK SEXTANT-2456
    0b111011, // [🬸] BLOCK SEXTANT-12456
    0b111100, // [🬹] BLOCK SEXTANT-3456
    0b111101, // [🬺] BLOCK SEXTANT-13456
    0b111110, // [🬻] BLOCK SEXTANT-23456
];

// Lookup table from octant Unicode range 0x1cd00..=0x1cde5 to octant pattern:
// `pattern` is a byte whose bits corresponds to elements on a 2 by 4 grid.
// The position of a octant for a bit position (0-indexed) is as follows:
// ╭───┬───╮
// │ 0 │ 1 │
// ├───┼───┤
// │ 2 │ 3 │
// ├───┼───┤
// │ 4 │ 5 │
// ├───┼───┤
// │ 6 │ 7 │
// ╰───┴───╯
const OCTANT_PATTERNS: [u8; 230] = [
    0b00000100, // 1CD00;BLOCK OCTANT-3
    0b00000110, // 1CD01;BLOCK OCTANT-23
    0b00000111, // 1CD02;BLOCK OCTANT-123
    0b00001000, // 1CD03;BLOCK OCTANT-4
    0b00001001, // 1CD04;BLOCK OCTANT-14
    0b00001011, // 1CD05;BLOCK OCTANT-124
    0b00001100, // 1CD06;BLOCK OCTANT-34
    0b00001101, // 1CD07;BLOCK OCTANT-134
    0b00001110, // 1CD08;BLOCK OCTANT-234
    0b00010000, // 1CD09;BLOCK OCTANT-5
    0b00010001, // 1CD0A;BLOCK OCTANT-15
    0b00010010, // 1CD0B;BLOCK OCTANT-25
    0b00010011, // 1CD0C;BLOCK OCTANT-125
    0b00010101, // 1CD0D;BLOCK OCTANT-135
    0b00010110, // 1CD0E;BLOCK OCTANT-235
    0b00010111, // 1CD0F;BLOCK OCTANT-1235
    0b00011000, // 1CD10;BLOCK OCTANT-45
    0b00011001, // 1CD11;BLOCK OCTANT-145
    0b00011010, // 1CD12;BLOCK OCTANT-245
    0b00011011, // 1CD13;BLOCK OCTANT-1245
    0b00011100, // 1CD14;BLOCK OCTANT-345
    0b00011101, // 1CD15;BLOCK OCTANT-1345
    0b00011110, // 1CD16;BLOCK OCTANT-2345
    0b00011111, // 1CD17;BLOCK OCTANT-12345
    0b00100000, // 1CD18;BLOCK OCTANT-6
    0b00100001, // 1CD19;BLOCK OCTANT-16
    0b00100010, // 1CD1A;BLOCK OCTANT-26
    0b00100011, // 1CD1B;BLOCK OCTANT-126
    0b00100100, // 1CD1C;BLOCK OCTANT-36
    0b00100101, // 1CD1D;BLOCK OCTANT-136
    0b00100110, // 1CD1E;BLOCK OCTANT-236
    0b00100111, // 1CD1F;BLOCK OCTANT-1236
    0b00101001, // 1CD20;BLOCK OCTANT-146
    0b00101010, // 1CD21;BLOCK OCTANT-246
    0b00101011, // 1CD22;BLOCK OCTANT-1246
    0b00101100, // 1CD23;BLOCK OCTANT-346
    0b00101101, // 1CD24;BLOCK OCTANT-1346
    0b00101110, // 1CD25;BLOCK OCTANT-2346
    0b00101111, // 1CD26;BLOCK OCTANT-12346
    0b00110000, // 1CD27;BLOCK OCTANT-56
    0b00110001, // 1CD28;BLOCK OCTANT-156
    0b00110010, // 1CD29;BLOCK OCTANT-256
    0b00110011, // 1CD2A;BLOCK OCTANT-1256
    0b00110100, // 1CD2B;BLOCK OCTANT-356
    0b00110101, // 1CD2C;BLOCK OCTANT-1356
    0b00110110, // 1CD2D;BLOCK OCTANT-2356
    0b00110111, // 1CD2E;BLOCK OCTANT-12356
    0b00111000, // 1CD2F;BLOCK OCTANT-456
    0b00111001, // 1CD30;BLOCK OCTANT-1456
    0b00111010, // 1CD31;BLOCK OCTANT-2456
    0b00111011, // 1CD32;BLOCK OCTANT-12456
    0b00111100, // 1CD33;BLOCK OCTANT-3456
    0b00111101, // 1CD34;BLOCK OCTANT-13456
    0b00111110, // 1CD35;BLOCK OCTANT-23456
    0b01000001, // 1CD36;BLOCK OCTANT-17
    0b01000010, // 1CD37;BLOCK OCTANT-27
    0b01000011, // 1CD38;BLOCK OCTANT-127
    0b01000100, // 1CD39;BLOCK OCTANT-37
    0b01000101, // 1CD3A;BLOCK OCTANT-137
    0b01000110, // 1CD3B;BLOCK OCTANT-237
    0b01000111, // 1CD3C;BLOCK OCTANT-1237
    0b01001000, // 1CD3D;BLOCK OCTANT-47
    0b01001001, // 1CD3E;BLOCK OCTANT-147
    0b01001010, // 1CD3F;BLOCK OCTANT-247
    0b01001011, // 1CD40;BLOCK OCTANT-1247
    0b01001100, // 1CD41;BLOCK OCTANT-347
    0b01001101, // 1CD42;BLOCK OCTANT-1347
    0b01001110, // 1CD43;BLOCK OCTANT-2347
    0b01001111, // 1CD44;BLOCK OCTANT-12347
    0b01010001, // 1CD45;BLOCK OCTANT-157
    0b01010010, // 1CD46;BLOCK OCTANT-257
    0b01010011, // 1CD47;BLOCK OCTANT-1257
    0b01010100, // 1CD48;BLOCK OCTANT-357
    0b01010110, // 1CD49;BLOCK OCTANT-2357
    0b01010111, // 1CD4A;BLOCK OCTANT-12357
    0b01011000, // 1CD4B;BLOCK OCTANT-457
    0b01011001, // 1CD4C;BLOCK OCTANT-1457
    0b01011011, // 1CD4D;BLOCK OCTANT-12457
    0b01011100, // 1CD4E;BLOCK OCTANT-3457
    0b01011101, // 1CD4F;BLOCK OCTANT-13457
    0b01011110, // 1CD50;BLOCK OCTANT-23457
    0b01100000, // 1CD51;BLOCK OCTANT-67
    0b01100001, // 1CD52;BLOCK OCTANT-167
    0b01100010, // 1CD53;BLOCK OCTANT-267
    0b01100011, // 1CD54;BLOCK OCTANT-1267
    0b01100100, // 1CD55;BLOCK OCTANT-367
    0b01100101, // 1CD56;BLOCK OCTANT-1367
    0b01100110, // 1CD57;BLOCK OCTANT-2367
    0b01100111, // 1CD58;BLOCK OCTANT-12367
    0b01101000, // 1CD59;BLOCK OCTANT-467
    0b01101001, // 1CD5A;BLOCK OCTANT-1467
    0b01101010, // 1CD5B;BLOCK OCTANT-2467
    0b01101011, // 1CD5C;BLOCK OCTANT-12467
    0b01101100, // 1CD5D;BLOCK OCTANT-3467
    0b01101101, // 1CD5E;BLOCK OCTANT-13467
    0b01101110, // 1CD5F;BLOCK OCTANT-23467
    0b01101111, // 1CD60;BLOCK OCTANT-123467
    0b01110000, // 1CD61;BLOCK OCTANT-567
    0b01110001, // 1CD62;BLOCK OCTANT-1567
    0b01110010, // 1CD63;BLOCK OCTANT-2567
    0b01110011, // 1CD64;BLOCK OCTANT-12567
    0b01110100, // 1CD65;BLOCK OCTANT-3567
    0b01110101, // 1CD66;BLOCK OCTANT-13567
    0b01110110, // 1CD67;BLOCK OCTANT-23567
    0b01110111, // 1CD68;BLOCK OCTANT-123567
    0b01111000, // 1CD69;BLOCK OCTANT-4567
    0b01111001, // 1CD6A;BLOCK OCTANT-14567
    0b01111010, // 1CD6B;BLOCK OCTANT-24567
    0b01111011, // 1CD6C;BLOCK OCTANT-124567
    0b01111100, // 1CD6D;BLOCK OCTANT-34567
    0b01111101, // 1CD6E;BLOCK OCTANT-134567
    0b01111110, // 1CD6F;BLOCK OCTANT-234567
    0b01111111, // 1CD70;BLOCK OCTANT-1234567
    0b10000001, // 1CD71;BLOCK OCTANT-18
    0b10000010, // 1CD72;BLOCK OCTANT-28
    0b10000011, // 1CD73;BLOCK OCTANT-128
    0b10000100, // 1CD74;BLOCK OCTANT-38
    0b10000101, // 1CD75;BLOCK OCTANT-138
    0b10000110, // 1CD76;BLOCK OCTANT-238
    0b10000111, // 1CD77;BLOCK OCTANT-1238
    0b10001000, // 1CD78;BLOCK OCTANT-48
    0b10001001, // 1CD79;BLOCK OCTANT-148
    0b10001010, // 1CD7A;BLOCK OCTANT-248
    0b10001011, // 1CD7B;BLOCK OCTANT-1248
    0b10001100, // 1CD7C;BLOCK OCTANT-348
    0b10001101, // 1CD7D;BLOCK OCTANT-1348
    0b10001110, // 1CD7E;BLOCK OCTANT-2348
    0b10001111, // 1CD7F;BLOCK OCTANT-12348
    0b10010000, // 1CD80;BLOCK OCTANT-58
    0b10010001, // 1CD81;BLOCK OCTANT-158
    0b10010010, // 1CD82;BLOCK OCTANT-258
    0b10010011, // 1CD83;BLOCK OCTANT-1258
    0b10010100, // 1CD84;BLOCK OCTANT-358
    0b10010101, // 1CD85;BLOCK OCTANT-1358
    0b10010110, // 1CD86;BLOCK OCTANT-2358
    0b10010111, // 1CD87;BLOCK OCTANT-12358
    0b10011000, // 1CD88;BLOCK OCTANT-458
    0b10011001, // 1CD89;BLOCK OCTANT-1458
    0b10011010, // 1CD8A;BLOCK OCTANT-2458
    0b10011011, // 1CD8B;BLOCK OCTANT-12458
    0b10011100, // 1CD8C;BLOCK OCTANT-3458
    0b10011101, // 1CD8D;BLOCK OCTANT-13458
    0b10011110, // 1CD8E;BLOCK OCTANT-23458
    0b10011111, // 1CD8F;BLOCK OCTANT-123458
    0b10100001, // 1CD90;BLOCK OCTANT-168
    0b10100010, // 1CD91;BLOCK OCTANT-268
    0b10100011, // 1CD92;BLOCK OCTANT-1268
    0b10100100, // 1CD93;BLOCK OCTANT-368
    0b10100110, // 1CD94;BLOCK OCTANT-2368
    0b10100111, // 1CD95;BLOCK OCTANT-12368
    0b10101000, // 1CD96;BLOCK OCTANT-468
    0b10101001, // 1CD97;BLOCK OCTANT-1468
    0b10101011, // 1CD98;BLOCK OCTANT-12468
    0b10101100, // 1CD99;BLOCK OCTANT-3468
    0b10101101, // 1CD9A;BLOCK OCTANT-13468
    0b10101110, // 1CD9B;BLOCK OCTANT-23468
    0b10110000, // 1CD9C;BLOCK OCTANT-568
    0b10110001, // 1CD9D;BLOCK OCTANT-1568
    0b10110010, // 1CD9E;BLOCK OCTANT-2568
    0b10110011, // 1CD9F;BLOCK OCTANT-12568
    0b10110100, // 1CDA0;BLOCK OCTANT-3568
    0b10110101, // 1CDA1;BLOCK OCTANT-13568
    0b10110110, // 1CDA2;BLOCK OCTANT-23568
    0b10110111, // 1CDA3;BLOCK OCTANT-123568
    0b10111000, // 1CDA4;BLOCK OCTANT-4568
    0b10111001, // 1CDA5;BLOCK OCTANT-14568
    0b10111010, // 1CDA6;BLOCK OCTANT-24568
    0b10111011, // 1CDA7;BLOCK OCTANT-124568
    0b10111100, // 1CDA8;BLOCK OCTANT-34568
    0b10111101, // 1CDA9;BLOCK OCTANT-134568
    0b10111110, // 1CDAA;BLOCK OCTANT-234568
    0b10111111, // 1CDAB;BLOCK OCTANT-1234568
    0b11000001, // 1CDAC;BLOCK OCTANT-178
    0b11000010, // 1CDAD;BLOCK OCTANT-278
    0b11000011, // 1CDAE;BLOCK OCTANT-1278
    0b11000100, // 1CDAF;BLOCK OCTANT-378
    0b11000101, // 1CDB0;BLOCK OCTANT-1378
    0b11000110, // 1CDB1;BLOCK OCTANT-2378
    0b11000111, // 1CDB2;BLOCK OCTANT-12378
    0b11001000, // 1CDB3;BLOCK OCTANT-478
    0b11001001, // 1CDB4;BLOCK OCTANT-1478
    0b11001010, // 1CDB5;BLOCK OCTANT-2478
    0b11001011, // 1CDB6;BLOCK OCTANT-12478
    0b11001100, // 1CDB7;BLOCK OCTANT-3478
    0b11001101, // 1CDB8;BLOCK OCTANT-13478
    0b11001110, // 1CDB9;BLOCK OCTANT-23478
    0b11001111, // 1CDBA;BLOCK OCTANT-123478
    0b11010000, // 1CDBB;BLOCK OCTANT-578
    0b11010001, // 1CDBC;BLOCK OCTANT-1578
    0b11010010, // 1CDBD;BLOCK OCTANT-2578
    0b11010011, // 1CDBE;BLOCK OCTANT-12578
    0b11010100, // 1CDBF;BLOCK OCTANT-3578
    0b11010101, // 1CDC0;BLOCK OCTANT-13578
    0b11010110, // 1CDC1;BLOCK OCTANT-23578
    0b11010111, // 1CDC2;BLOCK OCTANT-123578
    0b11011000, // 1CDC3;BLOCK OCTANT-4578
    0b11011001, // 1CDC4;BLOCK OCTANT-14578
    0b11011010, // 1CDC5;BLOCK OCTANT-24578
    0b11011011, // 1CDC6;BLOCK OCTANT-124578
    0b11011100, // 1CDC7;BLOCK OCTANT-34578
    0b11011101, // 1CDC8;BLOCK OCTANT-134578
    0b11011110, // 1CDC9;BLOCK OCTANT-234578
    0b11011111, // 1CDCA;BLOCK OCTANT-1234578
    0b11100000, // 1CDCB;BLOCK OCTANT-678
    0b11100001, // 1CDCC;BLOCK OCTANT-1678
    0b11100010, // 1CDCD;BLOCK OCTANT-2678
    0b11100011, // 1CDCE;BLOCK OCTANT-12678
    0b11100100, // 1CDCF;BLOCK OCTANT-3678
    0b11100101, // 1CDD0;BLOCK OCTANT-13678
    0b11100110, // 1CDD1;BLOCK OCTANT-23678
    0b11100111, // 1CDD2;BLOCK OCTANT-123678
    0b11101000, // 1CDD3;BLOCK OCTANT-4678
    0b11101001, // 1CDD4;BLOCK OCTANT-14678
    0b11101010, // 1CDD5;BLOCK OCTANT-24678
    0b11101011, // 1CDD6;BLOCK OCTANT-124678
    0b11101100, // 1CDD7;BLOCK OCTANT-34678
    0b11101101, // 1CDD8;BLOCK OCTANT-134678
    0b11101110, // 1CDD9;BLOCK OCTANT-234678
    0b11101111, // 1CDDA;BLOCK OCTANT-1234678
    0b11110001, // 1CDDB;BLOCK OCTANT-15678
    0b11110010, // 1CDDC;BLOCK OCTANT-25678
    0b11110011, // 1CDDD;BLOCK OCTANT-125678
    0b11110100, // 1CDDE;BLOCK OCTANT-35678
    0b11110110, // 1CDDF;BLOCK OCTANT-235678
    0b11110111, // 1CDE0;BLOCK OCTANT-1235678
    0b11111000, // 1CDE1;BLOCK OCTANT-45678
    0b11111001, // 1CDE2;BLOCK OCTANT-145678
    0b11111011, // 1CDE3;BLOCK OCTANT-1245678
    0b11111101, // 1CDE4;BLOCK OCTANT-1345678
    0b11111110, // 1CDE5;BLOCK OCTANT-2345678
];

impl BlockKey {
    pub fn filter_out_synthetic(glyphs: &mut Vec<char>) {
        let config = config::configuration();
        if config.custom_block_glyphs {
            glyphs.retain(|&c| Self::from_char(c).is_none());
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        let mut chars = s.chars();
        let first_char = chars.next()?;
        if chars.next().is_some() {
            None
        } else {
            Self::from_char(first_char)
        }
    }

    pub fn from_char(c: char) -> Option<Self> {
        let c = c as u32;
        Some(match c {
            // [─] BOX DRAWINGS LIGHT HORIZONTAL
            0x2500 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            }]),
            // [━] BOX DRAWINGS HEAVY HORIZONTAL
            0x2501 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::OutlineHeavy,
            }]),
            // [│] BOX DRAWINGS LIGHT VERTICAL
            0x2502 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            }]),
            // [┃] BOX DRAWINGS HEAVY VERTICAL
            0x2503 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::OutlineHeavy,
            }]),
            // [┄] BOX DRAWINGS LIGHT TRIPLE DASH HORIZONTAL
            // A dash segment is wider than the gap segment.
            // We use a 2:1 ratio, which gives 9 total segments
            // with a pattern of `-- -- -- `
            0x2504 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(2, 9), BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(3, 9), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(5, 9), BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(6, 9), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(8, 9), BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [┅] BOX DRAWINGS HEAVY TRIPLE DASH HORIZONTAL
            0x2505 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(2, 9), BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(3, 9), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(5, 9), BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(6, 9), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(8, 9), BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
            ]),
            // [┆] BOX DRAWINGS LIGHT TRIPLE DASH VERTICAL
            0x2506 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(2, 9)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(3, 9)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(5, 9)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(6, 9)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(8, 9)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [┇] BOX DRAWINGS HEAVY TRIPLE DASH VERTICAL
            0x2507 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(2, 9)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(3, 9)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(5, 9)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(6, 9)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(8, 9)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
            ]),
            // [┈] BOX DRAWINGS LIGHT QUADRUPLE DASH HORIZONTAL
            // A dash segment is wider than the gap segment.
            // We use a 2:1 ratio, which gives 12 total segments
            // with a pattern of `-- -- -- -- `
            0x2508 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(2, 12), BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(3, 12), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(5, 12), BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(6, 12), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(8, 12), BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(9, 12), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(11, 12), BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [┉] BOX DRAWINGS HEAVY QUADRUPLE DASH HORIZONTAL
            0x2509 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(2, 12), BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(3, 12), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(5, 12), BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(6, 12), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(8, 12), BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(9, 12), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(11, 12), BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
            ]),
            // [┊] BOX DRAWINGS LIGHT QUADRUPLE DASH VERTICAL
            0x250a => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(2, 12)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(3, 12)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(5, 12)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(6, 12)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(8, 12)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(9, 12)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(11, 12)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [┋] BOX DRAWINGS HEAVY QUADRUPLE DASH VERTICAL
            0x250b => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(2, 12)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(3, 12)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(5, 12)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(6, 12)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(8, 12)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(9, 12)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(11, 12)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
            ]),
            // [┌] BOX DRAWINGS LIGHT DOWN AND RIGHT
            0x250c => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            }]),
            // [┍] BOX DRAWINGS DOWN LIGHT AND RIGHT HEAVY
            0x250d => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Div(-2)),
                            BlockCoord::Frac(1, 2),
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
            ]),
            // [┎] BOX DRAWINGS DOWN HEAVY AND RIGHT LIGHT
            0x250e => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Div(-1)),
                            BlockCoord::Frac(1, 2),
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [┏] BOX DRAWINGS HEAVY DOWN AND RIGHT
            0x250f => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::OutlineHeavy,
            }]),

            // [┐] BOX DRAWINGS LIGHT DOWN AND LEFT
            0x2510 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            }]),
            // [┑] BOX DRAWINGS DOWN LIGHT AND LEFT HEAVY
            0x2511 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Div(2)),
                            BlockCoord::Frac(1, 2),
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
            ]),
            // [┒] BOX DRAWINGS DOWN HEAVY AND LEFT LIGHT
            0x2512 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Div(1)),
                            BlockCoord::Frac(1, 2),
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [┓] BOX DRAWINGS HEAVY DOWN AND LEFT
            0x2513 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::OutlineHeavy,
            }]),

            // [└] BOX DRAWINGS LIGHT UP AND RIGHT
            0x2514 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            }]),
            // [┕] BOX DRAWINGS UP LIGHT AND RIGHT HEAVY
            0x2515 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Div(-2)),
                            BlockCoord::Frac(1, 2),
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
            ]),
            // [┖] BOX DRAWINGS UP HEAVY AND RIGHT LIGHT
            0x2516 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Div(-1)),
                            BlockCoord::Frac(1, 2),
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [┗] BOX DRAWINGS HEAVY UP AND RIGHT
            0x2517 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::OutlineHeavy,
            }]),

            // [┘] BOX DRAWINGS LIGHT UP AND LEFT
            0x2518 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            }]),
            // [┙] BOX DRAWINGS UP LIGHT AND LEFT HEAVY
            0x2519 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Div(2)),
                            BlockCoord::Frac(1, 2),
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
            ]),
            // [┚] BOX DRAWINGS UP HEAVY AND LEFT LIGHT
            0x251a => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Div(1)),
                            BlockCoord::Frac(1, 2),
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [┛] BOX DRAWINGS HEAVY UP AND LEFT
            0x251b => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::OutlineHeavy,
            }]),

            // [├] BOX DRAWINGS LIGHT VERTICAL AND RIGHT
            0x251c => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                    PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            }]),
            // [┝] BOX DRAWINGS LIGHT VERTICAL LIGHT AND RIGHT HEAVY
            0x251d => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
            ]),
            // [┞] BOX DRAWINGS UP HEAVY and RIGHT DOWN LIGHT
            0x251e => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [┟] BOX DRAWINGS DOWN HEAVY and RIGHT UP LIGHT
            0x251f => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),

            // [┠] BOX DRAWINGS HEAVY VERTICAL and RIGHT LIGHT
            0x2520 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [┡] BOX DRAWINGS DOWN LIGHT AND RIGHT UP HEAVY
            0x2521 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
            ]),
            // [┢] BOX DRAWINGS UP LIGHT AND RIGHT DOWN HEAVY
            0x2522 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
            ]),
            // [┣] BOX DRAWINGS HEAVY VERTICAL and RIGHT
            0x2523 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::OutlineHeavy,
            }]),
            // [┤] BOX DRAWINGS LIGHT VERTICAL and LEFT
            0x2524 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            }]),
            // [┥] BOX DRAWINGS VERTICAL LIGHT and LEFT HEAVY
            0x2525 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
            ]),
            // [┦] BOX DRAWINGS UP HEAVY and LEFT DOWN LIGHT
            0x2526 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [┧] BOX DRAWINGS DOWN HEAVY and LEFT UP LIGHT
            0x2527 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [┨] BOX DRAWINGS VERTICAL HEAVY and LEFT LIGHT
            0x2528 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [┩] BOX DRAWINGS DOWN LIGHT and LEFT UP HEAVY
            0x2529 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
            ]),
            // [┪] BOX DRAWINGS UP LIGHT and LEFT DOWN HEAVY
            0x252a => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
            ]),
            // [┫] BOX DRAWINGS HEAVY VERTICAL and LEFT
            0x252b => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::OutlineHeavy,
            }]),
            // [┬] BOX DRAWINGS LIGHT DOWN AND HORIZONTAL
            0x252c => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            }]),
            // [┭] BOX DRAWINGS LEFT HEAVY AND RIGHT DOWN LIGHT
            0x252d => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [┮] BOX DRAWINGS RIGHT HEAVY AND LEFT DOWN LIGHT
            0x252e => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [┯] BOX DRAWINGS DOWN LIGHT AND HORIZONTAL HEAVY
            0x252f => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
            ]),

            // [┰] BOX DRAWINGS DOWN HEAVY AND HORIZONTAL LIGHT
            0x2530 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),

            // [┱] BOX DRAWINGS RIGHT LIGHT AND LEFT DOWN HEAVY
            0x2531 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
            ]),
            // [┲] BOX DRAWINGS LEFT LIGHT AND RIGHT DOWN HEAVY
            0x2532 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
            ]),
            // [┳] BOX DRAWINGS HEAVY DOWN AND HORIZONTAL
            0x2533 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::OutlineHeavy,
            }]),
            // [┴] BOX DRAWINGS LIGHT UP AND HORIZONTAL
            0x2534 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            }]),
            // [┵] BOX DRAWINGS LEFT HEAVY AND RIGHT UP LIGHT
            0x2535 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [┶] BOX DRAWINGS RIGHT HEAVY AND LEFT UP LIGHT
            0x2536 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [┷] BOX DRAWINGS UP LIGHT AND HORIZONTAL HEAVY
            0x2537 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
            ]),

            // [┸] BOX DRAWINGS UP HEAVY AND HORIZONTAL LIGHT
            0x2538 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),

            // [┹] BOX DRAWINGS RIGHT LIGHT AND LEFT UP HEAVY
            0x2539 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
            ]),
            // [┺] BOX DRAWINGS LEFT LIGHT AND RIGHT UP HEAVY
            0x253a => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
            ]),
            // [┻] BOX DRAWINGS HEAVY UP AND HORIZONTAL
            0x253b => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::OutlineHeavy,
            }]),
            // [┼] BOX DRAWINGS LIGHT VERTICAL AND HORIZONTAL
            0x253c => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                    PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            }]),
            // [┽] BOX DRAWINGS LEFT HEAVY AND RIGHT VERTICAL LIGHT
            0x253d => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [┾] BOX DRAWINGS RIGHT HEAVY AND LEFT VERTICAL LIGHT
            0x253e => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [┿] BOX DRAWINGS VERTICAL LIGHT AND HORIZONTAL HEAVY
            0x253f => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
            ]),
            // [╀] BOX DRAWINGS UP HEAVY AND DOWN HORIZONTAL LIGHT
            0x2540 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [╁] BOX DRAWINGS DOWN HEAVY AND UP HORIZONTAL LIGHT
            0x2541 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [╂] BOX DRAWINGS VERTICAL HEAVY AND HORIZONTAL LIGHT
            0x2542 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [╃] BOX DRAWINGS LEFT UP HEAVY and RIGHT DOWN LIGHT
            0x2543 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [╄] BOX DRAWINGS RIGHT UP HEAVY and LEFT DOWN LIGHT
            0x2544 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [╅] BOX DRAWINGS LEFT DOWN HEAVY and RIGHT UP LIGHT
            0x2545 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [╆] BOX DRAWINGS RIGHT DOWN HEAVY and LEFT UP LIGHT
            0x2546 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [╇] BOX DRAWINGS DOWN LIGHT AND UP HORIZONTAL HEAVY
            0x2547 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
            ]),
            // [╈] BOX DRAWINGS UP LIGHT AND DOWN HORIZONTAL HEAVY
            0x2548 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
            ]),
            // [╉] BOX DRAWINGS RIGHT LIGHT AND LEFT VERTICAL HEAVY
            0x2549 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
            ]),
            // [╊] BOX DRAWINGS LEFT LIGHT AND RIGHT VERTICAL HEAVY
            0x254a => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
            ]),
            // [╋] BOX DRAWINGS HEAVY VERTICAL AND HORIZONTAL
            0x254b => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                    PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::OutlineHeavy,
            }]),

            // [╌] BOX DRAWINGS LIGHT DOUBLE DASH HORIZONTAL
            // A dash segment is wider than the gap segment.
            // We use a 2:1 ratio, which gives 6 total segments
            // with a pattern of `-- -- `
            0x254c => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(2, 6), BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(3, 6), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(5, 6), BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [╍] BOX DRAWINGS HEAVY DOUBLE DASH HORIZONTAL
            0x254d => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(2, 6), BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(3, 6), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(5, 6), BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
            ]),
            // [╎] BOX DRAWINGS LIGHT DOUBLE DASH VERTICAL
            0x254e => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(2, 6)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(3, 6)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(5, 6)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [╏] BOX DRAWINGS HEAVY DOUBLE DASH VERTICAL
            0x254f => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(2, 6)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(3, 6)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(5, 6)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
            ]),

            // [═] BOX DRAWINGS DOUBLE HORIZONTAL
            0x2550 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::Zero,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::One,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::Zero,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::One,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [║] BOX DRAWINGS DOUBLE VERTICAL
            0x2551 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                            BlockCoord::Zero,
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                            BlockCoord::One,
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                            BlockCoord::Zero,
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                            BlockCoord::One,
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [╒] BOX DRAWINGS DOWN SINGLE AND RIGHT DOUBLE
            0x2552 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                        PolyCommand::LineTo(
                            BlockCoord::Frac(1, 2),
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::One,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::Frac(1, 2),
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::One,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [╓] BOX DRAWINGS DOWN DOUBLE AND RIGHT SINGLE
            0x2553 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                            BlockCoord::One,
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                            BlockCoord::Frac(1, 2),
                        ),
                        PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                            BlockCoord::Frac(1, 2),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                            BlockCoord::One,
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),

            // [╔] BOX DRAWINGS DOUBLE DOWN AND RIGHT
            0x2554 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                            BlockCoord::One,
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::One,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                            BlockCoord::One,
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::One,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [╕] BOX DRAWINGS DOWN SINGLE AND LEFT DOUBLE
            0x2555 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                        PolyCommand::LineTo(
                            BlockCoord::Frac(1, 2),
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::Zero,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::Frac(1, 2),
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::Zero,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [╖] BOX DRAWINGS DOWN DOUBLE AND LEFT SINGLE
            0x2556 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                            BlockCoord::One,
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                            BlockCoord::Frac(1, 2),
                        ),
                        PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                            BlockCoord::Frac(1, 2),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                            BlockCoord::One,
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [╗] BOX DRAWINGS DOUBLE DOWN AND LEFT
            0x2557 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                            BlockCoord::One,
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::Zero,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                            BlockCoord::One,
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::Zero,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [╘] BOX DRAWINGS UP SINGLE AND RIGHT DOUBLE
            0x2558 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                        PolyCommand::LineTo(
                            BlockCoord::Frac(1, 2),
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::One,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::Frac(1, 2),
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::One,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [╙] BOX DRAWINGS UP DOUBLE AND RIGHT SINGLE
            0x2559 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                            BlockCoord::Zero,
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                            BlockCoord::Frac(1, 2),
                        ),
                        PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                            BlockCoord::Frac(1, 2),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                            BlockCoord::Zero,
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [╚] BOX DRAWINGS DOUBLE UP AND RIGHT
            0x255a => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                            BlockCoord::Zero,
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::One,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                            BlockCoord::Zero,
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::One,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [╛] BOX DRAWINGS UP SINGLE AND LEFT DOUBLE
            0x255b => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                        PolyCommand::LineTo(
                            BlockCoord::Frac(1, 2),
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::Zero,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::Frac(1, 2),
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::Zero,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [╜] BOX DRAWINGS UP DOUBLE AND LEFT SINGLE
            0x255c => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                            BlockCoord::Zero,
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                            BlockCoord::Frac(1, 2),
                        ),
                        PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                            BlockCoord::Frac(1, 2),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                            BlockCoord::Zero,
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [╝] BOX DRAWINGS DOUBLE UP AND LEFT
            0x255d => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                            BlockCoord::Zero,
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::Zero,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                            BlockCoord::Zero,
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::Zero,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),

            // [╞] BOX DRAWINGS VERTICAL SINGLE AND RIGHT DOUBLE
            0x255e => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                        PolyCommand::LineTo(
                            BlockCoord::Frac(1, 2),
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::One,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::Frac(1, 2),
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        ),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::Frac(1, 2),
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::One,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [╟] BOX DRAWINGS VERTICAL DOUBLE AND RIGHT SINGLE
            0x255f => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                            BlockCoord::Zero,
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                            BlockCoord::One,
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                            BlockCoord::Zero,
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                            BlockCoord::Frac(1, 2),
                        ),
                        PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                            BlockCoord::Frac(1, 2),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                            BlockCoord::One,
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),

            // [╠] BOX DRAWINGS DOUBLE VERTICAL AND RIGHT
            0x2560 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                            BlockCoord::Zero,
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                            BlockCoord::One,
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                            BlockCoord::Zero,
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::One,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                            BlockCoord::One,
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::One,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [╡] BOX DRAWINGS VERTICAL SINGLE AND LEFT DOUBLE
            0x2561 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                        PolyCommand::LineTo(
                            BlockCoord::Frac(1, 2),
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::Zero,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::Frac(1, 2),
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        ),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::Frac(1, 2),
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::Zero,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [╢] BOX DRAWINGS VERTICAL DOUBLE AND LEFT SINGLE
            0x2562 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                            BlockCoord::Zero,
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                            BlockCoord::One,
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                            BlockCoord::Zero,
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                            BlockCoord::Frac(1, 2),
                        ),
                        PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                            BlockCoord::Frac(1, 2),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                            BlockCoord::One,
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [╣] BOX DRAWINGS DOUBLE VERTICAL AND LEFT
            0x2563 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                            BlockCoord::Zero,
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                            BlockCoord::One,
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                            BlockCoord::Zero,
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::Zero,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                            BlockCoord::One,
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::Zero,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [╤] BOX DRAWINGS DOWN SINGLE AND HORIZONTAL DOUBLE
            0x2564 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::Zero,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::One,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::Zero,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::Frac(1, 2),
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        ),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                        PolyCommand::LineTo(
                            BlockCoord::Frac(1, 2),
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::One,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [╥] BOX DRAWINGS DOWN DOUBLE AND HORIZONTAL SINGLE
            0x2565 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                            BlockCoord::Frac(1, 2),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                            BlockCoord::One,
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                            BlockCoord::Frac(1, 2),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                            BlockCoord::One,
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [╦] BOX DRAWINGS DOUBLE DOWN AND HORIZONTAL
            0x2566 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::Zero,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::One,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::Zero,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                            BlockCoord::One,
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::One,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                            BlockCoord::One,
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [╧] BOX DRAWINGS UP SINGLE AND HORIZONTAL DOUBLE
            0x2567 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::Zero,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::One,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::Zero,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::Frac(1, 2),
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        ),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                        PolyCommand::LineTo(
                            BlockCoord::Frac(1, 2),
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::One,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [╨] BOX DRAWINGS UP DOUBLE AND HORIZONTAL SINGLE
            0x2568 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                            BlockCoord::Frac(1, 2),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                            BlockCoord::Zero,
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                            BlockCoord::Frac(1, 2),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                            BlockCoord::Zero,
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [╩] BOX DRAWINGS DOUBLE UP AND HORIZONTAL
            0x2569 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::Zero,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::One,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::Zero,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                            BlockCoord::Zero,
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::One,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                            BlockCoord::Zero,
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [╪] BOX DRAWINGS VERTICAL SINGLE AND HORIZONTAL DOUBLE
            0x256a => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::Zero,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::One,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::Zero,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::One,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [╫] BOX DRAWINGS VERTICAL DOUBLE AND HORIZONTAL SINGLE
            0x256b => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                            BlockCoord::Zero,
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                            BlockCoord::One,
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                            BlockCoord::Zero,
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                            BlockCoord::One,
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),

            // [╬] BOX DRAWINGS DOUBLE VERTICAL AND HORIZONTAL
            0x256c => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::Zero,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                            BlockCoord::Zero,
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::One,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                            BlockCoord::Zero,
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::Zero,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(-1)),
                            BlockCoord::One,
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::One,
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                        ),
                        PolyCommand::LineTo(
                            BlockCoord::FracWithOffset(1, 2, LineScale::Mul(1)),
                            BlockCoord::One,
                        ),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),

            // [╭] BOX DRAWINGS LIGHT ARC DOWN AND RIGHT
            0x256d => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(3, 4)),
                    PolyCommand::QuadTo {
                        control: (BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        to: (BlockCoord::Frac(3, 4), BlockCoord::Frac(1, 2)),
                    },
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            }]),
            // [╮] BOX DRAWINGS LIGHT ARC DOWN AND LEFT
            0x256e => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(3, 4)),
                    PolyCommand::QuadTo {
                        control: (BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        to: (BlockCoord::Frac(1, 4), BlockCoord::Frac(1, 2)),
                    },
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            }]),
            // [╯] BOX DRAWINGS LIGHT ARC UP AND LEFT
            0x256f => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 4)),
                    PolyCommand::QuadTo {
                        control: (BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        to: (BlockCoord::Frac(1, 4), BlockCoord::Frac(1, 2)),
                    },
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            }]),
            // [╰] BOX DRAWINGS LIGHT ARC UP AND RIGHT
            0x2570 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 4)),
                    PolyCommand::QuadTo {
                        control: (BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        to: (BlockCoord::Frac(3, 4), BlockCoord::Frac(1, 2)),
                    },
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            }]),

            // [╱] BOX DRAWINGS LIGHT DIAGONAL UPPER RIGHT TO LOWER LEFT
            0x2571 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::One, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::One),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            }]),
            // [╲] BOX DRAWINGS LIGHT DIAGONAL UPPER LEFT TO LOWER RIGHT
            0x2572 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::One),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            }]),
            // [╳] BOX DRAWINGS LIGHT DIAGONAL CROSS
            0x2573 => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::One, BlockCoord::Zero),
                        PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::One),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Zero),
                        PolyCommand::LineTo(BlockCoord::One, BlockCoord::One),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [╴] BOX DRAWINGS LIGHT LEFT
            0x2574 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            }]),
            // [╵] BOX DRAWINGS LIGHT UP
            0x2575 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            }]),
            // [╶] BOX DRAWINGS LIGHT RIGHT
            0x2576 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            }]),
            // [╷] BOX DRAWINGS LIGHT DOWN
            0x2577 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            }]),
            // [╸] BOX DRAWINGS HEAVY LEFT
            0x2578 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::OutlineHeavy,
            }]),
            // [╹] BOX DRAWINGS HEAVY UP
            0x2579 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::OutlineHeavy,
            }]),
            // [╺] BOX DRAWINGS HEAVY RIGHT
            0x257a => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::OutlineHeavy,
            }]),
            // [╻] BOX DRAWINGS HEAVY DOWN
            0x257b => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::OutlineHeavy,
            }]),
            // [╼] BOX DRAWINGS LIGHT LEFT AND HEAVY RIGHT
            0x257c => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
            ]),
            // [╽] BOX DRAWINGS LIGHT UP AND HEAVY DOWN
            0x257d => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::Frac(1, 2),
                            BlockCoord::FracWithOffset(1, 2, LineScale::Div(-1)),
                        ),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
            ]),
            // [╾] BOX DRAWINGS HEAVY LEFT AND LIGHT RIGHT
            0x257e => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                        PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),
            // [╿] BOX DRAWINGS HEAVY UP AND LIGHT DOWN
            0x257f => Self::Poly(&[
                Poly {
                    path: &[
                        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(1, 2)),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::OutlineHeavy,
                },
                Poly {
                    path: &[
                        PolyCommand::MoveTo(
                            BlockCoord::Frac(1, 2),
                            BlockCoord::FracWithOffset(1, 2, LineScale::Div(-1)),
                        ),
                        PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                    ],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                },
            ]),

            // [▀] UPPER HALF BLOCK
            0x2580 => Self::Blocks(&[Block::UpperBlock(4)]),
            // [▁] LOWER 1 EIGHTH BLOCK
            0x2581 => Self::Blocks(&[Block::LowerBlock(1)]),
            // [▂] LOWER 2 EIGHTHS BLOCK
            0x2582 => Self::Blocks(&[Block::LowerBlock(2)]),
            // [▃] LOWER 3 EIGHTHS BLOCK
            0x2583 => Self::Blocks(&[Block::LowerBlock(3)]),
            // [▄] LOWER 4 EIGHTHS BLOCK
            0x2584 => Self::Blocks(&[Block::LowerBlock(4)]),
            // [▅] LOWER 5 EIGHTHS BLOCK
            0x2585 => Self::Blocks(&[Block::LowerBlock(5)]),
            // [▆] LOWER 6 EIGHTHS BLOCK
            0x2586 => Self::Blocks(&[Block::LowerBlock(6)]),
            // [▇] LOWER 7 EIGHTHS BLOCK
            0x2587 => Self::Blocks(&[Block::LowerBlock(7)]),
            // [█] FULL BLOCK
            0x2588 => Self::Blocks(&[Block::Custom(0, 8, 0, 8, BlockAlpha::Full)]),
            // [▉] LEFT 7 EIGHTHS BLOCK
            0x2589 => Self::Blocks(&[Block::LeftBlock(7)]),
            // [▊] LEFT 6 EIGHTHS BLOCK
            0x258a => Self::Blocks(&[Block::LeftBlock(6)]),
            // [▋] LEFT 5 EIGHTHS BLOCK
            0x258b => Self::Blocks(&[Block::LeftBlock(5)]),
            // [▌] LEFT 4 EIGHTHS BLOCK
            0x258c => Self::Blocks(&[Block::LeftBlock(4)]),
            // [▍] LEFT 3 EIGHTHS BLOCK
            0x258d => Self::Blocks(&[Block::LeftBlock(3)]),
            // [▎] LEFT 2 EIGHTHS BLOCK
            0x258e => Self::Blocks(&[Block::LeftBlock(2)]),
            // [▏] LEFT 1 EIGHTHS BLOCK
            0x258f => Self::Blocks(&[Block::LeftBlock(1)]),
            // [▐] RIGHT HALF BLOCK
            0x2590 => Self::Blocks(&[Block::RightBlock(4)]),
            // [░] LIGHT SHADE
            0x2591 => Self::Blocks(&[Block::Custom(0, 8, 0, 8, BlockAlpha::Light)]),
            // [▒] MEDIUM SHADE
            0x2592 => Self::Blocks(&[Block::Custom(0, 8, 0, 8, BlockAlpha::Medium)]),
            // [▓] DARK SHADE
            0x2593 => Self::Blocks(&[Block::Custom(0, 8, 0, 8, BlockAlpha::Dark)]),
            // [▔] UPPER ONE EIGHTH BLOCK
            0x2594 => Self::Blocks(&[Block::UpperBlock(1)]),
            // [▕] RIGHT ONE EIGHTH BLOCK
            0x2595 => Self::Blocks(&[Block::RightBlock(1)]),
            // [▖] QUADRANT LOWER LEFT
            0x2596 => Self::Blocks(&[Block::QuadrantLL]),
            // [▗] QUADRANT LOWER RIGHT
            0x2597 => Self::Blocks(&[Block::QuadrantLR]),
            // [▘] QUADRANT UPPER LEFT
            0x2598 => Self::Blocks(&[Block::QuadrantUL]),
            // [▙] QUADRANT UPPER LEFT AND LOWER LEFT AND LOWER RIGHT
            0x2599 => Self::Blocks(&[Block::QuadrantUL, Block::QuadrantLL, Block::QuadrantLR]),
            // [▚] QUADRANT UPPER LEFT AND LOWER RIGHT
            0x259a => Self::Blocks(&[Block::QuadrantUL, Block::QuadrantLR]),
            // [▛] QUADRANT UPPER LEFT AND UPPER RIGHT AND LOWER LEFT
            0x259b => Self::Blocks(&[Block::QuadrantUL, Block::QuadrantUR, Block::QuadrantLL]),
            // [▜] QUADRANT UPPER LEFT AND UPPER RIGHT AND LOWER RIGHT
            0x259c => Self::Blocks(&[Block::QuadrantUL, Block::QuadrantUR, Block::QuadrantLR]),
            // [▝] QUADRANT UPPER RIGHT
            0x259d => Self::Blocks(&[Block::QuadrantUR]),
            // [▞] QUADRANT UPPER RIGHT AND LOWER LEFT
            0x259e => Self::Blocks(&[Block::QuadrantUR, Block::QuadrantLL]),
            // [▟] QUADRANT UPPER RIGHT AND LOWER LEFT AND LOWER RIGHT
            0x259f => Self::Blocks(&[Block::QuadrantUR, Block::QuadrantLL, Block::QuadrantLR]),
            // Sextant blocks
            n @ 0x1fb00..=0x1fb3b => Self::Sextant(SEXTANT_PATTERNS[(n & 0x3f) as usize]),
            // Octant blocks
            n @ 0x1cd00..=0x1cde5 => Self::Octant(OCTANT_PATTERNS[(n & 0xff) as usize]),
            // [𜺠] RIGHT HALF LOWER ONE QUARTER BLOCK (corresponds to OCTANT-8)
            0x1cea0 => Self::Octant(0b10000000),
            // [𜺣; EFT HALF LOWER ONE QUARTER BLOCK (corresponds to OCTANT-7)
            0x1cea3 => Self::Octant(0b01000000),
            // [𜺨] LEFT HALF UPPER ONE QUARTER BLOCK (corresponds to OCTANT-1)
            0x1cea8 => Self::Octant(0b00000001),
            // [𜺫] RIGHT HALF UPPER ONE QUARTER BLOCK (corresponds to OCTANT-2)
            0x1ceab => Self::Octant(0b00000010),
            // [🯦] MIDDLE LEFT ONE QUARTER BLOCK (corresponds to OCTANT-35)
            0x1fbe6 => Self::Octant(0b00010100),
            // [🯧] MIDDLE RIGHT ONE QUARTER BLOCK (corresponds to OCTANT-46)
            0x1fbe7 => Self::Octant(0b00101000),
            // [🬼] LOWER LEFT BLOCK DIAGONAL LOWER MIDDLE LEFT TO LOWER CENTRE
            0x1fb3c => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(2, 3)),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Fill,
            }]),
            // [🬽] LOWER LEFT BLOCK DIAGONAL LOWER MIDDLE LEFT TO LOWER RIGHT
            0x1fb3d => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(2, 3)),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::One),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Fill,
            }]),
            // [🬾] LOWER LEFT BLOCK DIAGONAL UPPER MIDDLE LEFT TO LOWER CENTRE
            0x1fb3e => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 3)),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Fill,
            }]),
            // [🬿] LOWER LEFT BLOCK DIAGONAL UPPER MIDDLE LEFT TO LOWER RIGHT
            0x1fb3f => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 3)),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::One),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Fill,
            }]),
            // [🭀] LOWER LEFT BLOCK DIAGONAL UPPER LEFT TO LOWER CENTRE
            0x1fb40 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Fill,
            }]),
            // [🭁] LOWER RIGHT BLOCK DIAGONAL UPPER MIDDLE LEFT TO UPPER CENTRE
            0x1fb41 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Frac(1, 3)),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Fill,
            }]),
            // [🭂] LOWER RIGHT BLOCK DIAGONAL UPPER MIDDLE LEFT TO UPPER RIGHT
            0x1fb42 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::One, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Frac(1, 3)),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Fill,
            }]),
            // [🭃] LOWER RIGHT BLOCK DIAGONAL LOWER MIDDLE LEFT TO UPPER CENTRE
            0x1fb43 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Frac(2, 3)),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Fill,
            }]),
            // [🭄] LOWER RIGHT BLOCK DIAGONAL LOWER MIDDLE LEFT TO UPPER RIGHT
            0x1fb44 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::One, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Frac(2, 3)),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Fill,
            }]),
            // [🭅] LOWER RIGHT BLOCK DIAGONAL UPPER LEFT TO UPPER CENTRE
            0x1fb45 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::One),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Fill,
            }]),
            // [🭆] LOWER RIGHT BLOCK DIAGONAL LOWER MIDDLE LEFT TO UPPER MIDDLE RIGHT
            0x1fb46 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(2, 3)),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 3)),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::One),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Fill,
            }]),
            // [🭇] LOWER RIGHT BLOCK DIAGONAL LOWER CENTRE TO LOWER MIDDLE RIGHT
            0x1fb47 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(2, 3)),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::One),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Fill,
            }]),
            // [🭈] LOWER RIGHT BLOCK DIAGONAL LOWER LEFT TO LOWER MIDDLE RIGHT
            0x1fb48 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(2, 3)),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::One),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Fill,
            }]),
            // [🭉] LOWER RIGHT BLOCK DIAGONAL LOWER CENTRE TO UPPER MIDDLE RIGHT
            0x1fb49 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 3)),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::One),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Fill,
            }]),
            // [🭊] LOWER RIGHT BLOCK DIAGONAL LOWER LEFT TO UPPER MIDDLE RIGHT
            0x1fb4a => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 3)),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::One),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Fill,
            }]),
            // [🭋] LOWER RIGHT BLOCK DIAGONAL LOWER CENTRE TO UPPER RIGHT
            0x1fb4b => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::One),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Fill,
            }]),
            // [🭌] LOWER LEFT BLOCK DIAGONAL UPPER CENTRE TO UPPER MIDDLE RIGHT
            0x1fb4c => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 3)),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::One),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Fill,
            }]),
            // [🭍] LOWER LEFT BLOCK DIAGONAL UPPER LEFT TO UPPER MIDDLE RIGHT
            0x1fb4d => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 3)),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::One),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Fill,
            }]),
            // [🭎] LOWER LEFT BLOCK DIAGONAL UPPER CENTRE TO LOWER MIDDLE RIGHT
            0x1fb4e => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(2, 3)),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::One),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Fill,
            }]),
            // [🭏] LOWER LEFT BLOCK DIAGONAL UPPER LEFT TO LOWER MIDDLE RIGHT
            0x1fb4f => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(2, 3)),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::One),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Fill,
            }]),
            // [🭐] LOWER LEFT BLOCK DIAGONAL UPPER CENTRE TO LOWER RIGHT
            0x1fb50 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::One),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Fill,
            }]),
            // [🭑] LOWER LEFT BLOCK DIAGONAL UPPER MIDDLE LEFT TO LOWER MIDDLE RIGHT
            0x1fb51 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Frac(1, 3)),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(2, 3)),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::One),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Fill,
            }]),
            // [🭒] UPPER RIGHT BLOCK DIAGONAL LOWER MIDDLE LEFT TO LOWER CENTRE
            0x1fb52 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Frac(2, 3)),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Fill,
            }]),
            // [🭓] UPPER RIGHT BLOCK DIAGONAL LOWER MIDDLE LEFT TO LOWER RIGHT
            0x1fb53 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Frac(2, 3)),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Fill,
            }]),
            // [🭔] UPPER RIGHT BLOCK DIAGONAL UPPER MIDDLE LEFT TO LOWER CENTRE
            0x1fb54 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Frac(1, 3)),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Fill,
            }]),
            // [🭕] UPPER RIGHT BLOCK DIAGONAL UPPER MIDDLE LEFT TO LOWER RIGHT
            0x1fb55 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Frac(1, 3)),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Fill,
            }]),
            // [🭖] UPPER RIGHT BLOCK DIAGONAL UPPER LEFT TO LOWER CENTRE
            0x1fb56 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Fill,
            }]),
            // [🭗] UPPER LEFT BLOCK DIAGONAL UPPER MIDDLE LEFT TO UPPER CENTRE
            0x1fb57 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Frac(1, 3)),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Fill,
            }]),
            // [🭘] UPPER LEFT BLOCK DIAGONAL UPPER MIDDLE LEFT TO UPPER RIGHT
            0x1fb58 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Frac(1, 3)),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Fill,
            }]),
            // [🭙] UPPER LEFT BLOCK DIAGONAL LOWER MIDDLE LEFT TO UPPER CENTRE
            0x1fb59 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Frac(2, 3)),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Fill,
            }]),
            // [🭚] UPPER LEFT BLOCK DIAGONAL LOWER MIDDLE LEFT TO UPPER RIGHT
            0x1fb5a => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Frac(2, 3)),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Fill,
            }]),
            // [🭛] UPPER LEFT BLOCK DIAGONAL LOWER LEFT TO UPPER CENTRE
            0x1fb5b => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::One),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Fill,
            }]),
            // [🭜] UPPER LEFT BLOCK DIAGONAL LOWER MIDDLE LEFT TO UPPER MIDDLE RIGHT
            0x1fb5c => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 3)),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Frac(2, 3)),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Fill,
            }]),
            // [🭝] UPPER LEFT BLOCK DIAGONAL LOWER CENTRE TO LOWER MIDDLE RIGHT
            0x1fb5d => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(2, 3)),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::One),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Fill,
            }]),
            // [🭞] UPPER LEFT BLOCK DIAGONAL LOWER LEFT TO LOWER MIDDLE RIGHT
            0x1fb5e => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(2, 3)),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::One),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Fill,
            }]),
            // [🭟] UPPER LEFT BLOCK DIAGONAL LOWER CENTRE TO UPPER MIDDLE RIGHT
            0x1fb5f => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 3)),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::One),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Fill,
            }]),
            // [🭠] UPPER LEFT BLOCK DIAGONAL LOWER LEFT TO UPPER MIDDLE RIGHT
            0x1fb60 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 3)),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::One),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Fill,
            }]),
            // [🭡] UPPER LEFT BLOCK DIAGONAL LOWER CENTRE TO UPPER RIGHT
            0x1fb61 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::Frac(1, 2), BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::One),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Fill,
            }]),
            // [🭢] UPPER RIGHT BLOCK DIAGONAL UPPER CENTRE TO UPPER MIDDLE RIGHT
            0x1fb62 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 3)),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Fill,
            }]),
            // [🭣] UPPER RIGHT BLOCK DIAGONAL UPPER LEFT TO UPPER MIDDLE RIGHT
            0x1fb63 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 3)),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Fill,
            }]),
            // [🭤] UPPER RIGHT BLOCK DIAGONAL UPPER CENTRE TO LOWER MIDDLE RIGHT
            0x1fb64 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(2, 3)),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Fill,
            }]),
            // [🭥] UPPER RIGHT BLOCK DIAGONAL UPPER LEFT TO LOWER MIDDLE RIGHT
            0x1fb65 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(2, 3)),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Fill,
            }]),
            // [🭦] UPPER RIGHT BLOCK DIAGONAL UPPER CENTRE TO LOWER RIGHT
            0x1fb66 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::One),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Fill,
            }]),
            // [🭧] UPPER RIGHT BLOCK DIAGONAL UPPER MIDDLE LEFT TO LOWER MIDDLE RIGHT
            0x1fb67 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(2, 3)),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Frac(1, 3)),
                    PolyCommand::Close,
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Fill,
            }]),
            // [🭨] UPPER AND RIGHT AND LOWER TRIANGULAR THREE QUARTERS BLOCK
            0x1fb68 => Self::Triangles(
                Triangle::UPPER | Triangle::RIGHT | Triangle::LOWER,
                BlockAlpha::Full,
            ),
            // [🭩] LEFT AND LOWER AND RIGHT TRIANGULAR THREE QUARTERS BLOCK
            0x1fb69 => Self::Triangles(
                Triangle::LEFT | Triangle::LOWER | Triangle::RIGHT,
                BlockAlpha::Full,
            ),
            // [🭪] UPPER AND LEFT AND LOWER TRIANGULAR THREE QUARTERS BLOCK
            0x1fb6a => Self::Triangles(
                Triangle::UPPER | Triangle::LEFT | Triangle::LOWER,
                BlockAlpha::Full,
            ),
            // [🭫] LEFT AND UPPER AND RIGHT TRIANGULAR THREE QUARTERS BLOCK
            0x1fb6b => Self::Triangles(
                Triangle::LEFT | Triangle::UPPER | Triangle::RIGHT,
                BlockAlpha::Full,
            ),
            // [🭬] LEFT TRIANGULAR ONE QUARTER BLOCK
            0x1fb6c => Self::Triangles(Triangle::LEFT, BlockAlpha::Full),
            // [🭭] UPPER TRIANGULAR ONE QUARTER BLOCK
            0x1fb6d => Self::Triangles(Triangle::UPPER, BlockAlpha::Full),
            // [🭮] RIGHT TRIANGULAR ONE QUARTER BLOCK
            0x1fb6e => Self::Triangles(Triangle::RIGHT, BlockAlpha::Full),
            // [🭯] LOWER TRIANGULAR ONE QUARTER BLOCK
            0x1fb6f => Self::Triangles(Triangle::LOWER, BlockAlpha::Full),
            // [🭰] VERTICAL ONE EIGHTH BLOCK-2
            0x1fb70 => Self::Blocks(&[Block::VerticalBlock(1, 2)]),
            // [🭱] VERTICAL ONE EIGHTH BLOCK-3
            0x1fb71 => Self::Blocks(&[Block::VerticalBlock(2, 3)]),
            // [🭲] VERTICAL ONE EIGHTH BLOCK-4
            0x1fb72 => Self::Blocks(&[Block::VerticalBlock(3, 4)]),
            // [🭳] VERTICAL ONE EIGHTH BLOCK-5
            0x1fb73 => Self::Blocks(&[Block::VerticalBlock(4, 5)]),
            // [🭴] VERTICAL ONE EIGHTH BLOCK-6
            0x1fb74 => Self::Blocks(&[Block::VerticalBlock(5, 6)]),
            // [🭵] VERTICAL ONE EIGHTH BLOCK-7
            0x1fb75 => Self::Blocks(&[Block::VerticalBlock(6, 7)]),
            // [🭶] HORIZONTAL ONE EIGHTH BLOCK-2
            0x1fb76 => Self::Blocks(&[Block::HorizontalBlock(1, 2)]),
            // [🭷] HORIZONTAL ONE EIGHTH BLOCK-3
            0x1fb77 => Self::Blocks(&[Block::HorizontalBlock(2, 3)]),
            // [🭸] HORIZONTAL ONE EIGHTH BLOCK-4
            0x1fb78 => Self::Blocks(&[Block::HorizontalBlock(3, 4)]),
            // [🭹] HORIZONTAL ONE EIGHTH BLOCK-5
            0x1fb79 => Self::Blocks(&[Block::HorizontalBlock(4, 5)]),
            // [🭺] HORIZONTAL ONE EIGHTH BLOCK-6
            0x1fb7a => Self::Blocks(&[Block::HorizontalBlock(5, 6)]),
            // [🭻] HORIZONTAL ONE EIGHTH BLOCK-7
            0x1fb7b => Self::Blocks(&[Block::HorizontalBlock(6, 7)]),
            // [🭼] Left and lower one eighth block
            0x1fb7c => Self::Blocks(&[Block::LeftBlock(1), Block::LowerBlock(1)]),
            // [🭽] Left and upper one eighth block
            0x1fb7d => Self::Blocks(&[Block::LeftBlock(1), Block::UpperBlock(1)]),
            // [🭾] Right and upper one eighth block
            0x1fb7e => Self::Blocks(&[Block::RightBlock(1), Block::UpperBlock(1)]),
            // [🭿] Right and lower one eighth block
            0x1fb7f => Self::Blocks(&[Block::RightBlock(1), Block::LowerBlock(1)]),
            // [🮀] UPPER AND LOWER ONE EIGHTH BLOCK
            0x1fb80 => Self::Blocks(&[Block::UpperBlock(1), Block::LowerBlock(1)]),
            // [🮁] HORIZONTAL ONE EIGHTH BLOCK-1358
            0x1fb81 => Self::Blocks(&[
                Block::UpperBlock(1),
                Block::HorizontalBlock(2, 3),
                Block::HorizontalBlock(4, 5),
                Block::LowerBlock(1),
            ]),
            // [🮂] Upper One Quarter Block
            0x1fb82 => Self::Blocks(&[Block::UpperBlock(2)]),
            // [🮃] Upper three eighths block
            0x1fb83 => Self::Blocks(&[Block::UpperBlock(3)]),
            // [🮄] Upper five eighths block
            0x1fb84 => Self::Blocks(&[Block::UpperBlock(5)]),
            // [🮅] Upper three quarters block
            0x1fb85 => Self::Blocks(&[Block::UpperBlock(6)]),
            // [🮆] Upper seven eighths block
            0x1fb86 => Self::Blocks(&[Block::UpperBlock(7)]),
            // [🮇] Right One Quarter Block
            0x1fb87 => Self::Blocks(&[Block::RightBlock(2)]),
            // [🮈] Right three eighths block
            0x1fb88 => Self::Blocks(&[Block::RightBlock(3)]),
            // [🮉] Right five eighths block
            0x1fb89 => Self::Blocks(&[Block::RightBlock(5)]),
            // [🮊] Right three quarters block
            0x1fb8a => Self::Blocks(&[Block::RightBlock(6)]),
            // [🮋] Right seven eighths block
            0x1fb8b => Self::Blocks(&[Block::RightBlock(7)]),
            // [🮌] LEFT HALF MEDIUM SHADE
            0x1fb8c => Self::Blocks(&[Block::Custom(0, 4, 0, 8, BlockAlpha::Medium)]),
            // [🮍] RIGHT HALF MEDIUM SHADE
            0x1fb8d => Self::Blocks(&[Block::Custom(4, 8, 0, 8, BlockAlpha::Medium)]),
            // [🮎] UPPER HALF MEDIUM SHADE
            0x1fb8e => Self::Blocks(&[Block::Custom(0, 8, 0, 4, BlockAlpha::Medium)]),
            // [🮏] LOWER HALF MEDIUM SHADE
            0x1fb8f => Self::Blocks(&[Block::Custom(0, 8, 4, 8, BlockAlpha::Medium)]),
            // [🮐] INVERSE MEDIUM SHADE
            0x1fb90 => Self::Blocks(&[Block::Custom(0, 8, 0, 8, BlockAlpha::Medium)]),
            // [🮑] UPPER HALF BLOCK AND LOWER HALF INVERSE MEDIUM SHADE
            0x1fb91 => Self::Blocks(&[
                Block::UpperBlock(4),
                Block::Custom(0, 8, 4, 8, BlockAlpha::Medium),
            ]),
            // [🮒] UPPER HALF INVERSE MEDIUM SHADE AND LOWER HALF BLOCK
            0x1fb92 => Self::Blocks(&[
                Block::Custom(0, 8, 0, 4, BlockAlpha::Medium),
                Block::LowerBlock(4),
            ]),
            // [🮓] LEFT HALF BLOCK AND RIGHT HALF INVERSE MEDIUM SHADE
            // NOTE: not official!
            0x1fb93 => Self::Blocks(&[
                Block::LeftBlock(4),
                Block::Custom(4, 8, 0, 8, BlockAlpha::Medium),
            ]),
            // [🮔] LEFT HALF INVERSE MEDIUM SHADE AND RIGHT HALF BLOCK
            0x1fb94 => Self::Blocks(&[
                Block::Custom(0, 4, 0, 8, BlockAlpha::Medium),
                Block::RightBlock(4),
            ]),
            // [🮕] CHECKER BOARD FILL
            0x1fb95 => Self::Blocks(&[
                Block::Custom(0, 2, 0, 2, BlockAlpha::Full),
                Block::Custom(0, 2, 4, 6, BlockAlpha::Full),
                Block::Custom(2, 4, 2, 4, BlockAlpha::Full),
                Block::Custom(2, 4, 6, 8, BlockAlpha::Full),
                Block::Custom(4, 6, 0, 2, BlockAlpha::Full),
                Block::Custom(4, 6, 4, 6, BlockAlpha::Full),
                Block::Custom(6, 8, 2, 4, BlockAlpha::Full),
                Block::Custom(6, 8, 6, 8, BlockAlpha::Full),
            ]),
            // [🮖] INVERSE CHECKER BOARD FILL
            0x1fb96 => Self::Blocks(&[
                Block::Custom(0, 2, 2, 4, BlockAlpha::Full),
                Block::Custom(0, 2, 6, 8, BlockAlpha::Full),
                Block::Custom(2, 4, 0, 2, BlockAlpha::Full),
                Block::Custom(2, 4, 4, 6, BlockAlpha::Full),
                Block::Custom(4, 6, 2, 4, BlockAlpha::Full),
                Block::Custom(4, 6, 6, 8, BlockAlpha::Full),
                Block::Custom(6, 8, 0, 2, BlockAlpha::Full),
                Block::Custom(6, 8, 4, 6, BlockAlpha::Full),
            ]),
            // [🮗] HEAVY HORIZONTAL FILL
            0x1fb97 => Self::Blocks(&[Block::HorizontalBlock(2, 4), Block::HorizontalBlock(6, 8)]),
            // [🮘] UPPER LEFT TO LOWER RIGHT FILL
            // NOTE: This is a quick placeholder which doesn't scale correctly
            0x1fb98 => Self::Poly(&[
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
            0x1fb99 => Self::Poly(&[
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
            0x1fb9a => Self::Triangles(Triangle::UPPER | Triangle::LOWER, BlockAlpha::Full),
            // [🮛] LEFT AND RIGHT TRIANGULAR HALF BLOCK
            0x1fb9b => Self::Triangles(Triangle::LEFT | Triangle::RIGHT, BlockAlpha::Full),
            // [🮜] UPPER UPPER LEFT TRIANGULAR MEDIUM SHADE
            0x1fb9c => Self::Triangles(Triangle::LEFT | Triangle::UPPER, BlockAlpha::Medium),
            // [🮝] UPPER RIGHT TRIANGULAR MEDIUM SHADE
            0x1fb9d => Self::Triangles(Triangle::RIGHT | Triangle::UPPER, BlockAlpha::Medium),
            // [🮞] LOWER RIGHT TRIANGULAR MEDIUM SHADE
            0x1fb9e => Self::Triangles(Triangle::RIGHT | Triangle::LOWER, BlockAlpha::Medium),
            // [🮟] LOWER LEFT TRIANGULAR MEDIUM SHADE
            0x1fb9f => Self::Triangles(Triangle::LEFT | Triangle::LOWER, BlockAlpha::Medium),
            // [🮠] BOX DRAWINGS LIGHT DIAGONAL UPPER CENTRE TO MIDDLE LEFT
            0x1fba0 => Self::CellDiagonals(CellDiagonal::UPPER_LEFT),
            // [🮡] BOX DRAWINGS LIGHT DIAGONAL UPPER CENTRE TO MIDDLE RIGHT
            0x1fba1 => Self::CellDiagonals(CellDiagonal::UPPER_RIGHT),
            // [🮢] BOX DRAWINGS LIGHT DIAGONAL MIDDLE LEFT TO LOWER CENTRE
            0x1fba2 => Self::CellDiagonals(CellDiagonal::LOWER_LEFT),
            // [🮣] BOX DRAWINGS LIGHT DIAGONAL MIDDLE RIGHT TO LOWER CENTRE
            0x1fba3 => Self::CellDiagonals(CellDiagonal::LOWER_RIGHT),
            // [🮤] BOX DRAWINGS LIGHT DIAGONAL UPPER CENTRE TO MIDDLE LEFT TO LOWER CENTRE
            0x1fba4 => Self::CellDiagonals(CellDiagonal::UPPER_LEFT | CellDiagonal::LOWER_LEFT),
            // [🮥] BOX DRAWINGS LIGHT DIAGONAL UPPER CENTRE TO MIDDLE RIGHT TO LOWER CENTRE
            0x1fba5 => Self::CellDiagonals(CellDiagonal::UPPER_RIGHT | CellDiagonal::LOWER_RIGHT),
            // [🮦] BOX DRAWINGS LIGHT DIAGONAL MIDDLE LEFT TO LOWER CENTRE TO MIDDLE RIGHT
            0x1fba6 => Self::CellDiagonals(CellDiagonal::LOWER_LEFT | CellDiagonal::LOWER_RIGHT),
            // [🮧] BOX DRAWINGS LIGHT DIAGONAL MIDDLE LEFT TO UPPER CENTRE TO MIDDLE RIGHT
            0x1fba7 => Self::CellDiagonals(CellDiagonal::UPPER_LEFT | CellDiagonal::UPPER_RIGHT),
            // [🮨] BOX DRAWINGS LIGHT DIAGONAL UPPER CENTRE TO MIDDLE LEFT AND MIDDLE RIGHT TO LOWER CENTRE
            0x1fba8 => Self::CellDiagonals(CellDiagonal::UPPER_LEFT | CellDiagonal::LOWER_RIGHT),
            // [🮩] BOX DRAWINGS LIGHT DIAGONAL UPPER CENTRE TO MIDDLE RIGHT AND MIDDLE LEFT TO LOWER CENTRE
            0x1fba9 => Self::CellDiagonals(CellDiagonal::UPPER_RIGHT | CellDiagonal::LOWER_LEFT),
            // [🮪] BOX DRAWINGS LIGHT DIAGONAL UPPER CENTRE TO MIDDLE RIGHT TO LOWER CENTRE TO MIDDLE LEFT
            0x1fbaa => Self::CellDiagonals(
                CellDiagonal::UPPER_RIGHT | CellDiagonal::LOWER_LEFT | CellDiagonal::LOWER_RIGHT,
            ),
            // [🮫] BOX DRAWINGS LIGHT DIAGONAL UPPER CENTRE TO MIDDLE LEFT TO LOWER CENTRE TO MIDDLE RIGHT
            0x1fbab => Self::CellDiagonals(
                CellDiagonal::UPPER_LEFT | CellDiagonal::LOWER_LEFT | CellDiagonal::LOWER_RIGHT,
            ),
            // [🮬] BOX DRAWINGS LIGHT DIAGONAL MIDDLE LEFT TO UPPER CENTRE TO MIDDLE RIGHT TO LOWER CENTRE
            0x1fbac => Self::CellDiagonals(
                CellDiagonal::UPPER_LEFT | CellDiagonal::UPPER_RIGHT | CellDiagonal::LOWER_RIGHT,
            ),
            // [🮭] BOX DRAWINGS LIGHT DIAGONAL MIDDLE RIGHT TO UPPER CENTRE TO MIDDLE LEFT TO LOWER CENTRE
            0x1fbad => Self::CellDiagonals(
                CellDiagonal::UPPER_LEFT | CellDiagonal::UPPER_RIGHT | CellDiagonal::LOWER_LEFT,
            ),
            // [🮮] BOX DRAWINGS LIGHT DIAGONAL DIAMOND
            0x1fbae => Self::CellDiagonals(
                CellDiagonal::UPPER_LEFT
                    | CellDiagonal::UPPER_RIGHT
                    | CellDiagonal::LOWER_LEFT
                    | CellDiagonal::LOWER_RIGHT,
            ),
            // [🮯] BOX DRAWINGS LIGHT HORIZONTAL WITH VERTICAL STROKE
            0x1fbaf => Self::Poly(&[
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
            n @ 0x2800..=0x28ff => Self::Braille((n & 0xff) as u8),
            // [] Powerline filled right arrow
            0xe0b0 => Self::Poly(&[Poly {
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
            0xe0b1 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Frac(1, 2)),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::One),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            }]),
            // [] Powerline filled left arrow
            0xe0b2 => Self::Poly(&[Poly {
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
            0xe0b3 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::One, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::Zero, BlockCoord::Frac(1, 2)),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::One),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            }]),

            // [] Powerline filled left semicircle
            0xe0b4 => Self::Poly(&[Poly {
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
            0xe0b5 => Self::Poly(&[Poly {
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
            0xe0b6 => Self::Poly(&[Poly {
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
            0xe0b7 => Self::Poly(&[Poly {
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
            0xe0b8 => Self::Triangles(Triangle::LEFT | Triangle::LOWER, BlockAlpha::Full),
            // [] Powerline outline bottom left half triangle
            0xe0b9 => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::One),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            }]),
            // [] Powerline filled bottom right half triangle
            0xe0ba => Self::Triangles(Triangle::RIGHT | Triangle::LOWER, BlockAlpha::Full),
            // [] Powerline outline bottom right half triangle
            0xe0bb => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Zero),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            }]),
            // [] Powerline filled top left half triangle
            0xe0bc => Self::Poly(&[Poly {
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
            0xe0bd => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::One),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::Zero),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            }]),
            // [] Powerline filled top right half triangle
            0xe0be => Self::Triangles(Triangle::RIGHT | Triangle::UPPER, BlockAlpha::Full),
            // [] Powerline outline top right half triangle
            0xe0bf => Self::Poly(&[Poly {
                path: &[
                    PolyCommand::MoveTo(BlockCoord::Zero, BlockCoord::Zero),
                    PolyCommand::LineTo(BlockCoord::One, BlockCoord::One),
                ],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            }]),
            // [] Progress chunk - left empty
            0xee00 => Self::Progress(ProgressChunk::LEFT),
            // [] Progress chunk - middle empty
            0xee01 => Self::Progress(ProgressChunk::MIDDLE),
            // [] Progress chunk - right empty
            0xee02 => Self::Progress(ProgressChunk::RIGHT),
            // [] Progress chunk - left full
            0xee03 => Self::Progress(ProgressChunk::LEFT | ProgressChunk::FULL),
            // [] Progress chunk - middle full
            0xee04 => Self::Progress(ProgressChunk::MIDDLE | ProgressChunk::FULL),
            // [] Progress chunk - right full
            0xee05 => Self::Progress(ProgressChunk::RIGHT | ProgressChunk::FULL),
            n @ 0xee06..=0xee0b => Self::Spinner((n & 0xff) as u8 - 6),
            // [] Branch drawing horizontal
            0xF5D0 => Self::Branches(Branch::HORIZONTAL),
            // [] Branch drawing vertical
            0xF5D1 => Self::Branches(Branch::VERTICAL),
            // [] Branch drawing fade out to right
            0xF5D2 => Self::Poly(&[Poly {
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
            0xF5D3 => Self::Poly(&[Poly {
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
            0xF5D4 => Self::Poly(&[Poly {
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
            0xF5D5 => Self::Poly(&[Poly {
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
            0xF5D6 => Self::Branches(Branch::RIGHT_TO_DOWN),
            // [] Branch drawing arc down and left
            0xF5D7 => Self::Branches(Branch::LEFT_TO_DOWN),
            // [] Branch drawing arc up and right
            0xF5D8 => Self::Branches(Branch::RIGHT_TO_UP),
            // [] Branch drawing arc up and left
            0xF5D9 => Self::Branches(Branch::LEFT_TO_UP),
            // [] Branch drawing merge from lower and right
            0xF5DA => Self::Branches(Branch::VERTICAL | Branch::RIGHT_TO_UP),
            // [] Branch drawing branch to upper and right
            0xF5DB => Self::Branches(Branch::VERTICAL | Branch::RIGHT_TO_DOWN),
            // [] Branch drawing upper to right and lower to right
            0xF5DC => Self::Branches(Branch::RIGHT_TO_UP | Branch::RIGHT_TO_DOWN),
            // [] Branch drawing merge from left and down
            0xF5DD => Self::Branches(Branch::VERTICAL | Branch::LEFT_TO_UP),
            // [] Branch drawing branch to left and up
            0xF5DE => Self::Branches(Branch::VERTICAL | Branch::LEFT_TO_DOWN),
            // [] Branch drawing upper to left and lower to left
            0xF5DF => Self::Branches(Branch::LEFT_TO_UP | Branch::LEFT_TO_DOWN),
            // [] Branch drawing from left to right and left to down
            0xF5E0 => Self::Branches(Branch::HORIZONTAL | Branch::LEFT_TO_DOWN),
            // [] Branch drawing from right to left and right to down
            0xF5E1 => Self::Branches(Branch::HORIZONTAL | Branch::RIGHT_TO_DOWN),
            // [] Branch drawing from down to left and down to right
            0xF5E2 => Self::Branches(Branch::LEFT_TO_DOWN | Branch::RIGHT_TO_DOWN),
            // [] Branch drawing from left to up and left to right
            0xF5E3 => Self::Branches(Branch::HORIZONTAL | Branch::LEFT_TO_UP),
            // [] Branch drawing from right to up and right to left
            0xF5E4 => Self::Branches(Branch::HORIZONTAL | Branch::RIGHT_TO_UP),
            // [] Branch drawing from up to left and up to right
            0xF5E5 => Self::Branches(Branch::LEFT_TO_UP | Branch::RIGHT_TO_UP),
            // [] Branch drawing from up to left, up to right, and up to down
            0xF5E6 => Self::Branches(Branch::VERTICAL | Branch::LEFT_TO_UP | Branch::RIGHT_TO_UP),
            // [] Branch drawing from down to left, down to right, and down to up
            0xF5E7 => {
                Self::Branches(Branch::VERTICAL | Branch::LEFT_TO_DOWN | Branch::RIGHT_TO_DOWN)
            }
            // [] Branch drawing from left to right, left to up, and left to down
            0xF5E8 => {
                Self::Branches(Branch::HORIZONTAL | Branch::LEFT_TO_UP | Branch::LEFT_TO_DOWN)
            }
            // [] Branch drawing from right to left, right to up, and right to down
            0xF5E9 => {
                Self::Branches(Branch::HORIZONTAL | Branch::RIGHT_TO_DOWN | Branch::RIGHT_TO_UP)
            }
            // [] Branch drawing from up to down, up to left, and down to right
            0xF5EA => Self::Branches(Branch::VERTICAL | Branch::LEFT_TO_UP | Branch::RIGHT_TO_DOWN),
            // [] Branch drawing from up to down, up to right, and down to left
            0xF5EB => Self::Branches(Branch::VERTICAL | Branch::LEFT_TO_DOWN | Branch::RIGHT_TO_UP),
            // [] Branch drawing from left to right, left to up, and right to down
            0xF5EC => {
                Self::Branches(Branch::HORIZONTAL | Branch::LEFT_TO_UP | Branch::RIGHT_TO_DOWN)
            }
            // [] Branch drawing from left to right, left to down, and right to up
            0xF5ED => {
                Self::Branches(Branch::HORIZONTAL | Branch::LEFT_TO_DOWN | Branch::RIGHT_TO_UP)
            }
            // [] Branch drawing filled circle
            0xF5EE => Self::Branches(Branch::CIRCLE_FILLED),
            // [] Branch drawing outline circle
            0xF5EF => Self::Branches(Branch::CIRCLE_OUTLINE),
            // [] Branch drawing filled circle connected to right
            0xF5F0 => Self::Branches(Branch::CIRCLE_FILLED | Branch::RIGHT),
            // [] Branch drawing outline circle connected to right
            0xF5F1 => Self::Branches(Branch::CIRCLE_OUTLINE | Branch::RIGHT),
            // [] Branch drawing filled circle connected to left
            0xF5F2 => Self::Branches(Branch::CIRCLE_FILLED | Branch::LEFT),
            // [] Branch drawing outline circle connected to left
            0xF5F3 => Self::Branches(Branch::CIRCLE_OUTLINE | Branch::LEFT),
            // [] Branch drawing filled circle connected to left and right
            0xF5F4 => Self::Branches(Branch::CIRCLE_FILLED | Branch::LEFT | Branch::RIGHT),
            // [] Branch drawing outline circle connected to left and right
            0xF5F5 => Self::Branches(Branch::CIRCLE_OUTLINE | Branch::LEFT | Branch::RIGHT),
            // [] Branch drawing filled circle connected to down
            0xF5F6 => Self::Branches(Branch::CIRCLE_FILLED | Branch::DOWN),
            // [] Branch drawing outline circle connected to down
            0xF5F7 => Self::Branches(Branch::CIRCLE_OUTLINE | Branch::DOWN),
            // [] Branch drawing filled circle connected to up
            0xF5F8 => Self::Branches(Branch::CIRCLE_FILLED | Branch::UP),
            // [] Branch drawing outline circle connected to up
            0xF5F9 => Self::Branches(Branch::CIRCLE_OUTLINE | Branch::UP),
            // [] Branch drawing filled circle connected to up and down
            0xF5FA => Self::Branches(Branch::CIRCLE_FILLED | Branch::UP | Branch::DOWN),
            // [] Branch drawing outline circle connected to up and down
            0xF5FB => Self::Branches(Branch::CIRCLE_OUTLINE | Branch::UP | Branch::DOWN),
            // [] Branch drawing filled circle connected to right and down
            0xF5FC => Self::Branches(Branch::CIRCLE_FILLED | Branch::RIGHT | Branch::DOWN),
            // [] Branch drawing outline circle connected to right and down
            0xF5FD => Self::Branches(Branch::CIRCLE_OUTLINE | Branch::RIGHT | Branch::DOWN),
            // [] Branch drawing filled circle connected to left and down
            0xF5FE => Self::Branches(Branch::CIRCLE_FILLED | Branch::LEFT | Branch::DOWN),
            // [] Branch drawing outline circle connected to left and down
            0xF5FF => Self::Branches(Branch::CIRCLE_OUTLINE | Branch::LEFT | Branch::DOWN),
            // [] Branch drawing filled circle connected to right and up
            0xF600 => Self::Branches(Branch::CIRCLE_FILLED | Branch::RIGHT | Branch::UP),
            // [] Branch drawing outline circle connected to right and up
            0xF601 => Self::Branches(Branch::CIRCLE_OUTLINE | Branch::RIGHT | Branch::UP),
            // [] Branch drawing filled circle connected to left and up
            0xF602 => Self::Branches(Branch::CIRCLE_FILLED | Branch::LEFT | Branch::UP),
            // [] Branch drawing outline circle connected to left and up
            0xF603 => Self::Branches(Branch::CIRCLE_OUTLINE | Branch::LEFT | Branch::UP),
            // [] Branch drawing filled circle connected to right, up, and down
            0xF604 => {
                Self::Branches(Branch::CIRCLE_FILLED | Branch::RIGHT | Branch::UP | Branch::DOWN)
            }
            // [] Branch drawing outline circle connected to right, up, and down
            0xF605 => {
                Self::Branches(Branch::CIRCLE_OUTLINE | Branch::RIGHT | Branch::UP | Branch::DOWN)
            }
            // [] Branch drawing filled circle connected to left, up, and down
            0xF606 => {
                Self::Branches(Branch::CIRCLE_FILLED | Branch::LEFT | Branch::UP | Branch::DOWN)
            }
            // [] Branch drawing outline circle connected to left, up, and down
            0xF607 => {
                Self::Branches(Branch::CIRCLE_OUTLINE | Branch::LEFT | Branch::UP | Branch::DOWN)
            }
            // [] Branch drawing filled circle connected to left, right, and down
            0xF608 => {
                Self::Branches(Branch::CIRCLE_FILLED | Branch::LEFT | Branch::RIGHT | Branch::DOWN)
            }
            // [] Branch drawing outline circle connected to left, right, and down
            0xF609 => {
                Self::Branches(Branch::CIRCLE_OUTLINE | Branch::LEFT | Branch::RIGHT | Branch::DOWN)
            }
            // [] Branch drawing filled circle connected to left, right, and up
            0xF60A => {
                Self::Branches(Branch::CIRCLE_FILLED | Branch::LEFT | Branch::RIGHT | Branch::UP)
            }
            // [] Branch drawing outline circle connected to left, right, and up
            0xF60B => {
                Self::Branches(Branch::CIRCLE_OUTLINE | Branch::LEFT | Branch::RIGHT | Branch::UP)
            }
            // [] Branch drawing filled circle connected to left, right, up, and down
            0xF60C => Self::Branches(
                Branch::CIRCLE_FILLED | Branch::LEFT | Branch::RIGHT | Branch::UP | Branch::DOWN,
            ),
            // [] Branch drawing outline circle connected to left, right, up, and down
            0xF60D => Self::Branches(
                Branch::CIRCLE_OUTLINE | Branch::LEFT | Branch::RIGHT | Branch::UP | Branch::DOWN,
            ),
            _ => return None,
        })
    }

    pub fn from_cell_iter(cell: termwiz::surface::line::CellRef) -> Option<Self> {
        let mut chars = cell.str().chars();
        let first_char = chars.next()?;
        if chars.next().is_some() {
            None
        } else {
            Self::from_char(first_char)
        }
    }

    pub fn from_cell(cell: &termwiz::cell::Cell) -> Option<Self> {
        let mut chars = cell.str().chars();
        let first_char = chars.next()?;
        if chars.next().is_some() {
            None
        } else {
            Self::from_char(first_char)
        }
    }
}
