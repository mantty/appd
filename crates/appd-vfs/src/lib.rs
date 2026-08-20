#![deny(missing_docs)]
#![allow(clippy::missing_errors_doc)]

//! A small in-memory filesystem for the appd Worker contract.

use std::collections::BTreeMap;
use std::fmt::{self, Display, Formatter};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};

/// Maximum size of one virtual file.
pub const MAX_FILE_SIZE: u64 = 128 * 1024 * 1024;
/// Maximum length of a virtual path in Unicode scalar values.
pub const MAX_PATH_LENGTH: usize = 4096;
/// Maximum number of non-empty path segments.
pub const MAX_PATH_SEGMENTS: usize = 48;

/// The kind of a virtual node.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NodeType {
    /// A regular file.
    File,
    /// A directory.
    Directory,
    /// A symbolic link.
    Symlink,
}

/// A filesystem error category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorKind {
    /// The path or descriptor does not exist.
    NotFound,
    /// A path component is not a directory.
    NotDirectory,
    /// A directory was used as a file.
    IsDirectory,
    /// An entry already exists.
    AlreadyExists,
    /// A directory is not empty.
    NotEmpty,
    /// The target is read-only.
    ReadOnly,
    /// The operation is not permitted.
    NotPermitted,
    /// The operation would exceed the file-size limit.
    FileTooLarge,
    /// The operation would exceed the available virtual space.
    NoSpace,
    /// The path is invalid or escapes the virtual root.
    InvalidPath,
    /// The input is invalid for the requested operation.
    InvalidInput,
    /// A descriptor is invalid or already closed.
    InvalidDescriptor,
    /// Too many symbolic links were followed.
    SymlinkLoop,
    /// The operation is not implemented by this filesystem.
    Unsupported,
    /// The host bundle could not be read.
    Host,
    /// The random source failed.
    Entropy,
}

impl ErrorKind {
    /// Return the Node-style error code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::NotFound => "ENOENT",
            Self::NotDirectory => "ENOTDIR",
            Self::IsDirectory => "EISDIR",
            Self::AlreadyExists => "EEXIST",
            Self::NotEmpty => "ENOTEMPTY",
            Self::ReadOnly => "EROFS",
            Self::NotPermitted => "EPERM",
            Self::FileTooLarge => "EFBIG",
            Self::NoSpace => "ENOSPC",
            Self::InvalidPath | Self::InvalidInput => "EINVAL",
            Self::InvalidDescriptor => "EBADF",
            Self::SymlinkLoop => "ELOOP",
            Self::Unsupported => "ENOSYS",
            Self::Host | Self::Entropy => "EIO",
        }
    }

    const fn default_message(self) -> &'static str {
        match self {
            Self::NotFound => "no such file or directory",
            Self::NotDirectory => "not a directory",
            Self::IsDirectory => "is a directory",
            Self::AlreadyExists => "file already exists",
            Self::NotEmpty => "directory not empty",
            Self::ReadOnly => "read-only file system",
            Self::NotPermitted => "operation not permitted",
            Self::FileTooLarge => "file too large",
            Self::NoSpace => "no space left on device",
            Self::InvalidPath | Self::InvalidInput => "invalid argument",
            Self::InvalidDescriptor => "bad file descriptor",
            Self::SymlinkLoop => "too many levels of symbolic links",
            Self::Unsupported => "operation not supported",
            Self::Host => "host filesystem error",
            Self::Entropy => "random source failed",
        }
    }
}

impl Display for ErrorKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

/// A virtual filesystem failure.
#[derive(Debug)]
pub struct Error {
    kind: ErrorKind,
    path: String,
    message: String,
}

impl Error {
    fn new(kind: ErrorKind, path: impl Into<String>) -> Self {
        Self {
            message: kind.default_message().to_owned(),
            kind,
            path: path.into(),
        }
    }

    fn with_message(kind: ErrorKind, path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            kind,
            path: path.into(),
            message: message.into(),
        }
    }

    fn from_io(path: impl Into<String>, error: &std::io::Error) -> Self {
        let kind = match error.kind() {
            std::io::ErrorKind::NotFound => ErrorKind::NotFound,
            std::io::ErrorKind::PermissionDenied => ErrorKind::NotPermitted,
            std::io::ErrorKind::AlreadyExists => ErrorKind::AlreadyExists,
            std::io::ErrorKind::InvalidInput => ErrorKind::InvalidInput,
            _ => ErrorKind::Host,
        };
        Self::with_message(kind, path, error.to_string())
    }

    /// Return the error category.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// Return the Node-style error code.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        self.kind.code()
    }

    /// Return the virtual path associated with the error.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }
}

impl Display for Error {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code(), self.message)?;
        if !self.path.is_empty() {
            write!(formatter, ", path '{}'", self.path)?;
        }
        Ok(())
    }
}

impl std::error::Error for Error {}

/// Result type used by the virtual filesystem.
pub type Result<T> = std::result::Result<T, Error>;

/// Metadata returned for a virtual node.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Stat {
    /// Node kind.
    pub kind: NodeType,
    /// File length in bytes.
    pub size: u64,
    /// Whether the node accepts ordinary writes.
    pub writable: bool,
    /// Whether the file is a synthetic device.
    pub device: bool,
}

/// One entry returned when reading a directory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirectoryEntry {
    /// Entry name, without its parent path.
    pub name: String,
    /// Entry kind.
    pub kind: NodeType,
    /// Whether the entry is a synthetic character device.
    pub device: bool,
}

/// The immutable bundle directory shared by virtual filesystem instances.
#[derive(Clone, Debug)]
pub struct Bundle {
    root: Arc<PathBuf>,
}

impl Bundle {
    /// Create a bundle view over an appd `bundle` directory.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: Arc::new(root.into()),
        }
    }

    /// Return the host directory backing this bundle.
    #[must_use]
    pub fn root(&self) -> &Path {
        self.root.as_path()
    }
}

/// Options used when opening a virtual file.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OpenOptions {
    /// Open for reading.
    pub read: bool,
    /// Open for writing.
    pub write: bool,
    /// Place writes at the end of the file.
    pub append: bool,
    /// Create a missing file.
    pub create: bool,
    /// Fail if the file already exists.
    pub exclusive: bool,
    /// Truncate an existing file after opening.
    pub truncate: bool,
    /// Follow the final symbolic link.
    pub follow_symlinks: bool,
}

impl Default for OpenOptions {
    fn default() -> Self {
        Self {
            read: true,
            write: false,
            append: false,
            create: false,
            exclusive: false,
            truncate: false,
            follow_symlinks: true,
        }
    }
}

/// Options used when copying a file or directory tree.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CopyOptions {
    /// Copy directories recursively.
    pub recursive: bool,
    /// Replace an existing destination.
    pub force: bool,
    /// Fail when the destination already exists.
    pub error_on_exist: bool,
    /// Follow symbolic links in the source.
    pub dereference: bool,
}

/// A request-owned virtual filesystem.
pub struct VirtualFileSystem {
    root: Directory,
    descriptors: BTreeMap<u32, OpenFile>,
    next_descriptor: u32,
}

impl VirtualFileSystem {
    /// Create a filesystem with a shared readonly bundle and fresh `/tmp`.
    #[must_use]
    pub fn new(bundle: Bundle) -> Self {
        let tmp = Directory::memory(true);
        let bundle = Node::Directory(Directory::bundle(bundle, PathBuf::new()));
        let dev = Node::Directory(shared_devices());
        let root = Directory::with_entries(
            false,
            BTreeMap::from([
                ("bundle".to_owned(), bundle),
                ("dev".to_owned(), dev),
                ("tmp".to_owned(), Node::Directory(tmp)),
            ]),
        );
        Self {
            root,
            descriptors: BTreeMap::new(),
            next_descriptor: 3,
        }
    }

    /// Read a file completely.
    pub fn read_file(&self, path: &str) -> Result<Vec<u8>> {
        self.resolve(path, true)?.read_all(path)
    }

    /// Replace or append to a file.
    pub fn write_file(&mut self, path: &str, data: &[u8], append: bool) -> Result<()> {
        let options = OpenOptions {
            read: false,
            write: true,
            append,
            create: true,
            truncate: !append,
            ..OpenOptions::default()
        };
        let descriptor = self.open(path, options)?;
        let result = self.write(descriptor, data, None).map(|_| ());
        let close_result = self.close(descriptor);
        result.and(close_result)
    }

    /// Return metadata, following the final symbolic link.
    pub fn stat(&self, path: &str) -> Result<Stat> {
        self.resolve(path, true)?.stat()
    }

    /// Return metadata without following the final symbolic link.
    pub fn lstat(&self, path: &str) -> Result<Stat> {
        self.resolve(path, false)?.stat()
    }

    /// List a directory in lexical order.
    pub fn read_dir(&self, path: &str) -> Result<Vec<DirectoryEntry>> {
        self.resolve(path, true)?.read_dir(path)
    }

    /// List a directory tree in lexical order.
    pub fn walk(&self, path: &str) -> Result<Vec<(String, DirectoryEntry)>> {
        let mut entries = Vec::new();
        self.walk_directory(path, &mut entries)?;
        Ok(entries)
    }

    /// Check whether a path satisfies the requested access mode.
    pub fn access(&self, path: &str, mode: u32) -> Result<()> {
        if mode & !7 != 0 {
            return Err(Error::new(ErrorKind::InvalidInput, path));
        }
        if mode & 1 != 0 {
            return Err(Error::new(ErrorKind::NotFound, path));
        }
        let stat = self.stat(path)?;
        if mode & 2 != 0 && !stat.writable {
            return Err(Error::new(ErrorKind::NotFound, path));
        }
        Ok(())
    }

    /// Create a directory.
    pub fn mkdir(&mut self, path: &str, recursive: bool) -> Result<()> {
        let components = absolute_components(path)?;
        if components.is_empty() {
            return if recursive {
                Ok(())
            } else {
                Err(Error::new(ErrorKind::AlreadyExists, path))
            };
        }
        match self.resolve(path, true) {
            Ok(Node::Directory(_)) if recursive => return Ok(()),
            Ok(_) => return Err(Error::new(ErrorKind::AlreadyExists, path)),
            Err(error) if error.kind() != ErrorKind::NotFound => return Err(error),
            Err(_) => {}
        }
        if recursive {
            self.mkdir_recursive(&components, path)
        } else {
            let (parent, name) = self.parent(path)?;
            parent.insert(name, Node::Directory(Directory::memory(true)))
        }
    }

    /// Remove a file, directory, or symbolic link.
    pub fn remove(&mut self, path: &str, recursive: bool, force: bool) -> Result<()> {
        let (parent, name) = match self.parent(path) {
            Ok(value) => value,
            Err(error) if force && error.kind() == ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        let node = match parent.lookup(&name)? {
            Some(node) => node,
            None if force => return Ok(()),
            None => return Err(Error::new(ErrorKind::NotFound, path)),
        };
        if let Node::Directory(directory) = &node
            && !recursive
            && !directory.is_empty()?
        {
            return Err(Error::new(ErrorKind::NotEmpty, path));
        }
        parent
            .remove(&name)?
            .ok_or_else(|| Error::new(ErrorKind::NotFound, path))?;
        Ok(())
    }

    /// Remove a directory, optionally including its contents.
    pub fn remove_directory(&mut self, path: &str, recursive: bool) -> Result<()> {
        if !matches!(self.resolve(path, true)?, Node::Directory(_)) {
            return Err(Error::new(ErrorKind::NotDirectory, path));
        }
        self.remove(path, recursive, false)
    }

    /// Remove a regular file or symbolic link.
    pub fn remove_file(&mut self, path: &str) -> Result<()> {
        if matches!(self.resolve(path, false)?, Node::Directory(_)) {
            return Err(Error::new(ErrorKind::IsDirectory, path));
        }
        self.remove(path, false, false)
    }

    /// Rename a node within the virtual filesystem.
    pub fn rename(&mut self, from: &str, to: &str) -> Result<()> {
        let from_components = absolute_components(from)?;
        let to_components = absolute_components(to)?;
        if from_components == to_components {
            return Ok(());
        }
        if to_components.starts_with(&from_components) {
            return Err(Error::new(ErrorKind::InvalidInput, to));
        }
        let (source_parent, source_name) = self.parent(from)?;
        let source = source_parent
            .lookup(&source_name)?
            .ok_or_else(|| Error::new(ErrorKind::NotFound, from))?;
        let (destination_parent, destination_name) = self.parent(to)?;
        if !source_parent.writable() || !destination_parent.writable() {
            return Err(Error::new(
                ErrorKind::ReadOnly,
                if source_parent.writable() { to } else { from },
            ));
        }
        if let Some(existing) = destination_parent.lookup(&destination_name)? {
            if let Node::Directory(directory) = &existing
                && !directory.is_empty()?
            {
                return Err(Error::new(ErrorKind::NotEmpty, to));
            }
            destination_parent.remove(&destination_name)?;
        }
        destination_parent.insert(destination_name, source)?;
        source_parent.remove(&source_name)?;
        Ok(())
    }

    /// Copy a regular file.
    pub fn copy_file(&mut self, from: &str, to: &str) -> Result<()> {
        self.copy_file_with_options(from, to, false)
    }

    /// Copy a regular file with optional exclusive creation.
    pub fn copy_file_with_options(&mut self, from: &str, to: &str, exclusive: bool) -> Result<()> {
        let source = self.resolve(from, true)?;
        let bytes = source.read_all(from)?;
        if exclusive && self.resolve(to, true).is_ok() {
            return Err(Error::new(ErrorKind::AlreadyExists, to));
        }
        self.write_file(to, &bytes, false)
    }

    /// Copy a file or directory tree.
    pub fn copy(&mut self, from: &str, to: &str, options: CopyOptions) -> Result<()> {
        let source = self.resolve(from, options.dereference)?;
        let target = self.copy_target(from, to, &source)?;
        let target_exists = self.resolve(&target, false).is_ok();
        if target_exists && options.error_on_exist {
            return Err(Error::new(ErrorKind::AlreadyExists, &target));
        }
        if target_exists && !options.force {
            return Ok(());
        }
        if matches!(source, Node::Directory(_)) && !options.recursive {
            return Err(Error::new(ErrorKind::IsDirectory, from));
        }
        let node = self.copy_node(&source, from, options.dereference)?;
        let (parent, name) = self.parent(&target)?;
        if target_exists {
            parent.remove(&name)?;
        }
        parent.insert(name, node)
    }

    /// Create a hard link to a regular file.
    pub fn link(&mut self, existing: &str, new: &str) -> Result<()> {
        let source = self.resolve(existing, true)?;
        if !matches!(
            source,
            Node::File(FileNode::Memory(_) | FileNode::Bundle { .. })
        ) {
            return Err(Error::new(ErrorKind::Unsupported, existing));
        }
        let (parent, name) = self.parent(new)?;
        if parent.lookup(&name)?.is_some() {
            return Err(Error::new(ErrorKind::AlreadyExists, new));
        }
        parent.insert(name, source)
    }

    /// Create a unique temporary directory below an existing parent.
    pub fn make_temp_dir(&mut self, prefix: &str) -> Result<String> {
        let (parent, name) = self.parent(prefix)?;
        let mut suffix = [0_u8; 6];
        for _ in 0..100 {
            getrandom::fill(&mut suffix).map_err(|error| {
                Error::with_message(ErrorKind::Entropy, prefix, error.to_string())
            })?;
            let candidate = format!("{name}{}", random_suffix(&suffix));
            if parent.lookup(&candidate)?.is_none() {
                parent.insert(candidate.clone(), Node::Directory(Directory::memory(true)))?;
                let mut result = absolute_components(prefix)?;
                result.pop();
                result.push(candidate);
                return Ok(path_from_components(&result));
            }
        }
        Err(Error::new(ErrorKind::AlreadyExists, prefix))
    }

    /// Create a symbolic link.
    pub fn symlink(&mut self, target: &str, path: &str) -> Result<()> {
        if target.contains('\0') {
            return Err(Error::new(ErrorKind::InvalidInput, path));
        }
        let (parent, name) = self.parent(path)?;
        if parent.lookup(&name)?.is_some() {
            return Err(Error::new(ErrorKind::AlreadyExists, path));
        }
        parent.insert(name, Node::Symlink(target.to_owned()))
    }

    /// Read a symbolic link target.
    pub fn read_link(&self, path: &str) -> Result<String> {
        match self.resolve(path, false)? {
            Node::Symlink(target) => Ok(target),
            _ => Err(Error::new(ErrorKind::InvalidInput, path)),
        }
    }

    /// Return the normalized, symlink-resolved virtual path.
    pub fn realpath(&self, path: &str) -> Result<String> {
        let (_, components) = self.resolve_with_path(path, true)?;
        Ok(path_from_components(&components))
    }

    /// Open a file and return its descriptor.
    pub fn open(&mut self, path: &str, options: OpenOptions) -> Result<u32> {
        if !options.read && !options.write {
            return Err(Error::new(ErrorKind::InvalidInput, path));
        }
        let (node, existed) = match self.resolve(path, options.follow_symlinks) {
            Ok(node) => (node, true),
            Err(error) if error.kind() == ErrorKind::NotFound && options.create => {
                let (parent, name) = self.parent(path)?;
                if parent.lookup(&name)?.is_some() {
                    return Err(Error::new(ErrorKind::NotFound, path));
                }
                let node = Node::File(FileNode::memory());
                parent.insert(name, node.clone())?;
                (node, false)
            }
            Err(error) => return Err(error),
        };
        if existed && options.exclusive {
            return Err(Error::new(ErrorKind::AlreadyExists, path));
        }
        let file = match &node {
            Node::File(file) => file,
            Node::Directory(_) => return Err(Error::new(ErrorKind::IsDirectory, path)),
            Node::Symlink(_) => return Err(Error::new(ErrorKind::Unsupported, path)),
        };
        if options.write && !file.writable() {
            return Err(Error::new(ErrorKind::ReadOnly, path));
        }
        if options.truncate && options.write {
            file.resize(0, path)?;
        }
        let descriptor = self.next_descriptor;
        self.next_descriptor = self
            .next_descriptor
            .checked_add(1)
            .ok_or_else(|| Error::new(ErrorKind::InvalidDescriptor, path))?;
        self.descriptors.insert(
            descriptor,
            OpenFile {
                node,
                position: 0,
                read: options.read,
                write: options.write,
                append: options.append,
            },
        );
        Ok(descriptor)
    }

    /// Close a descriptor.
    pub fn close(&mut self, descriptor: u32) -> Result<()> {
        self.descriptors
            .remove(&descriptor)
            .map(|_| ())
            .ok_or_else(|| Error::new(ErrorKind::InvalidDescriptor, descriptor.to_string()))
    }

    /// Return descriptor metadata.
    pub fn fstat(&self, descriptor: u32) -> Result<Stat> {
        self.descriptor(descriptor)?.node.stat()
    }

    /// Read from a descriptor. `position` does not move the descriptor cursor.
    pub fn read(
        &mut self,
        descriptor: u32,
        length: usize,
        position: Option<u64>,
    ) -> Result<Vec<u8>> {
        let open = self
            .descriptors
            .get_mut(&descriptor)
            .ok_or_else(|| Error::new(ErrorKind::InvalidDescriptor, descriptor.to_string()))?;
        if !open.read {
            return Err(Error::new(ErrorKind::NotPermitted, descriptor.to_string()));
        }
        let offset = position.unwrap_or(open.position);
        let bytes = open.node.read(offset, length)?;
        if position.is_none() {
            open.position = offset.saturating_add(bytes.len() as u64);
        }
        Ok(bytes)
    }

    /// Write to a descriptor. `position` does not move the descriptor cursor.
    pub fn write(&mut self, descriptor: u32, data: &[u8], position: Option<u64>) -> Result<usize> {
        let open = self
            .descriptors
            .get_mut(&descriptor)
            .ok_or_else(|| Error::new(ErrorKind::InvalidDescriptor, descriptor.to_string()))?;
        if !open.write {
            return Err(Error::new(ErrorKind::NotPermitted, descriptor.to_string()));
        }
        let offset = position.unwrap_or(open.position);
        let written = open.node.write(offset, data, open.append)?;
        if position.is_none() {
            open.position = if open.append {
                open.node.size()?
            } else {
                offset.saturating_add(written as u64)
            };
        }
        Ok(written)
    }

    /// Read from a descriptor into multiple buffers.
    pub fn readv(
        &mut self,
        descriptor: u32,
        lengths: &[usize],
        position: Option<u64>,
    ) -> Result<Vec<Vec<u8>>> {
        self.descriptor(descriptor)?;
        let mut offset = position;
        let mut result = Vec::with_capacity(lengths.len());
        for length in lengths {
            let bytes = self.read(descriptor, *length, offset)?;
            if let Some(value) = &mut offset {
                *value = value.saturating_add(bytes.len() as u64);
            }
            result.push(bytes);
        }
        Ok(result)
    }

    /// Write multiple buffers to a descriptor.
    pub fn writev(
        &mut self,
        descriptor: u32,
        buffers: &[Vec<u8>],
        position: Option<u64>,
    ) -> Result<usize> {
        self.descriptor(descriptor)?;
        let mut offset = position;
        let mut written = 0;
        for buffer in buffers {
            let count = self.write(descriptor, buffer, offset)?;
            written += count;
            if let Some(value) = &mut offset {
                *value = value.saturating_add(count as u64);
            }
        }
        Ok(written)
    }

    /// Truncate a path.
    pub fn truncate(&mut self, path: &str, size: u64) -> Result<()> {
        self.resolve(path, true)?.resize(size, path)
    }

    /// Truncate a descriptor.
    pub fn ftruncate(&mut self, descriptor: u32, size: u64) -> Result<()> {
        let open = self.descriptor(descriptor)?;
        if !open.write {
            return Err(Error::new(ErrorKind::NotPermitted, descriptor.to_string()));
        }
        open.node.resize(size, &descriptor.to_string())
    }

    fn descriptor(&self, descriptor: u32) -> Result<&OpenFile> {
        self.descriptors
            .get(&descriptor)
            .ok_or_else(|| Error::new(ErrorKind::InvalidDescriptor, descriptor.to_string()))
    }

    fn mkdir_recursive(&mut self, components: &[String], path: &str) -> Result<()> {
        let mut current = Node::Directory(self.root.clone());
        for component in components {
            let directory = current
                .as_directory()
                .ok_or_else(|| Error::new(ErrorKind::NotDirectory, path))?;
            current = match directory.lookup(component)? {
                Some(Node::Directory(directory)) => Node::Directory(directory),
                Some(_) => return Err(Error::new(ErrorKind::NotDirectory, path)),
                None => {
                    let child = Node::Directory(Directory::memory(true));
                    directory.insert(component.clone(), child.clone())?;
                    child
                }
            };
        }
        Ok(())
    }

    fn walk_directory(
        &self,
        path: &str,
        entries: &mut Vec<(String, DirectoryEntry)>,
    ) -> Result<()> {
        for entry in self.read_dir(path)? {
            let child = if path == "/" {
                format!("/{name}", name = entry.name)
            } else {
                format!("{path}/{name}", name = entry.name)
            };
            entries.push((child.clone(), entry.clone()));
            if entry.kind == NodeType::Directory {
                self.walk_directory(&child, entries)?;
            }
        }
        Ok(())
    }

    fn copy_target(&self, from: &str, to: &str, source: &Node) -> Result<String> {
        let destination = self.resolve(to, true);
        if destination
            .as_ref()
            .is_ok_and(|node| matches!(node, Node::Directory(_)))
        {
            let name = absolute_components(from)?
                .last()
                .cloned()
                .ok_or_else(|| Error::new(ErrorKind::InvalidInput, from))?;
            return Ok(if to == "/" {
                format!("/{name}")
            } else {
                format!("{to}/{name}")
            });
        }
        if matches!(source, Node::Directory(_)) && destination.is_err() {
            let (_, name) = self.parent(to)?;
            if name.is_empty() {
                return Err(Error::new(ErrorKind::InvalidInput, to));
            }
        }
        Ok(to.to_owned())
    }

    fn copy_node(&self, node: &Node, path: &str, dereference: bool) -> Result<Node> {
        match node {
            Node::File(file) if file.is_device() => Err(Error::new(ErrorKind::Unsupported, path)),
            Node::File(file) => {
                let size = checked_size(file.size()?, path)?;
                let bytes = file.read(0, size)?;
                Ok(Node::File(FileNode::Memory(Arc::new(Mutex::new(bytes)))))
            }
            Node::Symlink(_) if !dereference => Ok(node.clone()),
            Node::Symlink(_) => {
                let resolved = self.resolve(path, true)?;
                self.copy_node(&resolved, path, dereference)
            }
            Node::Directory(directory) => {
                let target = Directory::memory(true);
                for entry in directory.entries()? {
                    let child_path = if path == "/" {
                        format!("/{name}", name = entry.name)
                    } else {
                        format!("{path}/{name}", name = entry.name)
                    };
                    let child = directory
                        .lookup(&entry.name)?
                        .ok_or_else(|| Error::new(ErrorKind::NotFound, &child_path))?;
                    target.insert(
                        entry.name,
                        self.copy_node(&child, &child_path, dereference)?,
                    )?;
                }
                Ok(Node::Directory(target))
            }
        }
    }

    fn parent(&self, path: &str) -> Result<(Directory, String)> {
        let mut components = absolute_components(path)?;
        let name = components
            .pop()
            .ok_or_else(|| Error::new(ErrorKind::InvalidInput, path))?;
        let (node, _) = self.resolve_components(components, true, path)?;
        let directory = node
            .as_directory()
            .ok_or_else(|| Error::new(ErrorKind::NotDirectory, path))?;
        Ok((directory, name))
    }

    fn resolve(&self, path: &str, follow_final: bool) -> Result<Node> {
        self.resolve_with_path(path, follow_final)
            .map(|(node, _)| node)
    }

    fn resolve_with_path(&self, path: &str, follow_final: bool) -> Result<(Node, Vec<String>)> {
        self.resolve_components(absolute_components(path)?, follow_final, path)
    }

    fn resolve_components(
        &self,
        mut pending: Vec<String>,
        follow_final: bool,
        path: &str,
    ) -> Result<(Node, Vec<String>)> {
        let root = Node::Directory(self.root.clone());
        let mut current = root.clone();
        let mut resolved = Vec::new();
        let mut index = 0;
        let mut symlinks = 0;
        loop {
            if index == pending.len() {
                return Ok((current, resolved));
            }
            let name = pending[index].clone();
            let directory = current
                .as_directory()
                .ok_or_else(|| Error::new(ErrorKind::NotDirectory, path))?;
            let child = directory
                .lookup(&name)?
                .ok_or_else(|| Error::new(ErrorKind::NotFound, path))?;
            let final_component = index + 1 == pending.len();
            if let Node::Symlink(target) = &child
                && (follow_final || !final_component)
            {
                symlinks += 1;
                if symlinks > 40 {
                    return Err(Error::new(ErrorKind::SymlinkLoop, path));
                }
                let mut next = target_components(&resolved, target, path)?;
                next.extend_from_slice(&pending[index + 1..]);
                pending = next;
                current = root.clone();
                resolved.clear();
                index = 0;
                continue;
            }
            current = child;
            resolved.push(name);
            index += 1;
        }
    }
}

#[derive(Clone)]
struct OpenFile {
    node: Node,
    position: u64,
    read: bool,
    write: bool,
    append: bool,
}

#[derive(Clone)]
enum Node {
    File(FileNode),
    Directory(Directory),
    Symlink(String),
}

impl Node {
    fn kind(&self) -> NodeType {
        match self {
            Self::File(_) => NodeType::File,
            Self::Directory(_) => NodeType::Directory,
            Self::Symlink(_) => NodeType::Symlink,
        }
    }

    fn as_directory(&self) -> Option<Directory> {
        match self {
            Self::Directory(directory) => Some(directory.clone()),
            _ => None,
        }
    }

    fn stat(&self) -> Result<Stat> {
        match self {
            Self::File(file) => file.stat(),
            Self::Directory(directory) => Ok(Stat {
                kind: NodeType::Directory,
                size: 0,
                writable: directory.writable(),
                device: false,
            }),
            Self::Symlink(target) => Ok(Stat {
                kind: NodeType::Symlink,
                size: target.len() as u64,
                writable: false,
                device: false,
            }),
        }
    }

    fn read_all(&self, path: &str) -> Result<Vec<u8>> {
        match self {
            Self::File(file) => {
                let size = file.size()?;
                let length =
                    usize::try_from(size).map_err(|_| Error::new(ErrorKind::FileTooLarge, path))?;
                file.read(0, length)
            }
            Self::Directory(_) => Err(Error::new(ErrorKind::IsDirectory, path)),
            Self::Symlink(_) => Err(Error::new(ErrorKind::Unsupported, path)),
        }
    }

    fn read_dir(&self, path: &str) -> Result<Vec<DirectoryEntry>> {
        match self {
            Self::Directory(directory) => directory.entries(),
            _ => Err(Error::new(ErrorKind::NotDirectory, path)),
        }
    }

    fn read(&self, offset: u64, length: usize) -> Result<Vec<u8>> {
        match self {
            Self::File(file) => file.read(offset, length),
            Self::Directory(_) => Err(Error::new(ErrorKind::IsDirectory, "")),
            Self::Symlink(_) => Err(Error::new(ErrorKind::Unsupported, "")),
        }
    }

    fn write(&self, offset: u64, data: &[u8], append: bool) -> Result<usize> {
        match self {
            Self::File(file) => file.write(offset, data, append),
            Self::Directory(_) => Err(Error::new(ErrorKind::IsDirectory, "")),
            Self::Symlink(_) => Err(Error::new(ErrorKind::Unsupported, "")),
        }
    }

    fn is_device(&self) -> bool {
        matches!(self, Self::File(FileNode::Device(_)))
    }

    fn size(&self) -> Result<u64> {
        match self {
            Self::File(file) => file.size(),
            Self::Directory(_) => Ok(0),
            Self::Symlink(target) => Ok(target.len() as u64),
        }
    }

    fn resize(&self, size: u64, path: &str) -> Result<()> {
        match self {
            Self::File(file) => file.resize(size, path),
            Self::Directory(_) => Err(Error::new(ErrorKind::IsDirectory, path)),
            Self::Symlink(_) => Err(Error::new(ErrorKind::Unsupported, path)),
        }
    }
}

#[derive(Clone)]
enum Directory {
    Memory {
        entries: Arc<Mutex<BTreeMap<String, Node>>>,
        writable: bool,
    },
    Bundle {
        bundle: Bundle,
        relative: PathBuf,
    },
    Static {
        entries: Arc<BTreeMap<String, Node>>,
    },
}

impl Directory {
    fn memory(writable: bool) -> Self {
        Self::Memory {
            entries: Arc::new(Mutex::new(BTreeMap::new())),
            writable,
        }
    }

    fn with_entries(writable: bool, entries: BTreeMap<String, Node>) -> Self {
        Self::Memory {
            entries: Arc::new(Mutex::new(entries)),
            writable,
        }
    }

    fn bundle(bundle: Bundle, relative: PathBuf) -> Self {
        Self::Bundle { bundle, relative }
    }

    fn lookup(&self, name: &str) -> Result<Option<Node>> {
        match self {
            Self::Memory { entries, .. } => Ok(lock(entries).get(name).cloned()),
            Self::Static { entries } => Ok(entries.get(name).cloned()),
            Self::Bundle { bundle, relative } => {
                let child = relative.join(name);
                bundle.node(&child)
            }
        }
    }

    fn entries(&self) -> Result<Vec<DirectoryEntry>> {
        let mut entries = match self {
            Self::Memory { entries, .. } => lock(entries).iter().map(directory_entry).collect(),
            Self::Static { entries } => entries.iter().map(directory_entry).collect(),
            Self::Bundle { bundle, relative } => bundle.entries(relative)?,
        };
        entries.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(entries)
    }

    fn insert(&self, name: String, node: Node) -> Result<()> {
        match self {
            Self::Memory { entries, writable } if *writable => {
                let mut entries = lock(entries);
                if entries.contains_key(&name) {
                    return Err(Error::new(ErrorKind::AlreadyExists, name));
                }
                entries.insert(name, node);
                Ok(())
            }
            _ => Err(Error::new(ErrorKind::ReadOnly, name)),
        }
    }

    fn remove(&self, name: &str) -> Result<Option<Node>> {
        match self {
            Self::Memory { entries, writable } if *writable => Ok(lock(entries).remove(name)),
            _ => Err(Error::new(ErrorKind::ReadOnly, name)),
        }
    }

    fn is_empty(&self) -> Result<bool> {
        Ok(self.entries()?.is_empty())
    }

    fn writable(&self) -> bool {
        match self {
            Self::Memory { writable, .. } => *writable,
            Self::Bundle { .. } | Self::Static { .. } => false,
        }
    }
}

fn directory_entry((name, node): (&String, &Node)) -> DirectoryEntry {
    DirectoryEntry {
        name: name.clone(),
        kind: node.kind(),
        device: node.is_device(),
    }
}

#[derive(Clone)]
enum FileNode {
    Memory(Arc<Mutex<Vec<u8>>>),
    Bundle { bundle: Bundle, relative: PathBuf },
    Device(Device),
}

impl FileNode {
    fn memory() -> Self {
        Self::Memory(Arc::new(Mutex::new(Vec::new())))
    }

    fn stat(&self) -> Result<Stat> {
        match self {
            Self::Memory(data) => Ok(Stat {
                kind: NodeType::File,
                size: lock(data).len() as u64,
                writable: true,
                device: false,
            }),
            Self::Bundle { bundle, relative } => {
                let metadata = bundle.metadata(relative)?;
                Ok(Stat {
                    kind: NodeType::File,
                    size: metadata.len(),
                    writable: false,
                    device: false,
                })
            }
            Self::Device(_) => Ok(Stat {
                kind: NodeType::File,
                size: 0,
                writable: true,
                device: true,
            }),
        }
    }

    fn size(&self) -> Result<u64> {
        Ok(self.stat()?.size)
    }

    fn writable(&self) -> bool {
        !matches!(self, Self::Bundle { .. })
    }

    fn is_device(&self) -> bool {
        matches!(self, Self::Device(_))
    }

    fn read(&self, offset: u64, length: usize) -> Result<Vec<u8>> {
        match self {
            Self::Memory(data) => Ok(read_memory(data, offset, length)),
            Self::Bundle { bundle, relative } => {
                let data = bundle.read(relative)?;
                Ok(read_bytes(&data, offset, length))
            }
            Self::Device(device) => device.read(length),
        }
    }

    fn write(&self, offset: u64, data: &[u8], append: bool) -> Result<usize> {
        match self {
            Self::Memory(bytes) => write_memory(bytes, offset, data, append),
            Self::Bundle { .. } => Err(Error::new(ErrorKind::ReadOnly, "")),
            Self::Device(device) => device.write(data),
        }
    }

    fn resize(&self, size: u64, path: &str) -> Result<()> {
        match self {
            Self::Memory(data) => {
                let size = checked_size(size, path)?;
                lock(data).resize(size, 0);
                Ok(())
            }
            Self::Bundle { .. } => Err(Error::new(ErrorKind::ReadOnly, path)),
            Self::Device(Device::Null | Device::Zero) => Ok(()),
            Self::Device(Device::Full | Device::Random) => {
                Err(Error::new(ErrorKind::NotPermitted, path))
            }
        }
    }
}

#[derive(Clone, Copy)]
enum Device {
    Null,
    Zero,
    Full,
    Random,
}

impl Device {
    fn read(self, length: usize) -> Result<Vec<u8>> {
        match self {
            Self::Null => Ok(Vec::new()),
            Self::Zero | Self::Full => Ok(vec![0; length]),
            Self::Random => {
                let mut data = vec![0; length];
                getrandom::fill(&mut data).map_err(|error| {
                    Error::with_message(ErrorKind::Entropy, "", error.to_string())
                })?;
                Ok(data)
            }
        }
    }

    fn write(self, data: &[u8]) -> Result<usize> {
        match self {
            Self::Null | Self::Zero => Ok(data.len()),
            Self::Full => Err(Error::new(ErrorKind::NoSpace, "")),
            Self::Random => Err(Error::new(ErrorKind::NotPermitted, "")),
        }
    }
}

fn shared_devices() -> Directory {
    static DEVICES: LazyLock<Directory> = LazyLock::new(|| Directory::Static {
        entries: Arc::new(BTreeMap::from([
            (
                "full".to_owned(),
                Node::File(FileNode::Device(Device::Full)),
            ),
            (
                "null".to_owned(),
                Node::File(FileNode::Device(Device::Null)),
            ),
            (
                "random".to_owned(),
                Node::File(FileNode::Device(Device::Random)),
            ),
            (
                "zero".to_owned(),
                Node::File(FileNode::Device(Device::Zero)),
            ),
        ])),
    });
    DEVICES.clone()
}

impl Bundle {
    fn node(&self, relative: &Path) -> Result<Option<Node>> {
        let metadata = match fs::symlink_metadata(self.root.join(relative)) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(Error::from_io(relative.display().to_string(), &error)),
        };
        let file_type = metadata.file_type();
        if file_type.is_symlink() {
            return Err(Error::new(
                ErrorKind::Unsupported,
                relative.display().to_string(),
            ));
        }
        if file_type.is_dir() {
            return Ok(Some(Node::Directory(Directory::bundle(
                self.clone(),
                relative.to_owned(),
            ))));
        }
        if file_type.is_file() {
            return Ok(Some(Node::File(FileNode::Bundle {
                bundle: self.clone(),
                relative: relative.to_owned(),
            })));
        }
        Err(Error::new(
            ErrorKind::Unsupported,
            relative.display().to_string(),
        ))
    }

    fn metadata(&self, relative: &Path) -> Result<fs::Metadata> {
        let metadata = fs::symlink_metadata(self.root.join(relative))
            .map_err(|error| Error::from_io(relative.display().to_string(), &error))?;
        if !metadata.file_type().is_file() {
            return Err(Error::new(
                ErrorKind::IsDirectory,
                relative.display().to_string(),
            ));
        }
        Ok(metadata)
    }

    fn read(&self, relative: &Path) -> Result<Vec<u8>> {
        self.metadata(relative)?;
        fs::read(self.root.join(relative))
            .map_err(|error| Error::from_io(relative.display().to_string(), &error))
    }

    fn entries(&self, relative: &Path) -> Result<Vec<DirectoryEntry>> {
        let root = self.root.join(relative);
        let metadata = match fs::symlink_metadata(&root) {
            Ok(metadata) => metadata,
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound
                    && relative.as_os_str().is_empty() =>
            {
                return Ok(Vec::new());
            }
            Err(error) => return Err(Error::from_io(relative.display().to_string(), &error)),
        };
        if !metadata.file_type().is_dir() {
            return Err(Error::new(
                ErrorKind::NotDirectory,
                relative.display().to_string(),
            ));
        }
        let mut entries = Vec::new();
        for entry in fs::read_dir(root)
            .map_err(|error| Error::from_io(relative.display().to_string(), &error))?
        {
            let entry =
                entry.map_err(|error| Error::from_io(relative.display().to_string(), &error))?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| Error::new(ErrorKind::InvalidPath, relative.display().to_string()))?;
            let file_type = entry
                .file_type()
                .map_err(|error| Error::from_io(name.clone(), &error))?;
            let kind = if file_type.is_file() {
                NodeType::File
            } else if file_type.is_dir() {
                NodeType::Directory
            } else {
                return Err(Error::new(ErrorKind::Unsupported, name));
            };
            entries.push(DirectoryEntry {
                name,
                kind,
                device: false,
            });
        }
        Ok(entries)
    }
}

fn absolute_components(path: &str) -> Result<Vec<String>> {
    if !path.starts_with('/') || path.contains('\0') || path.chars().count() > MAX_PATH_LENGTH {
        return Err(Error::new(ErrorKind::InvalidPath, path));
    }
    let mut components = Vec::new();
    apply_components(&mut components, path.split('/'), path)?;
    Ok(components)
}

fn target_components(base: &[String], target: &str, path: &str) -> Result<Vec<String>> {
    if target.contains('\0') || target.chars().count() > MAX_PATH_LENGTH {
        return Err(Error::new(ErrorKind::InvalidPath, path));
    }
    let mut components = if target.starts_with('/') {
        Vec::new()
    } else {
        base.to_vec()
    };
    apply_components(&mut components, target.split('/'), path)?;
    Ok(components)
}

fn apply_components<'a>(
    components: &mut Vec<String>,
    parts: impl IntoIterator<Item = &'a str>,
    path: &str,
) -> Result<()> {
    for part in parts {
        match part {
            "" | "." => {}
            ".." => {
                if components.pop().is_none() {
                    return Err(Error::new(ErrorKind::InvalidPath, path));
                }
            }
            part if part.contains('\\') => {
                return Err(Error::new(ErrorKind::InvalidPath, path));
            }
            part => {
                components.push(part.to_owned());
                if components.len() > MAX_PATH_SEGMENTS {
                    return Err(Error::new(ErrorKind::InvalidPath, path));
                }
            }
        }
    }
    Ok(())
}

fn path_from_components(components: &[String]) -> String {
    if components.is_empty() {
        "/".to_owned()
    } else {
        format!("/{}", components.join("/"))
    }
}

fn checked_size(size: u64, path: &str) -> Result<usize> {
    if size > MAX_FILE_SIZE {
        return Err(Error::new(ErrorKind::FileTooLarge, path));
    }
    usize::try_from(size).map_err(|_| Error::new(ErrorKind::FileTooLarge, path))
}

fn random_suffix(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    bytes
        .iter()
        .map(|byte| ALPHABET[usize::from(*byte) % ALPHABET.len()] as char)
        .collect()
}

fn read_memory(data: &Mutex<Vec<u8>>, offset: u64, length: usize) -> Vec<u8> {
    let data = lock(data);
    read_bytes(&data, offset, length)
}

fn read_bytes(data: &[u8], offset: u64, length: usize) -> Vec<u8> {
    let start = usize::try_from(offset).unwrap_or(usize::MAX);
    if start >= data.len() {
        return Vec::new();
    }
    let end = start.saturating_add(length).min(data.len());
    data[start..end].to_vec()
}

fn write_memory(data: &Mutex<Vec<u8>>, offset: u64, input: &[u8], append: bool) -> Result<usize> {
    let mut data = lock(data);
    let start = if append {
        data.len()
    } else {
        usize::try_from(offset).map_err(|_| Error::new(ErrorKind::FileTooLarge, ""))?
    };
    let end = start
        .checked_add(input.len())
        .ok_or_else(|| Error::new(ErrorKind::FileTooLarge, ""))?;
    if end as u64 > MAX_FILE_SIZE {
        return Err(Error::new(ErrorKind::FileTooLarge, ""));
    }
    if end > data.len() {
        data.resize(end, 0);
    }
    data[start..end].copy_from_slice(input);
    Ok(input.len())
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Bundle, CopyOptions, ErrorKind, MAX_PATH_LENGTH, MAX_PATH_SEGMENTS, NodeType, OpenOptions,
        VirtualFileSystem,
    };

    type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

    fn vfs(bundle: &std::path::Path) -> VirtualFileSystem {
        VirtualFileSystem::new(Bundle::new(bundle))
    }

    #[test]
    fn keeps_tmp_mutable_and_bundle_readonly() -> TestResult {
        let directory = tempfile::tempdir()?;
        std::fs::create_dir_all(directory.path().join("config"))?;
        std::fs::write(directory.path().join("config/app.json"), b"hello")?;
        let mut vfs = vfs(directory.path());

        assert_eq!(vfs.read_file("/bundle/config/app.json")?, b"hello");
        assert!(!vfs.stat("/bundle/config/app.json")?.writable);
        vfs.write_file("/tmp/value.txt", b"one", false)?;
        vfs.write_file("/tmp/value.txt", b" two", true)?;
        assert_eq!(vfs.read_file("/tmp/value.txt")?, b"one two");
        assert_eq!(vfs.read_dir("/bundle/config")?[0].name, "app.json");
        let second = VirtualFileSystem::new(Bundle::new(directory.path()));
        assert_eq!(
            second
                .stat("/tmp/value.txt")
                .err()
                .map(|error| error.kind()),
            Some(ErrorKind::NotFound)
        );
        let error = match vfs.write_file("/bundle/nope", b"x", false) {
            Ok(()) => return Err("readonly bundle accepted a write".into()),
            Err(error) => error,
        };
        assert_eq!(error.kind(), ErrorKind::ReadOnly);
        Ok(())
    }

    #[test]
    fn implements_devices() -> TestResult {
        let directory = tempfile::tempdir()?;
        let mut vfs = vfs(directory.path());
        let null = vfs.open(
            "/dev/null",
            OpenOptions {
                read: true,
                write: true,
                ..OpenOptions::default()
            },
        )?;
        assert!(vfs.read(null, 8, None)?.is_empty());
        assert_eq!(vfs.write(null, b"ignored", None)?, 7);
        vfs.close(null)?;

        let zero = vfs.open("/dev/zero", OpenOptions::default())?;
        assert_eq!(vfs.read(zero, 3, None)?, vec![0, 0, 0]);
        vfs.close(zero)?;

        let full = vfs.open(
            "/dev/full",
            OpenOptions {
                read: true,
                write: true,
                ..OpenOptions::default()
            },
        )?;
        assert_eq!(vfs.read(full, 2, None)?, vec![0, 0]);
        let Err(error) = vfs.write(full, b"x", None) else {
            return Err("/dev/full accepted a write".into());
        };
        assert_eq!(error.kind(), ErrorKind::NoSpace);
        Ok(())
    }

    #[test]
    fn handles_symlinks_and_descriptors() -> TestResult {
        let directory = tempfile::tempdir()?;
        let mut vfs = vfs(directory.path());
        vfs.write_file("/tmp/source", b"hello", false)?;
        assert_eq!(
            vfs.mkdir("/tmp/source", true)
                .err()
                .map(|error| error.kind()),
            Some(ErrorKind::AlreadyExists)
        );
        vfs.symlink("/tmp/source", "/tmp/link")?;
        assert_eq!(vfs.read_file("/tmp/link")?, b"hello");
        assert_eq!(vfs.lstat("/tmp/link")?.kind, NodeType::Symlink);
        assert_eq!(vfs.realpath("/tmp/link")?, "/tmp/source");

        let fd = vfs.open(
            "/tmp/source",
            OpenOptions {
                read: true,
                write: true,
                ..OpenOptions::default()
            },
        )?;
        assert_eq!(vfs.read(fd, 5, None)?, b"hello");
        vfs.write(fd, b"!", Some(5))?;
        assert_eq!(vfs.fstat(fd)?.size, 6);
        vfs.close(fd)?;
        assert_eq!(vfs.read_file("/tmp/source")?, b"hello!");
        Ok(())
    }

    #[test]
    fn enforces_descriptor_permissions_and_path_limits() -> TestResult {
        let directory = tempfile::tempdir()?;
        let mut vfs = vfs(directory.path());
        vfs.write_file("/tmp/value", b"hello", false)?;

        let read_only = vfs.open("/tmp/value", OpenOptions::default())?;
        assert_eq!(
            vfs.ftruncate(read_only, 0).err().map(|error| error.kind()),
            Some(ErrorKind::NotPermitted)
        );
        assert_eq!(vfs.read_file("/tmp/value")?, b"hello");
        vfs.close(read_only)?;

        let too_long = format!("/{}", "x".repeat(MAX_PATH_LENGTH));
        assert_eq!(
            vfs.stat(&too_long).err().map(|error| error.kind()),
            Some(ErrorKind::InvalidPath)
        );

        let too_deep = format!(
            "/{}",
            (0..=MAX_PATH_SEGMENTS)
                .map(|_| "x")
                .collect::<Vec<_>>()
                .join("/")
        );
        assert_eq!(
            vfs.stat(&too_deep).err().map(|error| error.kind()),
            Some(ErrorKind::InvalidPath)
        );
        Ok(())
    }

    #[test]
    fn supports_access_copy_links_temp_and_vectors() -> TestResult {
        let directory = tempfile::tempdir()?;
        let mut vfs = vfs(directory.path());
        vfs.write_file("/tmp/source.txt", b"hello", false)?;
        vfs.mkdir("/tmp/tree", false)?;
        vfs.write_file("/tmp/tree/nested.txt", b"nested", false)?;

        vfs.access("/tmp/source.txt", 0)?;
        assert_eq!(
            vfs.access("/bundle/missing", 0)
                .err()
                .map(|error| error.kind()),
            Some(ErrorKind::NotFound)
        );
        assert_eq!(
            vfs.access("/tmp/source.txt", 1)
                .err()
                .map(|error| error.kind()),
            Some(ErrorKind::NotFound)
        );

        vfs.link("/tmp/source.txt", "/tmp/hard-link.txt")?;
        vfs.copy(
            "/tmp/tree",
            "/tmp/tree-copy",
            CopyOptions {
                recursive: true,
                force: true,
                ..CopyOptions::default()
            },
        )?;
        assert_eq!(vfs.read_file("/tmp/hard-link.txt")?, b"hello");
        assert_eq!(vfs.read_file("/tmp/tree-copy/nested.txt")?, b"nested");

        let temp = vfs.make_temp_dir("/tmp/prefix-")?;
        assert!(temp.starts_with("/tmp/prefix-"));
        assert_eq!(vfs.stat(&temp)?.kind, NodeType::Directory);

        let descriptor = vfs.open(
            "/tmp/vector.bin",
            OpenOptions {
                read: true,
                write: true,
                create: true,
                ..OpenOptions::default()
            },
        )?;
        assert_eq!(
            vfs.writev(descriptor, &[b"ab".to_vec(), b"cd".to_vec()], Some(0))?,
            4
        );
        assert_eq!(
            vfs.readv(descriptor, &[2, 2], Some(0))?,
            vec![b"ab".to_vec(), b"cd".to_vec()]
        );
        vfs.close(descriptor)?;
        Ok(())
    }
}
