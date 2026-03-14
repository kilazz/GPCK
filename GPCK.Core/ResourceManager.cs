using System.Collections.Concurrent;

namespace GPCK.Core
{
    public class ResourceManager
    {
        private readonly VirtualFileSystem _vfs;
        private readonly ConcurrentDictionary<Guid, object> _loadedAssets = new();

        public ResourceManager(VirtualFileSystem vfs)
        {
            _vfs = vfs;
        }

        public async ValueTask<T> LoadAssetAsync<T>(string virtualPath, CancellationToken ct = default) where T : class
        {
            Guid assetId = AssetIdGenerator.Generate(virtualPath);
            return await LoadAssetRecursive<T>(assetId, ct).ConfigureAwait(false);
        }

        private async ValueTask<T> LoadAssetRecursive<T>(Guid assetId, CancellationToken ct) where T : class
        {
            if (_loadedAssets.TryGetValue(assetId, out var cached)) return (T)cached;

            if (!_vfs.TryGetEntryForId(assetId, out var archive, out var entry))
                throw new FileNotFoundException($"Asset {assetId} not found in VFS.");

            using var stream = archive.OpenRead(entry);
            object? result;

            if (typeof(T) == typeof(string))
            {
                using var reader = new StreamReader(stream);
                result = await reader.ReadToEndAsync(ct).ConfigureAwait(false);
            }
            else if (typeof(T) == typeof(byte[]))
            {
                using var ms = new MemoryStream();
                await stream.CopyToAsync(ms, ct).ConfigureAwait(false);
                result = ms.ToArray();
            }
            else
            {
                using var ms = new MemoryStream();
                await stream.CopyToAsync(ms, ct).ConfigureAwait(false);
                result = ms.ToArray();
            }

            if (result == null) throw new InvalidOperationException($"Failed to load asset {assetId}");

            _loadedAssets.TryAdd(assetId, result);
            return (T)result;
        }

        public void UnloadAll() => _loadedAssets.Clear();
    }
}
