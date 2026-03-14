using System.Buffers;
using System.Security.Cryptography;

namespace GPCK.Core
{
    public class ArchiveStream : Stream
    {
        private readonly GameArchive _archive;
        private readonly GameArchive.FileEntry _entry;

        private readonly bool _isCompressed;
        private readonly bool _isEncrypted;
        private readonly bool _isChunked;
        private readonly uint _method;

        private long _position;
        private AesGcm? _aes;

        private byte[]? _currentChunkData;
        private int _currentChunkIndex = -1;

        private ChunkTable.ChunkInfo[]? _chunkTable;
        private long _dataStartOffset;

        public ArchiveStream(GameArchive archive, GameArchive.FileEntry entry)
        {
            _archive = archive;
            _entry = entry;

            _isCompressed = (entry.Flags & GameArchive.FLAG_IS_COMPRESSED) != 0;
            _isEncrypted = (entry.Flags & GameArchive.FLAG_ENCRYPTED_META) != 0;
            _isChunked = (entry.Flags & GameArchive.FLAG_STREAMING) != 0;
            _method = entry.Flags & GameArchive.MASK_METHOD;

            if (_isEncrypted && _archive.DecryptionKey != null)
            {
                if (_archive.DecryptionKey.Length != 32) throw new ArgumentException("Key must be 32 bytes for AES-256-GCM");
                _aes = new AesGcm(_archive.DecryptionKey, 16);
            }

            if (_isChunked) InitializeChunkTable();
        }

        private void InitializeChunkTable()
        {
            int count = _entry.ChunkCount;

            if (_aes != null)
            {
                int encTableSize = 28 + (count * 8);
                byte[] encTable = new byte[encTableSize];
                _archive.ReadGtoc(encTable, _entry.ChunkTableOffset);

                byte[] decTable = new byte[count * 8];
                _aes.Decrypt(encTable.AsSpan(0, 12), encTable.AsSpan(28), encTable.AsSpan(12, 16), decTable);

                _chunkTable = ChunkTable.Read(decTable, count);
            }
            else
            {
                byte[] tableBuffer = new byte[count * 8];
                _archive.ReadGtoc(tableBuffer, _entry.ChunkTableOffset);
                _chunkTable = ChunkTable.Read(tableBuffer, count);
            }

            _dataStartOffset = _entry.DataOffset;
        }

        public override bool CanRead => true;
        public override bool CanSeek => true;
        public override bool CanWrite => false;
        public override long Length => _entry.OriginalSize;
        public override long Position { get => _position; set => _position = value; }

        public override int Read(Span<byte> buffer)
        {
            if (_position >= Length) return 0;
            int toRead = Math.Min(buffer.Length, (int)(Length - _position));
            int totalRead = 0;

            while (totalRead < toRead)
            {
                int chunkIdx = GetChunkIndexForPosition(_position, out long offsetInChunk);
                LoadChunk(chunkIdx);

                int available = (int)(_chunkTable![chunkIdx].OriginalSize - offsetInChunk);
                int copyCount = Math.Min(toRead - totalRead, available);

                _currentChunkData.AsSpan((int)offsetInChunk, copyCount).CopyTo(buffer.Slice(totalRead));
                _position += copyCount;
                totalRead += copyCount;
            }
            return totalRead;
        }

        public override int Read(byte[] buffer, int offset, int count) => Read(buffer.AsSpan(offset, count));

        private int GetChunkIndexForPosition(long pos, out long offsetInChunk)
        {
            long acc = 0;
            for (int i = 0; i < _chunkTable!.Length; i++)
            {
                if (pos < acc + _chunkTable[i].OriginalSize)
                {
                    offsetInChunk = pos - acc;
                    return i;
                }
                acc += _chunkTable[i].OriginalSize;
            }
            offsetInChunk = 0;
            return _chunkTable.Length - 1;
        }

        private void LoadChunk(int index)
        {
            if (_currentChunkIndex == index) return;

            long offset = _dataStartOffset;
            for (int i = 0; i < index; i++) offset += _chunkTable![i].CompressedSize;

            uint compSize = _chunkTable![index].CompressedSize;
            uint origSize = _chunkTable![index].OriginalSize;

            byte[] compBuffer = ArrayPool<byte>.Shared.Rent((int)compSize);
            try
            {
                int bytesRead = 0;
                while (bytesRead < compSize)
                {
                    int r = RandomAccess.Read(_archive.GetFileHandle(), compBuffer.AsSpan(bytesRead, (int)compSize - bytesRead), offset + bytesRead);
                    if (r <= 0) throw new EndOfStreamException($"Unexpected end of file. Read {bytesRead}/{compSize} bytes.");
                    bytesRead += r;
                }
                if (_currentChunkData == null || _currentChunkData.Length < origSize) _currentChunkData = new byte[origSize];
                DecompressInternal(compBuffer.AsSpan(0, (int)compSize), _currentChunkData, origSize);
                _currentChunkIndex = index;
            }
            finally { ArrayPool<byte>.Shared.Return(compBuffer); }
        }

        private unsafe void DecompressInternal(ReadOnlySpan<byte> source, byte[] destination, uint targetSize)
        {
            if (targetSize == 0 || source.Length == 0) return;
            if (!_isCompressed || source.Length == targetSize) { source.CopyTo(destination); return; }

            fixed (byte* pSrc = source, pDst = destination)
            {
                bool success = _method switch
                {
                    GameArchive.METHOD_GDEFLATE => CodecGDeflate.Decompress(pDst, targetSize, pSrc, (ulong)source.Length, 1),
                    GameArchive.METHOD_ZSTD => CodecZstd.ZSTD_decompress((IntPtr)pDst, targetSize, (IntPtr)pSrc, (ulong)source.Length) < (ulong)uint.MaxValue,
                    GameArchive.METHOD_LZ4 => CodecLZ4.LZ4_decompress_safe((IntPtr)pSrc, (IntPtr)pDst, source.Length, (int)targetSize) >= 0,
                    _ => false
                };
                if (!success) throw new IOException("Decompression failed");
            }
        }

        public override long Seek(long offset, SeekOrigin origin)
        {
            _position = Math.Clamp(origin switch { SeekOrigin.Begin => offset, SeekOrigin.Current => _position + offset, SeekOrigin.End => Length + offset, _ => _position }, 0, Length);
            return _position;
        }

        public override void Flush() { }
        public override void SetLength(long value) => throw new NotSupportedException();
        public override void Write(byte[] buffer, int offset, int count) => throw new NotSupportedException();
        protected override void Dispose(bool disposing) { _aes?.Dispose(); base.Dispose(disposing); }
    }
}
