use std::path::Path;

use crate::update::download::UpdateDiskSpace;

pub struct DesktopNativeUpdateSpace;

impl UpdateDiskSpace for DesktopNativeUpdateSpace {
    #[cfg(unix)]
    fn available(&self, path: &Path) -> Result<u64, std::io::Error> {
        let volume = rustix::fs::statvfs(path).map_err(std::io::Error::from)?;
        let blocks = u64::try_from(volume.f_bavail).map_err(std::io::Error::other)?;
        let size = u64::try_from(volume.f_frsize).map_err(std::io::Error::other)?;
        blocks
            .checked_mul(size)
            .ok_or_else(|| std::io::Error::other("the free space overflows"))
    }

    #[cfg(windows)]
    fn available(&self, path: &Path) -> Result<u64, std::io::Error> {
        let text = path
            .to_str()
            .ok_or_else(|| std::io::Error::other("the update cache path is not valid text"))?;
        let root = winsafe::GetVolumePathName(text).map_err(std::io::Error::other)?;
        let mut available = 0_u64;
        winsafe::GetDiskFreeSpaceEx(Some(&root), Some(&mut available), None, None)
            .map_err(std::io::Error::other)?;
        Ok(available)
    }
}
