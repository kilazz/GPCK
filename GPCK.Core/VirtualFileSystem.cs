using System.Diagnostics.CodeAnalysis;

namespace GPCK.Core
{
    /// <summary>
    /// Virtual File System (VFS).
    /// Allows mounting multiple .gtoc archives as layers.
    /// Priority: Last mounted archive overrides files from previous ones.
    /// </summary>
    public class VirtualFileSystem : IDisposable
    {
        private readonly List<GameArchive> _mountedArchives = new();
        private readonly Dictionary<Guid, int> _virtualLookup = new();
        private readonly List<string> _mountedDirectories = new();

        public int MountedArchiveCount => _mountedArchives.Count;
        public int MountedDirectoryCount => _mountedDirectories.Count;

        /// <summary>
        /// Mounts a .gtoc archive.
        /// </summary>
        public void MountArchive(string path)
        {
            var archive = new GameArchive(path);
            _mountedArchives.Add(archive);
            int archiveIndex = _mountedArchives.Count - 1;

            for (int i = 0; i < archive.FileCount; i++)
            {
                var entry = archive.GetEntryByIndex(i);
                _virtualLookup[entry.AssetId] = archiveIndex;
            }
        }

        /// <summary>
        /// Mounts a physical directory. Files in this directory will override files in archives.
        /// Useful for modding and local development.
        /// </summary>
        public void MountDirectory(string physicalPath)
        {
            if (!Directory.Exists(physicalPath))
                throw new DirectoryNotFoundException($"Directory not found: {physicalPath}");

            // Insert at the beginning so directories have the highest priority
            _mountedDirectories.Insert(0, physicalPath);
        }

        public bool FileExists(string virtualPath)
        {
            // 1. Check mounted directories first (Loose files override)
            foreach (var dir in _mountedDirectories)
            {
                string physicalPath = Path.Combine(dir, virtualPath);
                if (File.Exists(physicalPath))
                    return true;
            }

            // 2. Check archives
            Guid id = AssetIdGenerator.Generate(virtualPath);
            return _virtualLookup.ContainsKey(id);
        }

        public Stream OpenRead(string virtualPath)
        {
            // 1. Check mounted directories first (Loose files override)
            foreach (var dir in _mountedDirectories)
            {
                string physicalPath = Path.Combine(dir, virtualPath);
                if (File.Exists(physicalPath))
                {
                    return new FileStream(physicalPath, FileMode.Open, FileAccess.Read, FileShare.Read);
                }
            }

            // 2. Check archives
            Guid id = AssetIdGenerator.Generate(virtualPath);

            if (TryGetEntryForId(id, out var archive, out var entry))
            {
                return archive.OpenRead(entry);
            }

            throw new FileNotFoundException($"File not found in VFS: {virtualPath}");
        }

        public bool TryGetEntryForId(Guid id, [NotNullWhen(true)] out GameArchive? archive, out GameArchive.FileEntry entry)
        {
            if (_virtualLookup.TryGetValue(id, out int archiveIndex))
            {
                archive = _mountedArchives[archiveIndex];
                return archive.TryGetEntry(id, out entry);
            }
            archive = null;
            entry = default;
            return false;
        }

        public string GetSourceArchiveName(string virtualPath)
        {
            Guid id = AssetIdGenerator.Generate(virtualPath);
            if (_virtualLookup.TryGetValue(id, out int archiveIndex))
            {
                return Path.GetFileName(_mountedArchives[archiveIndex].FilePath);
            }
            return "None";
        }

        public void Dispose()
        {
            foreach (var archive in _mountedArchives)
            {
                archive.Dispose();
            }
            _mountedArchives.Clear();
            _virtualLookup.Clear();
        }
    }
}