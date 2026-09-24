use super::super::types::*;
use super::{OCTANT_PATTERNS, SEXTANT_PATTERNS};

pub(super) fn lookup(c: u32) -> Option<BlockKey> {
    Some(match c {
        // [▀] UPPER HALF BLOCK
        0x2580 => BlockKey::Blocks(&[Block::UpperBlock(4)]),
        // [▁] LOWER 1 EIGHTH BLOCK
        0x2581 => BlockKey::Blocks(&[Block::LowerBlock(1)]),
        // [▂] LOWER 2 EIGHTHS BLOCK
        0x2582 => BlockKey::Blocks(&[Block::LowerBlock(2)]),
        // [▃] LOWER 3 EIGHTHS BLOCK
        0x2583 => BlockKey::Blocks(&[Block::LowerBlock(3)]),
        // [▄] LOWER 4 EIGHTHS BLOCK
        0x2584 => BlockKey::Blocks(&[Block::LowerBlock(4)]),
        // [▅] LOWER 5 EIGHTHS BLOCK
        0x2585 => BlockKey::Blocks(&[Block::LowerBlock(5)]),
        // [▆] LOWER 6 EIGHTHS BLOCK
        0x2586 => BlockKey::Blocks(&[Block::LowerBlock(6)]),
        // [▇] LOWER 7 EIGHTHS BLOCK
        0x2587 => BlockKey::Blocks(&[Block::LowerBlock(7)]),
        // [█] FULL BLOCK
        0x2588 => BlockKey::Blocks(&[Block::Custom(0, 8, 0, 8, BlockAlpha::Full)]),
        // [▉] LEFT 7 EIGHTHS BLOCK
        0x2589 => BlockKey::Blocks(&[Block::LeftBlock(7)]),
        // [▊] LEFT 6 EIGHTHS BLOCK
        0x258a => BlockKey::Blocks(&[Block::LeftBlock(6)]),
        // [▋] LEFT 5 EIGHTHS BLOCK
        0x258b => BlockKey::Blocks(&[Block::LeftBlock(5)]),
        // [▌] LEFT 4 EIGHTHS BLOCK
        0x258c => BlockKey::Blocks(&[Block::LeftBlock(4)]),
        // [▍] LEFT 3 EIGHTHS BLOCK
        0x258d => BlockKey::Blocks(&[Block::LeftBlock(3)]),
        // [▎] LEFT 2 EIGHTHS BLOCK
        0x258e => BlockKey::Blocks(&[Block::LeftBlock(2)]),
        // [▏] LEFT 1 EIGHTHS BLOCK
        0x258f => BlockKey::Blocks(&[Block::LeftBlock(1)]),
        // [▐] RIGHT HALF BLOCK
        0x2590 => BlockKey::Blocks(&[Block::RightBlock(4)]),
        // [░] LIGHT SHADE
        0x2591 => BlockKey::Blocks(&[Block::Custom(0, 8, 0, 8, BlockAlpha::Light)]),
        // [▒] MEDIUM SHADE
        0x2592 => BlockKey::Blocks(&[Block::Custom(0, 8, 0, 8, BlockAlpha::Medium)]),
        // [▓] DARK SHADE
        0x2593 => BlockKey::Blocks(&[Block::Custom(0, 8, 0, 8, BlockAlpha::Dark)]),
        // [▔] UPPER ONE EIGHTH BLOCK
        0x2594 => BlockKey::Blocks(&[Block::UpperBlock(1)]),
        // [▕] RIGHT ONE EIGHTH BLOCK
        0x2595 => BlockKey::Blocks(&[Block::RightBlock(1)]),
        // [▖] QUADRANT LOWER LEFT
        0x2596 => BlockKey::Blocks(&[Block::QuadrantLL]),
        // [▗] QUADRANT LOWER RIGHT
        0x2597 => BlockKey::Blocks(&[Block::QuadrantLR]),
        // [▘] QUADRANT UPPER LEFT
        0x2598 => BlockKey::Blocks(&[Block::QuadrantUL]),
        // [▙] QUADRANT UPPER LEFT AND LOWER LEFT AND LOWER RIGHT
        0x2599 => BlockKey::Blocks(&[Block::QuadrantUL, Block::QuadrantLL, Block::QuadrantLR]),
        // [▚] QUADRANT UPPER LEFT AND LOWER RIGHT
        0x259a => BlockKey::Blocks(&[Block::QuadrantUL, Block::QuadrantLR]),
        // [▛] QUADRANT UPPER LEFT AND UPPER RIGHT AND LOWER LEFT
        0x259b => BlockKey::Blocks(&[Block::QuadrantUL, Block::QuadrantUR, Block::QuadrantLL]),
        // [▜] QUADRANT UPPER LEFT AND UPPER RIGHT AND LOWER RIGHT
        0x259c => BlockKey::Blocks(&[Block::QuadrantUL, Block::QuadrantUR, Block::QuadrantLR]),
        // [▝] QUADRANT UPPER RIGHT
        0x259d => BlockKey::Blocks(&[Block::QuadrantUR]),
        // [▞] QUADRANT UPPER RIGHT AND LOWER LEFT
        0x259e => BlockKey::Blocks(&[Block::QuadrantUR, Block::QuadrantLL]),
        // [▟] QUADRANT UPPER RIGHT AND LOWER LEFT AND LOWER RIGHT
        0x259f => BlockKey::Blocks(&[Block::QuadrantUR, Block::QuadrantLL, Block::QuadrantLR]),
        // Sextant blocks
        n @ 0x1fb00..=0x1fb3b => BlockKey::Sextant(SEXTANT_PATTERNS[(n & 0x3f) as usize]),
        // Octant blocks
        n @ 0x1cd00..=0x1cde5 => BlockKey::Octant(OCTANT_PATTERNS[(n & 0xff) as usize]),
        // [𜺠] RIGHT HALF LOWER ONE QUARTER BLOCK (corresponds to OCTANT-8)
        0x1cea0 => BlockKey::Octant(0b10000000),
        // [𜺣; EFT HALF LOWER ONE QUARTER BLOCK (corresponds to OCTANT-7)
        0x1cea3 => BlockKey::Octant(0b01000000),
        // [𜺨] LEFT HALF UPPER ONE QUARTER BLOCK (corresponds to OCTANT-1)
        0x1cea8 => BlockKey::Octant(0b00000001),
        // [𜺫] RIGHT HALF UPPER ONE QUARTER BLOCK (corresponds to OCTANT-2)
        0x1ceab => BlockKey::Octant(0b00000010),
        // [🯦] MIDDLE LEFT ONE QUARTER BLOCK (corresponds to OCTANT-35)
        0x1fbe6 => BlockKey::Octant(0b00010100),
        // [🯧] MIDDLE RIGHT ONE QUARTER BLOCK (corresponds to OCTANT-46)
        0x1fbe7 => BlockKey::Octant(0b00101000),
        // [🬼] LOWER LEFT BLOCK DIAGONAL LOWER MIDDLE LEFT TO LOWER CENTRE
        0x1fb3c => BlockKey::Poly(&[Poly {
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
        0x1fb3d => BlockKey::Poly(&[Poly {
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
        0x1fb3e => BlockKey::Poly(&[Poly {
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
        0x1fb3f => BlockKey::Poly(&[Poly {
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
        0x1fb40 => BlockKey::Poly(&[Poly {
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
        0x1fb41 => BlockKey::Poly(&[Poly {
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
        0x1fb42 => BlockKey::Poly(&[Poly {
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
        0x1fb43 => BlockKey::Poly(&[Poly {
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
        0x1fb44 => BlockKey::Poly(&[Poly {
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
        0x1fb45 => BlockKey::Poly(&[Poly {
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
        0x1fb46 => BlockKey::Poly(&[Poly {
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
        0x1fb47 => BlockKey::Poly(&[Poly {
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
        0x1fb48 => BlockKey::Poly(&[Poly {
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
        0x1fb49 => BlockKey::Poly(&[Poly {
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
        0x1fb4a => BlockKey::Poly(&[Poly {
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
        0x1fb4b => BlockKey::Poly(&[Poly {
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
        0x1fb4c => BlockKey::Poly(&[Poly {
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
        0x1fb4d => BlockKey::Poly(&[Poly {
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
        0x1fb4e => BlockKey::Poly(&[Poly {
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
        0x1fb4f => BlockKey::Poly(&[Poly {
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
        0x1fb50 => BlockKey::Poly(&[Poly {
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
        0x1fb51 => BlockKey::Poly(&[Poly {
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
        0x1fb52 => BlockKey::Poly(&[Poly {
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
        0x1fb53 => BlockKey::Poly(&[Poly {
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
        0x1fb54 => BlockKey::Poly(&[Poly {
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
        0x1fb55 => BlockKey::Poly(&[Poly {
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
        0x1fb56 => BlockKey::Poly(&[Poly {
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
        0x1fb57 => BlockKey::Poly(&[Poly {
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
        0x1fb58 => BlockKey::Poly(&[Poly {
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
        0x1fb59 => BlockKey::Poly(&[Poly {
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
        0x1fb5a => BlockKey::Poly(&[Poly {
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
        0x1fb5b => BlockKey::Poly(&[Poly {
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
        0x1fb5c => BlockKey::Poly(&[Poly {
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
        0x1fb5d => BlockKey::Poly(&[Poly {
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
        0x1fb5e => BlockKey::Poly(&[Poly {
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
        0x1fb5f => BlockKey::Poly(&[Poly {
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
        0x1fb60 => BlockKey::Poly(&[Poly {
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
        0x1fb61 => BlockKey::Poly(&[Poly {
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
        0x1fb62 => BlockKey::Poly(&[Poly {
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
        0x1fb63 => BlockKey::Poly(&[Poly {
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
        0x1fb64 => BlockKey::Poly(&[Poly {
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
        0x1fb65 => BlockKey::Poly(&[Poly {
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
        0x1fb66 => BlockKey::Poly(&[Poly {
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
        0x1fb67 => BlockKey::Poly(&[Poly {
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
        0x1fb68 => BlockKey::Triangles(
            Triangle::UPPER | Triangle::RIGHT | Triangle::LOWER,
            BlockAlpha::Full,
        ),
        // [🭩] LEFT AND LOWER AND RIGHT TRIANGULAR THREE QUARTERS BLOCK
        0x1fb69 => BlockKey::Triangles(
            Triangle::LEFT | Triangle::LOWER | Triangle::RIGHT,
            BlockAlpha::Full,
        ),
        // [🭪] UPPER AND LEFT AND LOWER TRIANGULAR THREE QUARTERS BLOCK
        0x1fb6a => BlockKey::Triangles(
            Triangle::UPPER | Triangle::LEFT | Triangle::LOWER,
            BlockAlpha::Full,
        ),
        // [🭫] LEFT AND UPPER AND RIGHT TRIANGULAR THREE QUARTERS BLOCK
        0x1fb6b => BlockKey::Triangles(
            Triangle::LEFT | Triangle::UPPER | Triangle::RIGHT,
            BlockAlpha::Full,
        ),
        // [🭬] LEFT TRIANGULAR ONE QUARTER BLOCK
        0x1fb6c => BlockKey::Triangles(Triangle::LEFT, BlockAlpha::Full),
        // [🭭] UPPER TRIANGULAR ONE QUARTER BLOCK
        0x1fb6d => BlockKey::Triangles(Triangle::UPPER, BlockAlpha::Full),
        // [🭮] RIGHT TRIANGULAR ONE QUARTER BLOCK
        0x1fb6e => BlockKey::Triangles(Triangle::RIGHT, BlockAlpha::Full),
        // [🭯] LOWER TRIANGULAR ONE QUARTER BLOCK
        0x1fb6f => BlockKey::Triangles(Triangle::LOWER, BlockAlpha::Full),
        // [🭰] VERTICAL ONE EIGHTH BLOCK-2
        0x1fb70 => BlockKey::Blocks(&[Block::VerticalBlock(1, 2)]),
        // [🭱] VERTICAL ONE EIGHTH BLOCK-3
        0x1fb71 => BlockKey::Blocks(&[Block::VerticalBlock(2, 3)]),
        // [🭲] VERTICAL ONE EIGHTH BLOCK-4
        0x1fb72 => BlockKey::Blocks(&[Block::VerticalBlock(3, 4)]),
        // [🭳] VERTICAL ONE EIGHTH BLOCK-5
        0x1fb73 => BlockKey::Blocks(&[Block::VerticalBlock(4, 5)]),
        // [🭴] VERTICAL ONE EIGHTH BLOCK-6
        0x1fb74 => BlockKey::Blocks(&[Block::VerticalBlock(5, 6)]),
        // [🭵] VERTICAL ONE EIGHTH BLOCK-7
        0x1fb75 => BlockKey::Blocks(&[Block::VerticalBlock(6, 7)]),
        // [🭶] HORIZONTAL ONE EIGHTH BLOCK-2
        0x1fb76 => BlockKey::Blocks(&[Block::HorizontalBlock(1, 2)]),
        // [🭷] HORIZONTAL ONE EIGHTH BLOCK-3
        0x1fb77 => BlockKey::Blocks(&[Block::HorizontalBlock(2, 3)]),
        // [🭸] HORIZONTAL ONE EIGHTH BLOCK-4
        0x1fb78 => BlockKey::Blocks(&[Block::HorizontalBlock(3, 4)]),
        // [🭹] HORIZONTAL ONE EIGHTH BLOCK-5
        0x1fb79 => BlockKey::Blocks(&[Block::HorizontalBlock(4, 5)]),
        // [🭺] HORIZONTAL ONE EIGHTH BLOCK-6
        0x1fb7a => BlockKey::Blocks(&[Block::HorizontalBlock(5, 6)]),
        // [🭻] HORIZONTAL ONE EIGHTH BLOCK-7
        0x1fb7b => BlockKey::Blocks(&[Block::HorizontalBlock(6, 7)]),
        // [🭼] Left and lower one eighth block
        0x1fb7c => BlockKey::Blocks(&[Block::LeftBlock(1), Block::LowerBlock(1)]),
        // [🭽] Left and upper one eighth block
        0x1fb7d => BlockKey::Blocks(&[Block::LeftBlock(1), Block::UpperBlock(1)]),
        // [🭾] Right and upper one eighth block
        0x1fb7e => BlockKey::Blocks(&[Block::RightBlock(1), Block::UpperBlock(1)]),
        // [🭿] Right and lower one eighth block
        0x1fb7f => BlockKey::Blocks(&[Block::RightBlock(1), Block::LowerBlock(1)]),
        // [🮀] UPPER AND LOWER ONE EIGHTH BLOCK
        0x1fb80 => BlockKey::Blocks(&[Block::UpperBlock(1), Block::LowerBlock(1)]),
        // [🮁] HORIZONTAL ONE EIGHTH BLOCK-1358
        0x1fb81 => BlockKey::Blocks(&[
            Block::UpperBlock(1),
            Block::HorizontalBlock(2, 3),
            Block::HorizontalBlock(4, 5),
            Block::LowerBlock(1),
        ]),
        // [🮂] Upper One Quarter Block
        0x1fb82 => BlockKey::Blocks(&[Block::UpperBlock(2)]),
        // [🮃] Upper three eighths block
        0x1fb83 => BlockKey::Blocks(&[Block::UpperBlock(3)]),
        // [🮄] Upper five eighths block
        0x1fb84 => BlockKey::Blocks(&[Block::UpperBlock(5)]),
        // [🮅] Upper three quarters block
        0x1fb85 => BlockKey::Blocks(&[Block::UpperBlock(6)]),
        // [🮆] Upper seven eighths block
        0x1fb86 => BlockKey::Blocks(&[Block::UpperBlock(7)]),
        // [🮇] Right One Quarter Block
        0x1fb87 => BlockKey::Blocks(&[Block::RightBlock(2)]),
        // [🮈] Right three eighths block
        0x1fb88 => BlockKey::Blocks(&[Block::RightBlock(3)]),
        // [🮉] Right five eighths block
        0x1fb89 => BlockKey::Blocks(&[Block::RightBlock(5)]),
        // [🮊] Right three quarters block
        0x1fb8a => BlockKey::Blocks(&[Block::RightBlock(6)]),
        // [🮋] Right seven eighths block
        0x1fb8b => BlockKey::Blocks(&[Block::RightBlock(7)]),
        // [🮌] LEFT HALF MEDIUM SHADE
        0x1fb8c => BlockKey::Blocks(&[Block::Custom(0, 4, 0, 8, BlockAlpha::Medium)]),
        // [🮍] RIGHT HALF MEDIUM SHADE
        0x1fb8d => BlockKey::Blocks(&[Block::Custom(4, 8, 0, 8, BlockAlpha::Medium)]),
        // [🮎] UPPER HALF MEDIUM SHADE
        0x1fb8e => BlockKey::Blocks(&[Block::Custom(0, 8, 0, 4, BlockAlpha::Medium)]),
        // [🮏] LOWER HALF MEDIUM SHADE
        0x1fb8f => BlockKey::Blocks(&[Block::Custom(0, 8, 4, 8, BlockAlpha::Medium)]),
        // [🮐] INVERSE MEDIUM SHADE
        0x1fb90 => BlockKey::Blocks(&[Block::Custom(0, 8, 0, 8, BlockAlpha::Medium)]),
        // [🮑] UPPER HALF BLOCK AND LOWER HALF INVERSE MEDIUM SHADE
        0x1fb91 => BlockKey::Blocks(&[
            Block::UpperBlock(4),
            Block::Custom(0, 8, 4, 8, BlockAlpha::Medium),
        ]),
        // [🮒] UPPER HALF INVERSE MEDIUM SHADE AND LOWER HALF BLOCK
        0x1fb92 => BlockKey::Blocks(&[
            Block::Custom(0, 8, 0, 4, BlockAlpha::Medium),
            Block::LowerBlock(4),
        ]),
        // [🮓] LEFT HALF BLOCK AND RIGHT HALF INVERSE MEDIUM SHADE
        // NOTE: not official!
        0x1fb93 => BlockKey::Blocks(&[
            Block::LeftBlock(4),
            Block::Custom(4, 8, 0, 8, BlockAlpha::Medium),
        ]),
        // [🮔] LEFT HALF INVERSE MEDIUM SHADE AND RIGHT HALF BLOCK
        0x1fb94 => BlockKey::Blocks(&[
            Block::Custom(0, 4, 0, 8, BlockAlpha::Medium),
            Block::RightBlock(4),
        ]),
        // [🮕] CHECKER BOARD FILL
        0x1fb95 => BlockKey::Blocks(&[
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
        0x1fb96 => BlockKey::Blocks(&[
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
        0x1fb97 => BlockKey::Blocks(&[Block::HorizontalBlock(2, 4), Block::HorizontalBlock(6, 8)]),
        // [🮘] UPPER LEFT TO LOWER RIGHT FILL
        // NOTE: This is a quick placeholder which doesn't scale correctly
        _ => return None,
    })
}
