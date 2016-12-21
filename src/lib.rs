extern crate flate2;

use std::fs::File;
use std::io::Read;

pub struct Chunk {
    typ: u32,
    data: Vec<u8>,
}

pub fn eat_u32(i: &mut usize, data: &[u8]) -> u32 {
    let mut b: u32 = 0;
    b |= data[3 + *i] as u32;
    b |= (data[2 + *i] as u32) << 8;
    b |= (data[1 + *i] as u32) << 16;
    b |= (data[0 + *i] as u32) << 24;
    *i += 4;
    return b;
}

pub fn read_chunks(contents: Vec<u8>) -> Vec<Chunk> {
    let mut build: Vec<Chunk> = Vec::new();
    let mut i: usize = 8;
    while i < contents.len() {
        let chunklen: u32 = eat_u32(&mut i, &contents);
        let typ: u32 = eat_u32(&mut i, &contents);
        println!("chunklen is {}, type={:x}", chunklen, typ);
        let bytes: Vec<u8> = Vec::from(&contents[i .. i + (chunklen as usize)]);
        // + 4 to skip past CRC.
        i = i + (chunklen as usize) + 4;
        build.push(Chunk{typ: typ, data: bytes});
        println!("Done read {} bytes", i);
    }
    return build;
}

pub fn read_png(filename: &str) -> Vec<Chunk> {
    let mut file = File::open(filename).unwrap();
    let mut contents: Vec<u8> = Vec::new();
    let result = file.read_to_end(&mut contents).unwrap();

    println!("Read {} bytes", result);

    let chunks: Vec<Chunk> = read_chunks(contents);
    return chunks;
}

#[derive(Default)]
pub struct IHDRInfo {
    width: u32,
    height: u32,
    bit_depth: u8,
    color_type: u8,
    compression_method: u8,
    filter_method: u8,
    interlace_method: u8,
}

pub fn process_ihdr(ch: &Chunk) -> IHDRInfo {
    assert!(ch.data.len() == 13);
    let mut info: IHDRInfo = IHDRInfo::default();
    let mut i: usize = 0;
    info.width = eat_u32(&mut i, &ch.data);
    info.height = eat_u32(&mut i, &ch.data);
    info.bit_depth = ch.data[i];
    info.color_type = ch.data[i + 1];
    info.compression_method = ch.data[i + 2];
    info.filter_method = ch.data[i + 3];
    info.interlace_method = ch.data[i + 4];
    return info;
}

#[derive(Clone, Copy)]
pub struct RGBA { dat: [u8; 4] }
pub struct Row { pixels: Vec<RGBA>, }

const ZERO_RGBA: RGBA = RGBA{dat:[0; 4]};

pub fn convert_to_rows(ihdr: &IHDRInfo, data: Vec<u8>) -> Vec<Row> {
    assert!(ihdr.bit_depth == 8);
    assert!(ihdr.color_type == 2);
    println!("data length: {}, height: {}, width: {}", data.len(), ihdr.height, ihdr.width);
    let pixelsize: usize = 3;
    let row_width: usize = (ihdr.width as usize) * pixelsize + 1;
    let prev_scan_line: Vec<RGBA> = vec![ZERO_RGBA; ihdr.width as usize];

    for rownum in 0 .. ihdr.height as usize {
        let mut i: usize = rownum * row_width;
        let filtertype: u8 = data[i];
        i += 1;

        let mut build: Vec<RGBA> = vec![ZERO_RGBA; ihdr.width as usize];

        let mut prev: RGBA = ZERO_RGBA;
        let mut prev_scan_line_prev: RGBA = ZERO_RGBA;

        for colnum in 0 .. ihdr.width as usize {
            let r: u8 = data[i + colnum*3];
            let g: u8 = data[i + colnum*3 + 1];
            let b: u8 = data[i + colnum*3 + 2];

            let pix: RGBA = RGBA{dat:[r, g, b, 0]};
            let up: RGBA = prev_scan_line[colnum];

            let newpix: RGBA = filter_pixel(filtertype, pix,
                                            prev,
                                            up,
                                            prev_scan_line_prev);

            build[colnum] = newpix;
            prev = pix;
            prev_scan_line_prev = up;
        }
    }
    return Vec::new();
}

pub fn filter_pixel(filtertype: u8, pix: RGBA, left: RGBA, up: RGBA, upleft: RGBA) -> RGBA {
    let mut build: RGBA = ZERO_RGBA;
    for i in 0..3 {
        build.dat[i] = match filtertype {
            0 => pix.dat[i],
            1 => pix.dat[i].wrapping_add(left.dat[i]),
            2 => pix.dat[i].wrapping_add(up.dat[i]),
            3 => pix.dat[i].wrapping_add((((left.dat[i] as u32) + (up.dat[i] as u32)) >> 1) as u8),
            4 => pix.dat[i].wrapping_add(paeth_predictor(left.dat[i] as i32, up.dat[i] as i32, upleft.dat[i] as i32)),
            _ => panic!("bad filtertype")
        }
    }
    return build;
}

use std::num;

pub fn paeth_predictor(a: i32, b: i32, c: i32) -> u8 {
    let p: i32 = a + b - c;
    let pa: i32 = (p - a).abs();
    let pb: i32 = (p - b).abs();
    let pc: i32 = (p - c).abs();
    if pa <= pb && pa <= pc {
        return a as u8;
    } else if pb <= pc {
        return b as u8;
    } else {
        return c as u8;
    }
}

pub fn process_png(png: Vec<Chunk>) {
    let ihdr: IHDRInfo = process_ihdr(&png[0]);

    assert!(ihdr.compression_method == 0);  // assert deflate
    assert!(ihdr.filter_method == 0);  // assert adaptive
    // TODON'T: this.
    assert!(ihdr.interlace_method == 0);  // assert non-interlaced

    let mut compressed_data: Vec<u8> = Vec::new();
    for i in 1..png.len()-1 {
        if png[i].typ == 0x49444154 {
            compressed_data.extend(&png[i].data);
        }
    }

    assert!(png[png.len()-1].typ == 0x49454e44);
    assert!(png[png.len()-1].data.len() == 0);

    let inflated_data: Vec<u8> = inflate_bytes(&compressed_data);

    let rows: Vec<Row> = convert_to_rows(&ihdr, inflated_data);

    let mut num_zeros: usize = 0;
    for row in rows.iter() {
        for pix in row.pixels.iter() {
            if pix.dat[0] == 0 && pix.dat[1] == 0 && pix.dat[2] == 0 {
                num_zeros += 1;
            }
        }
    }

    println!("There were {} zeros.", num_zeros);
}

use flate2::read::*;

pub fn inflate_bytes(b: &[u8]) -> Vec<u8> {
    let mut d = ZlibDecoder::new(b);
    let mut res: Vec<u8> = Vec::new();
    d.read_to_end(&mut res);
    println!("res length = {}", res.len());
    return res;
}

pub fn printbytes(bytes: &[u8]) {
    for i in 0..40 {
        print!("{:02x}", bytes[i]);
    }
    println!("");
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn it_works() {
        let v: Vec<Chunk> = read_png("PNG_demo.png");
        process_png(v);
    }
}
