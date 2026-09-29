//! Just enough protobuf decoding for the danmaku segments (`seg.so`).

pub enum Wire<'a> {
  Int(u64),
  Bytes(&'a [u8]),
}

fn varint(buf: &[u8], pos: &mut usize) -> Option<u64> {
  let mut value = 0u64;
  for shift in (0..64).step_by(7) {
    let byte = *buf.get(*pos)?;
    *pos += 1;
    value |= u64::from(byte & 0x7f) << shift;
    if byte < 0x80 {
      return Some(value);
    }
  }
  None
}

/// Top-level `(field number, value)` pairs of one message; stops at malformed input.
pub fn fields(buf: &[u8]) -> Vec<(u64, Wire<'_>)> {
  let mut out = Vec::new();
  let mut pos = 0;
  while let Some(key) = varint(buf, &mut pos) {
    let value = match key & 7 {
      0 => match varint(buf, &mut pos) {
        Some(v) => Wire::Int(v),
        None => break,
      },
      2 => {
        let Some(len) = varint(buf, &mut pos) else {
          break;
        };
        let Some(bytes) = buf.get(pos..pos + len as usize) else {
          break;
        };
        pos += len as usize;
        Wire::Bytes(bytes)
      }
      1 => {
        pos += 8;
        continue;
      }
      5 => {
        pos += 4;
        continue;
      }
      _ => break,
    };
    out.push((key >> 3, value));
  }
  out
}
