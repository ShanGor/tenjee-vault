use super::manifest::{bytes, decode, invalid, CHUNK};
use crate::error::VaultResult;
use serde::{de::DeserializeOwned, Serialize};
use std::io::{Read, Write};

pub const CONTROL: usize = 256 * 1024;
// Header: type:u8, payload length:u32. DATA contains entry:u32, offset:u64,
// range SHA256:32, followed by raw bytes. No content ever crosses WebView IPC.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Type {
    Hello = 1,
    Offer = 2,
    Page = 3,
    Plan = 4,
    Approval = 5,
    Begin = 6,
    Data = 7,
    End = 8,
    Window = 9,
    Checkpoint = 10,
    Finish = 11,
    Result = 12,
    Resume = 13,
}
pub struct Frame {
    pub kind: Type,
    pub data: Vec<u8>,
}
pub fn send(stream: &mut impl Write, kind: Type, payload: &[u8]) -> VaultResult<()> {
    let limit = if kind == Type::Data {
        CHUNK + 44
    } else {
        CONTROL
    };
    if payload.len() > limit {
        return Err(invalid("Transfer frame exceeds its limit"));
    }
    stream.write_all(&[kind as u8])?;
    stream.write_all(&(payload.len() as u32).to_be_bytes())?;
    stream.write_all(payload)?;
    Ok(())
}
pub fn control<T: Serialize>(stream: &mut impl Write, kind: Type, value: &T) -> VaultResult<()> {
    send(stream, kind, &bytes(value)?)?;
    stream.flush()?;
    Ok(())
}
pub fn receive(stream: &mut impl Read) -> VaultResult<Frame> {
    let mut header = [0; 5];
    stream.read_exact(&mut header)?;
    let kind = match header[0] {
        1 => Type::Hello,
        2 => Type::Offer,
        3 => Type::Page,
        4 => Type::Plan,
        5 => Type::Approval,
        6 => Type::Begin,
        7 => Type::Data,
        8 => Type::End,
        9 => Type::Window,
        10 => Type::Checkpoint,
        11 => Type::Finish,
        12 => Type::Result,
        13 => Type::Resume,
        _ => return Err(invalid("Unknown critical transfer frame")),
    };
    let length = u32::from_be_bytes(header[1..].try_into().unwrap()) as usize;
    if length
        > if kind == Type::Data {
            CHUNK + 44
        } else {
            CONTROL
        }
    {
        return Err(invalid("Peer frame exceeds its limit"));
    }
    if kind == Type::Data && length <= 44 {
        return Err(invalid("Empty data frame"));
    }
    let mut data = vec![0; length];
    stream.read_exact(&mut data)?;
    Ok(Frame { kind, data })
}
pub fn expect<T: DeserializeOwned>(stream: &mut impl Read, kind: Type) -> VaultResult<T> {
    let frame = receive(stream)?;
    if frame.kind != kind {
        return Err(invalid(format!(
            "Unexpected transfer state: {:?}, expected {kind:?}",
            frame.kind
        )));
    }
    decode(&frame.data)
}
pub fn data(
    stream: &mut impl Write,
    id: u32,
    offset: u64,
    digest: &[u8],
    part: &[u8],
) -> VaultResult<()> {
    if digest.len() != 32 || part.is_empty() || part.len() > CHUNK {
        return Err(invalid("Invalid data range"));
    }
    let mut payload = Vec::with_capacity(44 + part.len());
    payload.extend_from_slice(&id.to_be_bytes());
    payload.extend_from_slice(&offset.to_be_bytes());
    payload.extend_from_slice(digest);
    payload.extend_from_slice(part);
    send(stream, Type::Data, &payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_oversize_before_reading_and_preserves_binary_bytes() {
        let header = [7, 0x7f, 0xff, 0xff, 0xff];
        assert!(receive(&mut &header[..]).is_err());
        let mut wire = Vec::new();
        data(&mut wire, 3, 1 << 33, &[4; 32], &[0, 255, 128]).unwrap();
        let f = receive(&mut &wire[..]).unwrap();
        assert_eq!(f.kind, Type::Data);
        assert_eq!(&f.data[44..], &[0, 255, 128]);
        assert!(receive(&mut &[99, 0, 0, 0, 0][..]).is_err());
        assert!(receive(&mut &wire[..wire.len() - 1]).is_err());
    }
}
