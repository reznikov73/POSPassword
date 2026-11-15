//! Обмен сообщениями с рамкой: 4 байта длины (little-endian) и сам текст.
//!
//! Такой же формат использует браузер в Native Messaging и «Хранитель»
//! в именованном канале — одни и те же функции работают и там, и там.

use std::io::Read;

/// Сообщение длиннее мегабайта — это уже не запрос, а ошибка.
/// На не-Windows платформах кодек живёт только ради тестов.
#[cfg_attr(not(windows), allow(dead_code))]
pub const MAX_MESSAGE: usize = 1024 * 1024;

/// Читает одно сообщение. `Ok(None)` — поток закрыт ровно между сообщениями.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn read_frame<R: Read>(reader: &mut R) -> std::io::Result<Option<Vec<u8>>> {
    // Первый байт читаем отдельно, чтобы отличить чистое закрытие потока
    // (обычный конец сеанса) от оборванного сообщения.
    let mut head = [0u8; 1];
    if reader.read(&mut head)? == 0 {
        return Ok(None);
    }
    let mut rest = [0u8; 3];
    reader.read_exact(&mut rest)?;
    let mut length = head[0] as usize;
    for (shift, byte) in rest.iter().enumerate() {
        length |= (*byte as usize) << (8 * (shift + 1));
    }
    if length == 0 {
        return Ok(Some(Vec::new()));
    }
    if length > MAX_MESSAGE {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "сообщение длиннее мегабайта",
        ));
    }
    let mut frame = vec![0u8; length];
    reader.read_exact(&mut frame)?;
    Ok(Some(frame))
}

/// Пишет одно сообщение с рамкой.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn write_frame<W: std::io::Write>(writer: &mut W, data: &[u8]) -> std::io::Result<()> {
    let length = data.len() as u32;
    writer.write_all(&length.to_le_bytes())?;
    writer.write_all(data)?;
    writer.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn frame(data: &[u8]) -> Vec<u8> {
        let mut out = (data.len() as u32).to_le_bytes().to_vec();
        out.extend_from_slice(data);
        out
    }

    #[test]
    fn roundtrip_keeps_messages() {
        let mut buffer = Cursor::new(Vec::new());
        write_frame(&mut buffer, b"ping").expect("запись");
        write_frame(&mut buffer, b"{\"action\":\"logins\"}").expect("запись");
        let mut stream = Cursor::new(buffer.into_inner());
        assert_eq!(
            read_frame(&mut stream).expect("чтение"),
            Some(b"ping".to_vec())
        );
        assert_eq!(
            read_frame(&mut stream).expect("чтение"),
            Some(b"{\"action\":\"logins\"}".to_vec())
        );
        assert_eq!(read_frame(&mut stream).expect("конец"), None);
    }

    #[test]
    fn clean_close_between_messages_is_none() {
        let data = frame(b"hello");
        let mut stream = Cursor::new(data);
        assert_eq!(
            read_frame(&mut stream).expect("чтение"),
            Some(b"hello".to_vec())
        );
        // Поток закончился ровно после сообщения — это не ошибка.
        assert_eq!(read_frame(&mut stream).expect("конец"), None);
    }

    #[test]
    fn truncated_frame_is_an_error() {
        let data = frame(b"hello");
        let mut stream = Cursor::new(&data[..data.len() - 2]);
        assert!(read_frame(&mut stream).is_err());
    }

    #[test]
    fn oversize_frame_is_rejected() {
        let mut data = ((MAX_MESSAGE + 1) as u32).to_le_bytes().to_vec();
        data.extend_from_slice(&[0u8; 8]);
        let mut stream = Cursor::new(data);
        assert!(read_frame(&mut stream).is_err());
    }
}
