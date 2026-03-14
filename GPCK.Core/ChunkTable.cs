namespace GPCK.Core
{
    public class ChunkTable
    {
        public struct ChunkInfo
        {
            public uint CompressedSize;
            public uint OriginalSize;
        }

        public static byte[] Write(List<ChunkInfo> chunks)
        {
            byte[] table = new byte[chunks.Count * 8];
            for (int i = 0; i < chunks.Count; i++)
            {
                BitConverter.TryWriteBytes(table.AsSpan(i * 8, 4), chunks[i].CompressedSize);
                BitConverter.TryWriteBytes(table.AsSpan(i * 8 + 4, 4), chunks[i].OriginalSize);
            }
            return table;
        }

        public static ChunkInfo[] Read(ReadOnlySpan<byte> data, int count)
        {
            var chunks = new ChunkInfo[count];
            for (int i = 0; i < count; i++)
            {
                chunks[i] = new ChunkInfo
                {
                    CompressedSize = BitConverter.ToUInt32(data.Slice(i * 8, 4)),
                    OriginalSize = BitConverter.ToUInt32(data.Slice(i * 8 + 4, 4))
                };
            }
            return chunks;
        }
    }
}
