use crate::allocate::*;
use crate::osc::{base64_decode, base64_encode};
use core::fmt::Formatter;

pub(super) fn get<'a>(keys: &BTreeMap<&str, &'a str>, k: &str) -> Option<&'a str> {
    keys.get(k).copied()
}

pub(super) fn geti<T: core::str::FromStr>(keys: &BTreeMap<&str, &str>, k: &str) -> Option<T> {
    get(keys, k).and_then(|s| s.parse().ok())
}

pub(super) fn set<T: ToString>(
    keys: &mut BTreeMap<&'static str, String>,
    k: &'static str,
    v: &Option<T>,
) {
    if let Some(v) = v {
        keys.insert(k, v.to_string());
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum KittyImageData {
    /// The data bytes, baes64-encoded fragments.
    /// t='d'
    Direct(String),
    DirectBin(Vec<u8>),
    /// The path to a file containing the data.
    /// t='f'
    File {
        path: String,
        /// the amount of data to read.
        /// S=...
        data_size: Option<u32>,
        /// The offset at which to read.
        /// O=...
        data_offset: Option<u32>,
    },
    /// The path to a temporary file containing the data.
    /// If the path is in a known temporary location,
    /// it should be removed once the data has been read
    /// t='t'
    TemporaryFile {
        path: String,
        /// the amount of data to read.
        /// S=...
        data_size: Option<u32>,
        /// The offset at which to read.
        /// O=...
        data_offset: Option<u32>,
    },

    /// The name of a shared memory object.
    /// Can be opened via shm_open() and then should be removed
    /// via shm_unlink().
    /// On Windows, OpenFileMapping(), MapViewOfFile(), UnmapViewOfFile()
    /// and CloseHandle() are used to access and release the data.
    /// t='s'
    SharedMem {
        name: String,
        /// the amount of data to read.
        /// S=...
        data_size: Option<u32>,
        /// The offset at which to read.
        /// O=...
        data_offset: Option<u32>,
    },
}

impl core::fmt::Debug for KittyImageData {
    fn fmt(&self, fmt: &mut Formatter) -> core::fmt::Result {
        match self {
            Self::Direct(data) => write!(fmt, "Direct({} bytes of data)", data.len()),
            Self::DirectBin(data) => write!(fmt, "DirectBin({} bytes of data)", data.len()),
            Self::File {
                path,
                data_offset,
                data_size,
            } => fmt
                .debug_struct("File")
                .field("path", &path)
                .field("data_offset", &data_offset)
                .field("data_size", data_size)
                .finish(),
            Self::TemporaryFile {
                path,
                data_offset,
                data_size,
            } => fmt
                .debug_struct("TemporaryFile")
                .field("path", &path)
                .field("data_offset", &data_offset)
                .field("data_size", data_size)
                .finish(),
            Self::SharedMem {
                name,
                data_offset,
                data_size,
            } => fmt
                .debug_struct("SharedMem")
                .field("name", &name)
                .field("data_offset", &data_offset)
                .field("data_size", data_size)
                .finish(),
        }
    }
}

impl KittyImageData {
    pub(super) fn from_keys(keys: &BTreeMap<&str, &str>, payload: &[u8]) -> Option<Self> {
        let t = get(keys, "t").unwrap_or("d");

        match t {
            "d" => Some(Self::Direct(String::from_utf8(payload.to_vec()).ok()?)),
            "f" => Some(Self::File {
                path: String::from_utf8(base64_decode(payload).ok()?).ok()?,
                data_size: geti(keys, "S"),
                data_offset: geti(keys, "O"),
            }),
            "t" => Some(Self::TemporaryFile {
                path: String::from_utf8(base64_decode(payload).ok()?).ok()?,
                data_size: geti(keys, "S"),
                data_offset: geti(keys, "O"),
            }),
            "s" => Some(Self::SharedMem {
                name: String::from_utf8(base64_decode(payload).ok()?).ok()?,
                data_size: geti(keys, "S"),
                data_offset: geti(keys, "O"),
            }),
            _ => None,
        }
    }

    pub(super) fn to_keys(&self, keys: &mut BTreeMap<&'static str, String>) {
        match self {
            Self::Direct(d) => {
                keys.insert("payload", d.to_string());
            }
            Self::DirectBin(d) => {
                keys.insert("payload", base64_encode(d));
            }
            Self::File {
                path,
                data_offset,
                data_size,
            } => {
                keys.insert("t", "f".to_string());
                keys.insert("payload", base64_encode(path));
                set(keys, "S", data_size);
                set(keys, "S", data_offset);
            }
            Self::TemporaryFile {
                path,
                data_offset,
                data_size,
            } => {
                keys.insert("t", "t".to_string());
                keys.insert("payload", base64_encode(path));
                set(keys, "S", data_size);
                set(keys, "S", data_offset);
            }
            Self::SharedMem {
                name,
                data_offset,
                data_size,
            } => {
                keys.insert("t", "s".to_string());
                keys.insert("payload", base64_encode(name));
                set(keys, "S", data_size);
                set(keys, "S", data_offset);
            }
        }
    }

    /// Take the image data bytes.
    /// This operation is not repeatable as some of the sources require
    /// removing the underlying file or shared memory object as part
    /// of the read operaiton.
    #[cfg(feature = "kitty-shm")]
    pub fn load_data(self) -> std::io::Result<Vec<u8>> {
        use std::io::{Read, Seek};
        fn read_from_file(
            path: &str,
            data_offset: Option<u32>,
            data_size: Option<u32>,
        ) -> std::io::Result<Vec<u8>> {
            let mut f = std::fs::File::open(path)?;
            if let Some(offset) = data_offset {
                f.seek(std::io::SeekFrom::Start(offset.into()))?;
            }
            if let Some(len) = data_size {
                let mut res = vec![0u8; len as usize];
                f.read_exact(&mut res)?;
                Ok(res)
            } else {
                let mut res = vec![];
                f.read_to_end(&mut res)?;
                Ok(res)
            }
        }

        match self {
            Self::Direct(data) => base64_decode(data).map_err(|err| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("base64 decode: {err:#}"),
                )
            }),
            Self::DirectBin(bin) => Ok(bin),
            Self::File {
                path,
                data_offset,
                data_size,
            } => read_from_file(&path, data_offset, data_size),
            Self::TemporaryFile {
                path,
                data_offset,
                data_size,
            } => {
                let data = read_from_file(&path, data_offset, data_size)?;
                // need to sanity check that the path looks like a reasonable
                // temporary directory path before blindly unlinking it here.

                fn looks_like_temp_path(p: &str) -> bool {
                    if p.starts_with("/tmp/")
                        || p.starts_with("/var/tmp/")
                        || p.starts_with("/dev/shm/")
                    {
                        return true;
                    }

                    if let Ok(t) = std::env::var("TMPDIR")
                        && p.starts_with(&t)
                    {
                        return true;
                    }

                    false
                }

                if looks_like_temp_path(&path) {
                    if let Err(err) = std::fs::remove_file(&path) {
                        log::error!(
                            "Unable to remove kitty image protocol temporary file {}: {:#}",
                            path,
                            err
                        );
                    }
                } else {
                    log::warn!(
                        "kitty image protocol temporary file {} isn't in a known \
                                temporary directory; won't try to remove it",
                        path
                    );
                }

                Ok(data)
            }
            Self::SharedMem {
                name,
                data_offset,
                data_size,
            } => read_shared_memory_data(&name, data_offset, data_size),
        }
    }
}

#[cfg(all(feature = "kitty-shm", unix, not(target_os = "android")))]
fn read_shared_memory_data(
    name: &str,
    data_offset: Option<u32>,
    data_size: Option<u32>,
) -> std::result::Result<std::vec::Vec<u8>, std::io::Error> {
    use nix::sys::mman::{shm_open, shm_unlink};
    use std::fs::File;
    use std::io::{Read, Seek};

    let fd = shm_open(
        name,
        nix::fcntl::OFlag::O_RDONLY,
        nix::sys::stat::Mode::empty(),
    )
    .map_err(|_| {
        let err = std::io::Error::last_os_error();
        std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("shm_open {} failed: {:#}", name, err),
        )
    })?;
    let mut f = File::from(fd);
    if let Some(offset) = data_offset {
        f.seek(std::io::SeekFrom::Start(offset.into()))?;
    }
    let data = if let Some(len) = data_size {
        let mut res = vec![0u8; len as usize];
        f.read_exact(&mut res)?;
        res
    } else {
        let mut res = vec![];
        f.read_to_end(&mut res)?;
        res
    };

    if let Err(err) = shm_unlink(name) {
        log::warn!(
            "Unable to unlink kitty image protocol shm file {}: {:#}",
            name,
            err
        );
    }
    Ok(data)
}

#[cfg(all(feature = "kitty-shm", unix, target_os = "android"))]
fn read_shared_memory_data(
    _name: &str,
    _data_offset: Option<u32>,
    _data_size: Option<u32>,
) -> std::result::Result<std::vec::Vec<u8>, std::io::Error> {
    Err(std::io::ErrorKind::Unsupported.into())
}

#[cfg(all(feature = "kitty-shm", windows))]
mod win {
    use winapi::um::handleapi::CloseHandle;
    use winapi::um::memoryapi::{
        FILE_MAP_ALL_ACCESS, MapViewOfFile, OpenFileMappingW, UnmapViewOfFile, VirtualQuery,
    };
    use winapi::um::winnt::{HANDLE, MEMORY_BASIC_INFORMATION};

    struct HandleWrapper {
        handle: HANDLE,
    }

    struct SharedMemObject {
        _handle: HandleWrapper,
        buf: *mut u8,
    }

    impl Drop for HandleWrapper {
        fn drop(&mut self) {
            // SAFETY: `handle` was returned by `OpenFileMappingW` and is owned
            // exclusively by this wrapper, so it is closed exactly once here.
            unsafe {
                CloseHandle(self.handle);
            }
        }
    }

    impl Drop for SharedMemObject {
        fn drop(&mut self) {
            // SAFETY: `buf` was returned by the matching `MapViewOfFile` call
            // and is owned exclusively by this struct, so it is unmapped
            // exactly once here, before `_handle` closes the mapping object.
            unsafe {
                UnmapViewOfFile(self.buf as _);
            }
        }
    }

    /// Convert a rust string to a windows wide string
    fn wide_string(s: &str) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        std::ffi::OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    pub fn read_shared_memory_data(
        name: &str,
        data_offset: Option<u32>,
        data_size: Option<u32>,
    ) -> std::result::Result<std::vec::Vec<u8>, std::io::Error> {
        let wide_name = wide_string(name);

        // SAFETY: `wide_name` is a valid null-terminated UTF-16 string owned
        // by this function for the duration of the call; failure is reported
        // via a null return and handled below, not through UB.
        let handle = unsafe { OpenFileMappingW(FILE_MAP_ALL_ACCESS, 0, wide_name.as_ptr()) };
        if handle.is_null() {
            let err = std::io::Error::last_os_error();
            return Err(std::io::Error::other(format!(
                "OpenFileMappingW {} failed: {:#}",
                name, err
            )));
        }

        let handle_wrapper = HandleWrapper { handle };
        // SAFETY: `handle_wrapper.handle` was just verified non-null above,
        // so it is a valid file-mapping handle; mapping the whole object
        // (size 0) is a plain FFI call, failure reported via null return.
        let buf = unsafe { MapViewOfFile(handle_wrapper.handle, FILE_MAP_ALL_ACCESS, 0, 0, 0) };
        if buf.is_null() {
            let err = std::io::Error::last_os_error();
            return Err(std::io::Error::other(format!(
                "MapViewOfFile failed: {:#}",
                err
            )));
        }

        let shm = SharedMemObject {
            _handle: handle_wrapper,
            buf: buf as *mut u8,
        };

        let mut memory_info = MEMORY_BASIC_INFORMATION::default();
        // SAFETY: `shm.buf` is a valid pointer into the view mapped above and
        // is not dereferenced here (VirtualQuery only inspects the address
        // range); `memory_info` is a live, correctly-sized out-parameter
        // matching `MEMORY_BASIC_INFORMATION`'s layout.
        let res = unsafe {
            VirtualQuery(
                shm.buf as _,
                &mut memory_info as *mut MEMORY_BASIC_INFORMATION,
                std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
            )
        };
        if res == 0 {
            let err = std::io::Error::last_os_error();
            return Err(std::io::Error::other(format!(
                "Can't get the size of Shared Memory, VirtualQuery failed: {:#}",
                err
            )));
        }
        let mut size = memory_info.RegionSize;
        let offset = data_offset.unwrap_or(0) as usize;
        if offset >= size {
            return Err(std::io::Error::other(format!(
                "offset {} bigger than or equal to shm region size {}",
                offset, size
            )));
        }
        size = size.saturating_sub(offset);
        if let Some(val) = data_size {
            size = size.min(val as usize);
        }
        // SAFETY: `offset < size` was checked above and `size` was clamped to
        // the region reported by `VirtualQuery`, so `[buf+offset, buf+offset+size)`
        // lies entirely within the mapped view; `shm` is still alive (dropped
        // only after this function returns), keeping the mapping valid.
        let buf_slice = unsafe { std::slice::from_raw_parts(shm.buf.add(offset), size) };
        let data = buf_slice.to_vec();

        Ok(data)
    }
}

#[cfg(all(feature = "kitty-shm", windows))]
use win::read_shared_memory_data;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum KittyImageVerbosity {
    Verbose,
    OnlyErrors,
    Quiet,
}

impl KittyImageVerbosity {
    pub(super) fn from_keys(keys: &BTreeMap<&str, &str>) -> Option<Self> {
        match get(keys, "q") {
            None | Some("0") => Some(Self::Verbose),
            Some("1") => Some(Self::OnlyErrors),
            Some("2") => Some(Self::Quiet),
            _ => None,
        }
    }

    pub(super) fn to_keys(self, keys: &mut BTreeMap<&'static str, String>) {
        match self {
            Self::Verbose => {}
            Self::OnlyErrors => {
                keys.insert("q", "1".to_string());
            }
            Self::Quiet => {
                keys.insert("q", "2".to_string());
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KittyImageFormat {
    /// f=24
    Rgb,
    /// f=32
    Rgba,
    /// f=100
    Png,
}

impl KittyImageFormat {
    pub(super) fn from_keys(keys: &BTreeMap<&str, &str>) -> Option<Option<Self>> {
        match get(keys, "f") {
            None => Some(None),
            Some("32") => Some(Some(Self::Rgba)),
            Some("24") => Some(Some(Self::Rgb)),
            Some("100") => Some(Some(Self::Png)),
            _ => None,
        }
    }

    pub(super) fn to_keys(&self, keys: &mut BTreeMap<&'static str, String>) {
        match self {
            Self::Rgb => keys.insert("f", "24".to_string()),
            Self::Rgba => keys.insert("f", "32".to_string()),
            Self::Png => keys.insert("f", "100".to_string()),
        };
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KittyImageCompression {
    None,
    /// o='z'
    Deflate,
}

impl KittyImageCompression {
    pub(super) fn from_keys(keys: &BTreeMap<&str, &str>) -> Option<Self> {
        match get(keys, "o") {
            None => Some(Self::None),
            Some("z") => Some(Self::Deflate),
            _ => None,
        }
    }

    pub(super) fn to_keys(&self, keys: &mut BTreeMap<&'static str, String>) {
        match self {
            Self::None => {}
            Self::Deflate => {
                keys.insert("o", "z".to_string());
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KittyImageTransmit {
    /// f=...
    pub format: Option<KittyImageFormat>,
    /// combination of t=... and d=...
    pub data: KittyImageData,
    /// s=...
    pub width: Option<u32>,
    /// v=...
    pub height: Option<u32>,
    /// The image id.
    /// i=...
    pub image_id: Option<u32>,
    /// The image number
    /// I=...
    pub image_number: Option<u32>,
    /// o=...
    pub compression: KittyImageCompression,

    /// m=0 or m=1
    pub more_data_follows: bool,
}

impl KittyImageTransmit {
    pub(super) fn from_keys(keys: &BTreeMap<&str, &str>, payload: &[u8]) -> Option<Self> {
        Some(Self {
            format: KittyImageFormat::from_keys(keys)?,
            data: KittyImageData::from_keys(keys, payload)?,
            compression: KittyImageCompression::from_keys(keys)?,
            width: geti(keys, "s"),
            height: geti(keys, "v"),
            image_id: geti(keys, "i"),
            image_number: geti(keys, "I"),
            more_data_follows: match get(keys, "m") {
                None | Some("0") => false,
                Some("1") => true,
                _ => return None,
            },
        })
    }

    pub(super) fn to_keys(&self, keys: &mut BTreeMap<&'static str, String>) {
        if let Some(f) = &self.format {
            f.to_keys(keys);
        }

        set(keys, "s", &self.width);
        set(keys, "v", &self.height);
        set(keys, "i", &self.image_id);
        set(keys, "I", &self.image_number);
        if self.more_data_follows {
            keys.insert("m", "1".to_string());
        }

        self.compression.to_keys(keys);
        self.data.to_keys(keys);
    }
}
