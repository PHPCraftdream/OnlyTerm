#[cfg(not(feature = "std"))]
#[allow(unused)]
use num_traits::float::Float;
#[cfg(feature = "std")]
use std::sync::LazyLock;

#[cfg(feature = "std")]
static SRGB_TO_F32_TABLE: LazyLock<[f32; 256]> = LazyLock::new(generate_srgb8_to_linear_f32_table);
#[cfg(feature = "std")]
static F32_TO_U8_TABLE: LazyLock<[u32; 104]> = LazyLock::new(generate_linear_f32_to_srgb8_table);
#[cfg(feature = "std")]
static RGB_TO_SRGB_TABLE: LazyLock<[u8; 256]> = LazyLock::new(generate_rgb_to_srgb8_table);
#[cfg(feature = "std")]
static RGB_TO_F32_TABLE: LazyLock<[f32; 256]> = LazyLock::new(generate_rgb_to_linear_f32_table);

#[cfg(feature = "std")]
fn generate_rgb_to_srgb8_table() -> [u8; 256] {
    let mut table = [0; 256];
    for (val, entry) in table.iter_mut().enumerate() {
        let linear = (val as f32) / 255.0;
        *entry = linear_f32_to_srgb8_using_table(linear);
    }
    table
}

#[cfg(feature = "std")]
fn generate_rgb_to_linear_f32_table() -> [f32; 256] {
    let mut table = [0.; 256];
    for (val, entry) in table.iter_mut().enumerate() {
        *entry = (val as f32) / 255.0;
    }
    table
}

#[cfg(feature = "std")]
fn generate_srgb8_to_linear_f32_table() -> [f32; 256] {
    let mut table = [0.; 256];
    for (val, entry) in table.iter_mut().enumerate() {
        let c = (val as f32) / 255.0;
        *entry = if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        };
    }
    table
}

#[allow(clippy::unreadable_literal)]
#[cfg(feature = "std")]
fn generate_linear_f32_to_srgb8_table() -> [u32; 104] {
    // My intent was to generate this array on the fly using the code that is commented
    // out below.  It is based on this gist:
    // https://gist.github.com/rygorous/2203834
    // but for whatever reason, the rust translation yields different numbers.
    // I haven't had an opportunity to dig in to why that is, and I just wanted
    // to get things rolling, so we're in a slightly gross state for now.
    [
        0x0073000d, 0x007a000d, 0x0080000d, 0x0087000d, 0x008d000d, 0x0094000d, 0x009a000d,
        0x00a1000d, 0x00a7001a, 0x00b4001a, 0x00c1001a, 0x00ce001a, 0x00da001a, 0x00e7001a,
        0x00f4001a, 0x0101001a, 0x010e0033, 0x01280033, 0x01410033, 0x015b0033, 0x01750033,
        0x018f0033, 0x01a80033, 0x01c20033, 0x01dc0067, 0x020f0067, 0x02430067, 0x02760067,
        0x02aa0067, 0x02dd0067, 0x03110067, 0x03440067, 0x037800ce, 0x03df00ce, 0x044600ce,
        0x04ad00ce, 0x051400ce, 0x057b00c5, 0x05dd00bc, 0x063b00b5, 0x06970158, 0x07420142,
        0x07e30130, 0x087b0120, 0x090b0112, 0x09940106, 0x0a1700fc, 0x0a9500f2, 0x0b0f01cb,
        0x0bf401ae, 0x0ccb0195, 0x0d950180, 0x0e56016e, 0x0f0d015e, 0x0fbc0150, 0x10630143,
        0x11070264, 0x1238023e, 0x1357021d, 0x14660201, 0x156601e9, 0x165a01d3, 0x174401c0,
        0x182401af, 0x18fe0331, 0x1a9602fe, 0x1c1502d2, 0x1d7e02ad, 0x1ed4028d, 0x201a0270,
        0x21520256, 0x227d0240, 0x239f0443, 0x25c003fe, 0x27bf03c4, 0x29a10392, 0x2b6a0367,
        0x2d1d0341, 0x2ebe031f, 0x304d0300, 0x31d105b0, 0x34a80555, 0x37520507, 0x39d504c5,
        0x3c37048b, 0x3e7c0458, 0x40a8042a, 0x42bd0401, 0x44c20798, 0x488e071e, 0x4c1c06b6,
        0x4f76065d, 0x52a50610, 0x55ac05cc, 0x5892058f, 0x5b590559, 0x5e0c0a23, 0x631c0980,
        0x67db08f6, 0x6c55087f, 0x70940818, 0x74a007bd, 0x787d076c, 0x7c330723,
    ]
    /*
    let numexp = 13;
    let mantissa_msb = 3;
    let nbuckets = numexp << mantissa_msb;
    let bucketsize = 1 << (23 - mantissa_msb);
    let mantshift = 12;

    let mut table = [0;104];

    let sum_aa = bucketsize as f64;
    let mut sum_ab = 0.0f64;
    let mut sum_bb = 0.0f64;

    for i in 0..bucketsize {
        let j = (i >> mantshift) as f64;

        sum_ab += j;
        sum_bb += j * j;
    }

    let inv_det = 1.0 / (sum_aa * sum_bb - sum_ab * sum_ab);
    eprintln!("sum_ab={:e} sum_bb={:e} inv_det={:e}", sum_ab, sum_bb, inv_det);

    for bucket in 0..nbuckets {
        let start = ((127 - numexp) << 23) + bucket*bucketsize;

        let mut sum_a = 0.0;
        let mut sum_b = 0.0;

        for i in 0..bucketsize {
            let j = i >> mantshift;

            let val = linear_f32_to_srgbf32(f32::from_bits(start + i)) as f64 + 0.5;
            sum_a += val;
            sum_b += j as f64 * val;
        }

        let solved_a = inv_det * (sum_bb*sum_a - sum_ab*sum_b);
        let solved_b = inv_det * (sum_aa*sum_b - sum_ab*sum_a);
        let scaled_a = solved_a * 65536.0 / 512.0;
        let scaled_b = solved_b * 65536.0;

        let int_a = (scaled_a + 0.5) as u32;
        let int_b = (scaled_b + 0.5) as u32;

        table[bucket as usize] = (int_a << 16) + int_b;
    }

    table
    */
}

/// Convert from linear rgb in floating point form (0-1.0) to srgb in floating point (0-255.0)
pub(crate) fn linear_f32_to_srgbf32(f: f32) -> f32 {
    if f <= 0.04045 {
        f * 12.92
    } else {
        f.powf(1.0 / 2.4) * 1.055 - 0.055
    }
}

#[cfg(feature = "std")]
pub fn linear_u8_to_srgb8(f: u8) -> u8 {
    RGB_TO_SRGB_TABLE[f as usize]
}

#[cfg(feature = "std")]
fn linear_f32_to_srgb8_using_table(f: f32) -> u8 {
    #[allow(clippy::unreadable_literal)]
    const ALMOST_ONE: u32 = 0x3f7fffff;
    #[allow(clippy::unreadable_literal)]
    const MINVAL: u32 = (127 - 13) << 23;
    let minval = f32::from_bits(MINVAL);
    let almost_one = f32::from_bits(ALMOST_ONE);

    // `f.is_nan()` is checked explicitly because NaN compares false against
    // both bounds below (`NaN < minval` and `NaN > almost_one` are both
    // false), so a plain min/max-style clamp would let NaN fall through
    // unclamped and produce an out-of-bounds table index below.
    let f = if f.is_nan() || f < minval {
        minval
    } else if f > almost_one {
        almost_one
    } else {
        f
    };

    let f_bits = f.to_bits();
    // `f` is clamped to [minval, almost_one] above, so `f_bits - MINVAL` is in
    // [0, 0x67fffff] and `>> 20` yields 0..=103, always within the 104-entry table.
    let tab = F32_TO_U8_TABLE[((f_bits - MINVAL) >> 20) as usize];
    let bias = (tab >> 16) << 9;
    let scale = tab & 0xffff;

    let t = (f_bits >> 12) & 0xff;

    ((bias + scale * t) >> 16) as u8
}

pub(crate) fn linear_f32_to_srgb8(f: f32) -> u8 {
    #[cfg(feature = "std")]
    {
        linear_f32_to_srgb8_using_table(f)
    }
    #[cfg(not(feature = "std"))]
    {
        (linear_f32_to_srgbf32(f) * 255.) as u8
    }
}

/// Convert from srgb in u8 0-255 to linear floating point rgb 0-1.0
pub(crate) fn srgb8_to_linear_f32(val: u8) -> f32 {
    #[cfg(feature = "std")]
    {
        // `val` is a u8, always a valid index into the 256-entry table.
        SRGB_TO_F32_TABLE[val as usize]
    }
    #[cfg(not(feature = "std"))]
    {
        let c = (val as f32) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }
}

pub(crate) fn rgb_to_linear_f32(val: u8) -> f32 {
    #[cfg(feature = "std")]
    {
        // `val` is a u8, always a valid index into the 256-entry table.
        RGB_TO_F32_TABLE[val as usize]
    }
    #[cfg(not(feature = "std"))]
    {
        (val as f32) / 255.0
    }
}
