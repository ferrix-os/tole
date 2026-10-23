//! A pixmap as a PNG: eight-bit RGBA, unpremultiplied, no filter, the
//! image data deflated by `miniz_oxide`. Frames of one size as an
//! animated PNG (APNG), which browsers play and other readers show as
//! its first frame.

/// `pixels` (premultiplied RGBA, `width` × `height`) as a PNG file.
#[must_use]
pub fn encode(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
    let zlib = deflate(width, height, pixels);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(&mut out, b"IHDR", &header(width, height));
    chunk(&mut out, b"IDAT", &zlib);
    chunk(&mut out, b"IEND", &[]);
    out
}

/// `frames` (premultiplied RGBA, each `width` × `height`) as an animated
/// PNG, each shown for `delay_ms`, looping for ever.
#[must_use]
pub fn encode_animated(width: u32, height: u32, frames: &[Vec<u8>], delay_ms: u16) -> Vec<u8> {
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(&mut out, b"IHDR", &header(width, height));
    let count = u32::try_from(frames.len()).unwrap_or(u32::MAX);
    let mut actl = count.to_be_bytes().to_vec();
    actl.extend(0_u32.to_be_bytes());
    chunk(&mut out, b"acTL", &actl);
    let mut sequence = 0_u32;
    for (index, frame) in frames.iter().enumerate() {
        let mut fctl = sequence.to_be_bytes().to_vec();
        sequence += 1;
        fctl.extend(width.to_be_bytes());
        fctl.extend(height.to_be_bytes());
        fctl.extend(0_u32.to_be_bytes());
        fctl.extend(0_u32.to_be_bytes());
        fctl.extend(delay_ms.to_be_bytes());
        fctl.extend(1000_u16.to_be_bytes());
        // Each frame replaces the whole canvas.
        fctl.extend([0, 0]);
        chunk(&mut out, b"fcTL", &fctl);
        let zlib = deflate(width, height, frame);
        if index == 0 {
            chunk(&mut out, b"IDAT", &zlib);
        } else {
            let mut data = sequence.to_be_bytes().to_vec();
            sequence += 1;
            data.extend(zlib);
            chunk(&mut out, b"fdAT", &data);
        }
    }
    chunk(&mut out, b"IEND", &[]);
    out
}

/// The `IHDR` of an eight-bit RGBA image.
fn header(width: u32, height: u32) -> Vec<u8> {
    let mut header = Vec::with_capacity(13);
    header.extend(width.to_be_bytes());
    header.extend(height.to_be_bytes());
    header.extend([8, 6, 0, 0, 0]);
    header
}

/// `pixels` as rows of unpremultiplied RGBA, each led by filter 0,
/// deflated.
fn deflate(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
    let row = width as usize * 4;
    let mut raw = Vec::with_capacity((row + 1) * height as usize);
    for line in pixels.chunks_exact(row.max(1)).take(height as usize) {
        raw.push(0);
        for pixel in line.chunks_exact(4) {
            if let [r, g, b, a] = *pixel {
                raw.extend(unpremultiply([r, g, b, a]));
            }
        }
    }
    miniz_oxide::deflate::compress_to_vec_zlib(&raw, 6)
}

/// One premultiplied pixel back to straight alpha.
fn unpremultiply([r, g, b, a]: [u8; 4]) -> [u8; 4] {
    if a == 0 || a == 255 {
        return [r, g, b, a];
    }
    let undo =
        |channel: u8| ((u32::from(channel) * 255 + u32::from(a) / 2) / u32::from(a)).min(255) as u8;
    [undo(r), undo(g), undo(b), a]
}

/// A chunk: its length, type, data and CRC.
fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend((data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc = 0xffff_ffff_u32;
    for &byte in kind.iter().chain(data) {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    out.extend((!crc).to_be_bytes());
}
