//! The protobuf field writers every hand-encoded request body is built from.
//!
//! The collection and playlist endpoints have no compiled message in librespot,
//! so each body is assembled byte by byte. The golden tests beside each body
//! pin what these produce.

/// One varint, the shortest encoding of the value.
pub(crate) fn put_varint(mut value: u64, out: &mut Vec<u8>) {
    while value >= 0x80 {
        out.push((value as u8) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

/// One varint field: the tag, then the value.
pub(crate) fn put_varint_field(tag: u32, value: u64, out: &mut Vec<u8>) {
    put_varint(u64::from(tag) << 3, out);
    put_varint(value, out);
}

/// One length-delimited field: the tag, the length, then the bytes.
pub(crate) fn put_bytes(tag: u32, value: &[u8], out: &mut Vec<u8>) {
    put_varint((u64::from(tag) << 3) | 2, out);
    put_varint(value.len() as u64, out);
    out.extend_from_slice(value);
}

/// One string field, as its UTF-8 bytes.
pub(crate) fn put_str(tag: u32, value: &str, out: &mut Vec<u8>) {
    put_bytes(tag, value.as_bytes(), out);
}

/// One nested message field, as its encoded body.
pub(crate) fn put_message(tag: u32, body: &[u8], out: &mut Vec<u8>) {
    put_bytes(tag, body, out);
}

#[cfg(test)]
mod tests;
