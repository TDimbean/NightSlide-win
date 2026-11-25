use std::fmt;
use windows_result::{Error, HRESULT, Result};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_BINARY, RegCloseKey,
    RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};

/// Registry subkey for Night Light settings, relative to `HKEY_CURRENT_USER`.
pub const SETTINGS_KEY_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\CloudStore\Store\DefaultAccount\Current\default$windows.data.bluelightreduction.settings\windows.data.bluelightreduction.settings";

/// Registry subkey for Night Light state, relative to `HKEY_CURRENT_USER`.
pub const STATE_KEY_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\CloudStore\Store\DefaultAccount\Current\default$windows.data.bluelightreduction.bluelightreductionstate\windows.data.bluelightreduction.bluelightreductionstate";

const VALUE_NAME: &str = "Data";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Key {
    Settings,
    State,
}

impl Key {
    pub(crate) fn path(self) -> &'static str {
        match self {
            Self::Settings => SETTINGS_KEY_PATH,
            Self::State => STATE_KEY_PATH,
        }
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Settings => "Night Light settings",
            Self::State => "Night Light state",
        })
    }
}

pub(crate) trait Store {
    fn read(&self, key: Key) -> windows_result::Result<Vec<u8>>;
    fn write(&self, key: Key, data: &[u8]) -> windows_result::Result<()>;
}

#[derive(Debug)]
pub(crate) struct RegistryStore;

impl Store for RegistryStore {
    fn read(&self, key: Key) -> Result<Vec<u8>> {
        let handle = OpenKey::new(key, KEY_QUERY_VALUE)?;
        let name = wide(VALUE_NAME);
        let mut size = 0;
        let mut kind = 0;
        // SAFETY: The handle and terminated name are valid, and the first call only queries size.
        let code = unsafe {
            RegQueryValueExW(
                handle.0,
                name.as_ptr(),
                std::ptr::null(),
                &mut kind,
                std::ptr::null_mut(),
                &mut size,
            )
        };
        check(code)?;
        if kind != REG_BINARY {
            return Err(win32_error(13));
        }
        let mut data = vec![0; size as usize];
        // SAFETY: data has size bytes of writable storage and remains live for the call.
        let code = unsafe {
            RegQueryValueExW(
                handle.0,
                name.as_ptr(),
                std::ptr::null(),
                &mut kind,
                data.as_mut_ptr(),
                &mut size,
            )
        };
        check(code)?;
        if kind != REG_BINARY {
            return Err(win32_error(13));
        }
        data.truncate(size as usize);
        Ok(data)
    }

    fn write(&self, key: Key, data: &[u8]) -> Result<()> {
        let handle = OpenKey::new(key, KEY_SET_VALUE)?;
        let name = wide(VALUE_NAME);
        let size = u32::try_from(data.len()).map_err(|_| win32_error(13))?;
        // SAFETY: The handle is open for writes and the byte slice is live for the call.
        check(unsafe {
            RegSetValueExW(handle.0, name.as_ptr(), 0, REG_BINARY, data.as_ptr(), size)
        })
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn win32_error(code: u32) -> Error {
    Error::from_hresult(HRESULT::from_win32(code))
}

fn check(code: u32) -> Result<()> {
    if code == 0 {
        Ok(())
    } else {
        Err(win32_error(code))
    }
}

struct OpenKey(HKEY);

impl OpenKey {
    fn new(key: Key, access: u32) -> Result<Self> {
        let path = wide(key.path());
        let mut handle = std::ptr::null_mut();
        // SAFETY: The path is terminated and handle points to writable storage.
        check(unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, path.as_ptr(), 0, access, &mut handle) })?;
        Ok(Self(handle))
    }
}

impl Drop for OpenKey {
    fn drop(&mut self) {
        // SAFETY: This handle came from a successful RegOpenKeyExW call.
        unsafe { RegCloseKey(self.0) };
    }
}

#[cfg(test)]
pub(crate) mod memory {
    use super::Key;
    use super::Store;
    use std::collections::HashMap;
    use std::collections::VecDeque;
    use std::sync::Arc;
    use std::sync::Mutex;
    use windows_result::WIN32_ERROR;

    const ERROR_FILE_NOT_FOUND: WIN32_ERROR = WIN32_ERROR(2);

    #[derive(Debug, Clone, Default)]
    pub(crate) struct MemoryStore(Arc<Mutex<Inner>>);

    #[derive(Debug, Default)]
    struct Inner {
        values: HashMap<Key, Vec<u8>>,
        queued_reads: HashMap<Key, VecDeque<Result<Vec<u8>, WIN32_ERROR>>>,
        queued_writes: HashMap<Key, VecDeque<Result<(), WIN32_ERROR>>>,
        reads: Vec<Key>,
        writes: Vec<(Key, Vec<u8>)>,
    }

    impl MemoryStore {
        pub(crate) fn with(values: &[(Key, &[u8])]) -> Self {
            let store = Self::default();
            store.lock().values = values
                .iter()
                .map(|(key, data)| (*key, data.to_vec()))
                .collect();
            store
        }

        pub(crate) fn value(&self, key: Key) -> Option<Vec<u8>> {
            self.lock().values.get(&key).cloned()
        }

        pub(crate) fn queue_reads(&self, key: Key, results: Vec<Result<Vec<u8>, WIN32_ERROR>>) {
            self.lock()
                .queued_reads
                .entry(key)
                .or_default()
                .extend(results);
        }

        pub(crate) fn reads(&self) -> Vec<Key> {
            self.lock().reads.clone()
        }

        pub(crate) fn writes(&self) -> Vec<Key> {
            self.lock().writes.iter().map(|(key, _)| *key).collect()
        }

        pub(crate) fn write_attempts(&self) -> Vec<(Key, Vec<u8>)> {
            self.lock().writes.clone()
        }

        pub(crate) fn queue_writes(&self, key: Key, results: Vec<Result<(), WIN32_ERROR>>) {
            self.lock()
                .queued_writes
                .entry(key)
                .or_default()
                .extend(results);
        }

        fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
            self.0.lock().expect("memory store lock is not poisoned")
        }
    }

    impl Store for MemoryStore {
        fn read(&self, key: Key) -> windows_result::Result<Vec<u8>> {
            let mut inner = self.lock();
            inner.reads.push(key);
            if let Some(result) = inner
                .queued_reads
                .get_mut(&key)
                .and_then(VecDeque::pop_front)
            {
                return result.map_err(Into::into);
            }
            inner
                .values
                .get(&key)
                .cloned()
                .ok_or_else(|| ERROR_FILE_NOT_FOUND.into())
        }

        fn write(&self, key: Key, data: &[u8]) -> windows_result::Result<()> {
            let mut inner = self.lock();
            inner.writes.push((key, data.to_vec()));
            if let Some(result) = inner
                .queued_writes
                .get_mut(&key)
                .and_then(VecDeque::pop_front)
            {
                result?;
            }
            inner.values.insert(key, data.to_vec());
            Ok(())
        }
    }
}
