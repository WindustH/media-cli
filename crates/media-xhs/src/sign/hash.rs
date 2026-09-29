//! xxHash32 (xhshow `utils/hash.py`), used by x-rap-param.

const P1: u32 = 0x9E37_79B1;
const P2: u32 = 0x85EB_CA77;
const P3: u32 = 0xC2B2_AE3D;
const P4: u32 = 0x27D4_EB2F;
const P5: u32 = 0x1656_67B1;

fn word(buf: &[u8], pos: usize) -> u32 {
  u32::from_le_bytes([buf[pos], buf[pos + 1], buf[pos + 2], buf[pos + 3]])
}

fn round(acc: u32, word: u32) -> u32 {
  acc
    .wrapping_add(word.wrapping_mul(P2))
    .rotate_left(13)
    .wrapping_mul(P1)
}

pub fn xxh32(buf: &[u8], seed: u32) -> u32 {
  let len = buf.len();
  let mut pos = 0;
  let mut h = if len >= 16 {
    let mut acc = [
      seed.wrapping_add(P1).wrapping_add(P2),
      seed.wrapping_add(P2),
      seed,
      seed.wrapping_sub(P1),
    ];
    while pos + 16 <= len {
      for a in &mut acc {
        *a = round(*a, word(buf, pos));
        pos += 4;
      }
    }
    acc[0]
      .rotate_left(1)
      .wrapping_add(acc[1].rotate_left(7))
      .wrapping_add(acc[2].rotate_left(12))
      .wrapping_add(acc[3].rotate_left(18))
  } else {
    seed.wrapping_add(P5)
  };
  h = h.wrapping_add(len as u32);
  while pos + 4 <= len {
    h = h
      .wrapping_add(word(buf, pos).wrapping_mul(P3))
      .rotate_left(17)
      .wrapping_mul(P4);
    pos += 4;
  }
  while pos < len {
    h = h
      .wrapping_add(u32::from(buf[pos]).wrapping_mul(P5))
      .rotate_left(11)
      .wrapping_mul(P1);
    pos += 1;
  }
  h ^= h >> 15;
  h = h.wrapping_mul(P2);
  h ^= h >> 13;
  h = h.wrapping_mul(P3);
  h ^ (h >> 16)
}
