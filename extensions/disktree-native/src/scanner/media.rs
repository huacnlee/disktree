//! Whether a volume is on solid-state storage, which decides whether a
//! removal runs on several threads: a rotational disk seeks between them.
//!
//! macOS reads the `Medium Type` IOKit reports for the disk under the
//! volume. Linux reads the block queue's `rotational` flag, through the
//! mount's source device when the volume has no block device of its own
//! (btrfs subvolumes). Anything that cannot be told counts as rotational.

use std::{
    collections::HashMap,
    os::unix::fs::MetadataExt,
    path::Path,
    sync::{LazyLock, Mutex},
};

/// Answers per device, which does not change while it stays mounted.
static KNOWN: LazyLock<Mutex<HashMap<u64, bool>>> = LazyLock::new(Mutex::default);

pub(super) fn solid_state(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    let device = meta.dev();
    if let Some(known) = KNOWN
        .lock()
        .ok()
        .and_then(|known| known.get(&device).copied())
    {
        return known;
    }
    let solid = detect(path, device).unwrap_or(false);
    log::debug!(
        "{} is on {} storage",
        path.display(),
        if solid {
            "solid-state"
        } else {
            "rotational or unknown"
        }
    );
    if let Ok(mut known) = KNOWN.lock() {
        known.insert(device, solid);
    }
    solid
}

#[cfg(target_os = "linux")]
fn detect(_path: &Path, device: u64) -> Option<bool> {
    let flag = |device: u64| -> Option<bool> {
        let (major, minor) = (
            libc::major(device as libc::dev_t),
            libc::minor(device as libc::dev_t),
        );
        let block = std::fs::canonicalize(format!("/sys/dev/block/{major}:{minor}")).ok()?;
        // A partition has no queue of its own; its disk is the parent.
        [Some(block.as_path()), block.parent()]
            .into_iter()
            .flatten()
            .find_map(|dir| std::fs::read_to_string(dir.join("queue/rotational")).ok())
            .map(|text| text.trim() == "0")
    };
    flag(device).or_else(|| flag(std::fs::metadata(mount_source(device)?).ok()?.rdev()))
}

/// The `/dev` node mounted as `device`, from `/proc/self/mountinfo`:
/// `id parent major:minor root point options... - type source super`.
#[cfg(target_os = "linux")]
fn mount_source(device: u64) -> Option<String> {
    let wanted = format!(
        "{}:{}",
        libc::major(device as libc::dev_t),
        libc::minor(device as libc::dev_t)
    );
    let table = std::fs::read_to_string("/proc/self/mountinfo").ok()?;
    table.lines().find_map(|line| {
        let mut fields = line.split(' ');
        if fields.nth(2)? != wanted {
            return None;
        }
        let mut tail = fields.skip_while(|field| *field != "-").skip(2);
        tail.next()
            .filter(|source| source.starts_with("/dev/"))
            .map(String::from)
    })
}

#[cfg(target_os = "macos")]
#[allow(non_upper_case_globals, reason = "names from the IOKit headers")]
fn detect(path: &Path, _device: u64) -> Option<bool> {
    use std::ffi::{CStr, CString, c_char, c_void};
    use std::os::unix::ffi::OsStrExt;

    type CFTypeRef = *const c_void;
    const kCFStringEncodingUTF8: u32 = 0x0800_0100;
    const kIORegistryIterateRecursively: u32 = 1;
    const kIORegistryIterateParents: u32 = 2;
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFStringCreateWithCString(
            alloc: CFTypeRef,
            text: *const c_char,
            encoding: u32,
        ) -> CFTypeRef;
        fn CFGetTypeID(value: CFTypeRef) -> usize;
        fn CFDictionaryGetTypeID() -> usize;
        fn CFDictionaryGetValue(dictionary: CFTypeRef, key: CFTypeRef) -> CFTypeRef;
        fn CFEqual(left: CFTypeRef, right: CFTypeRef) -> u8;
        fn CFRelease(value: CFTypeRef);
    }
    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IOBSDNameMatching(port: u32, options: u32, name: *const c_char) -> CFTypeRef;
        fn IOServiceGetMatchingService(port: u32, matching: CFTypeRef) -> u32;
        fn IORegistryEntrySearchCFProperty(
            entry: u32,
            plane: *const c_char,
            key: CFTypeRef,
            allocator: CFTypeRef,
            options: u32,
        ) -> CFTypeRef;
        fn IOObjectRelease(object: u32) -> i32;
    }
    /// An owned Core Foundation object, released on drop.
    struct Owned(CFTypeRef);
    impl Owned {
        fn new(value: CFTypeRef) -> Option<Self> {
            (!value.is_null()).then_some(Self(value))
        }
        fn text(text: &CStr) -> Option<Self> {
            // SAFETY: `text` is a valid C string; the result is owned.
            Self::new(unsafe {
                CFStringCreateWithCString(std::ptr::null(), text.as_ptr(), kCFStringEncodingUTF8)
            })
        }
    }
    impl Drop for Owned {
        fn drop(&mut self) {
            // SAFETY: `self.0` is a non-null object this value owns.
            unsafe { CFRelease(self.0) };
        }
    }

    // The volume's device node, such as `/dev/disk3s5`.
    let path = CString::new(path.as_os_str().as_bytes()).ok()?;
    // SAFETY: `stat` is plain data the call fills.
    let stat = unsafe {
        let mut stat = std::mem::zeroed::<libc::statfs>();
        (libc::statfs(path.as_ptr(), &mut stat) == 0).then_some(stat)?
    };
    // SAFETY: the kernel NUL-terminates `f_mntfromname`.
    let source = unsafe { CStr::from_ptr(stat.f_mntfromname.as_ptr()) };
    let name = CString::new(source.to_bytes().strip_prefix(b"/dev/")?).ok()?;

    // SAFETY: the matching dictionary is consumed by the lookup; the service
    // is released below; every object created is owned by an `Owned`.
    unsafe {
        let matching = IOBSDNameMatching(0, 0, name.as_ptr());
        if matching.is_null() {
            return None;
        }
        let service = IOServiceGetMatchingService(0, matching);
        if service == 0 {
            return None;
        }
        // The media of an APFS volume sits on its container, which sits on
        // the physical store: the characteristics are on a parent.
        let key = Owned::text(c"Device Characteristics")?;
        let found = IORegistryEntrySearchCFProperty(
            service,
            c"IOService".as_ptr(),
            key.0,
            std::ptr::null(),
            kIORegistryIterateRecursively | kIORegistryIterateParents,
        );
        IOObjectRelease(service);
        let characteristics = Owned::new(found)?;
        if CFGetTypeID(characteristics.0) != CFDictionaryGetTypeID() {
            return None;
        }
        let medium = CFDictionaryGetValue(characteristics.0, Owned::text(c"Medium Type")?.0);
        if medium.is_null() {
            return None;
        }
        let solid = Owned::text(c"Solid State")?;
        let rotational = Owned::text(c"Rotational")?;
        if CFEqual(medium, solid.0) != 0 {
            Some(true)
        } else if CFEqual(medium, rotational.0) != 0 {
            Some(false)
        } else {
            None
        }
    }
}
