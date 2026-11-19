use super::super::{CaptureEvent, CaptureSpec};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::io::{self, Read, Write};

const MAX_FRAME: usize = 64 * 1024;
const VERSION: u8 = 1;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope<T> {
    version: u8,
    payload: T,
}

#[derive(Serialize, Deserialize)]
pub(super) enum Command {
    Configure(CaptureSpec),
    Freeze,
    ParameterBoundary,
}
#[derive(Serialize, Deserialize)]
pub(super) enum Reply {
    Ready,
    Event(CaptureEvent),
    Frozen { last_sequence: Option<u64> },
    Error(String),
}

pub(super) fn write_frame<T: Serialize>(writer: &mut impl Write, value: &T) -> io::Result<()> {
    let bytes = serde_json::to_vec(&Envelope {
        version: VERSION,
        payload: value,
    })
    .map_err(io::Error::other)?;
    if bytes.len() > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame too large",
        ));
    }
    writer.write_all(&(bytes.len() as u32).to_le_bytes())?;
    writer.write_all(&bytes)?;
    writer.flush()
}

pub(super) fn read_frame<T: DeserializeOwned>(reader: &mut impl Read) -> io::Result<Option<T>> {
    let mut header = [0; 4];
    match reader.read(&mut header[..1])? {
        0 => return Ok(None),
        1 => {}
        _ => unreachable!(),
    }
    reader.read_exact(&mut header[1..])?;
    let len = u32::from_le_bytes(header) as usize;
    if len == 0 || len > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid frame length",
        ));
    }
    let mut bytes = vec![0; len];
    reader.read_exact(&mut bytes)?;
    let envelope: Envelope<T> = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
    if envelope.version != VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unsupported recording helper version",
        ));
    }
    Ok(Some(envelope.payload))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn framing_rejects_truncated_and_oversized_input() {
        assert!(read_frame::<Command>(&mut &[1, 0, 0, 0][..]).is_err());
        assert!(read_frame::<Command>(&mut &((MAX_FRAME as u32 + 1).to_le_bytes())[..]).is_err());
        assert!(read_frame::<Command>(&mut &[][..]).unwrap().is_none());
    }
    #[test]
    fn freeze_round_trips() {
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &Command::Freeze).unwrap();
        assert!(matches!(
            read_frame::<Command>(&mut &bytes[..]).unwrap(),
            Some(Command::Freeze)
        ));
    }
    #[test]
    fn multiple_commands_preserve_order_and_eof() {
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &Command::ParameterBoundary).unwrap();
        write_frame(&mut bytes, &Command::ParameterBoundary).unwrap();
        write_frame(&mut bytes, &Command::Freeze).unwrap();
        let mut input = &bytes[..];
        assert!(matches!(
            read_frame::<Command>(&mut input).unwrap(),
            Some(Command::ParameterBoundary)
        ));
        assert!(matches!(
            read_frame::<Command>(&mut input).unwrap(),
            Some(Command::ParameterBoundary)
        ));
        assert!(matches!(
            read_frame::<Command>(&mut input).unwrap(),
            Some(Command::Freeze)
        ));
        assert!(read_frame::<Command>(&mut input).unwrap().is_none());
    }

    #[test]
    fn unsupported_version_cannot_be_used_as_a_helper_command() {
        let payload = serde_json::to_vec(&Envelope {
            version: VERSION + 1,
            payload: Command::Freeze,
        })
        .unwrap();
        let mut bytes = (payload.len() as u32).to_le_bytes().to_vec();
        bytes.extend(payload);
        assert_eq!(
            read_frame::<Command>(&mut &bytes[..]).err().unwrap().kind(),
            io::ErrorKind::InvalidData
        );
    }
}
