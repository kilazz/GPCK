namespace GPCK.Core
{
    public class PackageInfo
    {
        public string FilePath { get; set; } = string.Empty;
        public string Magic { get; set; } = string.Empty;
        public int Version { get; set; }
        public int FileCount { get; set; }
        public long TotalSize { get; set; }
        public bool HasDebugNames { get; set; }
        public List<PackageEntryInfo> Entries { get; set; } = new List<PackageEntryInfo>();
    }

    public class PackageEntryInfo
    {
        public string Path { get; set; } = string.Empty;
        public Guid AssetId { get; set; }
        public long Offset { get; set; }
        public long OriginalSize { get; set; }
        public long CompressedSize { get; set; }
        public long Alignment { get; set; }
        public string Method { get; set; } = "Store";
        public string MetadataInfo { get; set; } = ""; // e.g. "2048x2048 Mips:12"
    }
}
