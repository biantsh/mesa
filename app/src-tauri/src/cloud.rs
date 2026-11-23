use std::{fs, path::Path};

pub fn is_cloud_only(metadata: &fs::Metadata) -> bool {
    imp::is_cloud_only(metadata)
}

pub fn is_remote(path: &Path) -> bool {
    imp::is_remote(path)
}

pub fn is_cloud_only_path(path: &Path) -> bool {
    is_remote(path) || fs::metadata(path).is_ok_and(|metadata| is_cloud_only(&metadata))
}

#[cfg(windows)]
mod imp {
    use std::{fs, os::windows::fs::MetadataExt, path::Path};

    const OFFLINE: u32 = 0x0000_1000;
    const RECALL_ON_OPEN: u32 = 0x0004_0000;
    const RECALL_ON_DATA_ACCESS: u32 = 0x0040_0000;

    pub fn is_cloud_only(metadata: &fs::Metadata) -> bool {
        metadata.file_attributes() & (OFFLINE | RECALL_ON_OPEN | RECALL_ON_DATA_ACCESS) != 0
    }

    pub fn is_remote(_: &Path) -> bool {
        false
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::{fs, os::macos::fs::MetadataExt, path::Path};

    const SF_DATALESS: u32 = 0x4000_0000;

    pub fn is_cloud_only(metadata: &fs::Metadata) -> bool {
        metadata.st_flags() & SF_DATALESS != 0
    }

    pub fn is_remote(_: &Path) -> bool {
        false
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::{Mutex, PoisonError},
        time::{Duration, Instant},
    };

    const FRESH: Duration = Duration::from_secs(5);

    static MOUNTS: Mutex<Option<(Instant, Vec<(PathBuf, bool)>)>> = Mutex::new(None);

    pub fn is_cloud_only(_: &fs::Metadata) -> bool {
        false
    }

    pub fn is_remote(path: &Path) -> bool {
        let mut mounts = MOUNTS.lock().unwrap_or_else(PoisonError::into_inner);
        if mounts.as_ref().is_none_or(|(read, _)| read.elapsed() > FRESH) {
            let list = fs::read_to_string("/proc/self/mounts").unwrap_or_default();
            *mounts = Some((Instant::now(), super::mounts_in(&list)));
        }
        mounts
            .as_ref()
            .is_some_and(|(_, table)| super::is_on_remote(table, path))
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
mod imp {
    use std::{fs, path::Path};

    pub fn is_cloud_only(_: &fs::Metadata) -> bool {
        false
    }

    pub fn is_remote(_: &Path) -> bool {
        false
    }
}

#[cfg(any(target_os = "linux", test))]
const REMOTE_KINDS: &[&str] = &[
    "nfs", "nfs4", "cifs", "smb3", "smbfs", "ncpfs", "afs", "ceph", "davfs", "coda",
];

#[cfg(any(target_os = "linux", test))]
const REMOTE_FUSE: &[&str] = &[
    "sshfs",
    "rclone",
    "onedriver",
    "gvfsd-fuse",
    "s3fs",
    "gcsfuse",
    "google-drive-ocamlfuse",
    "curlftpfs",
    "davfs",
    "glusterfs",
    "ceph-fuse",
    "goofys",
    "blobfuse",
    "blobfuse2",
];

#[cfg(any(target_os = "linux", test))]
fn mounts_in(list: &str) -> Vec<(std::path::PathBuf, bool)> {
    list.lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let point = fields.nth(1)?;
            let kind = fields.next()?;
            let remote = match kind.strip_prefix("fuse.") {
                Some(name) => REMOTE_FUSE.contains(&name),
                None => REMOTE_KINDS.contains(&kind),
            };
            Some((crate::browse::unescape_mount_point(point), remote))
        })
        .collect()
}

#[cfg(any(target_os = "linux", test))]
fn is_on_remote(mounts: &[(std::path::PathBuf, bool)], path: &Path) -> bool {
    mounts
        .iter()
        .filter(|(point, _)| path.starts_with(point))
        .max_by_key(|(point, _)| point.as_os_str().len())
        .is_some_and(|&(_, remote)| remote)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOUNTS: &str = "\
/dev/nvme0n1p2 / ext4 rw,relatime 0 0
/dev/nvme0n1p1 /boot/efi vfat rw 0 0
gvfsd-fuse /run/user/1000/gvfs fuse.gvfsd-fuse rw,nosuid,nodev 0 0
remote: /home/me/Google\\040Drive fuse.rclone rw 0 0
server:/share /mnt/share nfs4 rw 0 0
/dev/sdb1 /media/me/USB fuseblk rw 0 0
vault /home/me/Vaults/Private fuse.gocryptfs rw 0 0
";

    fn remote(path: &str) -> bool {
        is_on_remote(&mounts_in(MOUNTS), Path::new(path))
    }

    #[test]
    fn finds_cloud_and_network_drives() {
        assert!(remote("/run/user/1000/gvfs/google-drive:host=gmail.com/report.pdf"));
        assert!(remote("/home/me/Google Drive/photo.jpg"));
        assert!(remote("/mnt/share/notes.txt"));
    }

    #[test]
    fn leaves_local_drives_alone() {
        assert!(!remote("/home/me/Documents/report.pdf"));
        assert!(!remote("/home/me/Google Drive Backup/photo.jpg"));
        assert!(!remote("/media/me/USB/photo.jpg"));
        assert!(!remote("/home/me/Vaults/Private/taxes.pdf"));
    }

    #[cfg(windows)]
    #[test]
    fn sees_online_only_files() {
        use windows::{
            Win32::Storage::FileSystem::{FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_OFFLINE, SetFileAttributesW},
            core::HSTRING,
        };
        let folder = std::env::temp_dir().join(format!("mesa-cloud-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        let file = folder.join("report.pdf");
        fs::write(&file, "x").unwrap();
        assert!(!is_cloud_only_path(&file));
        unsafe { SetFileAttributesW(&HSTRING::from(file.as_path()), FILE_ATTRIBUTE_OFFLINE) }.unwrap();
        assert!(is_cloud_only_path(&file));
        unsafe { SetFileAttributesW(&HSTRING::from(file.as_path()), FILE_ATTRIBUTE_NORMAL) }.unwrap();
        assert!(!is_cloud_only_path(&file));
        let _ = fs::remove_dir_all(&folder);
    }
}
