use super::{
    block_cache_sync_all, get_block_cache, BlockDevice, DirEntry, DiskInode, DiskInodeType,
    EasyFileSystem, DIRENT_SZ,
};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::{Mutex, MutexGuard};
/// Virtual filesystem layer over easy-fs
pub struct Inode {
    block_id: usize,
    block_offset: usize,
    fs: Arc<Mutex<EasyFileSystem>>,
    block_device: Arc<dyn BlockDevice>,
}

impl Inode {
    /// Create a vfs inode
    pub(crate) fn new(
        block_id: u32,
        block_offset: usize,
        fs: Arc<Mutex<EasyFileSystem>>,
        block_device: Arc<dyn BlockDevice>,
    ) -> Self {
        Self {
            block_id: block_id as usize,
            block_offset,
            fs,
            block_device,
        }
    }
    /// Call a function over a disk inode to read it
    fn read_disk_inode<V>(&self, f: impl FnOnce(&DiskInode) -> V) -> V {
        get_block_cache(self.block_id, Arc::clone(&self.block_device))
            .lock()
            .read(self.block_offset, f)
    }
    /// Call a function over a disk inode to modify it
    fn modify_disk_inode<V>(&self, f: impl FnOnce(&mut DiskInode) -> V) -> V {
        get_block_cache(self.block_id, Arc::clone(&self.block_device))
            .lock()
            .modify(self.block_offset, f)
    }
    /// Find inode under a disk inode by name
    fn find_inode_id(&self, name: &str, disk_inode: &DiskInode) -> Option<u32> {
        // assert it is a directory
        assert!(disk_inode.is_dir());
        let file_count = (disk_inode.size as usize) / DIRENT_SZ;
        let mut dirent = DirEntry::empty();
        for i in 0..file_count {
            assert_eq!(
                disk_inode.read_at(DIRENT_SZ * i, dirent.as_bytes_mut(), &self.block_device,),
                DIRENT_SZ,
            );
            if dirent.name() == name {
                return Some(dirent.inode_id() as u32);
            }
        }
        None
    }
    /// Find inode under current inode by name
    pub fn find(&self, name: &str) -> Option<Arc<Inode>> {
        if !Self::valid_name(name) {
            return None;
        }
        let mut fs = self.fs.lock();
        let id = self.read_disk_inode(|inode| self.find_inode_id(name, inode))?;
        Some(fs.get_inode(id, &self.fs))
    }
    /// Return the inode number represented by this handle.
    pub fn inode_id(&self) -> u32 {
        self.fs
            .lock()
            .get_inode_id(self.block_id as u32, self.block_offset)
    }

    /// Check whether this inode represents a directory.
    pub fn is_dir(&self) -> bool {
        self.read_disk_inode(|inode| inode.is_dir())
    }

    /// Count directory entries referring to the specified inode.
    pub fn count_links(&self, target_id: u32) -> u32 {
        let _fs = self.fs.lock();
        self.count_links_locked(target_id)
    }

    fn count_links_locked(&self, target_id: u32) -> u32 {
        self.read_disk_inode(|inode| {
            if !inode.is_dir() {
                return 0;
            }

            let file_count = inode.size as usize / DIRENT_SZ;
            let mut count = 0;

            for index in 0..file_count {
                let mut entry = DirEntry::empty();
                if inode.read_at(index * DIRENT_SZ, entry.as_bytes_mut(), &self.block_device)
                    != DIRENT_SZ
                {
                    continue;
                }

                if !entry.name().is_empty() && entry.inode_id() == target_id {
                    count += 1;
                }
            }

            count
        })
    }

    /// Increase the size of a disk inode
    fn increase_size(
        &self,
        new_size: u32,
        disk_inode: &mut DiskInode,
        fs: &mut MutexGuard<EasyFileSystem>,
    ) -> bool {
        if new_size < disk_inode.size {
            return true;
        }
        let blocks_needed = disk_inode.blocks_num_needed(new_size);
        let mut v: Vec<u32> = Vec::new();
        for _ in 0..blocks_needed {
            match fs.try_alloc_data() {
                Some(block) => v.push(block),
                None => {
                    for block in v {
                        fs.dealloc_data(block);
                    }
                    return false;
                }
            }
        }
        disk_inode.increase_size(new_size, v, &self.block_device);
        true
    }
    /// Create inode under current inode by name
    pub fn create(&self, name: &str) -> Option<Arc<Inode>> {
        if !Self::valid_name(name) || !self.is_dir() {
            return None;
        }
        let mut fs = self.fs.lock();
        let op = |root_inode: &DiskInode| {
            // assert it is a directory
            assert!(root_inode.is_dir());
            // has the file been created?
            self.find_inode_id(name, root_inode)
        };
        if self.read_disk_inode(op).is_some() {
            return None;
        }
        // create a new file
        // alloc a inode with an indirect block
        let new_inode_id = fs.try_alloc_inode()?;
        // initialize inode
        let (new_inode_block_id, new_inode_block_offset) = fs.get_disk_inode_pos(new_inode_id);
        get_block_cache(new_inode_block_id as usize, Arc::clone(&self.block_device))
            .lock()
            .modify(new_inode_block_offset, |new_inode: &mut DiskInode| {
                new_inode.initialize(DiskInodeType::File);
            });
        if !self.insert_entry(name, new_inode_id, &mut fs) {
            fs.dealloc_inode(new_inode_id);
            return None;
        }
        block_cache_sync_all();
        Some(fs.get_inode(new_inode_id, &self.fs))
        // release efs lock automatically by compiler
    }
    /// Create another directory entry referring to an existing file.
    pub fn link(&self, name: &str, target: &Inode) -> bool {
        if !self.is_dir()
            || target.is_dir()
            || !Self::valid_name(name)
            || !Arc::ptr_eq(&self.fs, &target.fs)
        {
            return false;
        }

        let target_id = target.inode_id();
        let mut fs = self.fs.lock();

        if self
            .read_disk_inode(|inode| self.find_inode_id(name, inode))
            .is_some()
        {
            return false;
        }

        let inserted = self.insert_entry(name, target_id, &mut fs);
        drop(fs);
        block_cache_sync_all();
        inserted
    }

    fn valid_name(name: &str) -> bool {
        !name.is_empty()
            && name.len() <= 27
            && name != "."
            && name != ".."
            && !name.contains('/')
            && !name.contains('\0')
    }

    /// Reuse removed directory slots before allocating more directory blocks.
    fn insert_entry(
        &self,
        name: &str,
        target_id: u32,
        fs: &mut MutexGuard<EasyFileSystem>,
    ) -> bool {
        self.modify_disk_inode(|dir| {
            let file_count = dir.size as usize / DIRENT_SZ;
            let mut slot = file_count;

            for index in 0..file_count {
                let mut entry = DirEntry::empty();
                if dir.read_at(index * DIRENT_SZ, entry.as_bytes_mut(), &self.block_device)
                    != DIRENT_SZ
                {
                    return false;
                }

                if entry.name().is_empty() {
                    slot = index;
                    break;
                }
            }

            if slot == file_count {
                if !self.increase_size(((file_count + 1) * DIRENT_SZ) as u32, dir, fs) {
                    return false;
                }
            }

            let entry = DirEntry::new(name, target_id);
            dir.write_at(slot * DIRENT_SZ, entry.as_bytes(), &self.block_device) == DIRENT_SZ
        })
    }

    /// Remove a name. The canonical inode handle defers reclamation until last close.
    pub fn unlink(&self, name: &str) -> bool {
        if !self.is_dir() || !Self::valid_name(name) {
            return false;
        }

        let target = match self.find(name) {
            Some(inode) => inode,
            None => return false,
        };

        if target.is_dir() {
            return false;
        }

        let target_id = target.inode_id();
        let fs = self.fs.lock();

        let index = self.read_disk_inode(|dir| {
            let file_count = dir.size as usize / DIRENT_SZ;

            for index in 0..file_count {
                let mut entry = DirEntry::empty();
                if dir.read_at(index * DIRENT_SZ, entry.as_bytes_mut(), &self.block_device)
                    != DIRENT_SZ
                {
                    continue;
                }

                if entry.name() == name && entry.inode_id() == target_id {
                    return Some(index);
                }
            }

            None
        });

        let index = match index {
            Some(index) => index,
            None => {
                drop(fs);
                return false;
            }
        };

        let removed = self.modify_disk_inode(|dir| {
            let empty = DirEntry::empty();
            dir.write_at(index * DIRENT_SZ, empty.as_bytes(), &self.block_device) == DIRENT_SZ
        });

        drop(fs);

        if !removed {
            return false;
        }

        block_cache_sync_all();

        true
    }

    /// List inodes under current inode
    pub fn ls(&self) -> Vec<String> {
        let _fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| {
            let file_count = (disk_inode.size as usize) / DIRENT_SZ;
            let mut v: Vec<String> = Vec::new();
            for i in 0..file_count {
                let mut dirent = DirEntry::empty();
                assert_eq!(
                    disk_inode.read_at(i * DIRENT_SZ, dirent.as_bytes_mut(), &self.block_device,),
                    DIRENT_SZ,
                );
                if !dirent.name().is_empty() {
                    v.push(String::from(dirent.name()));
                }
            }
            v
        })
    }
    /// Read data from current inode
    pub fn read_at(&self, offset: usize, buf: &mut [u8]) -> usize {
        let _fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| disk_inode.read_at(offset, buf, &self.block_device))
    }
    /// Write data to current inode
    pub fn write_at(&self, offset: usize, buf: &[u8]) -> usize {
        if buf.is_empty() {
            return 0;
        }
        let end = match offset.checked_add(buf.len()) {
            Some(end) if end <= (28 + 128 + 128 * 128) * crate::BLOCK_SZ => end,
            _ => return 0,
        };
        let mut fs = self.fs.lock();
        let size = self.modify_disk_inode(|disk_inode| {
            if !self.increase_size(end as u32, disk_inode, &mut fs) {
                return 0;
            }
            disk_inode.write_at(offset, buf, &self.block_device)
        });
        block_cache_sync_all();
        size
    }
    /// Clear the data in current inode
    pub fn clear(&self) {
        let mut fs = self.fs.lock();
        self.modify_disk_inode(|disk_inode| {
            let size = disk_inode.size;
            let data_blocks_dealloc = disk_inode.clear_size(&self.block_device);
            assert!(data_blocks_dealloc.len() == DiskInode::total_blocks(size) as usize);
            for data_block in data_blocks_dealloc.into_iter() {
                fs.dealloc_data(data_block);
            }
        });
        block_cache_sync_all();
    }
}

impl Drop for Inode {
    fn drop(&mut self) {
        let mut fs = self.fs.lock();
        let id = fs.get_inode_id(self.block_id as u32, self.block_offset);
        if id == 0 {
            return;
        }
        let (block, offset) = fs.get_disk_inode_pos(0);
        let linked = get_block_cache(block as usize, self.block_device.clone())
            .lock()
            .read(offset, |root: &DiskInode| {
                (0..root.size as usize / DIRENT_SZ).any(|index| {
                    let mut entry = DirEntry::empty();
                    root.read_at(index * DIRENT_SZ, entry.as_bytes_mut(), &self.block_device);
                    !entry.name().is_empty() && entry.inode_id() == id
                })
            });
        if linked {
            return;
        }
        // No directory entry or other live handle can now reference this inode.
        self.modify_disk_inode(|inode| {
            for block in inode.clear_size(&self.block_device) {
                fs.dealloc_data(block);
            }
        });
        fs.dealloc_inode(id);
        drop(fs);
        block_cache_sync_all();
    }
}
