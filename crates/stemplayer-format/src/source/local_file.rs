use std::fs::{File, Metadata, OpenOptions};
use std::io;
use std::path::Path;

#[cfg(windows)]
pub(super) fn open(path: &Path) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;

    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    const SECURITY_ANONYMOUS: u32 = 0;

    OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .security_qos_flags(SECURITY_ANONYMOUS)
        .open(path)
}

#[cfg(target_os = "linux")]
pub(super) fn open(path: &Path) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;

    #[cfg(any(
        target_arch = "arm",
        target_arch = "aarch64",
        target_arch = "powerpc",
        target_arch = "powerpc64",
        target_arch = "m68k"
    ))]
    const O_NOFOLLOW: i32 = 0x8000;
    #[cfg(not(any(
        target_arch = "arm",
        target_arch = "aarch64",
        target_arch = "powerpc",
        target_arch = "powerpc64",
        target_arch = "m68k"
    )))]
    const O_NOFOLLOW: i32 = 0x20000;
    #[cfg(any(
        target_arch = "mips",
        target_arch = "mips64",
        target_arch = "mips32r6",
        target_arch = "mips64r6"
    ))]
    const O_NONBLOCK: i32 = 128;
    #[cfg(any(target_arch = "sparc", target_arch = "sparc64"))]
    const O_NONBLOCK: i32 = 0x4000;
    #[cfg(not(any(
        target_arch = "mips",
        target_arch = "mips64",
        target_arch = "mips32r6",
        target_arch = "mips64r6",
        target_arch = "sparc",
        target_arch = "sparc64"
    )))]
    const O_NONBLOCK: i32 = 2048;

    OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW | O_NONBLOCK)
        .open(path)
}

#[cfg(target_os = "macos")]
pub(super) fn open(path: &Path) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;

    const O_NOFOLLOW: i32 = 0x100;
    const O_NONBLOCK: i32 = 0x4;

    OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW | O_NONBLOCK)
        .open(path)
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
pub(super) fn open(_path: &Path) -> io::Result<File> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Secure local file opening is unavailable on this platform",
    ))
}

#[cfg(windows)]
pub(super) fn file_key(file: &File, _metadata: &Metadata) -> io::Result<(u64, u64)> {
    if !winapi_util::file::typ(file)?.is_disk() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Local source is not a disk file",
        ));
    }
    let info = winapi_util::file::information(file)?;
    if info.file_index() == 0 {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Local source has no stable file identifier",
        ));
    }
    Ok((info.volume_serial_number(), info.file_index()))
}

#[cfg(unix)]
pub(super) fn file_key(_file: &File, metadata: &Metadata) -> io::Result<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;

    Ok((metadata.dev(), metadata.ino()))
}

#[cfg(not(any(windows, unix)))]
pub(super) fn file_key(_file: &File, _metadata: &Metadata) -> io::Result<(u64, u64)> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Stable local file identity is unavailable on this platform",
    ))
}
