use std::io::{self, Read, Write};

pub(super) const TEXT: u8 = 0x1;
pub(super) const CLOSE: u8 = 0x8;
const CONTINUATION: u8 = 0x0;
const BINARY: u8 = 0x2;
const PING: u8 = 0x9;
pub(super) const PONG: u8 = 0xa;
const CONTROL: usize = 125;
const MESSAGE: usize = 16 << 20;

pub(super) enum Message {
    Text(String),
    Binary,
    Ping(Vec<u8>),
    Close(Vec<u8>),
}

#[derive(Default)]
pub(super) struct Messages {
    partial: Option<(u8, Vec<u8>)>,
}

fn invalid(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, what.to_owned())
}

impl Messages {
    pub(super) fn next(&mut self, reader: &mut impl Read) -> io::Result<Message> {
        loop {
            let (fin, opcode, payload) = frame(reader)?;
            let (opcode, data) = match opcode {
                TEXT | BINARY if self.partial.is_some() => return Err(invalid("a new message inside a fragmented one")),
                TEXT | BINARY if !fin => {
                    self.partial = Some((opcode, payload));
                    continue;
                }
                TEXT | BINARY => (opcode, payload),
                CONTINUATION => {
                    let (_, data) = self.partial.as_mut().ok_or_else(|| invalid("a continuation without a first frame"))?;
                    if data.len() + payload.len() > MESSAGE {
                        return Err(invalid("the message is too big"));
                    }
                    data.extend_from_slice(&payload);
                    if !fin {
                        continue;
                    }
                    self.partial.take().ok_or_else(|| invalid("a continuation without a first frame"))?
                }
                PING => return Ok(Message::Ping(payload)),
                PONG => continue,
                CLOSE => return Ok(Message::Close(payload)),
                _ => return Err(invalid("an unknown opcode")),
            };
            return Ok(match opcode {
                TEXT => Message::Text(String::from_utf8(data).map_err(|_| invalid("a text message that is not UTF-8"))?),
                _ => Message::Binary,
            });
        }
    }
}

fn frame(reader: &mut impl Read) -> io::Result<(bool, u8, Vec<u8>)> {
    let mut head = [0u8; 2];
    reader.read_exact(&mut head)?;
    if head[1] & 0x80 == 0 {
        return Err(invalid("a client frame without a mask"));
    }
    let length = match head[1] & 0x7f {
        126 => {
            let mut n = [0u8; 2];
            reader.read_exact(&mut n)?;
            u64::from(u16::from_be_bytes(n))
        }
        127 => {
            let mut n = [0u8; 8];
            reader.read_exact(&mut n)?;
            u64::from_be_bytes(n)
        }
        n => u64::from(n),
    };
    if length > MESSAGE as u64 {
        return Err(invalid("the frame is too big"));
    }
    if head[0] & 0x08 != 0 && (head[0] & 0x80 == 0 || length > CONTROL as u64) {
        return Err(invalid("a control frame is fragmented or longer than 125 bytes"));
    }
    let mut key = [0u8; 4];
    reader.read_exact(&mut key)?;
    let mut payload = vec![0u8; length as usize];
    reader.read_exact(&mut payload)?;
    payload.iter_mut().enumerate().for_each(|(i, b)| *b ^= key[i % 4]);
    Ok((head[0] & 0x80 != 0, head[0] & 0x0f, payload))
}

pub(super) fn write(writer: &mut impl Write, opcode: u8, payload: &[u8]) -> io::Result<()> {
    let mut frame = Vec::with_capacity(payload.len() + 10);
    frame.push(0x80 | opcode);
    match payload.len() {
        n if n < 126 => frame.push(n as u8),
        n if n <= 0xffff => {
            frame.push(126);
            frame.extend_from_slice(&(n as u16).to_be_bytes());
        }
        n => {
            frame.push(127);
            frame.extend_from_slice(&(n as u64).to_be_bytes());
        }
    }
    frame.extend_from_slice(payload);
    writer.write_all(&frame)?;
    writer.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn masked(first: u8, payload: &[u8]) -> Vec<u8> {
        let mut frame = vec![first];
        match payload.len() {
            n if n < 126 => frame.push(0x80 | n as u8),
            n => {
                frame.push(0x80 | 126);
                frame.extend_from_slice(&(n as u16).to_be_bytes());
            }
        }
        frame.extend_from_slice(&[0, 0, 0, 0]);
        frame.extend_from_slice(payload);
        frame
    }

    #[test]
    fn control_frames_are_short_and_whole() {
        let next = |bytes: Vec<u8>| Messages::default().next(&mut bytes.as_slice());
        assert!(matches!(next(masked(0x80 | PING, b"hi")), Ok(Message::Ping(p)) if p == b"hi"));
        assert!(matches!(next(masked(0x80 | PING, &[0; 125])), Ok(Message::Ping(_))));
        assert!(next(masked(0x80 | PING, &[0; 126])).is_err());
        assert!(next(masked(PING, b"hi")).is_err());
        assert!(next(masked(CLOSE, &[0x03, 0xe8])).is_err());
        assert!(matches!(next([masked(TEXT, b"a"), masked(0x80, b"b")].concat()), Ok(Message::Text(t)) if t == "ab"));
    }
}
