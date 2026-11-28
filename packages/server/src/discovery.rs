use serde::{Deserialize, Serialize};
use std::{
    env, fs, io,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Clone, Serialize, Deserialize)]
pub struct Endpoint {
    pub port: u16,
    pub token: String,
}

impl Endpoint {
    pub fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

fn env_path(name: &str) -> io::Result<PathBuf> {
    env::var_os(name)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other(format!("{name} is unset")))
}

#[cfg(windows)]
fn paths() -> io::Result<(PathBuf, PathBuf, PathBuf)> {
    let base = env_path("LOCALAPPDATA")?.join("QO");
    Ok((base.join("run"), base.join("config"), base.join("state")))
}

#[cfg(unix)]
fn paths() -> io::Result<(PathBuf, PathBuf, PathBuf)> {
    let home = env_path("HOME")?;
    let state = env_path("XDG_STATE_HOME")
        .unwrap_or_else(|_| home.join(".local/state"))
        .join("qo");
    let runtime = env_path("XDG_RUNTIME_DIR")
        .map(|p| p.join("qo"))
        .unwrap_or_else(|_| state.join("run"));
    let config = env_path("XDG_CONFIG_HOME")
        .unwrap_or_else(|_| home.join(".config"))
        .join("qo");
    Ok((runtime, config, state))
}

pub fn runtime_dir() -> io::Result<PathBuf> {
    Ok(paths()?.0)
}

pub fn private_dirs() -> io::Result<[PathBuf; 3]> {
    let (run, config, state) = paths()?;
    Ok([run, config, state])
}

pub(crate) fn state_dir() -> io::Result<PathBuf> {
    Ok(paths()?.2)
}

#[cfg(feature = "embedded-web")]
pub(crate) fn workspaces() -> io::Result<PathBuf> {
    #[cfg(windows)]
    let base = env_path("LOCALAPPDATA")?.join("QO");
    #[cfg(unix)]
    let base = env_path("XDG_DATA_HOME")
        .or_else(|_| env_path("HOME").map(|p| p.join(".local/share")))?
        .join("qo");
    Ok(base.join("workspaces"))
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct Settings {
    pub port: u16,
    pub login_link_seconds: u64,
    pub event_retention_days: u64,
    pub event_retention_bytes: u64,
    pub approval_ttl_seconds: u64,
}

impl Default for Settings {
    fn default() -> Self {
        // A stable unprivileged port keeps browser bookmarks usable across restarts.
        // Two minutes leaves time to switch to the browser and bounds a lost link's lifetime.
        Self {
            port: 47261,
            login_link_seconds: 120,
            event_retention_days: 7, // Cover clients offline over a weekend.
            event_retention_bytes: 256 * 1024 * 1024, // Bound disk use from high-frequency output.
            approval_ttl_seconds: 600, // Ten minutes to inspect an operation before renewal.
        }
    }
}

impl Settings {
    pub fn load() -> io::Result<Self> {
        let settings: Self = match fs::read(paths()?.1.join("settings.json")) {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => Self::default(),
            Err(e) => return Err(e),
        };
        if settings.port == 0
            || settings.login_link_seconds == 0
            || std::time::Instant::now()
                .checked_add(Duration::from_secs(settings.login_link_seconds))
                .is_none()
        {
            return Err(io::Error::other("invalid QO port or loginLinkSeconds"));
        }
        if settings.event_retention_days == 0 || settings.event_retention_bytes == 0 {
            return Err(io::Error::other(
                "eventRetentionDays and eventRetentionBytes must be positive",
            ));
        }
        settings.approval_ttl()?;
        Ok(settings)
    }

    pub(crate) fn approval_ttl(&self) -> io::Result<chrono::TimeDelta> {
        i64::try_from(self.approval_ttl_seconds)
            .ok()
            .and_then(chrono::TimeDelta::try_seconds)
            .filter(|d| {
                chrono::Utc::now()
                    .checked_add_signed(*d)
                    .is_some_and(|at| chrono::Datelike::year(&at) <= 9999)
                    && std::time::Instant::now()
                        .checked_add(Duration::from_secs(self.approval_ttl_seconds))
                        .is_some()
            })
            .ok_or_else(|| io::Error::other("invalid approvalTtlSeconds"))
    }
}

pub fn read_desktop_endpoint() -> io::Result<Endpoint> {
    Ok(serde_json::from_slice(&fs::read(
        runtime_dir()?.join("desktop.json"),
    )?)?)
}

pub(crate) fn publish(dir: &Path, name: &str, endpoint: &Endpoint) -> io::Result<()> {
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    serde_json::to_writer(tmp.as_file_mut(), endpoint)?;
    tmp.as_file().sync_all()?;
    tmp.persist(dir.join(name))?;
    Ok(())
}

#[cfg(unix)]
pub(crate) fn private_dir(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)?;
    if !path.symlink_metadata()?.is_dir() {
        return Err(io::Error::other("invalid runtime directory"));
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(windows)]
pub(crate) fn private_dir(path: &Path) -> io::Result<()> {
    use std::{os::windows::ffi::OsStrExt, ptr};
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{
            Authorization::{
                ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
            },
            DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, SetFileSecurityW,
        },
    };
    fs::create_dir_all(path)?;
    if !path.symlink_metadata()?.is_dir() {
        return Err(io::Error::other("invalid runtime directory"));
    }
    // OWNER RIGHTS, inherited by child files and directories. Protect before publishing secrets.
    let sddl: Vec<u16> = "D:P(A;OICI;FA;;;OW)".encode_utf16().chain([0]).collect();
    let path: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    let mut descriptor = ptr::null_mut();
    // SAFETY: both strings are NUL-terminated; Windows allocates the descriptor, freed below.
    unsafe {
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            ptr::null_mut(),
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let ok = SetFileSecurityW(
            path.as_ptr(),
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            descriptor,
        );
        let result = if ok == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        };
        LocalFree(descriptor);
        result
    }
}
