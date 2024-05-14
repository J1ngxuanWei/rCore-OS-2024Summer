# rcore-5.13


## 试验情况

已经通过全部ch6样例。

## 对easy-fs修改

这部分修改主要是添加辅助函数。

首先是在初始化Stat的时候，我们需要拿到一个inonb，这个是放在底层的目录项的，因此我们需要调用easy-fs的接口来获得，并不在OS中获得:

```rust
///1
    pub fn find_id(&self, name: &str) -> Option<u32> {
        self.read_disk_inode(|disk_inode| {
            self.find_inode_id(name, disk_inode)
                .map(|inode_id| inode_id)
        })
    }
```

然后是一个删除函数，这个主要用于彻底的取消所有目录项中的这个`name`所代表的文件，本质上不是删除文件，是清空这个目录项，因为我们后续不希望find到这个`name`：

```rust
    /// Find inode under a disk inode by name and remove
    fn find_inode_id_remove(&self, name: &str, disk_inode: &mut DiskInode) -> Option<u32> {
        // assert it is a directory
        assert!(disk_inode.is_dir());
        let file_count = (disk_inode.size as usize) / DIRENT_SZ;
        let mut dirent = DirEntry::empty();
        let mut direntff = DirEntry::empty();
        for i in 0..file_count {
            assert_eq!(
                disk_inode.read_at(DIRENT_SZ * i, dirent.as_bytes_mut(), &self.block_device,),
                DIRENT_SZ,
            );
            if dirent.name() == name {
                disk_inode.write_at(DIRENT_SZ * i, direntff.as_bytes_mut(), &self.block_device);
                return Some(0);
            }
        }
        None
    }
    ///remove
    #[allow(unused)]
    pub fn remove(&self, name: &str) -> Option<isize> {
        let fs = self.fs.lock();
        self.modify_disk_inode(|disk_inode| {
            self.find_inode_id_remove(name, disk_inode)
                .map(|inode_id| 0)
        })
    }
```

## OS

这一部分是主要的内容。

首先把OSInode的需要存的东西先放进去;

```rust
pub struct OSInode {
    readable: bool,
    writable: bool,
    inner: UPSafeCell<OSInodeInner>,
    name: String,
    stat: Stat,
}
```

随后我们建立一个OSInode的管理器，用于管理所有的OSInode：

```rust
pub struct OSInodeManager {
    inodes: Vec<OSInode>,
}

impl OSInodeManager {
    pub fn new() -> Self {
        Self { inodes: Vec::new() }
    }
    pub fn add_inode(&mut self, inode: OSInode) {
        self.inodes.push(inode);
    }
    pub fn find_inode(&self, name: &str) -> Option<&OSInode> {
        for inode in self.inodes.iter() {
            if inode.name == name {
                return Some(inode);
            }
        }
        None
    }
    #[allow(unused)]
    pub fn fresh(&mut self) {
        let mut vv: Vec<(u64, u64)> = Vec::new();
        for (i, inode) in self.inodes.iter().enumerate() {
            //println!("name: {}, ino: {}, nlink: {}", inode.name, inode.get_ino(), inode.stat.nlink);
            let mut stat = inode.stat;
            let ino = inode.get_ino();
            let mut ll: u64 = 0;
            for j in 0..self.inodes.len() {
                if self.inodes[j].get_ino() == ino {
                    ll += 1;
                }
            }
            vv.push((ino, ll));
        }
        vv.sort_by(|a, b| a.1.cmp(&b.1));
        let mut vfv: Vec<(u64, u64)> = Vec::new();
        vfv.push((vv[0].0, vv[0].1));
        for i in vv.iter() {
            let l = vfv.len() - 1;
            if i.0 == vfv[l].0 {
                vfv[l].1 = i.1;
            } else {
                vfv.push((i.0, i.1));
            }
        }
        for i in vfv.iter() {
            for iin in self.inodes.iter_mut() {
                //println!("name: {}, ino: {}, nlink: {}", iin.name, iin.get_ino(), iin.stat.nlink);
                if iin.get_ino() == i.0 {
                    iin.set_link(i.1 as u32);
                }
            }
        }
    }
    #[allow(unused)]
    pub fn fresh_one(&mut self, ino: u64, lik: u32, name: String) {
        //println!("fresh one ino: {}, lik: {}, name: {}", ino, lik, name);
        let mut ind = 0;
        let mut fg = false;
        for (i, iin) in self.inodes.iter_mut().enumerate() {
            if iin.name == name {
                ind = i;
                fg = true;
            }
            if iin.get_ino() == ino {
                iin.set_link(lik);
            }
        }
        if fg {
            self.inodes.remove(ind);
        }
    }
}
```

然后我们实现几个辅助函数，主要是添加和查找，以及fresh系列函数，其用于在OSInode变化后，刷新Vec中所有OSInode的连接数。

管理器采用全局lazy初始化：

```rust
    ///1
    pub static ref OSINODE_MANAGER: UPSafeCell<OSInodeManager> =
        unsafe { UPSafeCell::new(OSInodeManager::new()) };
```

当然，我们修改了OSInode，其new函数也需要修改：

```rust
pub fn new(readable: bool, writable: bool, inode: Arc<Inode>, name: &str) -> Self {
        let mut dirent = DirEntry::empty();
        let siz = inode.read_at(0, dirent.as_bytes_mut());
        // 下面应该不用考虑DIR，先这样，不过再说
        if siz == 32 {
            Self {
                readable,
                writable,
                inner: unsafe { UPSafeCell::new(OSInodeInner { offset: 0, inode }) },
                stat: Stat {
                    dev: 0,
                    ino: dirent.inode_id() as u64,
                    mode: StatMode::DIR,
                    nlink: 0,
                    pad: [0; 7],
                },
                name: String::from(name),
            }
        } else {
            let na: Vec<String> = ROOT_INODE.ls();
            let mut nub: u64 = 0;
            for i in na.iter() {
                let ff: &str = i;
                if let Some(id) = ROOT_INODE.find_id(ff) {
                    nub = id as u64;
                    break;
                }
            }
            Self {
                readable,
                writable,
                inner: unsafe { UPSafeCell::new(OSInodeInner { offset: 0, inode }) },
                stat: Stat {
                    dev: 0,
                    ino: nub,
                    mode: StatMode::FILE,
                    nlink: 1,
                    pad: [0; 7],
                },
                name: String::from(name),
            }
        }
    }
```

这里我实现了对目录和文件的区分，不过测试样例好像没有检查`StatMode::DIR`的情况，全部设置为文件也能过。

然后是一个`open_file`函数，因为我们修改了OSInode的大部分内容，因此其不能简单的创建返回了，需要结合name的搜索和管理器的搜索来综合返回一个值：

```rust
pub fn open_file(name: &str, flags: OpenFlags) -> Option<Arc<OSInode>> {
    let (readable, writable) = flags.read_write();
    if flags.contains(OpenFlags::CREATE) {
        if let Some(inode) = ROOT_INODE.find(name) {
            // clear size
            inode.clear();
            let tn = Arc::new(OSInode::new(readable, writable, inode.clone(), name));
            let nn = OSInode::new(readable, writable, inode, name);
            OSINODE_MANAGER.exclusive_access().add_inode(nn);
            Some(tn)
        } else {
            // create file
            ROOT_INODE.create(name).map(|inode| {
                let tn = Arc::new(OSInode::new(readable, writable, inode.clone(), name));
                let nn = OSInode::new(readable, writable, inode, name);
                OSINODE_MANAGER.exclusive_access().add_inode(nn);
                tn
            })
        }
    } else {
        if let Some(r) = ROOT_INODE.find(name).map(|inode| {
            if flags.contains(OpenFlags::TRUNC) {
                inode.clear();
            }
            let tn = Arc::new(OSInode::new(readable, writable, inode.clone(), name));
            let nn = OSInode::new(readable, writable, inode, name);
            OSINODE_MANAGER.exclusive_access().add_inode(nn);
            tn
        }) {
            //println!("1111");
            Some(r)
        } else {
            //println!("2222");
            let mut fff = OSINODE_MANAGER.exclusive_access();
            if let Some(node) = fff.find_inode(name) {
                let inner = node.inner.exclusive_access();
                let tn = Arc::new(OSInode::new(readable, writable, inner.inode.clone(), name));
                Some(tn)
            } else {
                None
            }
        }
    }
}
```

最后，我们需要实现几个辅助功能，我们希望通过fd拿到的file可以获得更多的内部信息：

```rust
pub trait File: Send + Sync {
    ///1
    fn get_stat(&self) -> &Stat;
    ///2
    fn get_name(&self) -> String;
}
```

我们为File特征添加两个辅助的方法，然后分别实现他们。

## syscall实现

现在实现就比较简单了，我们定义一个从新名字生成OSInode的函数，用于添加连接：

```rust
///1
#[allow(unused)]
pub fn new_fromname(name: &str, newname: &str) -> Option<Arc<OSInode>> {
    let mut fff = OSINODE_MANAGER.exclusive_access();
    if let Some(oldnode) = fff.find_inode(name) {
        let nn = OSInode::new(
            oldnode.readable,
            oldnode.writable,
            oldnode.inner.exclusive_access().inode.clone(),
            newname,
        );
        let tn = Arc::new(OSInode::new(
            oldnode.readable,
            oldnode.writable,
            oldnode.inner.exclusive_access().inode.clone(),
            newname,
        ));
        fff.add_inode(nn);
        fff.fresh();
        Some(tn)
    } else {
        return None;
    }
}
```

这样我们调用这个函数就能添加硬链接。

以及一个从名字解除连接的函数：

```rust
///3
#[allow(unused)]
pub fn rmv_fromna(name: String) -> isize {
    ROOT_INODE.remove(&name).map(|inode| 1);
    let mut ino: u64 = 0;
    let mut lik: u32 = 0;
    if let Some(ii) = OSINODE_MANAGER.exclusive_access().find_inode(&name) {
        ino = ii.get_ino();
        lik = ii.stat.nlink;
    } else {
        //println!("error: file {} not found", name);
    }
    OSINODE_MANAGER
        .exclusive_access()
        .fresh_one(ino, lik - 1, name);
    0
}
```

 `sys_fstat`的实现跟gettime是一样的，只不过获取的结构体变了而已，在此不多作记录。
 
值得说明的是，`close`函数需要做一定的修改，因为我们需要删除其管理器内的OSInode，并且需要刷新所有的连接数：

```rust
if let Some(file) = &inner.fd_table[fd] {
        let file = file.clone();
        let namm = file.get_name();
        let mut ino: u64 = 0;
        let mut nlink: u32 = 0;
        let mut fl = false;
        if let Some(sta) = OSINODE_MANAGER.exclusive_access().find_inode(&namm) {
            let stat = sta.get_stat();
            ino = stat.ino;
            nlink = stat.nlink - 1;
            fl = true;
        }
        if fl {
            rmv_fromno(ino, nlink, namm);
            inner.fd_table[fd].take();
        }
    } else {
        return -1;
    }
    0
```

最后是对与read和write的修改，因为我们可能是在close之后使用，因此我们改为从管理器查对应的文件来访问。



## 做ch8

### 移植ch6

手动cv。。。

突然发现不需要移植，nice。

但是好像需要移植gettime。

好在并不复杂

### 实现ch8

首先先看一下，mutex的只需要实现block锁就行了，用户的lib库默认开启睡眠，不使用互斥锁。

其实锁的样例可以偷鸡。。

但是还是写一下吧。








