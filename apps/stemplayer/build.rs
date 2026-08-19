fn main() {
    slint_build::compile("ui/main.slint").expect("Slint UI must compile");
    #[cfg(target_os = "windows")]
    build_windows_icon();
}

#[cfg(target_os = "windows")]
fn build_windows_icon() {
    use std::{env, fs, path::PathBuf};

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo must provide OUT_DIR"));
    let icon_path = out_dir.join("stemplayer.ico");
    fs::write(&icon_path, icon_bytes()).expect("Stemplayer icon must be writable");
    let resource_path = out_dir.join("stemplayer.rc");
    let icon_path = icon_path.to_string_lossy().replace('\\', "/");
    fs::write(&resource_path, format!("1 ICON \"{icon_path}\"\n"))
        .expect("Windows resource file must be writable");
    embed_resource::compile(&resource_path, embed_resource::NONE)
        .manifest_optional()
        .expect("Stemplayer icon must be embedded");
}

#[cfg(target_os = "windows")]
fn icon_bytes() -> Vec<u8> {
    const SIZES: [u32; 4] = [16, 32, 48, 256];
    let images = SIZES.into_iter().map(icon_image).collect::<Vec<_>>();
    let mut bytes =
        Vec::with_capacity(6 + images.len() * 16 + images.iter().map(Vec::len).sum::<usize>());
    bytes.extend_from_slice(&[0, 0, 1, 0, 4, 0]);
    let mut offset = 6_u32 + 16 * images.len() as u32;
    for (size, image) in SIZES.into_iter().zip(&images) {
        bytes.extend_from_slice(&[
            if size == 256 { 0 } else { size as u8 },
            if size == 256 { 0 } else { size as u8 },
            0,
            0,
        ]);
        bytes.extend_from_slice(&[1, 0, 32, 0]);
        bytes.extend_from_slice(&(image.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&offset.to_le_bytes());
        offset = offset.saturating_add(image.len() as u32);
    }
    for image in images {
        bytes.extend_from_slice(&image);
    }
    bytes
}

#[cfg(target_os = "windows")]
fn icon_image(size: u32) -> Vec<u8> {
    let mask_row_bytes = size.div_ceil(32) * 4;
    let image_size = 40 + size * size * 4 + mask_row_bytes * size;
    let mut bytes = Vec::with_capacity(image_size as usize);
    bytes.extend_from_slice(&40_u32.to_le_bytes());
    bytes.extend_from_slice(&(size as i32).to_le_bytes());
    bytes.extend_from_slice(&(size.saturating_mul(2) as i32).to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&32_u16.to_le_bytes());
    bytes.extend_from_slice(&[0; 24]);
    for y in (0..size).rev() {
        for x in 0..size {
            let [red, green, blue, alpha] = icon_pixel(x, y, size);
            bytes.extend_from_slice(&[blue, green, red, alpha]);
        }
    }
    bytes.resize(bytes.len() + (mask_row_bytes * size) as usize, 0);
    bytes
}

#[cfg(target_os = "windows")]
fn icon_pixel(x: u32, y: u32, size: u32) -> [u8; 4] {
    let center = size / 2;
    let radius = size.saturating_mul(7) / 16;
    let dx = x as i32 - center as i32;
    let dy = y as i32 - center as i32;
    if dx.saturating_mul(dx) + dy.saturating_mul(dy) > (radius * radius) as i32 {
        return [0, 0, 0, 0];
    }
    let bar_width = (size / 12).max(1);
    let bar_left = size * 5 / 16;
    let bar_step = (size * 3 / 32).max(1);
    let bar_index = x.saturating_sub(bar_left) / bar_step;
    if bar_index < 4
        && x >= bar_left + bar_index * bar_step
        && x < bar_left + bar_index * bar_step + bar_width
        && y >= size * (5 + bar_index) / 16
        && y < size * 11 / 16
    {
        return match bar_index {
            0 => [0, 167, 189, 255],
            1 => [212, 134, 18, 255],
            2 => [54, 168, 87, 255],
            _ => [207, 77, 139, 255],
        };
    }
    [22, 31, 43, 255]
}
