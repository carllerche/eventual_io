use bytes::{Bytes, ToBytes};
use eventual::Future;
use eio;
use eio::frame::{Frame, Len};

#[test]
pub fn test_framing_exact_sized_chunk() {
    let s = stream(vec![b"foo", b"bar", b"baz"])
        .frame(Len::new(3));

    let chunks: Vec<Bytes> = s.iter().collect();

    assert_eq!(chunks.len(), 3);
    assert_eq!(chunks[0], b"foo".to_bytes());
    assert_eq!(chunks[1], b"bar".to_bytes());
    assert_eq!(chunks[2], b"baz".to_bytes());
}

#[test]
pub fn test_framing_non_uniform_chunks() {
    let s = stream(vec![b"fo", b"obarb", b"a", b"z"])
        .frame(Len::new(3));

    let chunks: Vec<Bytes> = s.iter().collect();

    assert_eq!(chunks.len(), 3);
    assert_eq!(chunks[0], b"foo".to_bytes());
    assert_eq!(chunks[1], b"bar".to_bytes());
    assert_eq!(chunks[2], b"baz".to_bytes());
}

#[test]
pub fn test_one_frame() {
    let s = stream(vec![b"foobarbaz"])
        .frame_one(Len::new(3));

    let chunks: Vec<Bytes> = s.iter().collect();

    assert_eq!(2, chunks.len());
    assert_eq!(chunks[0], b"foo".to_bytes());
    assert_eq!(chunks[1], b"barbaz".to_bytes());
}

fn stream(mut chunks: Vec<&'static [u8]>) -> eio::Stream<Bytes> {
    Future::lazy(move || {
        if chunks.is_empty() {
            return Ok(None);
        }

        let val = chunks.remove(0);
        Ok(Some((Bytes::from_slice(val), stream(chunks))))
    }).to_stream()
}
