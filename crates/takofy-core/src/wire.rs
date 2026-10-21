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
mod tests {
    use super::{put_bytes, put_message, put_str, put_varint, put_varint_field};

    #[test]
    fn a_short_varint_is_one_byte() {
        let mut out = Vec::new();
        put_varint(0x7f, &mut out);
        assert_eq!(out, vec![0x7f]);
    }

    #[test]
    fn a_wide_varint_sets_the_continuation_bit_on_every_byte_but_the_last() {
        let mut out = Vec::new();
        put_varint(300, &mut out);
        assert_eq!(out, vec![0xac, 0x02]);
        let mut widest = Vec::new();
        put_varint(u64::MAX, &mut widest);
        assert_eq!(widest.len(), 10);
        assert!(widest[..9].iter().all(|byte| byte & 0x80 != 0));
        assert_eq!(widest[9], 0x01);
    }

    #[test]
    fn a_varint_field_writes_its_tag_then_its_value() {
        let mut out = Vec::new();
        put_varint_field(3, 1, &mut out);
        assert_eq!(out, vec![0x18, 0x01]);
    }

    #[test]
    fn a_length_delimited_field_writes_its_tag_its_length_and_its_bytes() {
        let mut out = Vec::new();
        put_bytes(1, &[0xde, 0xad], &mut out);
        assert_eq!(out, vec![0x0a, 0x02, 0xde, 0xad]);
    }

    #[test]
    fn a_string_is_its_utf8_bytes_and_an_empty_one_is_an_empty_field() {
        let mut out = Vec::new();
        put_str(1, "alice", &mut out);
        assert_eq!(out, vec![0x0a, 0x05, 0x61, 0x6c, 0x69, 0x63, 0x65]);

        let mut empty = Vec::new();
        put_str(1, "", &mut empty);
        assert_eq!(empty, vec![0x0a, 0x00]);
    }

    #[test]
    fn a_nested_message_is_length_prefixed() {
        let mut inner = Vec::new();
        put_varint_field(1, 1, &mut inner);
        let mut out = Vec::new();
        put_message(4, &inner, &mut out);
        assert_eq!(out, vec![0x22, 0x02, 0x08, 0x01]);
    }
}
